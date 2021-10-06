//! Use this module to see how to extend
//! the `meio` with completely own types
//! of messages, but reusing existing
//! queues.

use anyhow::Error;
use async_trait::async_trait;
use meio::handlers::{Handler, Priority};
use meio::{Actor, Context};

// TODO: Add `custom_act` method the the `Address`.

pub trait CustomAction: Send + 'static {}

#[async_trait]
pub trait CustomActionHandler<I: CustomAction>: Actor {
    async fn handle(&mut self, input: I, _ctx: &mut Context<Self>) -> Result<(), Error>;
}

struct CustomActionHandlerImpl<I> {
    input: Option<I>,
}

#[async_trait]
impl<A, I> Handler<A> for CustomActionHandlerImpl<I>
where
    A: CustomActionHandler<I>,
    I: CustomAction,
{
    fn priority(&self) -> Priority {
        Priority::Normal
    }

    async fn handle(&mut self, actor: &mut A, ctx: &mut Context<A>) -> Result<(), Error> {
        let input = self.input.take().expect("action handler called twice");
        actor.handle(input, ctx).await
    }
}

#[async_trait]
trait ExtendedHandler: Actor {
    async fn handle(&mut self, ctx: &mut Context<Self>) -> Result<(), Error>;
}

struct ExtendedAction {}

impl CustomAction for ExtendedAction {}

#[async_trait]
impl<T: Actor> CustomActionHandler<ExtendedAction> for T
where
    T: ExtendedHandler,
{
    async fn handle(
        &mut self,
        _input: ExtendedAction,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        ExtendedHandler::handle(self, ctx).await
    }
}
