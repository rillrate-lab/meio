//! Contains message of the `Actor`'s lifecycle.

use crate::actor_runtime::Actor;
use crate::handlers::{Action, ActionHandler, Operation};
use crate::ids::{Id, IdOf};
use crate::linkage::Address;
use crate::lite_runtime::{LiteTask, StopSender};
use anyhow::Error;
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

    // TODO: Rename to `insert_actor`
    pub fn insert<T>(&mut self, address: Address<T>, group: A::GroupBy)
    where
        T: Actor + ActionHandler<Interrupt<A>>,
    {
        let stage = self.stages.entry(group.clone()).or_default();
        let id: Id = address.id().into();
        stage.ids.insert(id.clone());
        let mut notifier = LifecycleNotifier::once(address, Operation::Forward, Interrupt::new());
        if stage.terminating {
            log::warn!(
                "Actor added into the terminating state (interrupt it immediately): {}",
                id
            );
            if let Err(err) = notifier.notify() {
                log::error!("Can't interrupt actor {:?} immediately: {}", id, err);
            }
        }
        let record = Record { group, notifier };
        self.records.insert(id, record);
    }

    pub fn insert_task<T>(&mut self, stopper: StopSender, group: A::GroupBy)
    where
        T: LiteTask,
    {
        let stage = self.stages.entry(group.clone()).or_default();
        let id = stopper.id();
        stage.ids.insert(id.clone());
        let mut notifier = LifecycleNotifier::stop(stopper);
        if stage.terminating {
            log::warn!(
                "Task added into the terminating state (interrupt it immediately): {}",
                id
            );
            if let Err(err) = notifier.notify() {
                log::error!("Can't interrupt task {:?} immediately: {}", id, err);
            }
        }
        let record = Record { group, notifier };
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
    pub fn ignore() -> Box<Self> {
        Box::new(|| Ok(()))
    }

    pub fn once<A, M>(mut address: Address<A>, operation: Operation, msg: M) -> Box<Self>
    where
        A: Actor + ActionHandler<M>,
        M: Action,
    {
        let mut msg = Some(msg);
        let notifier = move || {
            if let Some(msg) = msg.take() {
                // TODO: Take the priority into account (don't put all in hp)
                address.send_hp_direct(operation.clone(), msg)
            } else {
                Err(Error::msg(
                    "Attempt to send the second notification that can be sent once only.",
                ))
            }
        };
        Box::new(notifier)
    }

    // TODO: Require <T: LiteTask>
    pub fn stop(stopper: StopSender) -> Box<Self> {
        let notifier = move || stopper.stop();
        Box::new(notifier)
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
        // Normal priority, because it's a special event
        // that never sent by channels and used in-place.
        // But the type implements `Action` to be used by
        // ordinary handler type.
        false
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
        // Only `Interrupt` event has high-priority,
        // because all actors have to react to it as fast
        // as possible even in case when all queues are full.
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
        // It has normal priority, because `Done` message have to be the
        // latest in a queue of messages. No other message will be generated,
        // because `Actor` is terminated when its runtime sends `Done` event.
        false
    }
}

#[derive(Debug)]
pub(crate) struct TaskDone<T: LiteTask> {
    pub id: IdOf<T>,
    pub output: Option<T::Output>,
}

impl<T: LiteTask> TaskDone<T> {
    pub(crate) fn new(id: IdOf<T>) -> Self {
        Self { id, output: None }
    }
}

impl<T: LiteTask> Action for TaskDone<T> {
    fn is_high_priority(&self) -> bool {
        // It has normal priority, because `Done` message have to be the
        // latest in a queue of messages.
        // If it will be high priority than the `Task` can send messages that will
        // be delivered after `TaskDone` message will be received.
        false
    }
}
