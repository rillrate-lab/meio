//! This module contains an internal channel used by runtimes to
//! send signal about finished supervised activities of shutdown signals.

pub mod controller;
pub mod operator;

pub use controller::{Controller, Status};
pub(crate) use operator::{Operator, Signal};

use crate::Id;
use futures::channel::mpsc;
use tokio::sync::watch;

// TODO: Make it private when `Context::standalone` will appear,
// because sub-actors already spawned by methods of the `Context`.
/// Alias for using `Supervisor::None` for `spawn` calling.
pub type Supervisor = Option<Controller>;

/// Creates a pair of `Controller` and `Operator`.
pub(crate) fn pair(id: Id, supervisor: Option<Controller>) -> (Controller, Operator) {
    let (op_tx, op_rx) = mpsc::unbounded();
    let (status_tx, status_rx) = watch::channel(Status::Alive);
    let controller = Controller::new(id.clone(), status_rx, op_tx);
    let operator = Operator::new(id, status_tx, op_rx, supervisor);
    (controller, operator)
}
