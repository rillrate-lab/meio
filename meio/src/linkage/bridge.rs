//! This module contains `Bridge` to interconnect two `Actor`s.

use crate::actor_runtime::Actor;
use crate::handlers::Consumer;
use crate::linkage::Address;
use anyhow::Error;
use tokio::sync::mpsc;

/// Creates a new channel to communicate `Actor`s.
pub fn channel<L, R>() -> (Bridge<L, R>, Bridge<R, L>) {
    let (left_tx, left_rx) = mpsc::unbounded_channel();
    let (right_tx, right_rx) = mpsc::unbounded_channel();
    let left_bridge = Bridge {
        rx: Some(left_rx),
        tx: right_tx,
    };
    let right_bridge = Bridge {
        rx: Some(right_rx),
        tx: left_tx,
    };
    (left_bridge, right_bridge)
}

/// The `Bridge` that contains a sender to another side actor and a receiver
/// that can be binded to an `Actor`.
pub struct Bridge<I, O> {
    // TODO: Store `Actor`'s id?
    rx: Option<mpsc::UnboundedReceiver<I>>,
    tx: mpsc::UnboundedSender<O>,
}

impl<I, O> Bridge<I, O>
where
    I: Send + 'static,
    O: Send + 'static,
{
    /// Bind the bridge to this `Actor`.
    pub fn bind<A>(&mut self, address: &mut Address<A>) -> Result<(), Error>
    where
        A: Actor + Consumer<I>,
    {
        if let Some(rx) = self.rx.take() {
            address.attach(rx);
            Ok(())
        } else {
            Err(Error::msg("The bridge already binded."))
        }
    }

    /// Send a message to the another side of the connection.
    pub fn send(&mut self, msg: O) -> Result<(), Error> {
        self.tx
            .send(msg)
            .map_err(|_| Error::msg("Can't send a message with the bridge."))?;
        Ok(())
    }
}
