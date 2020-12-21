use crate::actor_runtime::Actor;
use crate::handlers::{ActionHandler, Operation};
use crate::ids::{Id, IdOf};
use crate::lifecycle::{LifecycleNotifier, TaskDone};
use crate::linkage::Address;
use crate::lite_runtime::{stop_channel, LiteTask, StopReceiver, StopSender, TaskStopped};

// TODO: Spawn lite task with no supervisor (like Actors can do)
pub(crate) fn spawn<T, S>(task: T, supervisor: Option<Address<S>>) -> StopSender
where
    T: LiteTask,
    S: Actor + ActionHandler<TaskDone<T>>,
{
    let id = Id::of_task(&task);
    let (stop_sender, stop_receiver) = stop_channel(id.clone());
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
    let runtime = LiteRuntime {
        id,
        task,
        done_notifier,
        stop_receiver,
    };
    tokio::spawn(runtime.entrypoint());
    stop_sender
}

struct LiteRuntime<T: LiteTask> {
    // TODO: Use `IdOf` here
    id: Id,
    task: T,
    done_notifier: Box<dyn LifecycleNotifier>,
    stop_receiver: StopReceiver,
}

impl<T: LiteTask> LiteRuntime<T> {
    async fn entrypoint(mut self) {
        log::info!("Task started: {:?}", self.id);
        if let Err(err) = self.task.routine(self.stop_receiver).await {
            if let Err(real_err) = err.downcast::<TaskStopped>() {
                // Can't downcast. It was a real error.
                log::error!("Task failed: {:?}: {}", self.id, real_err);
            }
        }
        log::info!("Task finished: {:?}", self.id);
        if let Err(err) = self.done_notifier.notify() {
            log::error!(
                "Can't send done notification from the task {:?}: {}",
                self.id,
                err
            );
        }
    }
}
