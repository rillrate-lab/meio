use crate::handlers::Action;
use crate::ids::Id;
use crate::linkage::recipient::ActionRecipient;
use anyhow::Error;
use std::collections::HashMap;

/// The set of multiple recipients that sends actions in parallel.
#[derive(Debug, Default)]
pub struct MultiRecipient<T> {
    recipients: HashMap<Id, Box<dyn ActionRecipient<T>>>,
}

impl<T> MultiRecipient<T>
where
    T: Action + Clone,
{
    /// Adds a `Recipient`.
    pub fn insert(&mut self, recipient: Box<dyn ActionRecipient<T>>) {
        let id = recipient.id_ref().to_owned();
        self.recipients.insert(id, recipient);
    }

    /// Remove the recipient by `Id`.
    pub fn remove(&mut self, id: &Id) -> Option<Box<dyn ActionRecipient<T>>> {
        self.recipients.remove(id)
    }

    /// Sends action to all in parallel.
    pub async fn act_all(&mut self, action: T) -> Result<(), Error> {
        let futs = self
            .recipients
            .values_mut()
            .map(|recipient| recipient.act(action.clone()));
        let err = futures::future::join_all(futs)
            .await
            .into_iter()
            .find(Result::is_err);
        if let Some(Err(err)) = err {
            Err(err)
        } else {
            Ok(())
        }
    }
}
