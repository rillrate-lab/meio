//! Contains message of the `Actor`'s lifecycle.

use crate::handlers::Operation;
use crate::{Action, ActionHandler, Actor, Address, Id, IdOf};
use anyhow::{anyhow, Error};
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;

struct Stage {
    terminating: bool,
    ids: HashSet<Id>,
}

impl Default for Stage {
    fn default() -> Self {
        Self {
            terminating: false,
            ids: HashSet::new(),
        }
    }
}

impl Stage {
    fn is_finished(&self) -> bool {
        self.terminating && self.ids.is_empty()
    }
}

struct Record<T> {
    group: T,
    notifier: Box<dyn LifecycleNotifier>,
}

// TODO: Rename to Terminator again
pub(crate) struct LifetimeTracker<T: Actor> {
    terminating: bool,
    prioritized: Vec<T::GroupBy>,
    stages: HashMap<T::GroupBy, Stage>,
    records: HashMap<Id, Record<T::GroupBy>>,
}

// TODO: Change T to A
impl<A: Actor> LifetimeTracker<A> {
    // TODO: Make the constructor private
    pub fn new() -> Self {
        Self {
            terminating: false,
            // TODO: with_capacity 0 ?
            prioritized: Vec::new(),
            stages: HashMap::new(),
            records: HashMap::new(),
        }
    }

    pub fn is_terminating(&self) -> bool {
        self.terminating
    }

    pub fn insert<T: Actor>(&mut self, address: Address<T>, group: A::GroupBy)
    where
        T: Actor + ActionHandler<Interrupt<A>>,
    {
        let stage = self.stages.entry(group.clone()).or_default();
        let id: Id = address.id().into();
        stage.ids.insert(id.clone());
        let notifier = LifecycleNotifier::once(address, Operation::Forward, Interrupt::new());
        let mut record = Record { group, notifier };
        if stage.terminating {
            log::warn!(
                "Actor added into the terminating state (interrupt it immediately): {}",
                id
            );
            if let Err(err) = record.notifier.notify() {
                log::error!("Can't interrupt actor {:?} immediately: {}", id, err);
            }
        }
        self.records.insert(id, record);
    }

    pub fn remove(&mut self, id: &Id) {
        if let Some(record) = self.records.remove(id) {
            if let Some(stage) = self.stages.get_mut(&record.group) {
                stage.ids.remove(id);
            }
        }
        if self.terminating {
            self.try_terminate_next();
        }
    }

    // TODO: Change `Vec` to `OrderedSet`
    pub fn prioritize_termination_by(&mut self, groups: Vec<A::GroupBy>) {
        self.prioritized = groups;
    }

    pub fn is_finished(&self) -> bool {
        self.stages.values().all(Stage::is_finished)
    }

    fn stage_sequence(&self) -> Vec<A::GroupBy> {
        let stages_to_term: HashSet<_> = self.stages.keys().cloned().collect();
        let prior_set: HashSet<_> = self.prioritized.iter().cloned().collect();
        let remained = stages_to_term.difference(&prior_set).cloned();
        let mut sequence = self.prioritized.clone();
        sequence.extend(remained);
        sequence
    }

    fn try_terminate_next(&mut self) {
        self.terminating = true;
        for stage_name in self.stage_sequence() {
            if let Some(stage) = self.stages.get_mut(&stage_name) {
                if !stage.terminating {
                    stage.terminating = true;
                    for id in stage.ids.iter() {
                        if let Some(record) = self.records.get_mut(id) {
                            if let Err(err) = record.notifier.notify() {
                                log::error!(
                                    "Can't notify the supervisor about actor with {:?} termination: {}",
                                    id,
                                    err
                                );
                            }
                        }
                    }
                }
                if !stage.is_finished() {
                    break;
                }
            }
        }
    }

    pub fn start_termination(&mut self) {
        self.try_terminate_next();
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
    pub fn once<A, M>(mut address: Address<A>, operation: Operation, msg: M) -> Box<Self>
    where
        A: Actor + ActionHandler<M>,
        M: Action,
    {
        let mut msg = Some(msg);
        let notifier = move || {
            if let Some(msg) = msg.take() {
                address.send_hp_direct(operation.clone(), msg)
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
pub(crate) struct Awake<T: Actor> {
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
pub(crate) struct Interrupt<T: Actor> {
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
pub(crate) struct Done<T: Actor> {
    pub id: IdOf<T>,
}

impl<T: Actor> Done<T> {
    pub(crate) fn new(id: IdOf<T>) -> Self {
        Self { id }
    }
}

impl<T: Actor> Action for Done<T> {
    fn is_high_priority(&self) -> bool {
        true
    }
}
