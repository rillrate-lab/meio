//! Module contains stream to connect signals to actors.

use crate::handlers::Action;
use futures::{stream::BoxStream, FutureExt, StreamExt};
use tokio::signal;

/// `Ctrl-C` signal handler.
pub struct CtrlC;

impl Action for CtrlC {}

impl CtrlC {
    /// Creates an attachable stream of `Ctrl-C` for an `Actor`.
    pub fn stream() -> BoxStream<'static, CtrlC> {
        signal::ctrl_c()
            .into_stream()
            .map(|res| {
                if let Err(err) = res {
                    log::error!("Ctrl-C signal handler failed: {}", err);
                }
                CtrlC
            })
            .boxed()
    }
}
