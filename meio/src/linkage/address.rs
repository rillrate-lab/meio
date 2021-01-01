//! This module contains `Address` to interact with an `Actor`.

use super::{ActionRecipient, InteractionRecipient};
use crate::actor_runtime::{Actor, Status};
use crate::compat::watch;
use crate::handlers::{
    Action, ActionHandler, Consumer, Envelope, HpEnvelope, InstantAction, InstantActionHandler,
    Interact, Interaction, InteractionHandler, InterruptedBy, Operation, Scheduled, ScheduledItem,
    StreamItem,
};
use crate::ids::{Id, IdOf};
use crate::lifecycle::Interrupt;
use anyhow::Error;
use futures::channel::{mpsc, oneshot};
use futures::{SinkExt, Stream, StreamExt};
use std::convert::identity;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::time::Instant;

/// `Address` to send messages to `Actor`.
///
/// Can be compared each other to identify senders to
/// the same `Actor`.
pub struct Address<A: Actor> {
    // Plain `Id` used (not `IdOf`), because it's `Sync`.
    id: Id,
    /// High-priority messages sender
    hp_msg_tx: mpsc::UnboundedSender<HpEnvelope<A>>,
    /// Ordinary priority messages sender
    msg_tx: mpsc::Sender<Envelope<A>>,
    join_rx: watch::Receiver<Status>,
}

impl<A: Actor> Clone for Address<A> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            hp_msg_tx: self.hp_msg_tx.clone(),
            msg_tx: self.msg_tx.clone(),
            join_rx: self.join_rx.clone(),
        }
    }
}

impl<A: Actor> fmt::Debug for Address<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // TODO: Id cloned here. Fix!
        f.debug_tuple("Address").field(&self.id).finish()
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
    pub(crate) fn new(
        id: Id,
        hp_msg_tx: mpsc::UnboundedSender<HpEnvelope<A>>,
        msg_tx: mpsc::Sender<Envelope<A>>,
        join_rx: watch::Receiver<Status>,
    ) -> Self {
        Self {
            id,
            hp_msg_tx,
            msg_tx,
            join_rx,
        }
    }

    /// Returns a typed id of the `Actor`.
    pub fn id(&self) -> IdOf<A> {
        IdOf::new(self.id.clone())
    }

    /// Just sends an `Action` to the `Actor`.
    pub async fn act<I>(&mut self, input: I) -> Result<(), Error>
    where
        I: Action,
        A: ActionHandler<I>,
    {
        let envelope = Envelope::new(input);
        self.msg_tx.send(envelope).await.map_err(Error::from)
    }

    /// Just sends an `Action` to the `Actor`.
    pub fn instant<I>(&mut self, input: I) -> Result<(), Error>
    where
        I: InstantAction,
        A: InstantActionHandler<I>,
    {
        self.send_hp_direct(Operation::Forward, input)
    }

    /// Just sends an `Action` to the `Actor`.
    pub fn schedule<I>(&mut self, input: I, deadline: Instant) -> Result<(), Error>
    where
        I: Send + 'static,
        A: Scheduled<I>,
    {
        let operation = Operation::Schedule {
            deadline: deadline.clone(),
        };
        let wrapped = ScheduledItem {
            timestamp: deadline,
            item: input,
        };
        self.send_hp_direct(operation, wrapped)
    }

    /// Interacts with an `Actor` and waits for the result of the `Interaction`.
    pub async fn interact<I>(&mut self, request: I) -> Result<I::Output, Error>
    where
        I: Interaction,
        A: InteractionHandler<I>,
    {
        let (responder, rx) = oneshot::channel();
        let input = Interact { request, responder };
        self.act(input).await?;
        rx.await.map_err(Error::from).and_then(identity)
    }

    /// Waits when the `Actor` will be terminated.
    pub async fn join(&mut self) {
        // TODO: tokio 0.3
        // while self.join_rx.changed().await.is_ok() {
        while self.join_rx.recv().await.is_some() {
            if *self.join_rx.borrow() == Status::Stop {
                break;
            }
        }
    }

    /// Sends a service message using the high-priority queue.
    pub(crate) fn send_hp_direct<I>(&mut self, operation: Operation, input: I) -> Result<(), Error>
    where
        I: InstantAction,
        A: InstantActionHandler<I>,
    {
        let envelope = Envelope::instant(input);
        let msg = HpEnvelope {
            operation,
            envelope,
        };
        self.hp_msg_tx
            .unbounded_send(msg)
            .map_err(|_| Error::msg("can't send a high-priority service message"))
    }

    /// Sends an `Interrupt` event.
    ///
    /// It required a `Context` parameter just to restrict using it in
    /// methods other from handlers.
    pub(crate) fn interrupt_by<T>(&mut self) -> Result<(), Error>
    where
        A: InterruptedBy<T>,
        T: Actor,
    {
        self.send_hp_direct(Operation::Forward, Interrupt::new())
    }

    /// Attaches a `Stream` of event to an `Actor`.
    pub fn attach<S>(&self, stream: S)
    where
        A: Consumer<S::Item>,
        S: Stream + Send + Unpin + 'static,
        S::Item: Send + 'static,
    {
        let forwarder = Forwarder {
            stream,
            address: self.clone(),
        };
        // WARNING! Don't return `JoinHandle` because user can
        // accidentally `.await` it and block a handler.
        crate::compat::spawn_async(forwarder.entrypoint());
    }

    /// Returns a `Link` to an `Actor`.
    /// `Link` is a convenient concept for creating wrappers for
    /// `Address` that provides methods instead of using message types
    /// directly. It allows also to use private message types opaquely.
    pub fn link<T>(&self) -> T
    where
        T: From<Self>,
    {
        T::from(self.clone())
    }

    /// Returns an `ActionRecipient` instance.
    pub fn action_recipient<T>(&self) -> Box<dyn ActionRecipient<T>>
    where
        T: Action,
        A: ActionHandler<T>,
    {
        Box::new(self.clone())
    }

    /// Returns an `InteractionRecipient` instance.
    pub fn interaction_recipient<T>(&self) -> Box<dyn InteractionRecipient<T>>
    where
        T: Interaction,
        A: InteractionHandler<T>,
    {
        Box::new(self.clone())
    }
}

impl<T, A> Into<Box<dyn ActionRecipient<T>>> for Address<A>
where
    T: Action,
    A: Actor + ActionHandler<T>,
{
    fn into(self) -> Box<dyn ActionRecipient<T>> {
        Box::new(self)
    }
}

impl<T, A> Into<Box<dyn InteractionRecipient<T>>> for Address<A>
where
    T: Interaction,
    A: Actor + InteractionHandler<T>,
{
    fn into(self) -> Box<dyn InteractionRecipient<T>> {
        Box::new(self)
    }
}

/// This worker receives items from a stream and send them as actions
/// into an `Actor`.
struct Forwarder<S: Stream, A: Actor> {
    stream: S,
    address: Address<A>,
}

impl<S, A> Forwarder<S, A>
where
    S: Stream + Unpin,
    S::Item: Send + 'static,
    A: Actor + ActionHandler<StreamItem<S::Item>>,
{
    async fn entrypoint(mut self) {
        while let Some(item) = self.stream.next().await {
            let action = StreamItem { item };
            if let Err(err) = self.address.act(action).await {
                log::error!(
                    "Can't send an event to {:?} form a background stream: {}. Breaking...",
                    self.address.id(),
                    err
                );
                break;
            }
        }
    }
}
