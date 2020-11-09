//! This module contains recipients that works like addresses with
//! a single incoming message type.

use crate::{
    Action, ActionHandler, ActionPerformer, Actor, Address, Interaction, InteractionHandler,
    InteractionPerformer,
};
use anyhow::Error;
use async_trait::async_trait;
use std::fmt;
use std::sync::Arc;

trait ActionRecipientGenerator<I>: Send + Sync {
    fn generate(&self) -> Box<dyn ActionPerformer<I>>;
}

impl<T, I> ActionRecipientGenerator<I> for T
where
    T: Send + Sync,
    T: Fn() -> Box<dyn ActionPerformer<I>>,
{
    fn generate(&self) -> Box<dyn ActionPerformer<I>> {
        (self)()
    }
}

/// Actor-agnosic `ActionPerformer`.
pub struct ActionRecipient<I> {
    generator: Arc<dyn ActionRecipientGenerator<I>>,
    performer: Box<dyn ActionPerformer<I>>,
}

impl<I> Clone for ActionRecipient<I> {
    fn clone(&self) -> Self {
        let performer = self.generator.generate();
        Self {
            generator: self.generator.clone(),
            performer,
        }
    }
}

impl<I> fmt::Debug for ActionRecipient<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let type_name = std::any::type_name::<I>();
        f.debug_tuple("ActionRecipient").field(&type_name).finish()
    }
}

#[async_trait]
impl<I> ActionPerformer<I> for ActionRecipient<I>
where
    I: Action,
{
    async fn act(&mut self, input: I) -> Result<(), Error> {
        self.performer.act(input).await
    }
}

impl<A, I> From<Address<A>> for ActionRecipient<I>
where
    A: Actor,
    A: ActionHandler<I>,
    I: Action,
{
    fn from(address: Address<A>) -> Self {
        let generator = move || -> Box<dyn ActionPerformer<I>> { Box::new(address.clone()) };
        let performer = generator.generate();
        Self {
            generator: Arc::new(generator),
            performer,
        }
    }
}

trait InteractionRecipientGenerator<I>: Send + Sync {
    fn generate(&self) -> Box<dyn InteractionPerformer<I>>;
}

impl<T, I> InteractionRecipientGenerator<I> for T
where
    T: Send + Sync,
    T: Fn() -> Box<dyn InteractionPerformer<I>>,
{
    fn generate(&self) -> Box<dyn InteractionPerformer<I>> {
        (self)()
    }
}

/// Actor-agnosic `InteractionPerformer`.
pub struct InteractionRecipient<I> {
    generator: Arc<dyn InteractionRecipientGenerator<I>>,
    performer: Box<dyn InteractionPerformer<I>>,
}

impl<I> Clone for InteractionRecipient<I> {
    fn clone(&self) -> Self {
        let performer = self.generator.generate();
        Self {
            generator: self.generator.clone(),
            performer,
        }
    }
}

impl<I> fmt::Debug for InteractionRecipient<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let type_name = std::any::type_name::<I>();
        f.debug_tuple("InteractionRecipient")
            .field(&type_name)
            .finish()
    }
}

#[async_trait]
impl<I> InteractionPerformer<I> for InteractionRecipient<I>
where
    I: Interaction,
{
    async fn interact(&mut self, input: I) -> Result<I::Output, Error> {
        self.performer.interact(input).await
    }
}

impl<A, I> From<Address<A>> for InteractionRecipient<I>
where
    A: Actor,
    A: InteractionHandler<I>,
    I: Interaction,
{
    fn from(address: Address<A>) -> Self {
        let generator = move || -> Box<dyn InteractionPerformer<I>> { Box::new(address.clone()) };
        let performer = generator.generate();
        Self {
            generator: Arc::new(generator),
            performer,
        }
    }
}
