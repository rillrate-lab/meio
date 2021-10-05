//! This module contains useful tasks that you can attach to an `Actor`.

use crate::actor_runtime::{Actor, Context};
use crate::handlers::{Action, ActionHandler, TaskEliminated};
use crate::ids::IdOf;
use crate::linkage::{ActionRecipient, Address};
use crate::lite_runtime::LiteTask;
use crate::lite_runtime::TaskError;
use anyhow::Error;
use async_trait::async_trait;
use std::fmt::Debug;
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// Contorls the `HeartBeat` parameters.
pub struct HeartBeatHandle {
    duration: watch::Sender<Duration>,
}

impl HeartBeatHandle {
    /// Creates a new `HeartBeat` handle.
    pub fn new(duration: Duration) -> Self {
        let (tx, _rx) = watch::channel(duration);
        Self { duration: tx }
    }
    /// Change the `Duration` of the `HeartBeat`.
    pub fn update(&mut self, duration: Duration) -> Result<(), Error> {
        self.duration.send(duration)?;
        Ok(())
    }
}

/// The lite task that sends ticks to a `Recipient`.
#[derive(Debug)]
pub struct HeartBeat {
    duration: watch::Receiver<Duration>,
    recipient: Box<dyn ActionRecipient<Tick>>,
}

impl HeartBeat {
    /// Creates a new `HeartBeat` lite task.
    pub fn new<T>(duration: Duration, address: Address<T>) -> Self
    where
        T: Actor + ActionHandler<Tick>,
    {
        let handle = HeartBeatHandle::new(duration);
        Self::new_with_handle(&handle, address)
    }

    /// Creates a new `HeartBeat` lite task.
    /// With `HeartBeatHandle` to change `duration` on the fly.
    pub fn new_with_handle<T>(handle: &HeartBeatHandle, address: Address<T>) -> Self
    where
        T: Actor + ActionHandler<Tick>,
    {
        let rx = handle.duration.subscribe();
        Self {
            duration: rx,
            recipient: Box::new(address),
        }
    }
}

/// `Tick` value that sent by `HeartBeat` lite task.
#[derive(Debug)]
pub struct Tick(pub Instant);

impl Action for Tick {}

#[async_trait]
impl LiteTask for HeartBeat {
    type Output = ();

    async fn repeatable_routine(&mut self) -> Result<Option<Self::Output>, Error> {
        // IMPORTANT: Don't use `schedule` to avoid late beats: when the task was canceled,
        // but teh scheduled messages still remained in the actor's queue.
        let tick = Tick(Instant::now());
        self.recipient.act(tick).await?;
        // StopSender can be used to interrupt it
        Ok(None)
    }

    async fn routine_wait(&mut self, _last_attempt: Instant, _succeed: bool) {
        use tokio::time::{sleep_until, timeout_at};
        let now = Instant::now();
        loop {
            let duration = *self.duration.borrow();
            let instant = (now + duration).into();
            let res = timeout_at(instant, self.duration.changed()).await;
            match res {
                Ok(Ok(())) => {
                    // Changed. Double check the duration.
                    continue;
                }
                Ok(Err(_)) => {
                    // No sender that can change the duration. Just waiting.
                    sleep_until(instant).await;
                    break;
                }
                Err(_elapsed) => {
                    // Time elapsed. Repeat `routine` again
                    break;
                }
            }
        }
    }
}

/// Handler of heartbeat events and a task
#[async_trait]
pub trait OnTick: Actor {
    /// Called when tick received.
    ///
    /// Also `tick` will be called after lite task initialization.
    async fn tick(&mut self, tick: Tick, ctx: &mut Context<Self>) -> Result<(), Error>;
    /// Called when the heartbeat task finished or interrupted.
    async fn done(&mut self, ctx: &mut Context<Self>) -> Result<(), Error>;
}

#[async_trait]
impl<T> ActionHandler<Tick> for T
where
    T: OnTick,
{
    async fn handle(&mut self, tick: Tick, ctx: &mut Context<Self>) -> Result<(), Error> {
        OnTick::tick(self, tick, ctx).await
    }
}

// TODO: Support custom tags.
#[async_trait]
impl<T> TaskEliminated<HeartBeat, ()> for T
where
    T: OnTick,
{
    async fn handle(
        &mut self,
        _id: IdOf<HeartBeat>,
        _tag: (),
        _result: Result<(), TaskError>,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        OnTick::done(self, ctx).await
    }
}
