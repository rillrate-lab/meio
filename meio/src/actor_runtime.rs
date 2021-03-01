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
//! # use meio::{Actor, Context, IdOf, TaskEliminated, TaskError, LiteTask};
//! trait SpecificTask: LiteTask {}
//!
//! struct MyActor {}
//!
//! impl Actor for MyActor {
//!     type GroupBy = ();
//! }
//!
//! #[async_trait]
//! impl<T: SpecificTask> TaskEliminated<T> for MyActor {
//!     async fn handle(
//!         &mut self,
//!         id: IdOf<T>,
//!         result: Result<T::Output, TaskError>,
//!         ctx: &mut Context<Self>,
//!     ) -> Result<(), Error> {
//!         Ok(())
//!     }
//! }
//! ```

use crate::compat::watch;
use crate::forwarders::StreamForwarder;
use crate::handlers::{
    ActionHandler, Consumer, Eliminated, Envelope, Interact, Interaction, InteractionDone,
    InteractionTask, InterruptedBy, Operation, Parcel, StartedBy, TaskEliminated,
};
use crate::ids::{Id, IdOf};
use crate::lifecycle::{Awake, Done, LifecycleNotifier, LifetimeTracker};
use crate::linkage::Address;
use crate::lite_runtime::{self, LiteTask, TaskAddress};
use anyhow::Error;
use async_trait::async_trait;
use futures::channel::mpsc;
use futures::stream::{pending, FusedStream};
use futures::{select_biased, Stream, StreamExt};
use std::hash::Hash;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
enum Reason {
    #[error("Actor is terminating...")]
    Terminating,
}

const MESSAGES_CHANNEL_DEPTH: usize = 32;

/// The main trait. Your structs have to implement it to
/// be compatible with `ActorRuntime` and `Address` system.
///
/// **Recommended** to implement reactive activities.
#[async_trait]
pub trait Actor: Sized + Send + 'static {
    /// Specifies how to group child actors.
    type GroupBy: Clone + Send + Eq + Hash;

    /// Returns unique name of the `Actor`.
    /// Uses `Uuid` by default.
    fn name(&self) -> String {
        let uuid = Uuid::new_v4();
        format!("Actor:{}({})", std::any::type_name::<Self>(), uuid)
    }

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
pub(crate) fn spawn<A, S>(actor: A, supervisor: Option<Address<S>>) -> Address<A>
where
    A: Actor + StartedBy<S>,
    S: Actor + Eliminated<A>,
{
    let id = Id::of_actor(&actor);
    let (hp_msg_tx, hp_msg_rx) = mpsc::unbounded();
    let (msg_tx, msg_rx) = mpsc::channel(MESSAGES_CHANNEL_DEPTH);
    let (join_tx, join_rx) = watch::channel(Status::Alive);
    let address = Address::new(id, hp_msg_tx, msg_tx, join_rx);
    let id: Id = address.id().into();
    // There is `Envelope` here, because it will be processed at start and
    // will never been sent to prevent other messages come before the `Awake`.
    let awake_envelope = Envelope::instant(Awake::new());
    let done_notifier = {
        match supervisor {
            None => LifecycleNotifier::ignore(),
            Some(super_addr) => {
                let op = Operation::Done { id };
                LifecycleNotifier::once(super_addr, op)
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
        msg_rx,
        hp_msg_rx,
        join_tx,
    };
    crate::compat::spawn_async(runtime.entrypoint());
    address
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
    pub fn spawn_actor<T>(&mut self, actor: T, group: A::GroupBy) -> Address<T>
    where
        T: Actor + StartedBy<A> + InterruptedBy<A>,
        A: Eliminated<T>,
    {
        let address = spawn(actor, Some(self.address.clone()));
        self.lifetime_tracker.insert(address.clone(), group);
        address
    }

    /// Starts and binds a `Task`.
    pub fn spawn_task<T>(&mut self, task: T, group: A::GroupBy) -> TaskAddress<T>
    where
        T: LiteTask,
        A: TaskEliminated<T>,
    {
        let stopper = lite_runtime::spawn(task, Some(self.address.clone()));
        self.lifetime_tracker.insert_task(stopper.clone(), group);
        stopper
    }

    /// Spawns interaction task that forwards the result of an interaction.
    pub fn attach<S>(&mut self, stream: S, group: A::GroupBy)
    where
        S: Stream + Unpin + Send + 'static,
        S::Item: Send,
        A: Consumer<S::Item>,
    {
        let forwarder = StreamForwarder::new(stream, self.address.clone());
        self.spawn_task(forwarder, group);
    }

    pub fn track_interaction<I>(&mut self, task: InteractionTask<I>, group: A::GroupBy)
    where
        I: Interaction,
        A: InteractionDone<I>,
    {
        self.spawn_task(task, group);
    }

    /// Interrupts an `Actor`.
    pub fn interrupt<T>(&mut self, address: &mut Address<T>) -> Result<(), Error>
    where
        T: Actor + InterruptedBy<A>,
    {
        address.interrupt_by()
    }

    /// Returns true if the shutdown process is in progress.
    pub fn is_terminating(&self) -> bool {
        self.lifetime_tracker.is_terminating()
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

    /// Increases the priority of the `Actor`'s type.
    pub fn termination_sequence(&mut self, sequence: Vec<A::GroupBy>) {
        self.lifetime_tracker.prioritize_termination_by(sequence);
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
}

/// `ActorRuntime` for `Actor`.
pub struct ActorRuntime<A: Actor> {
    id: IdOf<A>,
    actor: A,
    context: Context<A>,
    awake_envelope: Option<Envelope<A>>,
    done_notifier: Box<dyn LifecycleNotifier<Done<A>>>,
    /// `Receiver` that have to be used to receive incoming messages.
    msg_rx: mpsc::Receiver<Envelope<A>>,
    /// High-priority receiver
    hp_msg_rx: mpsc::UnboundedReceiver<Parcel<A>>,
    /// Sends a signal when the `Actor` completely stopped.
    join_tx: watch::Sender<Status>,
}

impl<A: Actor> ActorRuntime<A> {
    /// The `entrypoint` of the `ActorRuntime` that calls `routine` method.
    async fn entrypoint(mut self) {
        log::info!("Actor started: {:?}", self.id);
        let mut awake_envelope = self
            .awake_envelope
            .take()
            .expect("awake envelope has to be set in spawn method!");
        let awake_res = awake_envelope
            .handle(&mut self.actor, &mut self.context)
            .await;
        match awake_res {
            Ok(_) => {
                self.routine().await;
            }
            Err(err) => {
                log::error!(
                    "Can't call awake notification handler of the actor {:?}: {}",
                    self.id,
                    err
                );
            }
        }
        log::info!("Actor finished: {:?}", self.id);
        let done_event = Done::new(self.id.clone());
        if let Err(err) = self.done_notifier.notify(done_event) {
            log::error!(
                "Can't send done notification from the actor {:?}: {}",
                self.id,
                err
            );
        }
        if !self.join_tx.is_closed() {
            if let Err(_err) = self.join_tx.send(Status::Stop) {
                log::error!("Can't release joiners of {:?}", self.id);
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
                hp_envelope = self.hp_msg_rx.next() => {
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
                                log::trace!("Scheduled events: {}", scheduled_queue.get_ref().len());
                                process_envelope = None;
                            }
                        }
                        if let Some(mut envelope) = process_envelope {
                            let handle_res = envelope.handle(&mut self.actor, &mut self.context).await;
                            if let Err(err) = handle_res {
                                log::error!("Handler for {:?} (high-priority) failed: {}", self.id, err);
                            }
                        }
                    } else {
                        // Even if all `Address` dropped `Actor` can do something useful on
                        // background. Than don't terminate actors without `Addresses`, because
                        // it still has controllers.
                        // Background tasks = something spawned that `Actors` waits for finishing.
                        log::trace!("Messages stream of {:?} (high-priority) drained.", self.id);
                        if let Err(err) = self.actor.instant_queue_drained(&mut self.context).await {
                            log::error!("Queue (high-priority) drained handler {:?} failed: {}", self.id, err);
                        }
                    }
                }
                opt_delayed_envelope = maybe_queue.next() => {
                    if let Some(delayed_envelope) = opt_delayed_envelope {
                        match delayed_envelope {
                            Ok(expired) => {
                                log::trace!("Execute scheduled event. Remained: {}", scheduled_queue.get_ref().len());
                                let mut envelope = expired.into_inner();
                                let handle_res = envelope.handle(&mut self.actor, &mut self.context).await;
                                if let Err(err) = handle_res {
                                    log::error!("Handler for {:?} (scheduled) failed: {}", self.id, err);
                                }
                            }
                            Err(err) => {
                                log::error!("Failed scheduled execution for {:?}: {}", self.id, err);
                            }
                        }
                    } else {
                        log::error!("Delay queue of {} closed.", self.id);
                        if let Err(err) = self.actor.instant_queue_drained(&mut self.context).await {
                            log::error!("Queue (high-priority) drained handler {:?} failed: {}", self.id, err);
                        }
                    }
                }
                lp_envelope = self.msg_rx.next() => {
                    if let Some(mut envelope) = lp_envelope {
                        let handle_res = envelope.handle(&mut self.actor, &mut self.context).await;
                        if let Err(err) = handle_res {
                            log::error!("Handler for {:?} failed: {}", self.id, err);
                        }
                    } else {
                        // Even if all `Address` dropped `Actor` can do something useful on
                        // background. Than don't terminate actors without `Addresses`, because
                        // it still has controllers.
                        // Background tasks = something spawned that `Actors` waits for finishing.
                        log::trace!("Messages stream of {:?} drained.", self.id);
                        if let Err(err) = self.actor.queue_drained(&mut self.context).await {
                            log::error!("Queue drained handler {:?} failed: {}", self.id, err);
                        }
                    }
                }
            }
            /*
            let inspection_res = self.actor.inspection(&mut self.context).await;
            if let Err(err) = inspection_res {
                log::error!("Inspection of {:?} failed: {}", self.id, err);
            }
            */
        }
    }
}
