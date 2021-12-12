//! This module contains `Address` to interact with an `Actor`.

use super::{ActionRecipient, InteractionRecipient};
use crate::actor_runtime::{Actor, Status};
use crate::compat::watch;
use crate::forwarders::AttachStream;
use crate::handlers::{
    Action, ActionHandler, Consumer, Envelope, Handler, InstantAction, InstantActionHandler,
    Interact, Interaction, InteractionHandler, InteractionTask, InterruptedBy, Operation, Parcel,
    Priority, Scheduled, ScheduledItem, StreamAcceptor, TerminateBy, TerminatedBy,
};
use crate::ids::{Id, IdOf};
use crate::lifecycle::Interrupt;
use crate::lite_runtime::Tag;
use anyhow::Error;
use futures::Stream;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::time::Instant;
use tokio::sync::mpsc;

/// Pre-created `Address` that can be used in spawning an actor.
pub(crate) struct AddressPair<A: Actor> {
    pub(crate) joint: AddressJoint<A>,
    pub(crate) address: Address<A>,
}

impl<A: Actor> AddressPair<A> {
    /// Creates an `AddressPair` for the actor.
    pub(crate) fn new_for(_actor: &A) -> Self {
        let id = Id::unique();
        Self::new_with_id(id)
    }

    /// Create a new independent pair
    pub fn new_with_id(id: Id) -> Self {
        let (hp_msg_tx, hp_msg_rx) = mpsc::unbounded_channel();
        let (msg_tx, msg_rx) = mpsc::unbounded_channel();
        let (join_tx, join_rx) = watch::channel(Status::Alive);
        let joint = AddressJoint {
            msg_rx,
            hp_msg_rx,
            join_tx,
        };
        let address = Address {
            id,
            hp_msg_tx,
            msg_tx,
            join_rx,
        };
        Self { joint, address }
    }

    /// Gets address of the pair.
    pub fn address(&self) -> &Address<A> {
        &self.address
    }
}

/// Receiver for data sent by `Address`.
pub(crate) struct AddressJoint<A: Actor> {
    /// `Receiver` that have to be used to receive incoming messages.
    pub msg_rx: mpsc::UnboundedReceiver<Envelope<A>>,
    /// High-priority receiver
    pub hp_msg_rx: mpsc::UnboundedReceiver<Parcel<A>>,
    /// Sends a signal when the `Actor` completely stopped.
    pub join_tx: watch::Sender<Status>,
}

/// `Address` to send messages to `Actor`.
///
/// Can be compared each other to identify senders to
/// the same `Actor`.
pub struct Address<A: Actor> {
    // Plain `Id` used (not `IdOf`), because it's `Sync`.
    id: Id,
    /// High-priority messages sender
    hp_msg_tx: mpsc::UnboundedSender<Parcel<A>>,
    /// Ordinary priority messages sender
    msg_tx: mpsc::UnboundedSender<Envelope<A>>,
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
    /// Returns a typed id of the `Actor`.
    pub fn id(&self) -> IdOf<A> {
        IdOf::new(self.id.clone())
    }

    pub(crate) fn raw_id(&self) -> &Id {
        &self.id
    }

    /// Just sends an `Action` to the `Actor`.
    pub fn act<I>(&self, input: I) -> Result<(), Error>
    where
        I: Action,
        A: ActionHandler<I>,
    {
        let envelope = Envelope::new(input);
        self.normal_priority_send(envelope)
    }

    /// Just sends an `Action` to the `Actor`.
    pub fn instant<I>(&self, input: I) -> Result<(), Error>
    where
        I: InstantAction,
        A: InstantActionHandler<I>,
    {
        let parcel = Parcel::new(Operation::Forward, input);
        self.high_priority_send(parcel)
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
        let parcel = Parcel::new(operation, wrapped);
        self.high_priority_send(parcel)
    }

    /// Send a `Parcel` to unpacking.
    pub fn unpack_parcel(&self, parcel: Parcel<A>) -> Result<(), Error> {
        self.high_priority_send(parcel)
    }

    fn high_priority_send(&self, parcel: Parcel<A>) -> Result<(), Error> {
        self.hp_msg_tx
            .send(parcel)
            .map_err(|_| Error::msg("can't send a high-priority service message"))
    }

    fn normal_priority_send(&self, envelope: Envelope<A>) -> Result<(), Error> {
        self.msg_tx
            .send(envelope)
            // TODO: Improve that
            .map_err(|err| Error::msg(err.to_string()))
    }

    /// Send `Handler` as an event
    pub fn send_event(&self, handler: impl Handler<A>) -> Result<(), Error> {
        let priority = handler.priority();
        let envelope = Envelope::from_handler(handler);
        match priority {
            Priority::Normal => self.normal_priority_send(envelope),
            Priority::Instant => {
                let parcel = Parcel::from_envelope(envelope);
                self.high_priority_send(parcel)
            }
        }
    }

    /// Interacts with an `Actor` and waits for the result of the `Interaction`.
    ///
    /// `ActionHandler` required instead of `InteractionHandler` to make it possible
    /// to work with both types of handler, because `ActionHandler` can be used
    /// for long running interaction and prevent blocking of the actor's routine.
    ///
    /// To avoid blocking you shouldn't `await` the result of this `Interaction`,
    /// but create a `Future` and `await` in a separate coroutine of in a `LiteTask`.
    // Change this method carefully. Since `InteractionRecipient` implemented for
    // all addresses if `InteractionRecipient::interact` method won't have the same
    // name like this method it can give unwanted recursion.
    pub fn interact<I>(&self, request: I) -> InteractionTask<I>
    where
        I: Interaction,
        // IMPORTANT! Not `trait InteractionHandler<_>` has to be used here!
        // It makes this method more flexible and implementor can keep
        // `InteractionResponder` for a while to send response later/async.
        A: ActionHandler<Interact<I>>,
    {
        InteractionTask::new(self, request)
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

    /// Sends an `Interrupt` event.
    ///
    /// It required a `Context` parameter just to restrict using it in
    /// methods other from handlers.
    pub(crate) fn interrupt_by<T>(&self) -> Result<(), Error>
    where
        A: InterruptedBy<T>,
        T: Actor,
    {
        let parcel = Parcel::new(Operation::Forward, Interrupt::new());
        self.high_priority_send(parcel)
    }

    /// Send termination signal to the actor through the normal priority queue.
    pub fn terminate_by<T>(&self) -> Result<(), Error>
    where
        A: TerminatedBy<T>,
        T: 'static,
    {
        let input = TerminateBy::new();
        let envelope = Envelope::new(input);
        self.normal_priority_send(envelope)
    }

    /// Attaches a `Stream` of event to an `Actor`.
    /// Optimized for intensive streams. For moderate flow you still can
    /// use ordinary `Action`s and `act` method calls.
    ///
    /// It spawns a routine that groups multiple items into a single chunk
    /// to reduce amount as `async` calls of a handler.
    pub fn attach<S, M>(&mut self, stream: S, tag: M) -> Result<(), Error>
    where
        A: Consumer<S::Item> + StreamAcceptor<S::Item>,
        S: Stream + Send + Unpin + 'static,
        S::Item: Send + 'static,
        M: Tag,
    {
        let msg = AttachStream::new(stream, tag);
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

impl<T, A> From<Address<A>> for Box<dyn ActionRecipient<T>>
where
    T: Action,
    A: Actor + ActionHandler<T>,
{
    fn from(address: Address<A>) -> Self {
        Box::new(address)
    }
}

impl<T, A> From<Address<A>> for Box<dyn InteractionRecipient<T>>
where
    T: Interaction,
    A: Actor + InteractionHandler<T>,
{
    fn from(address: Address<A>) -> Self {
        Box::new(address)
    }
}
