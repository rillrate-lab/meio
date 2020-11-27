use crate::{lifecycle::Interrupt, Action, ActionHandler, Actor, Context, Envelope, Id};
use anyhow::{anyhow, Error};
use futures::channel::mpsc;
use std::fmt;
use std::hash::{Hash, Hasher};

pub struct Controller<A: Actor> {
    id: Id,
    // TODO: Add watch with `async join` method
    /// High-priority messages sender
    hp_msg_tx: mpsc::UnboundedSender<HpEnvelope<A>>,
}

impl<A: Actor> Clone for Controller<A> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            hp_msg_tx: self.hp_msg_tx.clone(),
        }
    }
}

impl<A: Actor> fmt::Debug for Controller<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Controller").field(&self.id).finish()
    }
}

impl<A: Actor> PartialEq for Controller<A> {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

impl<A: Actor> Eq for Controller<A> {}

impl<A: Actor> Hash for Controller<A> {
    fn hash<H: Hasher>(&self, state: &mut H)
    where
        Self: Sized,
    {
        self.id.hash(state);
    }
}

impl<A: Actor> Controller<A> {
    pub(crate) fn new(id: Id, hp_msg_tx: mpsc::UnboundedSender<HpEnvelope<A>>) -> Self {
        Self { id, hp_msg_tx }
    }

    /// Returns `Id` of the controller.
    pub fn id(&self) -> Id {
        self.id.clone()
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

    /// Sends an `Interrrupt` event.
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
}
