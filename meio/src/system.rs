//! This module contains `System` actor.

use crate::actor_runtime::{Actor, Context};
use crate::handlers::{Eliminated, StartedBy};
use crate::ids::IdOf;
use crate::linkage::Address;
use anyhow::Error;
use async_trait::async_trait;

/// Spawns a standalone `Actor` that has no `Supervisor`.
pub fn spawn<A>(actor: A) -> Address<A>
where
    A: Actor + StartedBy<System>,
{
    crate::actor_runtime::spawn(actor, Option::<Address<System>>::None)
}

/// Virtual actor that represents the system/environment.
pub enum System {}

impl Actor for System {
    type GroupBy = ();
}

#[async_trait]
impl<T: Actor> Eliminated<T> for System {
    async fn handle(&mut self, _id: IdOf<T>, _ctx: &mut Context<Self>) -> Result<(), Error> {
        // TODO: Maybe change this in the future...
        unreachable!("The system has no Address and no one actor actually binded to it.")
    }
}
