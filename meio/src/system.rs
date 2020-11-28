//! This module contains `System` actor.

use crate::{actor_runtime, Actor, Address, Context, Eliminated, StartedBy, TypedId};
use anyhow::Error;
use async_trait::async_trait;

/// Virtual actor that represents the system/environment.
pub enum System {}

impl Actor for System {}

#[async_trait]
impl<T: Actor> Eliminated<T> for System {
    async fn handle(&mut self, _id: TypedId<T>, _ctx: &mut Context<Self>) -> Result<(), Error> {
        // TODO: Maybe change this in the future...
        unreachable!("The system has no Address and no one actor actually binded to it.")
    }
}

impl System {
    /// Spawns a standalone `Actor` that has no `Supervisor`.
    pub fn spawn<A>(actor: A) -> Address<A>
    where
        A: Actor + StartedBy<System>,
    {
        actor_runtime::spawn(actor, Option::<Address<System>>::None)
    }
}
