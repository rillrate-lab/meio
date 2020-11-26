//! Contains message of the `Actor`'s lifecycle.

use crate::{Action, ActionHandler, Actor, Address, Context, Id, LiteTask, TypedId};
use anyhow::{anyhow, Error};
use std::collections::HashMap;
use std::marker::PhantomData;

/// Tracks the `Address`es by `Id`s.
pub struct LifetimeTracker<A: Actor> {
    map: HashMap<TypedId<A>, Address<A>>,
}

impl<A: Actor> LifetimeTracker<A> {
    /// Creates a new tracker.
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Inserts an `Address` to the `Tracker`.
    pub fn insert(&mut self, address: Address<A>) {
        self.map.insert(address.id(), address);
    }

    /// Tries to remove the `Address` from the `Tracker`.
    pub fn remove(&mut self, id: &TypedId<A>) -> Option<Address<A>> {
        self.map.remove(&id)
    }

    /// Sends `Interrupt` message to all addresses in a set.
    pub fn interrupt_all_by<T>(&mut self, ctx: &Context<T>)
    where
        A: ActionHandler<Interrupt<T>>,
        T: Actor,
    {
        for addr in self.map.values_mut() {
            addr.interrupt_by(ctx);
        }
    }
}

pub(crate) trait LifecycleNotifier: Send {
    fn notify(&mut self) -> Result<(), Error>;
}

impl<T> LifecycleNotifier for T
where
    T: FnMut() -> Result<(), Error>,
    T: Send,
{
    fn notify(&mut self) -> Result<(), Error> {
        (self)()
    }
}

impl dyn LifecycleNotifier {
    pub fn once<A, M>(address: &Address<A>, msg: M) -> Box<Self>
    where
        A: Actor + ActionHandler<M>,
        M: Action,
    {
        let mut addr = address.clone();
        let mut msg = Some(msg);
        let notifier = move || {
            if let Some(msg) = msg.take() {
                addr.send_hp_direct(msg)
            } else {
                Err(anyhow!(
                    "Attempt to send the second notification that can be sent once only."
                ))
            }
        };
        Box::new(notifier)
    }

    pub fn ignore() -> Box<Self> {
        Box::new(|| Ok(()))
    }
}

/// This message sent by a `Supervisor` to a spawned child actor.
#[derive(Debug)]
pub struct Awake<T: Actor> {
    _origin: PhantomData<T>,
}

impl<T: Actor> Awake<T> {
    pub(crate) fn new() -> Self {
        Self {
            _origin: PhantomData,
        }
    }
}

impl<T: Actor> Action for Awake<T> {
    fn is_high_priority(&self) -> bool {
        true
    }
}

/// The event to ask an `Actor` to interrupt its activity.
#[derive(Debug)]
pub struct Interrupt<T: Actor> {
    _origin: PhantomData<T>,
}

impl<T: Actor> Interrupt<T> {
    pub(crate) fn new() -> Self {
        Self {
            _origin: PhantomData,
        }
    }
}

impl<T: Actor> Action for Interrupt<T> {
    fn is_high_priority(&self) -> bool {
        true
    }
}

/// Notifies when `Actor`'s activity is completed.
#[derive(Debug)]
pub struct Done<T: Actor> {
    id: Id,
    _origin: PhantomData<T>,
}

impl<T: Actor> Done<T> {
    pub(crate) fn new(id: Id) -> Self {
        Self {
            id,
            _origin: PhantomData,
        }
    }
}

impl<T: Actor> Action for Done<T> {
    fn is_high_priority(&self) -> bool {
        true
    }
}

/// Notifies when `LiteTask` is finished.
#[derive(Debug)]
pub struct TaskDone<T: LiteTask> {
    id: Id,
    _origin: PhantomData<T>,
}

impl<T: LiteTask> TaskDone<T> {
    pub(crate) fn new(id: Id) -> Self {
        Self {
            id,
            _origin: PhantomData,
        }
    }
}

impl<T: LiteTask> Action for TaskDone<T> {
    fn is_high_priority(&self) -> bool {
        true
    }
}

/*
 * struct Supervisor {
 *   address?
 * }
 *
 * impl Supervisor {
 *   /// The method that allow a child to ask the supervisor to shutdown.
 *   /// It sends `Shutdown` message, the supervisor can ignore it.
 *   fn shutdown();
 * }
*/
