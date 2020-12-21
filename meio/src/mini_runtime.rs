use crate::actor_runtime::Actor;
use crate::handlers::{ActionHandler, Operation};
use crate::ids::{Id, IdOf};
use crate::lifecycle::{LifecycleNotifier, TaskDone};
use crate::linkage::Address;
use crate::lite_runtime::{LiteTask, StopReceiver};
use anyhow::Error;
use async_trait::async_trait;
use uuid::Uuid;

// TODO: Spawn lite task with no supervisor (like Actors can do)
pub(crate) fn spawn<T, S>(task: T, supervisor: Option<Address<S>>)
where
    T: LiteTask,
    S: Actor + ActionHandler<TaskDone<T>>,
{
    let id = Id::of_task(&task);
    let id_of = IdOf::<T>::new(id.clone());
    let done_notifier = {
        match supervisor {
            None => LifecycleNotifier::ignore(),
            Some(super_addr) => {
                let event = TaskDone::new(id_of);
                let op = Operation::Done { id: id.clone() };
                LifecycleNotifier::once(super_addr, op, event)
            }
        }
    };
}
