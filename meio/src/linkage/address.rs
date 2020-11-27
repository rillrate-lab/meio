//! This module contains `Address` to interact with an `Actor`.

use crate::handlers::{Operation, Envelope, HpEnvelope, Interact, Interaction, Joiner};
use crate::{
    lifecycle::Interrupt, Action, ActionHandler, ActionPerformer, ActionRecipient, Actor, Context,
    Id, Notifier, TypedId, System,
};
use anyhow::{anyhow, Error};
use futures::channel::{mpsc, oneshot};
use futures::{SinkExt, Stream, StreamExt};
use std::convert::identity;
use std::fmt;
use std::hash::{Hash, Hasher};
use tokio::task::JoinHandle;

/// `Address` to send messages to `Actor`.
///
/// Can be compared each other to identify senders to
/// the same `Actor`.
pub struct Address<A: Actor> {
    id: Id,
    /// High-priority messages sender
    hp_msg_tx: mpsc::UnboundedSender<HpEnvelope<A>>,
    /// Ordinary priority messages sender
    msg_tx: mpsc::Sender<Envelope<A>>,
}

/*
impl<A: Actor> Into<Controller<A>> for Address<A> {
    fn into(self) -> Controller<A> {
        self.controller
    }
}
*/

impl<A: Actor> Clone for Address<A> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            hp_msg_tx: self.hp_msg_tx.clone(),
            msg_tx: self.msg_tx.clone(),
        }
    }
}

impl<A: Actor> fmt::Debug for Address<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // TODO: Id cloned here. Fix!
        f.debug_tuple("Address")
            .field(&self.id)
            .finish()
    }
}

impl<A: Actor> PartialEq for Address<A> {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

impl<A: Actor> Eq for Address<A> {}

impl<A: Actor> Hash for Address<A> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<A: Actor> Address<A> {
    pub(crate) fn new(id: Id, hp_msg_tx: mpsc::UnboundedSender<HpEnvelope<A>>, msg_tx: mpsc::Sender<Envelope<A>>) -> Self {
        Self { id, hp_msg_tx, msg_tx }
    }

    /// Returns a typed id of the `Actor`.
    pub fn id(&self) -> TypedId<A> {
        TypedId::new(self.id.clone())
    }

    pub async fn act<I>(&mut self, input: I) -> Result<(), Error>
    where
        I: Action,
        A: ActionHandler<I>,
    {
        let high_priority = input.is_high_priority();
        let envelope = Envelope::action(input);
        self.send(envelope, high_priority).await
    }

    pub async fn interact<T: Interaction>(&mut self, request: T) -> Result<T::Output, Error>
    {
        let (responder, rx) = oneshot::channel();
        let msg = Interact {
            request,
            responder,
        };
        rx.await.map_err(Error::from).and_then(identity)
    }

    pub async fn join(&mut self) -> Result<(), Error> {
        let (responder, rx) = oneshot::channel();
        let msg = Joiner {
            responder,
        };
        rx.await.map_err(Error::from).and_then(identity)
    }

    /// **Internal method.** Use `action` or `interaction` instead.
    /// It sends `Message` wrapped with `Envelope` to `Actor`.
    ///
    /// `high_priority` flag inidicates that it have to be send with high priority.
    pub(crate) async fn send(
        &mut self,
        msg: Envelope<A>,
        high_priority: bool,
    ) -> Result<(), Error> {
        if high_priority {
            self.send_hp_direct(Operation::Forward, msg)
        } else {
            self.msg_tx.send(msg).await.map_err(Error::from)
        }
    }

    /// Sends a service message using the high-priority queue.
    pub(crate) fn send_hp_direct(
        &mut self,
        operation: Operation,
        envelope: Envelope<A>,
    ) -> Result<(), Error> {
        let msg = HpEnvelope {
            operation,
            envelope,
        };
        self.hp_msg_tx
            .unbounded_send(msg)
            .map_err(|_| anyhow!("can't send a high-priority service message"))
    }

    /// Sends a service message using the high-priority queue.
    pub(crate) fn send_hp<T>(&mut self, msg: T) -> Result<(), Error>
    where
        T: Action,
        A: ActionHandler<T>,
    {
        let envelope = Envelope::action(msg);
        self.send_hp_direct(Operation::Forward, envelope)
    }

    /// Sends an `Interrupt` event.
    ///
    /// It required a `Context` parameter just to restrict using it in
    /// methods other from handlers.
    pub fn interrupt_by<T>(&mut self, _ctx: &Context<T>) -> Result<(), Error>
    where
        A: ActionHandler<Interrupt<T>>,
        T: Actor,
    {
        self.send_hp(Interrupt::new())
    }
    /// Forwards the stream into a flow of events to an `Actor`.
    async fn forward<S>(id: Id, mut stream: S, mut recipient: ActionRecipient<S::Item>)
    where
        A: ActionHandler<S::Item>,
        S: Stream + Unpin,
        S::Item: Action,
    {
        while let Some(action) = stream.next().await {
            if let Err(err) = recipient.act(action).await {
                log::error!(
                    "Can't send an event to {:?} form a background stream: {}. Breaking...",
                    id,
                    err
                );
                break;
            }
        }
    }

    /// Attaches a `Stream` of event to an `Actor`.
    pub fn attach<S>(&mut self, stream: S) -> JoinHandle<()>
    where
        A: ActionHandler<S::Item>,
        S: Stream + Send + Unpin + 'static,
        S::Item: Action,
    {
        let recipient = self.action_recipient();
        let id = self.id();
        let fut = Self::forward(id.into(), stream, recipient);
        tokio::spawn(fut)
    }

    /// Generates `ActionRecipient`.
    pub fn action_recipient<I>(&self) -> ActionRecipient<I>
    where
        A: ActionHandler<I>,
        I: Action,
    {
        ActionRecipient::from(self.clone())
    }

    /*
    /// Generates `InteractionRecipient`.
    pub fn interaction_recipient<I>(&self) -> InteractionRecipient<I>
    where
        A: InteractionHandler<I>,
        I: Interaction,
    {
        InteractionRecipient::from(self.clone())
    }
    */

    /*
    /// Gives a `Controller` of that entity.
    pub fn controller(&self) -> Controller<A> {
        self.controller.clone()
    }
    */

    /*
    /// Creates the notifier that will use a provided message for notifications.
    pub fn notifier<I>(&self, message: I) -> Notifier<I>
    where
        A: ActionHandler<I>,
        I: Action + Clone,
    {
        Notifier::new(self.action_recipient(), message)
    }
    */

    pub fn shutdown(&mut self) -> Result<(), Error>
    where
        A: ActionHandler<Interrupt<System>>,
    {
        self.send_hp(Interrupt::<System>::new())
    }
}
