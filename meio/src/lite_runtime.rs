//! This module contains `LiteTask` trait and the runtime to execute it.

use crate::{
    channel,
    lifecycle::{LifecycleNotifier, TaskDone},
    ActionHandler, Actor, Address, Controller, Id, Operator, Signal,
};
use anyhow::Error;
use async_trait::async_trait;
use derive_more::{From, Into};
use futures::{select_biased, FutureExt, StreamExt};
use tokio::sync::watch;
use uuid::Uuid;

/// Contains a receiver with a status of a task.
#[derive(Debug, From, Into)]
pub struct ShutdownReceiver {
    /// Use `Into` to extract that `Receiver`.
    status: watch::Receiver<LiteStatus>,
}

impl ShutdownReceiver {
    /// Converts the `Receiver` into a `Future` that just returns `()` when task finished.
    pub async fn just_done(mut self) {
        // TODO: tokio 0.3
        // while self.status.changed().await.is_ok() {
        while self.status.recv().await.is_some() {
            if self.status.borrow().is_done() {
                break;
            }
        }
    }
}

/// Spawns `Lite` task and return `Controller` to manage that.
pub(crate) fn task<L, S>(lite_task: L, supervisor: Address<S>) -> Controller
where
    L: LiteTask,
    S: Actor + ActionHandler<TaskDone<L>>,
{
    let id = Id::of_task(&lite_task);
    // TODO: Replace this `Controller`
    let (controller, operator) = channel::pair(id, None);
    let id = controller.id();
    let event = TaskDone::new(id.clone());
    let done_notifier = LifecycleNotifier::once(supervisor.controller(), event);
    let runtime = LiteRuntime {
        id,
        lite_task: Some(lite_task),
        operator,
        done_notifier,
    };
    tokio::spawn(runtime.entrypoint());
    controller
}

/// Status of the task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LiteStatus {
    /// Task is alive and working.
    Alive,
    /// Task had finished.
    Stop,
}

impl LiteStatus {
    /// Is task finished yet?
    pub fn is_done(&self) -> bool {
        *self == LiteStatus::Stop
    }
}

/// Minimalistic actor that hasn't `Address`.
///
/// **Recommended** to implement sequences or intensive loops (routines).
#[async_trait]
pub trait LiteTask: Sized + Send + 'static {
    /// Returns unique name of the `LiteTask`.
    /// Uses `Uuid` by default.
    fn name(&self) -> String {
        let uuid = Uuid::new_v4();
        format!("Task:{}({})", std::any::type_name::<Self>(), uuid)
    }

    /// Routine of the task that can contain loops.
    /// It can taks into accout provided receiver to implement graceful interruption.
    async fn routine(self, signal: ShutdownReceiver) -> Result<(), Error>;
}

struct LiteRuntime<L: LiteTask> {
    id: Id,
    lite_task: Option<L>,
    operator: Operator, // aka Context here
    done_notifier: Box<dyn LifecycleNotifier>,
}

impl<L: LiteTask> LiteRuntime<L> {
    async fn entrypoint(mut self) {
        self.operator.initialize();
        log::info!("LiteTask started: {:?}", self.id);
        self.routine().await;
        log::info!("LiteTask finished: {:?}", self.id);
        // TODO: Dry errors printing for notifiers for tasks and actors
        self.done_notifier.notify();
        self.operator.finalize();
    }

    async fn routine(&mut self) {
        let (shutdown_tx, shutdown_rx) = watch::channel(LiteStatus::Alive);
        let lite_task = self.lite_task.take().unwrap();
        let mut routine = lite_task.routine(shutdown_rx.into()).fuse();
        let mut shutdown = Some(shutdown_tx);
        loop {
            select_biased! {
                /*
                event = self.operator.next() => {
                    log::trace!("Stop signal received: {:?} for task {:?}", event, self.id);
                    if let Some(signal) = event {
                        match signal {
                            Signal::Shutdown => {
                                // Ok
                            }
                            Signal::Finished { .. } => {
                                // Because tasks don't terminate them:
                                panic!("Tasks don't support supervised childs.");
                            }
                        }
                    } else {
                        log::error!("task controller couldn't be closed");
                    }
                    // TODO: Check that origin is none!
                    if let Some(tx) = shutdown.take() {
                        // TODO: tokio 0.3
                        // if tx.send(LiteStatus::Stop).is_err() {
                        if tx.broadcast(LiteStatus::Stop).is_err() {
                            log::error!("Can't send shutdown signal to: {:?}", self.id);
                        }
                        // Wait for `done` instead of `stop_signal` used for actors
                    }
                }
                */
                done = routine => {
                    if let Err(err) = done {
                        log::error!("LiteTask {:?} failed: {}", self.id, err);
                    }
                    break;
                }
            }
        }
    }
}
