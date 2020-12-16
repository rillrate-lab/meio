//! This module contains useful tasks that you can attach to an `Actor`.

use crate::handlers::Action;
use crate::linkage::{ActionPerformer, ActionRecipient};
use crate::lite_runtime::{LiteTask, ShutdownReceiver};
use anyhow::Error;
use async_trait::async_trait;
use futures::{select, FutureExt, StreamExt};
use std::time::Duration;
use tokio::time::{interval, Instant};

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
    async fn routine(mut self, mut signal: ShutdownReceiver) -> Result<(), Error> {
        let mut ticks = interval(self.duration).map(Tick).fuse();

        let recipient = &mut self.recipient;

        loop {
            select! {
                _ = signal => {
                    break;
                }
                tick = ticks.select_next_some() => {
                    recipient.act(tick).await?;
                }
            }
        }

        Ok(())
    }
}
