//! `Controller` in importat part of the all interaction with
//! actors and tasks. It's also a part of the `Address`.

use crate::{Id, Signal};
use futures::channel::mpsc;
use std::fmt;
use std::hash::{Hash, Hasher};
use tokio::sync::watch;

/// `Status` of the `Actor`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// `Actor` spawned and working
    Alive,
    /// `Actor` finished
    Done,
}

/// A special `Address` for `Actors` termination.
#[derive(Clone)]
pub struct Controller {
    id: Id,
    status_rx: watch::Receiver<Status>,
    op_tx: mpsc::UnboundedSender<Signal>,
}

impl fmt::Debug for Controller {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Controller").field(&self.id).finish()
    }
}

impl PartialEq for Controller {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

impl Eq for Controller {}

impl Hash for Controller {
    fn hash<H: Hasher>(&self, state: &mut H)
    where
        Self: Sized,
    {
        self.id.hash(state);
    }
}

impl Controller {
    pub(super) fn new(
        id: Id,
        status_rx: watch::Receiver<Status>,
        op_tx: mpsc::UnboundedSender<Signal>,
    ) -> Self {
        Self {
            id,
            status_rx,
            op_tx,
        }
    }

    /// Returns `Id` of the controller.
    pub fn id(&self) -> Id {
        self.id.clone()
    }

    /// Start shutdown process for the `Actor`.
    #[deprecated(since = "0.25.0", note = "Use `Address::interrupt` call instead.")]
    pub fn shutdown(&mut self) {
        let operation = Signal::Shutdown;
        self.send_op(operation);
    }

    /// Waits when the `Actor` will be terminated.
    pub async fn join(&mut self) {
        // TODO: tokio 0.3
        // while self.status_rx.changed().await.is_ok() {
        while self.status_rx.recv().await.is_some() {
            if *self.status_rx.borrow() == Status::Done {
                break;
            }
        }
    }

    pub(crate) fn send_op(&mut self, operation: Signal) {
        let res = self.op_tx.unbounded_send(operation);
        if let Err(err) = res {
            log::error!("Can't send {:?} to {:?}", err.into_inner(), self.id);
        }
    }
}
