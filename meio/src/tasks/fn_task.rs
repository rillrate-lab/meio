//! Support functions as `LiteTask`s.

use crate::actor_runtime::{Actor, Context};
use crate::handlers::TaskEliminated;
use crate::ids::{Id, IdOf};
use crate::lite_runtime::{LiteTask, Tag, TaskError};
use anyhow::Error;
use async_trait::async_trait;
use futures::Future;

/// Functional `LiteTask`.
pub struct FnTask<T>(pub T);

/// Allows to use async closures as lite tasks.
#[async_trait]
impl<T, O> LiteTask for FnTask<T>
where
    T: Future<Output = Result<O, Error>> + Send + 'static,
    O: Send,
{
    type Output = O;

    fn log_target(&self) -> &str {
        "FnTask"
    }

    async fn interruptable_routine(mut self) -> Result<Self::Output, Error> {
        self.0.await
    }
}

/// Called when functional `LiteTask` terminated.
#[async_trait]
pub trait FnTaskEliminated<O, M: Tag>: Actor {
    /// Called when the `Task` finished.
    async fn handle(
        &mut self,
        id: Id,
        tag: M,
        result: Result<O, TaskError>,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error>;
}

#[async_trait]
impl<T, F, M> TaskEliminated<FnTask<F>, M> for T
where
    FnTask<F>: LiteTask,
    T: FnTaskEliminated<<FnTask<F> as LiteTask>::Output, M>,
    M: Tag,
{
    async fn handle(
        &mut self,
        id: IdOf<FnTask<F>>,
        tag: M,
        result: Result<<FnTask<F> as LiteTask>::Output, TaskError>,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        FnTaskEliminated::handle(self, id.into(), tag, result, ctx).await
    }
}
