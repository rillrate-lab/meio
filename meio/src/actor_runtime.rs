//! This module contains `Actor` trait and the runtime to execute it.

// TODO: Fix imports
use crate::{
    lifecycle::{self, Awake, Done, Interrupt, LifecycleNotifier, LifetimeTracker},
    linkage::controller::{Controller, HpEnvelope, Operation},
    ActionHandler, Address, Envelope, Id, LiteTask,
    Task,
};
use anyhow::Error;
use async_trait::async_trait;
use futures::channel::mpsc;
use futures::{select_biased, StreamExt};
use uuid::Uuid;

const MESSAGES_CHANNEL_DEPTH: usize = 32;

// TODO: Move system somewhere
/// Virtual actor that represents the system/environment.
pub enum System {}

impl Actor for System {}

#[async_trait]
impl ActionHandler<lifecycle::Awake<Self>> for System {
    async fn handle(
        &mut self,
        _event: lifecycle::Awake<Self>,
        _ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        unreachable!()
    }
}

#[async_trait]
impl<T: Actor> ActionHandler<lifecycle::Done<T>> for System {
    async fn handle(
        &mut self,
        _event: lifecycle::Done<T>,
        _ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        unreachable!()
    }
}

/// Spawns a standalone `Actor` that has no `Supervisor`.
pub fn standalone<A>(actor: A) -> Address<A>
where
    A: Actor + ActionHandler<Awake<System>>,
{
    spawn(actor, Option::<Address<System>>::None)
}

/// Spawns `Actor` in `ActorRuntime`.
// TODO: No `Option`! Use `static Address<System>` instead.
// It can be possible when `Controller` and `Operator` will be removed.
fn spawn<A, S>(actor: A, supervisor: Option<Address<S>>) -> Address<A>
where
    A: Actor + ActionHandler<Awake<S>>,
    S: Actor + ActionHandler<Done<A>>,
{
    let id = Id::of_actor(&actor);
    let (hp_msg_tx, hp_msg_rx) = mpsc::unbounded();
    let controller = Controller::new(id, hp_msg_tx);
    let id = controller.id();
    let (msg_tx, msg_rx) = mpsc::channel(MESSAGES_CHANNEL_DEPTH);
    let address = Address::new(controller, msg_tx);
    let awake_envelope = Envelope::action(Awake::new());
    let done_notifier = {
        match supervisor {
            None => LifecycleNotifier::ignore(),
            Some(super_addr) => {
                let event = Done::new(address.id());
                let op = Operation::Done { id: id.clone() };
                LifecycleNotifier::once(super_addr.controller(), op, event)
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
    pub fn bind_actor<T>(&mut self, actor: T) -> Address<T>
    where
        T: Actor + ActionHandler<Awake<A>> + ActionHandler<Interrupt<A>>,
        A: ActionHandler<Done<T>>,
    {
        let address = spawn(actor, Some(self.address.clone()));
        self.lifetime_tracker.insert(address.controller());
        address
    }

    /// Starts and binds an `Actor`.
    pub fn bind_task<T>(&mut self, task: T) -> Address<Task<T>>
    where
        T: LiteTask,
        A: ActionHandler<Done<Task<T>>>,
    {
        let actor = Task::new(task);
        self.bind_actor(actor)
    }

    /// Stops the runtime of the `Actor` on one message will be processed after this call.
    ///
    /// It's recommended way to terminate `Actor` is the `shutdown` method.
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

// TODO: Maybe add `S: Supervisor` parameter to
// avoid using blind notifiers, etc?
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
}

impl<A: Actor> ActorRuntime<A> {
    /// The `entrypoint` of the `ActorRuntime` that calls `routine` method.
    async fn entrypoint(mut self) {
        //self.operator.initialize();
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
        // It's important to finalize `Operator` after `terminate` call,
        // because that can contain some activities for parent `Actor`.
        // Unregistering ids for example.
        if let Err(err) = self.done_notifier.notify() {
            log::error!(
                "Can't send done notification from the actor {:?}: {}",
                self.id,
                err
            );
        }
        //self.operator.finalize();
    }

    async fn routine(&mut self) {
        while self.context.alive {
            select_biased! {
                /*
                event = self.operator.next() => {
                    log::trace!("Stop signal received: {:?} for {:?}", event, self.id);
                    // Because `Operator` contained an instance of the `Controller`.
                    let signal = event.expect("actor controller couldn't be closed");
                    let child = signal.into();
                    let progress = self.context.terminator.track_child_or_stop_signal(child);
                    if progress == TerminationProgress::SafeToStop {
                        log::info!("Actor {:?} is completed.", self.id);
                        self.msg_rx.close();
                        break;
                    }
                }
                */
                hp_envelope = self.hp_msg_rx.next() => {
                    if let Some(mut hp_env) = hp_envelope {
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
