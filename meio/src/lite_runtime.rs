use crate::handlers::{StartedBy, InterruptedBy};
use crate::{lifecycle, Actor, Action, ActionHandler, Address, Context};
use anyhow::Error;
use async_trait::async_trait;
use derive_more::{From, Into};
use tokio::sync::watch;
use uuid::Uuid;

/// Contains a receiver with a status of a task.
#[derive(Debug, From, Into)]
pub struct ShutdownReceiver {
    /// Use `Into` to extract that `Receiver`.
    status: watch::Receiver<lifecycle::Status>,
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

struct TaskFinished;

impl Action for TaskFinished {}

struct LiteRuntime<T: LiteTask> {
    task: T,
    shutdown_rx: watch::Receiver<lifecycle::Status>,
}

impl<T: LiteTask> LiteRuntime<T> {
    async fn entrypoint(self, mut actor: Address<Task<T>>) {
        let name = self.task.name();
        log::info!("Starting the task: {:?}", name);
        let shutdown_receiver = ShutdownReceiver {
            status: self.shutdown_rx,
        };
        let res = self.task.routine(shutdown_receiver).await;
        if let Err(err) = res {
            log::error!("LiteTask {} failed with: {}", name, err);
        }
        log::info!("Finishing the task: {:?}", name);
        actor.act(TaskFinished).await;
    }
}

pub struct Task<T: LiteTask> {
    name: String,
    runtime: Option<LiteRuntime<T>>,
    shutdown_tx: watch::Sender<lifecycle::Status>,
}

impl<T: LiteTask> Task<T> {
    pub(crate) fn new(task: T) -> Self {
        let name = task.name();
        let (shutdown_tx, shutdown_rx) = watch::channel(lifecycle::Status::Alive);
        let runtime = LiteRuntime {
            task,
            shutdown_rx,
        };
        Self {
            name,
            runtime: Some(runtime),
            shutdown_tx,
        }
    }
}

#[async_trait]
impl<T, S> StartedBy<S> for Task<T>
where
    T: LiteTask,
    S: Actor,
{
    async fn handle(
        &mut self,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        if let Some(runtime) = self.runtime.take() {
            let address = ctx.address().clone();
            tokio::spawn(runtime.entrypoint(address));
        } else {
            log::error!("Task runtime received the second Awake event!");
        }
        Ok(())
    }
}

#[async_trait]
impl<T, S> InterruptedBy<S> for Task<T>
where
    T: LiteTask,
    S: Actor,
{
    async fn handle(&mut self, _ctx: &mut Context<Self>) -> Result<(), Error> {
        self.shutdown_tx.broadcast(lifecycle::Status::Stop)?;
        Ok(())
    }
}

#[async_trait]
impl<T> ActionHandler<TaskFinished> for Task<T>
where
    T: LiteTask,
{
    async fn handle(
        &mut self,
        _event: TaskFinished,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        ctx.shutdown();
        Ok(())
    }
}


impl<T: LiteTask> Actor for Task<T> {
    fn name(&self) -> String {
        self.name.clone()
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
