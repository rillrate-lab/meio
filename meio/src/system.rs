//! This module contains `System` actor.

use crate::actor_runtime::{Actor, Context};
use crate::handlers::{Eliminated, InterruptedBy, StartedBy};
use crate::ids::IdOf;
use crate::linkage::{Address, AddressPair};
#[cfg(not(feature = "wasm"))]
use crate::signal;
use anyhow::Error;
use async_trait::async_trait;
#[cfg(not(feature = "wasm"))]
use futures::{select, FutureExt, StreamExt};

/// Virtual actor that represents the system/environment.
pub enum System {}

impl Actor for System {
    type GroupBy = ();

    fn log_target(&self) -> &str {
        "System"
    }
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
        let pair = AddressPair::new();
        let address = pair.address().clone();
        crate::actor_runtime::spawn(actor, Option::<Address<Self>>::None, pair);
        address
    }

    /// Spawns an `Actor` and wait for its termination (normally or by `SIGINT` interruption).
    #[cfg(not(feature = "wasm"))]
    pub async fn spawn_and_wait<A>(actor: A)
    where
        A: Actor + StartedBy<Self> + InterruptedBy<Self>,
    {
        let address = System::spawn(actor);
        let result = System::wait_or_interrupt(address).await;
        if let Err(err) = result {
            log::error!("Can't wait for the actor: {}", err);
        }
    }

    /// Waits either `Actor` interrupted or terminated.
    /// If user sends `SIGINT` signal than the `Actor` will receive `InterruptedBy<System>` event,
    /// but for the second signal the function just returned to let the app terminate without waiting
    /// for any active task.
    #[cfg(not(feature = "wasm"))]
    pub async fn wait_or_interrupt<A>(address: Address<A>) -> Result<(), Error>
    where
        A: Actor + InterruptedBy<Self>,
    {
        let mut signals = signal::CtrlC::stream().fuse();
        let join_addr = address.clone();
        let mut joiner = join_addr.join().boxed().fuse();
        let mut first_attempt = true;
        loop {
            select! {
                _interrupt = signals.select_next_some() => {
                    log::trace!("Ctrl-C received");
                    if first_attempt {
                        first_attempt = false;
                        address.interrupt_by()?;
                    } else {
                        break;
                    }
                }
                _done = joiner => {
                    log::trace!("Actor spawned by System done: {:?}", address);
                    break;
                }
            }
        }
        Ok(())
    }

    /// Interrupts an `Actor`.
    pub fn interrupt<A>(address: &Address<A>) -> Result<(), Error>
    where
        A: Actor + InterruptedBy<Self>,
    {
        address.interrupt_by()
    }
}
