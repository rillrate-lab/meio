//! Contains the custom type of action for actors.

use anyhow::Error;
use async_trait::async_trait;
use meio::handlers::{Handler, Priority};
use meio::prelude::{Actor, Context};

// TODO: Add `custom_act` method the the `Address`.

/// The fully own type of messages.
pub trait CustomAction: Send + 'static {}

/// The trait for actors for handling incoming `CustomAction` instances.
#[async_trait]
pub trait CustomActionHandler<I: CustomAction>: Actor {
    /// Handles the incoming message.
    async fn handle(&mut self, input: I, _ctx: &mut Context<Self>) -> Result<(), Error>;
}

/// The envelope for handler's message.
///
/// It wraps a `CustomAction` and implements a `Handler` to
/// `take` the value from the inner `Option` and call the
/// specific handler trait.
struct CustomActionHandlerImpl<I> {
    input: Option<I>,
}

/// Implementation of a `Handler` for `CustomActionHandlerImpl` wrapper.
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
