//! Contains utilities to manage supervised childs/tasks termination.

use crate::{Actor, Address, Controller, Id};
use std::collections::HashMap;

/// The state of termination.
#[derive(Debug, PartialEq, Eq)]
pub enum TerminationProgress {
    /// The `Actor` needs more time to clean up everything.
    ShouldWaitMore,
    /// It's ready to stop now.
    SafeToStop,
}

/// Tracks supervised activities and tries to terminate them in parallel.
pub struct Stage {
    stage_id: String,
    terminating: bool,
    supervised: HashMap<Id, Controller>,
    // All stages have to be terminated if the `Stage` with `vital` flag drained.
    vital: bool,
}

impl Stage {
    /// Creates a new empty `Stage` instance.
    fn new(stage_id: String) -> Self {
        Self {
            stage_id,
            terminating: false,
            supervised: HashMap::new(),
            vital: false,
        }
    }

    /// Is it empty? Can be used for cases when you
    /// need to terminate an `Actor` when the background task
    /// finished. For example if a background task processes
    /// network interactions and a session `Actor` completely
    /// related to it. But `Actor` can still have `terminate`
    /// method to unregister session and did something like that.
    pub fn is_drained(&self) -> bool {
        self.supervised.is_empty()
    }

    /// The `Stage` is vital and stop the `Actor` if its drained.
    pub fn is_vital(&self) -> bool {
        self.vital
    }

    /// Is `Stage` completely finished?
    pub fn is_done(&self) -> bool {
        self.is_terminating() && self.is_drained()
    }

    /// Is it terminating because `Shutdown` signal received yet.
    pub fn is_terminating(&self) -> bool {
        self.terminating
    }

    /// Makes the `Stage` vital and equate its completion to a stop signal receiving.
    pub fn make_vital(&mut self) {
        self.vital = true;
    }

    /// Inserts a `Controller` to spervise it. The method requires a
    /// `Controller` (not `Id`), because it will send `shutdown` signal
    /// if the `Stage` will receive `Shutdown` signal from the `Actor`
    /// that holds and manages that `Stage` instance.
    pub fn insert(&mut self, mut controller: Controller) {
        if self.terminating {
            controller.shutdown();
        } else {
            self.supervised.insert(controller.id(), controller);
        }
    }

    /// Internal function for starting termination and send `Shutdown` signal
    /// to all childs.
    fn start_termination(&mut self) {
        log::debug!("Terminating stage: {}", self.stage_id);
        self.terminating = true;
        for controller in self.supervised.values_mut() {
            controller.shutdown();
        }
    }

    /// Internal function that just removes an id from awaiting/supervised list.
    fn absorb(&mut self, id: &Id) -> bool {
        self.supervised.remove(id).is_some()
    }
}

/// Chains multiple stages into a sequence.
pub struct Terminator {
    related_id: Id,
    named_stages: HashMap<&'static str, usize>,
    stages: Vec<Stage>,
    stop_signal_received: bool,
    need_stop_signal: bool,
}

impl Terminator {
    /// Creates a new termination chain.
    pub fn new(related_id: Id) -> Self {
        Self {
            related_id,
            named_stages: HashMap::new(),
            stages: Vec::new(),
            stop_signal_received: false,
            need_stop_signal: true,
        }
    }

    /// Resets flag that stop is not required and wait for
    /// child actors only.
    pub fn stop_not_required(&mut self) {
        self.need_stop_signal = false;
    }

    /// Crates a named stage and register it in the termination order.
    ///
    /// # Panics
    ///
    /// Panics if stage already exists.
    pub fn named_stage<A: Actor>(&mut self) -> &mut Stage {
        let stage_id = std::any::type_name::<A>();
        let next_idx = self.stages.len();
        self.named_stages
            .insert(stage_id, next_idx)
            .expect_none("duplicated named stage");
        self.new_stage(stage_id)
    }

    /// Inserts a `Controller` into a named stage.
    ///
    /// # Panics
    ///
    /// Panics is the stage is not exists (not created with `named_stage` before).
    pub fn insert_to_named_stage<A: Actor>(&mut self, address: Address<A>) -> &mut Stage {
        let stage_id = std::any::type_name::<A>();
        let idx = self
            .named_stages
            .get_mut(stage_id)
            .expect("named stage not exists");
        let stage = self.stages.get_mut(*idx).expect("wrong named stage index");
        stage.insert(address.controller());
        stage
    }

    /// Adds a controller to the separate stage.
    pub fn insert_to_single_stage<C: Into<Controller>>(&mut self, pre_controller: C) -> &mut Stage {
        let controller = pre_controller.into();
        let stage = self.new_stage(controller.id().as_ref());
        stage.insert(controller);
        stage
    }

    /// Creates a new stage that can be marked as the default stage.
    fn new_stage(&mut self, stage_id: &str) -> &mut Stage {
        let full_id = format!("{}.{}", self.related_id, stage_id);
        let term = Stage::new(full_id);
        self.stages.push(term);
        let stage = self.stages.last_mut().expect("stages list broken");
        stage
    }

    fn try_terminate_next(&mut self) {
        // Terminate them in the reversed direction.
        for stage in self.stages.iter_mut().rev() {
            if stage.is_terminating() {
                if stage.is_drained() {
                    // Just go to the next stage
                } else {
                    // Wait for the next child will notify the owner of this stage
                    break;
                }
            } else {
                stage.start_termination();
            }
        }
    }

    /// You have to call this every time you've received a signal
    /// with a child `Id`.
    pub fn track_child(&mut self, child: Id) {
        let mut consumed = false;
        // Check the in the reversed direction, because the latest will be terminated earlier.
        for stage in self.stages.iter_mut().rev() {
            if stage.absorb(&child) {
                consumed = true;
                if stage.is_vital() && stage.is_drained() {
                    log::debug!("Vital Stage {} drained. Begin termination.", stage.stage_id);
                    self.stop_signal_received = true;
                }
                break;
            }
        }
        if !consumed {
            log::error!("Unknown child for stages chain: {:?}", child);
        }
    }

    /// The main method to use )
    pub fn track_child_or_stop_signal(&mut self, child: Option<Id>) -> TerminationProgress {
        match child {
            None => {
                self.stop_signal_received = true;
                // Start termination
                self.try_terminate_next();
            }
            Some(id) => {
                self.track_child(id);
                if self.stop_signal_received {
                    // Continue termination
                    self.try_terminate_next();
                }
            }
        }
        let all_stages_drained = self.stages.iter().all(Stage::is_drained);
        if all_stages_drained {
            if self.need_stop_signal {
                if self.stop_signal_received {
                    // All done: no childs, had stop signal
                    TerminationProgress::SafeToStop
                } else {
                    // Have to wait for the stop signal
                    TerminationProgress::ShouldWaitMore
                }
            } else {
                // No childs, stop not required
                TerminationProgress::SafeToStop
            }
        } else {
            // Has more to drain
            TerminationProgress::ShouldWaitMore
        }
    }
}
