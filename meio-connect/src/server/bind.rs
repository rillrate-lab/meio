//! Helpers for binding a server to a socket address.
//!
//! Also contains interactions to get notifications about it.

use super::{HttpServer, HttpServerLink};
use anyhow::Error;
use async_trait::async_trait;
use derive_more::From;
use meio::prelude::{
    Action, ActionHandler, Context, Interact, Interaction, InteractionResponder, InteractionTask,
};
use std::net::SocketAddr;

/// The interaction event for waiting for the address of the server.
///
/// It's used to ensure the server has binded to an address and other
/// parts can use it to connect to it.
pub struct WaitForAddress;

impl Interaction for WaitForAddress {
    type Output = SocketAddr;
}

impl HttpServerLink {
    /// Creates a waiting task for server's address.
    pub fn wait_for_address(&self) -> InteractionTask<WaitForAddress> {
        self.address.interact(WaitForAddress)
    }
}

#[async_trait]
impl ActionHandler<Interact<WaitForAddress>> for HttpServer {
    async fn handle(
        &mut self,
        msg: Interact<WaitForAddress>,
        _ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        match &mut self.addr_state {
            AddrState::NotAssignedYet { listeners } => {
                listeners.push(msg.responder);
            }
            AddrState::Assigned { addr } => {
                if let Err(err) = msg.responder.send(Ok(*addr)) {
                    log::error!(target: &self.log_target, "Can't send address result {:?} to the listener.", err);
                }
            }
        }
        Ok(())
    }
}

pub(super) enum AddrState {
    NotAssignedYet {
        listeners: Vec<InteractionResponder<SocketAddr>>,
    },
    Assigned {
        addr: SocketAddr,
    },
}

impl Default for AddrState {
    fn default() -> Self {
        Self::NotAssignedYet {
            listeners: Vec::new(),
        }
    }
}

#[derive(From)]
pub(super) struct AddrReady {
    addr: SocketAddr,
}

impl Action for AddrReady {}

#[async_trait]
impl ActionHandler<AddrReady> for HttpServer {
    async fn handle(&mut self, msg: AddrReady, _ctx: &mut Context<Self>) -> Result<(), Error> {
        let addr = msg.addr;
        let mut new_state = AddrState::Assigned { addr };
        std::mem::swap(&mut self.addr_state, &mut new_state);
        if let AddrState::NotAssignedYet { listeners } = new_state {
            for listener in listeners {
                if let Err(err) = listener.send(Ok(addr)) {
                    log::error!(target: &self.log_target, "Can't send address result {:?} to the listener.", err);
                }
            }
        }
        Ok(())
    }
}
