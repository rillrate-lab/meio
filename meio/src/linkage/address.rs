//! This module contains `Address` to interact with an `Actor`.

use super::controller::{Controller, Operation};
use crate::{
    lifecycle::Interrupt, Action, ActionHandler, ActionPerformer, ActionRecipient, Actor, Context,
    Envelope, Id, Notifier, TypedId,
    Interaction,
};
use anyhow::{anyhow, Error};
use derive_more::{Deref, DerefMut};
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
#[derive(Deref, DerefMut)]
pub struct Address<A: Actor> {
    #[deref]
    #[deref_mut]
    controller: Controller<A>,
    /// Ordinary priority messages sender
    msg_tx: mpsc::Sender<Envelope<A>>,
}

impl<A: Actor> Into<Controller<A>> for Address<A> {
    fn into(self) -> Controller<A> {
        self.controller
    }
}

impl<A: Actor> Clone for Address<A> {
    fn clone(&self) -> Self {
        Self {
            controller: self.controller.clone(),
            msg_tx: self.msg_tx.clone(),
        }
    }
}

impl<A: Actor> fmt::Debug for Address<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // TODO: Id cloned here. Fix!
        f.debug_tuple("Address")
            .field(&self.controller.id())
            .finish()
    }
}

impl<A: Actor> PartialEq for Address<A> {
    fn eq(&self, other: &Self) -> bool {
        self.controller.eq(&other.controller)
    }
}

impl<A: Actor> Eq for Address<A> {}

impl<A: Actor> Hash for Address<A> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.controller.hash(state);
    }
}

impl<A: Actor> Address<A> {
    pub(crate) fn new(controller: Controller<A>, msg_tx: mpsc::Sender<Envelope<A>>) -> Self {
        Self { controller, msg_tx }
    }

    /// Returns a typed id of the `Actor`.
    pub fn id(&self) -> TypedId<A> {
        TypedId::new(self.controller.id())
    }

    pub async fn interact<IN, OUT>(&mut self, request: IN) -> Result<OUT, Error>
    where
        IN: Send + 'static,
        OUT: Send + 'static,
        A: ActionHandler<Interaction<IN, OUT>>,
    {
        let (responder, rx) = oneshot::channel();
        let msg = Interaction {
            request,
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
            self.controller.send_hp_direct(Operation::Forward, msg)
        } else {
            self.msg_tx.send(msg).await.map_err(Error::from)
        }
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
        let id = self.controller.id();
        let fut = Self::forward(id, stream, recipient);
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

    /// Gives a `Controller` of that entity.
    pub fn controller(&self) -> Controller<A> {
        self.controller.clone()
    }

    /// Creates the notifier that will use a provided message for notifications.
    pub fn notifier<I>(&self, message: I) -> Notifier<I>
    where
        A: ActionHandler<I>,
        I: Action + Clone,
    {
        Notifier::new(self.action_recipient(), message)
    }

    pub async fn join(&mut self) {
        // TODO: Get the `oneshot` instance using `Interaction` call (the `Actor` has to register
        // it and execute later) and `await` it.
    }
}
