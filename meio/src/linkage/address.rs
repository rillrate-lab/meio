//! This module contains `Address` to interact with an `Actor`.

use super::{ActionRecipient, InteractionRecipient};
use crate::actor_runtime::{Actor, Status};
use crate::compat::watch;
use crate::forwarders::AttachStream;
use crate::handlers::{
    Action, ActionHandler, Consumer, Envelope, HpEnvelope, InstantAction, InstantActionHandler,
    Interact, Interaction, InteractionHandler, InterruptedBy, Operation, Parcel, Scheduled,
    ScheduledItem,
};
use crate::ids::{Id, IdOf};
use crate::lifecycle::Interrupt;
use anyhow::Error;
use futures::channel::{mpsc, oneshot};
use futures::{SinkExt, Stream};
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

    pub(crate) fn raw_id(&self) -> &Id {
        &self.id
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
    pub fn instant<I>(&self, input: I) -> Result<(), Error>
    where
        I: InstantAction,
        A: InstantActionHandler<I>,
    {
        self.send_hp_direct(Operation::Forward, input)
    }

    /// Just sends an `Action` to the `Actor`.
    pub fn schedule<I>(&self, input: I, deadline: Instant) -> Result<(), Error>
    where
        I: Send + 'static,
        A: Scheduled<I>,
    {
        let operation = Operation::Schedule { deadline };
        let wrapped = ScheduledItem {
            timestamp: deadline,
            item: input,
        };
        self.send_hp_direct(operation, wrapped)
    }

    /// Send a `Parcel` to unpacking.
    pub fn send_parcel(&self, parcel: Parcel<A>) -> Result<(), Error> {
        // TODO: Use `send_hp_direct`
        let msg = HpEnvelope {
            operation: Operation::Forward,
            envelope: parcel.into(),
        };
        self.hp_msg_tx
            .unbounded_send(msg)
            .map_err(|_| Error::msg("can't send a parcel to high-priority messages queue"))
    }

    // TODO: Add a `LiteTask` and can forward the result of a long running
    // `Interaction` to the `Actor`.
    //
    // TODO: Add `InteractionResponse` trait, to wait for the result of interaction.
    // It has to be implemented as a `LiteTask` to make every interaction
    // interruptable.
    //
    // TODO: Add `interaction` method to a version that always spawns
    // a `LiteTask` for an interaction. BUT! Keep `interact` method that
    // is very important for non-meio usage of an `Address`.

    // TODO: Return `InteractAndWait` future instance, instead of the `Result`
    /// Interacts with an `Actor` and waits for the result of the `Interaction`.
    ///
    /// `ActionHandler` required instead of `InteractionHandler` to make it possible
    /// to work with both types of handler, because `ActionHandler` can be used
    /// for long running interaction and prevent blocking of the actor's routine.
    ///
    /// To avoid blocking you shouldn't `await` the result of this `Interaction`,
    /// but create a `Future` and `await` in a separate coroutine of in a `LiteTask`.
    pub async fn interact_and_wait<I>(&mut self, request: I) -> Result<I::Output, Error>
    where
        I: Interaction,
        // ! Not `InteractionHandler` has to be used here !
        A: ActionHandler<Interact<I>>,
    {
        let (responder, rx) = oneshot::channel();
        let input = Interact { request, responder };
        self.act(input).await?;
        rx.await.map_err(Error::from).and_then(identity)
    }

    /// Waits when the `Actor` will be terminated.
    ///
    /// It consumes address, because it useless after termination.
    /// Also it prevents blocking queue if `Actor` uses it to detect
    /// the right time for termination.
    pub async fn join(self) {
        let mut rx = self.join_rx.clone();
        drop(self);
        while rx.changed().await.is_ok() {
            if *rx.borrow() == Status::Stop {
                break;
            }
        }
    }

    /// Sends a service message using the high-priority queue.
    pub(crate) fn send_hp_direct<I>(&self, operation: Operation, input: I) -> Result<(), Error>
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
    pub(crate) fn interrupt_by<T>(&self) -> Result<(), Error>
    where
        A: InterruptedBy<T>,
        T: Actor,
    {
        self.send_hp_direct(Operation::Forward, Interrupt::new())
    }

    /// Attaches a `Stream` of event to an `Actor`.
    /// Optimized for intensive streams. For moderate flow you still can
    /// use ordinary `Action`s and `act` method calls.
    ///
    /// It spawns a routine that groups multiple items into a single chunk
    /// to reduce amount as `async` calls of a handler.
    pub fn attach<S>(&mut self, stream: S) -> Result<(), Error>
    where
        A: Consumer<S::Item>,
        S: Stream + Send + Unpin + 'static,
        S::Item: Send + 'static,
    {
        let msg = AttachStream::new(stream);
        self.instant(msg)
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
