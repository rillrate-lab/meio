//! Notifications for actors.

use crate::{Action, ActionPerformer, ActionRecipient};
use anyhow::Error;

/// `Notifier` is a universal `Address` that only sends
/// the specific message.
pub struct Notifier<M> {
    performer: ActionRecipient<M>,
    message: M,
}

impl<M: Action + Clone> Notifier<M> {
    pub(crate) fn new(performer: ActionRecipient<M>, message: M) -> Self {
        Self { performer, message }
    }

    /// Sends the notification to the `Actor`.
    pub async fn notify(&mut self) -> Result<(), Error> {
        let message = self.message.clone();
        self.performer.act(message).await
    }
}
