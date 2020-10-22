//! Contains utilities to manage supervised childs/tasks termination.

use crate::{Controller, Id};
use std::collections::{HashMap, VecDeque};

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
}

impl Stage {
    /// Creates a new empty `Stage` instance.
    fn new(stage_id: String) -> Self {
        Self {
            stage_id,
            terminating: false,
            supervised: HashMap::new(),
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

    /// Is `Stage` completely finished?
    pub fn is_done(&self) -> bool {
        self.is_terminating() && self.is_drained()
    }

    /// Is it terminating because `Shutdown` signal received yet.
    pub fn is_terminating(&self) -> bool {
        self.terminating
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
    default_stage: Option<usize>,
    stages: VecDeque<Stage>,
    stop_signal_received: bool,
    need_stop_signal: bool,
}

impl Terminator {
    /// Creates a new termination chain.
    pub fn new(related_id: Id) -> Self {
        Self {
            related_id,
            default_stage: None,
            stages: VecDeque::new(),
            stop_signal_received: false,
            need_stop_signal: true,
        }
    }

    /// Resets flag that stop is not required and wait for
    /// child actors only.
    pub fn stop_not_required(&mut self) {
        self.need_stop_signal = false;
    }

    /// Returns the default stage.
    /// Default stage is a container that should be used to put dynamical
    /// tasks that created during actor's lifetime.
    pub fn stage(&mut self) -> &mut Stage {
        let idx = self.default_stage.expect("default stage not set");
        self.stages
            .get_mut(idx)
            .expect("invalid index of the default stage")
    }

    // TODO: Put to tail!
    /// Creates a new stage that can be marked as the default stage.
    pub fn new_stage(&mut self, stage_id: &str, default: bool) -> &mut Stage {
        if default {
            // Points to the latest inserted element.
            self.default_stage = Some(0);
        } else {
            // If `default_stage` was set than move it forward since
            // one `Stage` will be added to the front of the list.
            if let Some(default_stage) = self.default_stage.as_mut() {
                *default_stage += 1;
            }
        }
        let full_id = format!("{}.{}", self.related_id, stage_id);
        let term = Stage::new(full_id);
        self.stages.push_front(term);
        self.stages.get_mut(0).expect("stages list broken")
    }

    fn try_terminate_next(&mut self) {
        for stage in self.stages.iter_mut() {
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
        for term in self.stages.iter_mut() {
            if term.absorb(&child) {
                consumed = true;
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
