//! This module contains `Actor` trait and the runtime to execute it.

// TODO: Fix imports
use crate::{
    handlers::{Eliminated, HpEnvelope, InterruptedBy, Operation, StartedBy},
    lifecycle::{Awake, Done, LifecycleNotifier, LifetimeTracker},
    ActionHandler, Address, Envelope, Id, LiteTask, Task,
};
use async_trait::async_trait;
use futures::channel::mpsc;
use futures::{select_biased, StreamExt};
use tokio::sync::watch;
use uuid::Uuid;

const MESSAGES_CHANNEL_DEPTH: usize = 32;

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
    A: Actor + ActionHandler<Awake<S>>,
    S: Actor + ActionHandler<Done<A>>,
{
    let id = Id::of_actor(&actor);
    let (hp_msg_tx, hp_msg_rx) = mpsc::unbounded();
    let (msg_tx, msg_rx) = mpsc::channel(MESSAGES_CHANNEL_DEPTH);
    let (join_tx, join_rx) = watch::channel(Status::Alive);
    let address = Address::new(id, hp_msg_tx, msg_tx, join_rx);
    let id: Id = address.id().into();
    // There is `Envelope` here, because it will be processed at start and
    // will never been sent to prevent other messages come before the `Awake`.
    let awake_envelope = Envelope::new(Awake::new());
    let done_notifier = {
        match supervisor {
            None => LifecycleNotifier::ignore(),
            Some(super_addr) => {
                let event = Done::new(address.id());
                let op = Operation::Done { id: id.clone() };
                LifecycleNotifier::once(super_addr, op, event)
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
        id,
        actor,
        context,
        awake_envelope: Some(awake_envelope),
        done_notifier,
        msg_rx,
        hp_msg_rx,
        join_tx,
    };
    tokio::spawn(runtime.entrypoint());
    address
}

/// The main trait. Your structs have to implement it to
/// be compatible with `ActorRuntime` and `Address` system.
///
/// **Recommended** to implement reactive activities.
#[async_trait]
pub trait Actor: Sized + Send + 'static {
    /// Specifies how to group child actors.
    type GroupBy;

    /// Returns unique name of the `Actor`.
    /// Uses `Uuid` by default.
    fn name(&self) -> String {
        let uuid = Uuid::new_v4();
        format!("Actor:{}({})", std::any::type_name::<Self>(), uuid)
    }
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
    pub fn bind_actor<T>(&mut self, actor: T, _group: A::GroupBy) -> Address<T>
    where
        T: Actor + StartedBy<A> + InterruptedBy<A>,
        A: Eliminated<T>,
    {
        let address = spawn(actor, Some(self.address.clone()));
        self.lifetime_tracker.insert(address.clone());
        address
    }

    /// Starts and binds an `Actor`.
    pub fn bind_task<T>(&mut self, task: T, group: A::GroupBy) -> Address<Task<T>>
    where
        T: LiteTask,
        A: Eliminated<Task<T>>,
    {
        let actor = Task::new(task);
        self.bind_actor(actor, group)
    }

    /// Returns true if the shutdown process is in progress.
    pub fn is_terminating(&self) -> bool {
        self.lifetime_tracker.is_terminating()
    }

    // TODO: Change to `termination_sequence` where list of groups expected.
    /// Increases the priority of the `Actor`'s type.
    pub fn terminate_earlier<T>(&mut self) {
        self.lifetime_tracker.prioritize_termination::<T>();
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
    id: Id,
    actor: A,
    context: Context<A>,
    awake_envelope: Option<Envelope<A>>,
    done_notifier: Box<dyn LifecycleNotifier>,
    /// `Receiver` that have to be used to receive incoming messages.
    msg_rx: mpsc::Receiver<Envelope<A>>,
    /// High-priority receiver
    hp_msg_rx: mpsc::UnboundedReceiver<HpEnvelope<A>>,
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
        if let Err(err) = self.done_notifier.notify() {
            log::error!(
                "Can't send done notification from the actor {:?}: {}",
                self.id,
                err
            );
        }
        // TODO: Activate this check for tokio 0.3
        //if !self.join_tx.is_closed() {
        if let Err(_err) = self.join_tx.broadcast(Status::Stop) {
            // TODO: Activate this log for tokio 0.3
            //log::error!("Can't release joiners of {:?}", self.id);
        }
        //}
    }

    async fn routine(&mut self) {
        while self.context.alive {
            select_biased! {
                hp_envelope = self.hp_msg_rx.next() => {
                    if let Some(hp_env) = hp_envelope {
                        let mut envelope = hp_env.envelope;
                        match hp_env.operation {
                            Operation::Forward => {
                            }
                            Operation::Done { id } => {
                                self.context.lifetime_tracker.remove(&id);
                                if self.context.lifetime_tracker.is_finished() {

                                    self.context.stop();
                                }
                            }
                        }
                        let handle_res = envelope.handle(&mut self.actor, &mut self.context).await;
                        if let Err(err) = handle_res {
                            log::error!("Handler for {:?} (high-priority) failed: {}", self.id, err);
                        }
                    } else {
                        // Even if all `Address` dropped `Actor` can do something useful on
                        // background. Than don't terminate actors without `Addresses`, because
                        // it still has controllers.
                        // Background tasks = something spawned that `Actors` waits for finishing.
                        log::trace!("Messages stream of {:?} (high-priority) drained.", self.id);
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
                    }
                }
            }
        }
    }
}
