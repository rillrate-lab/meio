//! This module contains useful tasks that you can attach to an `Actor`.

use crate::handlers::Action;
use crate::linkage::{ActionPerformer, ActionRecipient};
use crate::lite_runtime::LiteTask;
use anyhow::Error;
use async_trait::async_trait;
use std::time::{Duration, Instant};

/// The lite task that sends ticks to a `Recipient`.
#[derive(Debug)]
pub struct HeartBeat {
    duration: Duration,
    recipient: ActionRecipient<Tick>,
}

impl HeartBeat {
    /// Creates a new `HeartBeat` lite task.
    pub fn new<T>(duration: Duration, address: T) -> Self
    where
        ActionRecipient<Tick>: From<T>,
    {
        Self {
            duration,
            recipient: address.into(),
        }
    }
}

/// `Tick` value that sent by `HeartBeat` lite task.
#[derive(Debug)]
pub struct Tick(pub Instant);

impl Action for Tick {}

#[async_trait]
impl LiteTask for HeartBeat {
    async fn repeatable_routine(&mut self) -> Result<(), Error> {
        let tick = Tick(Instant::now());
        // IMPORTANT: Don't use `schedule` to avoid late beats: when the task was canceled,
        // but teh scheduled messages still remained in the actor's queue.
        self.recipient.act(tick).await
    }

    fn retry_delay(&self, _last_attempt: Instant) -> Duration {
        self.duration
    }
}
