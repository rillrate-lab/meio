//! This module contains `Actor` trait and the runtime to execute it.

use crate::{
    channel, Address, Controller, Envelope, Id, Operator, Supervisor, TerminationProgress,
    Terminator,
};
use anyhow::Error;
use async_trait::async_trait;
use futures::channel::mpsc;
use futures::{select_biased, StreamExt};
use uuid::Uuid;

const MESSAGES_CHANNEL_DEPTH: usize = 32;

/// Spawns `Actor` in `ActorRuntime`.
fn spawn<A: Actor>(actor: A, supervisor: Option<impl Into<Controller>>) -> Address<A> {
    let id = Id::of_actor(&actor);
    let supervisor = supervisor.map(Into::into);
    let (controller, operator) = channel::pair(id, supervisor);
    let id = controller.id();
    let (msg_tx, msg_rx) = mpsc::channel(MESSAGES_CHANNEL_DEPTH);
    let (hp_msg_tx, hp_msg_rx) = mpsc::unbounded();
    let address = Address::new(controller, msg_tx, hp_msg_tx);
    let context = Context {
        address: address.clone(),
        terminator: Terminator::new(id.clone()),
    };
    let runtime = ActorRuntime {
        id,
        actor,
        context,
        operator,
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

    /// Starts an actor
    fn start<T>(self, supervisor: Option<T>) -> Address<Self>
    where
        T: Into<Controller> + Send,
    {
        spawn(self, supervisor)
    }

    /// Called when actor initializing.
    /// You should spawn workers here if needed.
    ///
    /// Don't worry if this method failed. `terminate` will
    /// be called in any case and you can finalize all workers there.
    async fn initialize(&mut self, _ctx: &mut Context<Self>) -> Result<(), Error> {
        Ok(())
    }

    /// Called on every potential shutdown signal received.
    async fn stop_signal(
        &mut self,
        child: Option<Id>,
        ctx: &mut Context<Self>,
    ) -> TerminationProgress {
        ctx.terminator().track_child_or_stop_signal(child)
    }

    /// Called when actor teminating.
    /// This method doesn't have a result to don't allow you to
    /// use `?` operator that can interrupt termination and resources
    /// releasing process.
    /// You should also terminate workers here.
    ///
    /// This method will be called even if initialization failed.
    ///
    /// If you have optional fields filled during initialization,
    /// don't `unwrap` them, because they can be `None`.
    async fn terminate(&mut self) /* NO RESULT! IMPORTANT! */ {}
}

/// `Context` of a `ActorRuntime` that contains `Address` and `Receiver`.
pub struct Context<A: Actor> {
    address: Address<A>,
    terminator: Terminator,
}

impl<A: Actor> Context<A> {
    /// Returns an instance of the `Address`.
    pub fn address(&mut self) -> &mut Address<A> {
        &mut self.address
    }

    /// Returns a `Supervisor` link of the `Actor`.
    pub fn bind(&mut self) -> Supervisor {
        Some(self.address.controller())
    }

    /// Returns a reference to an `Address`.
    pub fn terminator(&mut self) -> &mut Terminator {
        &mut self.terminator
    }
}

/// `ActorRuntime` for `Actor`.
pub struct ActorRuntime<A: Actor> {
    id: Id,
    actor: A,
    context: Context<A>,
    operator: Operator,
    /// `Receiver` that have to be used to receive incoming messages.
    msg_rx: mpsc::Receiver<Envelope<A>>,
    /// High-priority receiver
    hp_msg_rx: mpsc::UnboundedReceiver<Envelope<A>>,
}

impl<A: Actor> ActorRuntime<A> {
    /// The `entrypoint` of the `ActorRuntime` that calls `routine` method.
    async fn entrypoint(mut self) {
        self.operator.initialize();
        log::info!("Starting actor: {:?}", self.id);
        if let Err(err) = self.actor.initialize(&mut self.context).await {
            log::error!("Initialization failed: {:?}", err);
        } else {
            self.routine().await;
        }
        // Terminate even if it hadn't initialized properly.
        self.actor.terminate().await;
        log::info!("Actor finished: {:?}", self.id);
        // It's important to finalize `Operator` after `terminate` call,
        // because that can contain some activities for parent `Actor`.
        // Unregistering ids for example.
        self.operator.finalize();
    }

    async fn routine(&mut self) {
        loop {
            select_biased! {
                event = self.operator.next() => {
                    log::trace!("Stop signal received: {:?} for {:?}", event, self.id);
                    // Because `Operator` contained an instance of the `Controller`.
                    let signal = event.expect("actor controller couldn't be closed");
                    let progress = self.actor.stop_signal(signal.into(), &mut self.context).await;
                    if progress == TerminationProgress::SafeToStop {
                        log::info!("Actor {:?} is completed.", self.id);
                        self.msg_rx.close();
                        break;
                    }
                }
                hp_envelope = self.hp_msg_rx.next() => {
                    if let Some(mut envelope) = hp_envelope {
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
