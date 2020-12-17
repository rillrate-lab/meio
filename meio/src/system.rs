//! This module contains `System` actor.

use crate::actor_runtime::{Actor, Context};
use crate::handlers::{Eliminated, InterruptedBy, StartedBy};
use crate::ids::IdOf;
use crate::linkage::Address;
use crate::signal;
use anyhow::Error;
use async_trait::async_trait;
use futures::{select, FutureExt, StreamExt};

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

impl System {
    /// Spawns a standalone `Actor` that has no `Supervisor`.
    pub fn spawn<A>(actor: A) -> Address<A>
    where
        A: Actor + StartedBy<Self>,
    {
        crate::actor_runtime::spawn(actor, Option::<Address<Self>>::None)
    }

    /// Spawns an `Actor` and wait for its termination (normally or by `SIGINT` interruption).
    pub async fn spawn_and_wait<A>(actor: A)
    where
        A: Actor + StartedBy<Self> + InterruptedBy<Self>,
    {
        let mut address = System::spawn(actor);
        let result = System::wait_or_interrupt(&mut address).await;
        if let Err(err) = result {
            log::error!("Can't wait for the actor: {}", err);
        }
    }

    /// Waits either `Actor` interrupted or terminated.
    /// If user sends `SIGINT` signal than the `Actor` will receive `InterruptedBy<System>` event,
    /// but for the second signal the function just returned to let the app terminate without waiting
    /// for any active task.
    pub async fn wait_or_interrupt<A>(address: &mut Address<A>) -> Result<(), Error>
    where
        A: Actor + InterruptedBy<Self>,
    {
        let mut signals = signal::CtrlC::stream().fuse();
        let mut join_addr = address.clone();
        let mut joiner = join_addr.join().boxed().fuse();
        let mut first_attempt = true;
        loop {
            select! {
                _interrupt = signals.select_next_some() => {
                    if first_attempt {
                        first_attempt = false;
                        address.interrupt()?;
                    } else {
                        break;
                    }
                }
                _done = joiner => {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Interrupts an `Actor`.
    pub fn interrupt<A>(address: &mut Address<A>) -> Result<(), Error>
    where
        A: Actor + InterruptedBy<Self>,
    {
        address.interrupt()
    }
}
