//! This module contains `System` actor.

use crate::ids::IdOf;
use crate::{Actor, Context, Eliminated};
use anyhow::Error;
use async_trait::async_trait;

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
