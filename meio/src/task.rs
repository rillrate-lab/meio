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

/// The lite task that sends ticks to a `Recipient`.
#[derive(Debug)]
pub struct HeartBeat {
    duration: Duration,
    recipient: Box<dyn ActionRecipient<Tick>>,
}

impl HeartBeat {
    /// Creates a new `HeartBeat` lite task.
    pub fn new<T>(duration: Duration, address: Address<T>) -> Self
    where
        T: Actor + ActionHandler<Tick>,
    {
        Self {
            duration,
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

    fn retry_delay(&self, _last_attempt: Instant) -> Duration {
        self.duration
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
