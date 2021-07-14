use crate::ids::IdOf;
use crate::lite_runtime::TaskAddress;
use std::collections::HashMap;

/// Set of tasks that can be stopped together.
#[derive(Debug)]
pub struct TaskDistributor<T> {
    tasks: HashMap<IdOf<T>, TaskAddress<T>>,
}

impl<T> Default for TaskDistributor<T> {
    fn default() -> Self {
        Self {
            tasks: HashMap::new(),
        }
    }
}

impl<T> TaskDistributor<T> {
    /// Creates a new set of tasks.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<T> TaskDistributor<T> {
    /// Inserts a task into the set.
    pub fn insert(&mut self, addr: TaskAddress<T>) {
        self.tasks.insert(addr.id(), addr);
    }

    /// Removes a task from the set by `IfOd`.
    pub fn get(&mut self, id: &IdOf<T>) -> Option<&TaskAddress<T>> {
        self.tasks.get(id)
    }

    /// Removes a task from the set by `IfOd`.
    pub fn remove(&mut self, id: &IdOf<T>) -> Option<TaskAddress<T>> {
        self.tasks.remove(id)
    }

    /// Send stop signal to all tasks.
    pub fn stop_all(&self) {
        for addr in self.tasks.values() {
            if let Err(err) = addr.stop() {
                log::error!("Can't stop the task {:?}: {}", addr.id(), err);
            }
        }
    }

    /// Size of the set of tasks.
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Is this set empty?
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
}
