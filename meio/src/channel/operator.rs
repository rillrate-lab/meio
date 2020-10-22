//! `Operator` helps runtime to perform controlling operations.

use crate::{Controller, Id, Status};
use derive_more::{Deref, DerefMut};
use futures::channel::mpsc;
use tokio::sync::watch;

/// `Envelope` for messages sent to an `Actor`.
#[derive(Debug)]
pub(crate) enum Signal {
    Shutdown,
    Finished { child: Id },
}

impl Into<Option<Id>> for Signal {
    fn into(self) -> Option<Id> {
        match self {
            Self::Shutdown => None,
            Self::Finished { child } => Some(child),
        }
    }
}

/// Special struct to process `Opeartion` messages properly.
#[derive(Deref, DerefMut)]
pub(crate) struct Operator {
    /// Used to send Stop signal if the external routine finished.
    id: Id,
    status_tx: watch::Sender<Status>,
    #[deref]
    #[deref_mut]
    op_rx: mpsc::UnboundedReceiver<Signal>,
    supervisor: Option<Controller>,
}

impl Operator {
    pub(super) fn new(
        id: Id,
        status_tx: watch::Sender<Status>,
        op_rx: mpsc::UnboundedReceiver<Signal>,
        supervisor: Option<Controller>,
    ) -> Self {
        Self {
            id,
            status_tx,
            op_rx,
            supervisor,
        }
    }

    pub fn initialize(&mut self) {}

    pub fn finalize(&mut self) {
        if let Some(mut supervisor) = self.supervisor.take() {
            let child = self.id.clone();
            let operation = Signal::Finished { child };
            supervisor.send_op(operation);
        }
        // TODO: tokio 0.3
        // self.status_tx.send(Status::Done).ok();
        self.status_tx.broadcast(Status::Done).ok();
    }
}
