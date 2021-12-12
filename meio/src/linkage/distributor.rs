use crate::handlers::Action;
use crate::ids::Id;
use crate::linkage::recipient::ActionRecipient;
use anyhow::Error;
use std::collections::HashMap;

/// The set of multiple recipients that sends actions in parallel.
#[derive(Debug)]
pub struct Distributor<T> {
    recipients: HashMap<Id, Box<dyn ActionRecipient<T>>>,
}

impl<T> Default for Distributor<T> {
    fn default() -> Self {
        Self {
            recipients: HashMap::new(),
        }
    }
}

impl<T> Distributor<T> {
    /// Creates a new set of recipients.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<T> Distributor<T>
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
        self.recipients
            .values_mut()
            .map(|recipient| recipient.act(action.clone()))
            .find(Result::is_err)
            .transpose()
            .map(drop)
    }

    /// Size of the set of recipients.
    pub fn len(&self) -> usize {
        self.recipients.len()
    }

    /// Is this set empty?
    pub fn is_empty(&self) -> bool {
        self.recipients.is_empty()
    }
}
