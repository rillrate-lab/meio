//! Contains message of the `Actor`'s lifecycle.

use crate::{Action, ActionHandler, Actor, Address, Context, Id, LiteTask, TypedId};
use anyhow::{anyhow, Error};
use std::any::{type_name, Any};
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;

pub struct LifetimeTrackerOf<T> {
    terminating: bool,
    prioritized: Vec<&'static str>,
    vital: HashSet<&'static str>,
    type_to_map: HashMap<&'static str, HashMap<Id, Box<dyn Any>>>,
    _actor: PhantomData<T>,
}

impl<T: Actor> LifetimeTrackerOf<T> {
    // TODO: Make the constructor private
    pub fn new() -> Self {
        Self {
            terminating: false,
            // TODO: with_capacity 0 ?
            prioritized: Vec::new(),
            vital: HashSet::new(),
            type_to_map: HashMap::new(),
            _actor: PhantomData,
        }
    }

    pub fn insert<A: Actor>(&mut self, address: Address<A>) {
        let type_name = type_name::<A>();
        let map = self.type_to_map.entry(type_name).or_default();
        let id = address.id().id;
        let boxed = Box::new(address);
        map.insert(id, boxed);
    }

    pub fn remove<A: Actor>(&mut self, id: &TypedId<A>) -> Option<Box<Address<A>>> {
        let type_name = type_name::<A>();
        self.type_to_map
            .get_mut(type_name)?
            .remove(&id.id)
            .and_then(|boxed| boxed.downcast().ok())
    }

    pub fn track<A: Actor>(&mut self, id: &TypedId<A>, ctx: &mut Context<T>) {
        let addr = self.remove(id);
        let type_name = type_name::<A>();
        if addr.is_some() && self.vital.contains(type_name) {
            self.terminating = true;
        }
        self.try_terminate_next(ctx);
    }

    pub fn prioritize_termination<A>(&mut self) {
        let type_name = type_name::<A>();
        self.prioritized.push(type_name);
    }

    pub fn mark_vital<A>(&mut self) {
        let type_name = type_name::<A>();
        self.vital.insert(type_name);
    }

    fn is_ready_to_stop(&self) -> bool {
        self.type_to_map.values().all(HashMap::is_empty)
    }

    fn try_terminate_next(&mut self, ctx: &mut Context<T>) {
        if self.is_ready_to_stop() {
            ctx.stop();
        } else {
            for level in self.prioritized.iter() {
                todo!();
            }
            todo!("terminate the next stage in a queue, or others using set diff to get others...");
        }
    }

    pub fn start_termination(&mut self, ctx: &mut Context<T>) {
        self.terminating = true;
        self.try_terminate_next(ctx);
    }
}

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

    /// Checks the map is empty.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
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
    pub id: TypedId<T>,
}

impl<T: Actor> Done<T> {
    pub(crate) fn new(id: TypedId<T>) -> Self {
        Self { id }
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
    pub id: Id,
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
