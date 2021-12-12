//! This module contains `Actor` trait and the runtime to execute it.
//!
//! ###
//!
//! If you want to limit types that can be used as `LiteTasks` you can
//! add a trait that required `ListTask` and implement `TaskEliminated`
//! for your trait only. The compiler will not allow you to spawn other
//! task except with your type.
//!
//! Example:
//!
//! ```
//! # use anyhow::Error;
//! # use async_trait::async_trait;
//! # use meio::prelude::{Actor, Context, IdOf, TaskEliminated, TaskError, LiteTask};
//!
//! trait SpecificTask: LiteTask {}
//!
//! struct MyActor {}
//!
//! impl Actor for MyActor {
//!     type GroupBy = ();
//!
//!     fn log_target(&self) -> &str { "MyActor" }
//! }
//!
//! #[async_trait]
//! impl<T: SpecificTask> TaskEliminated<T, ()> for MyActor {
//!     async fn handle(
//!         &mut self,
//!         id: IdOf<T>,
//!         tag: (),
//!         result: Result<T::Output, TaskError>,
//!         ctx: &mut Context<Self>,
//!     ) -> Result<(), Error> {
//!         Ok(())
//!     }
//! }
//! ```

use crate::forwarders::StreamForwarder;
use crate::handlers::{
    Consumer, Eliminated, Envelope, Interaction, InteractionDone, InteractionTask, InterruptedBy,
    Operation, StartedBy, TaskEliminated,
};
use crate::ids::{Id, IdOf};
use crate::lifecycle::{Awake, Done, LifecycleNotifier, LifetimeTracker};
use crate::linkage::{Address, AddressJoint, AddressPair};
use crate::lite_runtime::{self, LiteTask, Tag, TaskAddress};
use anyhow::Error;
use async_trait::async_trait;
use futures::stream::{pending, FusedStream};
use futures::{select_biased, FutureExt, Stream, StreamExt};
use std::hash::Hash;
use thiserror::Error;

#[derive(Debug, Error)]
enum Reason {
    #[error("Actor is terminating...")]
    Terminating,
}

/// Declares sequence of groups termination.
pub trait TerminationSequence: Sized {
    /// Returns a termination sequence.
    fn termination_sequence() -> Vec<Self>;
}

impl TerminationSequence for () {
    fn termination_sequence() -> Vec<Self> {
        vec![()]
    }
}

/// The main trait. Your structs have to implement it to
/// be compatible with `ActorRuntime` and `Address` system.
///
/// **Recommended** to implement reactive activities.
#[async_trait]
pub trait Actor: Sized + Send + 'static {
    /// Specifies how to group child actors.
    type GroupBy: TerminationSequence + Clone + Send + Eq + Hash;

    /// The log target for the `Actor`.
    fn log_target(&self) -> &str;

    #[doc(hidden)] // Not ready yet
    /// Called when `Action` queue drained (no more messages will be sent).
    async fn queue_drained(&mut self, _ctx: &mut Context<Self>) -> Result<(), Error> {
        Ok(())
    }

    #[doc(hidden)] // Not ready yet
    /// Called when `InstantAction` queue drained (no more messages will be sent).
    async fn instant_queue_drained(&mut self, _ctx: &mut Context<Self>) -> Result<(), Error> {
        Ok(())
    }
}

/// Status of the task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    /// Task is alive and working.
    Alive,
    /// Task had finished.
    Stop,
}

impl Status {
    /// Is task finished yet?
    pub fn is_done(&self) -> bool {
        *self == Status::Stop
    }
}

/// Spawns `Actor` in `ActorRuntime`.
// TODO: No `Option`! Use `static Address<System>` instead.
// It can be possible when `Controller` and `Operator` will be removed.
pub(crate) fn spawn<A, S>(actor: A, supervisor: Option<Address<S>>, address_pair: AddressPair<A>)
where
    A: Actor + StartedBy<S>,
    S: Actor + Eliminated<A>,
{
    let AddressPair { joint, address } = address_pair;
    let id: Id = address.id().into();
    // There is `Envelope` here, because it will be processed at start and
    // will never been sent to prevent other messages come before the `Awake`.
    let awake_envelope = Envelope::instant(Awake::new());
    let done_notifier = {
        match supervisor {
            None => <dyn LifecycleNotifier<_>>::ignore(),
            Some(super_addr) => {
                let op = Operation::Done { id };
                <dyn LifecycleNotifier<_>>::once(super_addr, op)
            }
        }
    };
    let context = Context {
        alive: true,
        address: address.clone(),
        lifetime_tracker: LifetimeTracker::new(),
        //terminator: Terminator::new(id.clone()),
    };
    let runtime = ActorRuntime {
        id: address.id(),
        actor,
        context,
        awake_envelope: Some(awake_envelope),
        done_notifier,
        joint,
    };
    crate::compat::spawn_async(runtime.entrypoint());
}

/// `Context` of a `ActorRuntime` that contains `Address` and `Receiver`.
pub struct Context<A: Actor> {
    alive: bool,
    address: Address<A>,
    lifetime_tracker: LifetimeTracker<A>,
    //terminator: Terminator,
}

impl<A: Actor> Context<A> {
    /// Returns an instance of the `Address`.
    pub fn address(&mut self) -> &mut Address<A> {
        &mut self.address
    }

    /// Starts and binds an `Actor`.
    pub fn spawn_actor_with_addr<T>(&mut self, actor: T, pair: AddressPair<T>, group: A::GroupBy)
    where
        T: Actor + StartedBy<A> + InterruptedBy<A>,
        A: Eliminated<T>,
    {
        let address = pair.address().clone();
        spawn(actor, Some(self.address.clone()), pair);
        self.lifetime_tracker.insert(address, group);
    }

    /// Starts and binds an `Actor`.
    pub fn spawn_actor<T>(&mut self, actor: T, group: A::GroupBy) -> Address<T>
    where
        T: Actor + StartedBy<A> + InterruptedBy<A>,
        A: Eliminated<T>,
    {
        let pair = AddressPair::new();
        let address = pair.address().clone();
        self.spawn_actor_with_addr(actor, pair, group);
        address
    }

    /// Starts and binds a `Task`.
    pub fn spawn_task<T, M>(&mut self, task: T, tag: M, group: A::GroupBy) -> TaskAddress<T>
    where
        T: LiteTask,
        A: TaskEliminated<T, M>,
        M: Tag,
    {
        let stopper = lite_runtime::spawn(task, tag, Some(self.address.clone()));
        self.lifetime_tracker.insert_task(stopper.clone(), group);
        stopper
    }

    /// Spawns interaction task that forwards the result of an interaction.
    pub fn attach<S, M>(&mut self, stream: S, tag: M, group: A::GroupBy)
    where
        S: Stream + Unpin + Send + 'static,
        S::Item: Send,
        A: Consumer<S::Item>,
        M: Tag,
    {
        let forwarder = StreamForwarder::new(stream, self.address.clone());
        self.spawn_task(forwarder, tag, group);
    }

    /// Spawns `InteractionTask` as a `LiteTask` and await the result as an `Action`
    /// that will call `InteractionDone` handler.
    pub fn track_interaction<I, M>(&mut self, task: InteractionTask<I>, tag: M, group: A::GroupBy)
    where
        I: Interaction,
        A: InteractionDone<I, M>,
        M: Tag,
    {
        self.spawn_task(task, tag, group);
    }

    /// Interrupts an `Actor`.
    pub fn interrupt<T>(&mut self, address: &mut Address<T>) -> Result<(), Error>
    where
        T: Actor + InterruptedBy<A>,
    {
        address.interrupt_by()
    }

    /// Returns `Error` if the `Actor` is terminating.
    /// Useful for checking in handlers.
    pub fn not_terminating(&self) -> Result<(), Error> {
        if self.is_terminating() {
            Err(Reason::Terminating.into())
        } else {
            Ok(())
        }
    }

    /// Stops the runtime of the `Actor` on one message will be processed after this call.
    ///
    /// It's recommended way to terminate `Actor` is the `shutdown` method.
    ///
    /// > Attention! Termination process will never started here and all spawned actors
    /// and tasks will be orphaned.
    pub fn stop(&mut self) {
        self.alive = false;
    }

    /// Starts graceful termination of the `Actor`.
    pub fn shutdown(&mut self) {
        self.lifetime_tracker.start_termination();
        if self.lifetime_tracker.is_finished() {
            self.stop();
        }
    }

    // TODO: Maybe provide a reference to the `LifetimeTracker`
    // instead of recalling methods below:

    /// Sends interruption signal to the sepcific group of actors and tasks.
    pub fn terminate_group(&mut self, group: A::GroupBy) {
        self.lifetime_tracker.terminate_group(group)
    }

    /// Returns true if the shutdown process is in progress.
    pub fn is_terminating(&self) -> bool {
        self.lifetime_tracker.is_terminating()
    }

    /// Increases the priority of the `Actor`'s type.
    ///
    /// Used internally before `StartedBy` call.
    fn termination_sequence(&mut self, sequence: Vec<A::GroupBy>) {
        self.lifetime_tracker.termination_sequence(sequence);
    }
}

/// `ActorRuntime` for `Actor`.
pub struct ActorRuntime<A: Actor> {
    id: IdOf<A>,
    actor: A,
    context: Context<A>,
    awake_envelope: Option<Envelope<A>>,
    done_notifier: Box<dyn LifecycleNotifier<Done<A>>>,
    joint: AddressJoint<A>,
}

impl<A: Actor> ActorRuntime<A> {
    /// The `entrypoint` of the `ActorRuntime` that calls `routine` method.
    async fn entrypoint(mut self) {
        log::info!(target: self.actor.log_target(), "Actor started: {}", self.id);
        let mut awake_envelope = self
            .awake_envelope
            .take()
            .expect("awake envelope has to be set in spawn method!");
        let term_seq = A::GroupBy::termination_sequence();
        self.context.termination_sequence(term_seq);
        let awake_res = awake_envelope
            .handle(&mut self.actor, &mut self.context)
            .await;
        match awake_res {
            Ok(_) => {
                self.routine().await;
            }
            Err(err) => {
                log::error!(
                    target: self.actor.log_target(),
                    "Can't call awake notification handler of the actor {:?}: {}",
                    self.id,
                    err
                );
            }
        }
        log::info!(target: self.actor.log_target(), "Actor finished: {}", self.id);
        let done_event = Done::new(self.id.clone());
        if let Err(err) = self.done_notifier.notify(done_event) {
            log::error!(
                target: self.actor.log_target(),
                "Can't send done notification from the actor {:?}: {}",
                self.id,
                err
            );
        }
        if !self.joint.join_tx.is_closed() {
            if let Err(_err) = self.joint.join_tx.send(Status::Stop) {
                log::error!(target: self.actor.log_target(), "Can't release joiners of {}", self.id);
            }
        }
    }

    async fn routine(&mut self) {
        let mut scheduled_queue = crate::compat::DelayQueue::<Envelope<A>>::new().fuse();
        let mut pendel = pending();
        while self.context.alive {
            // This is a workaround not to call `DelayQueue` if it has no items,
            // because its resumable, but returns `None` and brokes (closes) `FusedStream`.
            // And after some delay no any future scheduled event occured.
            // TODO: Create `tokio_util` PR to make `DelayQueue` unstoppable (always pending)
            // Awaiting for: https://github.com/tokio-rs/tokio/issues/3407
            let maybe_queue: &mut (dyn FusedStream<Item = Result<_, _>> + Unpin + Send) =
                if scheduled_queue.get_ref().len() > 0 {
                    &mut scheduled_queue
                } else {
                    &mut pendel
                };
            select_biased! {
                hp_envelope = self.joint.hp_msg_rx.recv().fuse() => {
                    if let Some(hp_env) = hp_envelope {
                        let envelope = hp_env.envelope;
                        let process_envelope;
                        match hp_env.operation {
                            Operation::Forward => {
                                process_envelope = Some(envelope);
                            }
                            Operation::Done { id } => {
                                self.context.lifetime_tracker.remove(&id);
                                if self.context.lifetime_tracker.is_finished() {

                                    self.context.stop();
                                }
                                process_envelope = Some(envelope);
                            }
                            Operation::Schedule { deadline } => {
                                scheduled_queue.get_mut().insert_at(envelope, deadline);
                                log::trace!(target: self.actor.log_target(), "Scheduled events: {}", scheduled_queue.get_ref().len());
                                process_envelope = None;
                            }
                        }
                        if let Some(mut envelope) = process_envelope {
                            let handle_res = envelope.handle(&mut self.actor, &mut self.context).await;
                            if let Err(err) = handle_res {
                                log::error!(target: self.actor.log_target(), "Handler for {} (high-priority) failed: {}", self.id, err);
                            }
                        }
                    } else {
                        // Even if all `Address` dropped `Actor` can do something useful on
                        // background. Than don't terminate actors without `Addresses`, because
                        // it still has controllers.
                        // Background tasks = something spawned that `Actors` waits for finishing.
                        log::trace!(target: self.actor.log_target(), "Messages stream of {} (high-priority) drained.", self.id);
                        if let Err(err) = self.actor.instant_queue_drained(&mut self.context).await {
                            log::error!(target: self.actor.log_target(), "Queue (high-priority) drained handler {} failed: {}", self.id, err);
                        }
                    }
                }
                opt_delayed_envelope = maybe_queue.next() => {
                    if let Some(delayed_envelope) = opt_delayed_envelope {
                        match delayed_envelope {
                            Ok(expired) => {
                                log::trace!(target: self.actor.log_target(), "Execute scheduled event. Remained: {}", scheduled_queue.get_ref().len());
                                let mut envelope = expired.into_inner();
                                let handle_res = envelope.handle(&mut self.actor, &mut self.context).await;
                                if let Err(err) = handle_res {
                                    log::error!(target: self.actor.log_target(), "Handler for {} (scheduled) failed: {}", self.id, err);
                                }
                            }
                            Err(err) => {
                                log::error!(target: self.actor.log_target(), "Failed scheduled execution for {}: {}", self.id, err);
                            }
                        }
                    } else {
                        log::error!(target: self.actor.log_target(), "Delay queue of {} closed.", self.id);
                        if let Err(err) = self.actor.instant_queue_drained(&mut self.context).await {
                            log::error!(target: self.actor.log_target(), "Queue (high-priority) drained handler {} failed: {}", self.id, err);
                        }
                    }
                }
                lp_envelope = self.joint.msg_rx.recv().fuse() => {
                    if let Some(mut envelope) = lp_envelope {
                        let handle_res = envelope.handle(&mut self.actor, &mut self.context).await;
                        if let Err(err) = handle_res {
                            log::error!(target: self.actor.log_target(), "Handler for {} failed: {}", self.id, err);
                        }
                    } else {
                        // Even if all `Address` dropped `Actor` can do something useful on
                        // background. Than don't terminate actors without `Addresses`, because
                        // it still has controllers.
                        // Background tasks = something spawned that `Actors` waits for finishing.
                        log::trace!(target: self.actor.log_target(), "Messages stream of {} drained.", self.id);
                        if let Err(err) = self.actor.queue_drained(&mut self.context).await {
                            log::error!(target: self.actor.log_target(), "Queue drained handler {} failed: {}", self.id, err);
                        }
                    }
                }
            }
            /*
            let inspection_res = self.actor.inspection(&mut self.context).await;
            if let Err(err) = inspection_res {
                log::error!(target: self.actor.log_target(), "Inspection of {:?} failed: {}", self.id, err);
            }
            */
        }
    }
}
