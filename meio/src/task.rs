//! This module contains useful tasks that you can attach to an `Actor`.

use crate::actor_runtime::Actor;
use crate::handlers::{Action, ActionHandler};
use crate::linkage::{ActionRecipient, Address};
use crate::lite_runtime::LiteTask;
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

    async fn repeatable_routine(&mut self) -> Result<Self::Output, Error> {
        // IMPORTANT: Don't use `schedule` to avoid late beats: when the task was canceled,
        // but teh scheduled messages still remained in the actor's queue.
        let tick = Tick(Instant::now());
        self.recipient.act(tick).await
    }

    fn retry_delay(&self, _last_attempt: Instant) -> Duration {
        self.duration
    }
}
