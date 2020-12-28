//! This module contains useful tasks that you can attach to an `Actor`.

use crate::actor_runtime::Actor;
use crate::handlers::{Action, ActionHandler};
use crate::linkage::Address;
use crate::lite_runtime::LiteTask;
use anyhow::Error;
use async_trait::async_trait;
use std::time::{Duration, Instant};

/// Any `Address` of an `Actor` that can receive `Tick` actions.
#[async_trait]
trait TickRecipient: Send + 'static {
    /// Send `Tick` to an `Actor`.
    async fn tick(&mut self) -> Result<(), Error>;
}

#[async_trait]
impl<T> TickRecipient for Address<T>
where
    T: Actor + ActionHandler<Tick>,
{
    async fn tick(&mut self) -> Result<(), Error> {
        let tick = Tick(Instant::now());
        self.act(tick).await
    }
}

/// The lite task that sends ticks to a `Recipient`.
//#[derive(Debug)]
pub struct HeartBeat {
    duration: Duration,
    recipient: Box<dyn TickRecipient>,
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
        self.recipient.tick().await
    }

    fn retry_delay(&self, _last_attempt: Instant) -> Duration {
        self.duration
    }
}
