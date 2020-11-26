//! Contains message of the `Actor`'s lifecycle.

use crate::{
    Action, ActionHandler, ActionRecipient, Actor, Address, Context, Id, LiteTask, TypedId,
};
use anyhow::{anyhow, Error};
use std::any::{type_name, Any};
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;

struct Record {
    address: Box<dyn Any>,
    notifier: Box<dyn LifecycleNotifier>,
}

impl Record {
    fn new<T, A>(address: Address<A>) -> Self
    where
        A: Actor + ActionHandler<Interrupt<T>>,
        T: Actor,
    {
        let notifier = LifecycleNotifier::once(&address, Interrupt::new());
        let address = Box::new(address.clone());
        Self { address, notifier }
    }
}

struct Stage {
    terminating: bool,
    map: HashMap<Id, Record>,
}

impl Default for Stage {
    fn default() -> Self {
        Self {
            terminating: false,
            map: HashMap::new(),
        }
    }
}

impl Stage {
    fn terminated(&mut self) -> bool {
        if !self.terminating {
            self.terminating = true;
            for record in self.map.values_mut() {
                record.notifier.notify();
            }
        }
        self.map.is_empty()
    }
}

pub struct LifetimeTracker<T: Actor> {
    terminating: bool,
    prioritized: Vec<&'static str>,
    vital: HashSet<&'static str>,
    stages: HashMap<&'static str, Stage>,
    _actor: PhantomData<T>,
}

impl<T: Actor> LifetimeTracker<T> {
    // TODO: Make the constructor private
    pub fn new() -> Self {
        Self {
            terminating: false,
            // TODO: with_capacity 0 ?
            prioritized: Vec::new(),
            vital: HashSet::new(),
            stages: HashMap::new(),
            _actor: PhantomData,
        }
    }

    pub fn insert<A: Actor>(&mut self, address: Address<A>)
    where
        A: Actor + ActionHandler<Interrupt<T>>,
    {
        let type_name = type_name::<A>();
        let stage = self.stages.entry(type_name).or_default();
        let id = address.id().id;
        let record = Record::new(address);
        stage.map.insert(id, record);
    }

    pub fn remove<A: Actor>(&mut self, id: &TypedId<A>) -> Option<Box<Address<A>>> {
        let type_name = type_name::<A>();
        self.stages
            .get_mut(type_name)?
            .map
            .remove(&id.id)
            .and_then(|record| record.address.downcast().ok())
    }

    pub fn track<A: Actor>(&mut self, id: &TypedId<A>, ctx: &mut Context<T>) {
        let type_name = type_name::<A>();
        let addr = self.remove(id);
        if addr.is_some() && self.vital.contains(type_name) {
            self.try_terminate_next(ctx);
        }
    }

    pub fn prioritize_termination<A>(&mut self) {
        let type_name = type_name::<A>();
        self.prioritized.push(type_name);
    }

    pub fn mark_vital<A>(&mut self) {
        let type_name = type_name::<A>();
        self.vital.insert(type_name);
    }

    fn try_terminate_next(&mut self, ctx: &mut Context<T>) {
        self.terminating = true;
        let mut remained_stages: HashSet<&'static str> = self.stages.keys().cloned().collect();
        for stage_name in self.prioritized.iter() {
            remained_stages.remove(stage_name);
            if let Some(stage) = self.stages.get_mut(stage_name) {
                if !stage.terminated() {
                    return;
                }
            }
        }
        for stage_name in remained_stages.drain() {
            if let Some(stage) = self.stages.get_mut(stage_name) {
                if !stage.terminated() {
                    return;
                }
            }
        }
        ctx.stop();
    }

    pub fn start_termination(&mut self, ctx: &mut Context<T>) {
        self.try_terminate_next(ctx);
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
