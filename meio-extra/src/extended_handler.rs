//! The extensions for the `CustomAction`.
//!
//! Like `Consumer` implements `ActionHandler` trait
//! in the standard `meio` handlers.

use super::custom_action::*;
use anyhow::Error;
use async_trait::async_trait;
use meio::prelude::{Actor, Context};

/// The handler for special types of actions.
#[async_trait]
trait ExtendedHandler: Actor {
    /// Handle the action in this method.
    async fn handle(&mut self, value: u8, ctx: &mut Context<Self>) -> Result<(), Error>;
    /// Other method for special cases.
    async fn done(&mut self, ctx: &mut Context<Self>) -> Result<(), Error>;
}

/// The example of customized action with own inner value.
struct ExtendedAction {
    inner_value: Option<u8>,
}

/// Implement `CustomAction` for it.
///
/// Implementing `Action` is meaningless, because it's impossible
/// to implement `ActionHandler` in the third-party crate (here).
impl CustomAction for ExtendedAction {}

#[async_trait]
impl<T: Actor> CustomActionHandler<ExtendedAction> for T
where
    T: ExtendedHandler,
{
    async fn handle(
        &mut self,
        input: ExtendedAction,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        if let Some(value) = input.inner_value {
            // Call the `ExtendedHandler` with the provided value
            ExtendedHandler::handle(self, value, ctx).await
        } else {
            // No more messages expected, or empty value, or other signal.
            ExtendedHandler::done(self, ctx).await
        }
    }
}
