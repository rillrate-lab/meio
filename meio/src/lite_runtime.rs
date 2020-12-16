use crate::actor_runtime::{Actor, Context, Status};
use crate::handlers::{Action, ActionHandler, InterruptedBy, StartedBy};
use crate::linkage::Address;
use anyhow::Error;
use async_trait::async_trait;
use derive_more::Into;
use futures::{
    future::{select, Either, FusedFuture},
    Future, FutureExt,
};
use std::pin::Pin;
use tokio::sync::watch;
use uuid::Uuid;

pub trait DoneSignal: Future<Output = ()> + FusedFuture + Send {}

impl<T> DoneSignal for T where T: Future<Output = ()> + FusedFuture + Send {}

/// Contains a receiver with a status of a task.
#[derive(Debug, Into, Clone)]
pub struct ShutdownReceiver {
    status: watch::Receiver<Status>,
}

impl ShutdownReceiver {
    fn new(status: watch::Receiver<Status>) -> Self {
        Self { status }
    }

    /// Returns a `Future` that completed when `Done` signal received.
    pub fn just_done(self) -> Pin<Box<dyn DoneSignal>> {
        Box::pin(just_done(self.status).fuse())
    }

    /// Tries to execute provided `Future` to completion if the `ShutdownReceived`
    /// won't interrupted during that time.
    pub async fn or<Fut>(&mut self, fut: Fut) -> Result<Fut::Output, Error>
    where
        Fut: Future,
    {
        tokio::pin!(fut);
        let either = select(self.clone().just_done(), fut).await;
        match either {
            Either::Left((_done, _rem_fut)) => {
                Err(Error::msg("task received an interruption signal"))
            }
            Either::Right((output, _rem_fut)) => Ok(output),
        }
    }
}

async fn just_done(mut status: watch::Receiver<Status>) {
    // TODO: tokio 0.3
    // while status.changed().await.is_ok() {
    while status.recv().await.is_some() {
        if status.borrow().is_done() {
            break;
        }
    }
}

struct LiteTaskFinished;

impl Action for LiteTaskFinished {}

struct LiteRuntime<T: LiteTask> {
    task: T,
    shutdown_rx: watch::Receiver<Status>,
}

impl<T: LiteTask> LiteRuntime<T> {
    async fn entrypoint(self, mut actor: Address<Task<T>>) {
        let name = self.task.name();
        log::info!("Starting the task: {:?}", name);
        let shutdown_receiver = ShutdownReceiver::new(self.shutdown_rx);
        let res = self.task.routine(shutdown_receiver).await;
        if let Err(err) = res {
            log::error!("LiteTask {} failed with: {}", name, err);
        }
        log::info!("Finishing the task: {:?}", name);
        if let Err(err) = actor.act(LiteTaskFinished).await {
            log::error!(
                "Can't notify a Task about LiteTask routine termination: {}",
                err
            );
        }
    }
}

/// The `Actor` that spawns and controls a `LiteTask`.
pub struct Task<T: LiteTask> {
    name: String,
    runtime: Option<LiteRuntime<T>>,
    shutdown_tx: watch::Sender<Status>,
}

impl<T: LiteTask> Task<T> {
    pub(crate) fn new(task: T) -> Self {
        let name = task.name();
        let (shutdown_tx, shutdown_rx) = watch::channel(Status::Alive);
        let runtime = LiteRuntime { task, shutdown_rx };
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
    async fn handle(&mut self, ctx: &mut Context<Self>) -> Result<(), Error> {
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
        self.shutdown_tx.broadcast(Status::Stop)?;
        Ok(())
    }
}

#[async_trait]
impl<T> ActionHandler<LiteTaskFinished> for Task<T>
where
    T: LiteTask,
{
    async fn handle(
        &mut self,
        _event: LiteTaskFinished,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        ctx.shutdown();
        Ok(())
    }
}

impl<T: LiteTask> Actor for Task<T> {
    type GroupBy = ();

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
