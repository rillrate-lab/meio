use crate::actor_runtime::{Actor, Status};
use crate::compat::watch;
use crate::handlers::{Operation, TaskEliminated};
use crate::ids::{Id, IdOf};
use crate::lifecycle::{LifecycleNotifier, TaskDone};
use crate::linkage::Address;
use anyhow::Error;
use async_trait::async_trait;
use futures::{
    future::{select, Either, FusedFuture},
    Future, FutureExt,
};
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use uuid::Uuid;

/// Custom tag for `LiteTask`.
/// Attached to a runtime.
pub trait Tag: Send + 'static {}

impl Tag for () {}

/// Minimalistic actor that hasn't `Address`.
///
/// **Recommended** to implement sequences or intensive loops (routines).
#[async_trait]
pub trait LiteTask: Sized + Send + 'static {
    /// The result of a finished task.
    type Output: Send;

    /// Returns unique name of the `LiteTask`.
    /// Uses `Uuid` by default.
    fn name(&self) -> String {
        let uuid = Uuid::new_v4();
        format!("Task:{}({})", std::any::type_name::<Self>(), uuid)
    }

    /// Routine of the task that can contain loops.
    /// It can taks into accout provided receiver to implement graceful interruption.
    ///
    /// By default uses the following calling chain that you can override at any step:
    /// `routine` -> `interruptable_routine` -> `repeatable_routine` -> `retry_at` -> `retry_delay`
    async fn routine(self, mut stop: StopReceiver) -> Result<Self::Output, Error> {
        stop.or(self.interruptable_routine())
            .await
            .map_err(Error::from)
            // TODO: use `flatten` instead
            .and_then(std::convert::identity)
    }

    /// Routine that can be unconditionally interrupted.
    async fn interruptable_routine(mut self) -> Result<Self::Output, Error> {
        self.pre_repeatable_routine().await?;
        loop {
            let last_attempt = Instant::now();
            let routine_result = self.repeatable_routine().await;
            match routine_result {
                Ok(Some(output)) => {
                    break Ok(output);
                }
                Ok(None) => {
                    // Continue
                    self.routine_wait(last_attempt, true).await;
                }
                Err(err) => {
                    log::error!("Routine {} failed: {}", self.name(), err);
                    self.routine_wait(last_attempt, false).await;
                }
            }
        }
    }

    /// Called before `repeatable_routine` for initialization.
    /// The routine will be interrupted if this method failed.
    async fn pre_repeatable_routine(&mut self) -> Result<(), Error> {
        Ok(())
    }

    /// Routine that will be repeated till fail or success.
    ///
    /// To stop it you should return `Some(value)`.
    async fn repeatable_routine(&mut self) -> Result<Option<Self::Output>, Error> {
        Ok(None)
    }

    /// Check of the every intaration of a routine.
    async fn routine_wait(&mut self, _last_attempt: Instant, _succeed: bool) {
        let duration = Duration::from_secs(5);
        crate::compat::delay(duration).await
    }
}

pub(crate) fn spawn<T, S, M>(
    task: T,
    tag: M,
    supervisor: Option<Address<S>>,
    custom_name: Option<String>,
) -> TaskAddress<T>
where
    T: LiteTask,
    S: Actor + TaskEliminated<T, M>,
    M: Tag,
{
    let task_name = custom_name.unwrap_or_else(|| task.name());
    let id = Id::new(task_name);
    let (stop_sender, stop_receiver) = make_stop_channel(id.clone());
    let id_of = IdOf::<T>::new(id.clone());
    let done_notifier = {
        match supervisor {
            None => <dyn LifecycleNotifier<_>>::ignore(),
            Some(super_addr) => {
                //let event = TaskDone::new(id_of.clone());
                let op = Operation::Done { id };
                <dyn LifecycleNotifier<_>>::once(super_addr, op)
            }
        }
    };
    let runtime = LiteRuntime {
        id: id_of,
        task,
        done_notifier,
        stop_receiver,
        tag,
    };
    crate::compat::spawn_async(runtime.entrypoint());
    stop_sender
}

/// Just receives a stop signal.
pub trait StopSignal: Future<Output = ()> + FusedFuture + Send {}

impl<T> StopSignal for T where T: Future<Output = ()> + FusedFuture + Send {}

#[derive(Debug, Error)]
#[error("task interrupted by a signal")]
pub struct TaskStopped;

fn make_stop_channel<T>(id: Id) -> (TaskAddress<T>, StopReceiver) {
    let (tx, rx) = watch::channel(Status::Alive);
    let stop_sender = StopSender { tx: Arc::new(tx) };
    let address = TaskAddress {
        id: IdOf::new(id),
        stop_sender,
    };
    let receiver = StopReceiver { status: rx };
    (address, receiver)
}

/// Contains a sender to update a status of a task.
#[derive(Debug, Clone)]
pub struct StopSender {
    tx: Arc<watch::Sender<Status>>,
}

impl StopSender {
    /// Send a stop signal to the task.
    pub fn stop(&self) -> Result<(), Error> {
        self.tx.send(Status::Stop).map_err(Error::from)
    }
}

impl<T> From<TaskAddress<T>> for StopSender {
    fn from(addr: TaskAddress<T>) -> Self {
        addr.stop_sender
    }
}

/// Address of a spawned task.
///
/// It can be used to interrupt the task.
#[derive(Debug)]
pub struct TaskAddress<T> {
    id: IdOf<T>,
    stop_sender: StopSender,
}

impl<T> Clone for TaskAddress<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id(),
            stop_sender: self.stop_sender.clone(),
        }
    }
}

impl<T> TaskAddress<T> {
    /// Id of the task.
    pub fn id(&self) -> IdOf<T> {
        self.id.clone()
    }

    /// Send a stop signal to the task.
    pub fn stop(&self) -> Result<(), Error> {
        self.stop_sender.stop()
    }
}

impl<T: LiteTask> PartialEq for TaskAddress<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

impl<T: LiteTask> Eq for TaskAddress<T> {}

impl<T: LiteTask> Hash for TaskAddress<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

/// Contains a receiver with a status of a task.
#[derive(Debug, Clone)]
pub struct StopReceiver {
    status: watch::Receiver<Status>,
}

impl StopReceiver {
    /// Returns `true` is the task can be alive.
    pub fn is_alive(&self) -> bool {
        *self.status.borrow() == Status::Alive
    }

    /// Returns a `Future` that completed when `Done` signal received.
    pub fn into_future(self) -> Pin<Box<dyn StopSignal>> {
        Box::pin(just_done(self.status).fuse())
    }

    /// Tries to execute provided `Future` to completion if the `ShutdownReceived`
    /// won't interrupted during that time.
    pub async fn or<Fut>(&mut self, fut: Fut) -> Result<Fut::Output, TaskStopped>
    where
        Fut: Future,
    {
        let fut = Box::pin(fut);
        let either = select(self.clone().into_future(), fut).await;
        match either {
            Either::Left((_done, _rem_fut)) => Err(TaskStopped),
            Either::Right((output, _rem_fut)) => Ok(output),
        }
    }
}

async fn just_done(mut status: watch::Receiver<Status>) {
    while status.changed().await.is_ok() {
        if status.borrow().is_done() {
            break;
        }
    }
}

/// An error that can happen in a task.
#[derive(Debug, Error)]
pub enum TaskError {
    /// The task was interrupted.
    #[error("task was interrupted")]
    Interrupted,
    /// Task had any other error.
    #[error("task failed: {0}")]
    Other(Error),
}

impl TaskError {
    /// If the task was interrupted it returs `true`.
    pub fn is_interrupted(&self) -> bool {
        matches!(self, Self::Interrupted)
    }

    /// Converts the error into other error value if
    /// the task wasn't interrupted.
    pub fn into_other(self) -> Option<Error> {
        match self {
            Self::Interrupted => None,
            Self::Other(err) => Some(err),
        }
    }

    #[deprecated(since = "0.86.2", note = "Use ordinary `match` for checking.")]
    /// Moves interrupted flag from `err` to the optional result.
    pub fn swap<T>(result: Result<T, Self>) -> Result<Option<T>, Error> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(Self::Interrupted) => Ok(None),
            Err(Self::Other(err)) => Err(err),
        }
    }
}

impl From<Error> for TaskError {
    fn from(error: Error) -> Self {
        match error.downcast::<TaskStopped>() {
            Ok(_stopped) => Self::Interrupted,
            Err(other) => Self::Other(other),
        }
    }
}

struct LiteRuntime<T: LiteTask, M: Tag> {
    id: IdOf<T>,
    task: T,
    // TODO: Add T::Output to TaskDone
    done_notifier: Box<dyn LifecycleNotifier<TaskDone<T, M>>>,
    stop_receiver: StopReceiver,
    tag: M,
}

impl<T: LiteTask, M: Tag> LiteRuntime<T, M> {
    async fn entrypoint(mut self) {
        log::info!("Task started: {:?}", self.id);
        let res = self
            .task
            .routine(self.stop_receiver)
            .await
            .map_err(TaskError::from);
        if let Err(err) = res.as_ref() {
            if !err.is_interrupted() {
                // Can't downcast. It was a real error.
                log::error!("Task failed: {:?}: {}", self.id, err);
            }
        }
        log::info!("Task finished: {:?}", self.id);
        // TODO: Add result to it
        let task_done = TaskDone::new(self.id.clone(), self.tag, res);
        if let Err(err) = self.done_notifier.notify(task_done) {
            log::error!(
                "Can't send done notification from the task {:?}: {}",
                self.id,
                err
            );
        }
    }
}
