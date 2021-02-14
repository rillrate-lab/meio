use crate::actor_runtime::{Actor, Context};
use crate::handlers::{Consumer, InstantAction, InstantActionHandler, Interaction, StreamItem};
use crate::linkage::{ActionRecipient, InteractionRecipient};
use crate::lite_runtime::LiteTask;
use anyhow::Error;
use async_trait::async_trait;
use futures::{stream::ReadyChunks, Stream, StreamExt};

/// This worker receives items from a stream and send them as actions
/// into an `Actor`.
pub(crate) struct StreamForwarder<S: Stream> {
    stream: ReadyChunks<S>,
    recipient: Box<dyn ActionRecipient<StreamItem<S::Item>>>,
}

impl<S> StreamForwarder<S>
where
    S: Stream,
    S::Item: Send + 'static,
{
    pub fn new(stream: S, recipient: impl ActionRecipient<StreamItem<S::Item>>) -> Self {
        Self {
            stream: stream.ready_chunks(16),
            recipient: Box::new(recipient),
        }
    }
}

#[async_trait]
impl<S> LiteTask for StreamForwarder<S>
where
    S: Stream + Unpin + Send + 'static,
    S::Item: Send + 'static,
{
    type Output = ();

    async fn interruptable_routine(mut self) -> Result<Self::Output, Error> {
        while let Some(item) = self.stream.next().await {
            let action = StreamItem::Chunk(item);
            self.recipient.act(action).await?;
        }
        let action = StreamItem::Done;
        self.recipient.act(action).await?;
        Ok(())
    }
}

/// Allows to attach the specific termination group to an attached stream.
pub trait StreamGroup<I>: Actor {
    /// The group of the task that works for gathering a stream.
    fn stream_group(&self) -> Self::GroupBy;
}

pub(crate) struct AttachStream<S> {
    stream: S,
}

impl<S> AttachStream<S> {
    pub fn new(stream: S) -> Self {
        Self { stream }
    }
}

impl<S> InstantAction for AttachStream<S> where S: Stream + Send + 'static {}

#[async_trait]
impl<T, S> InstantActionHandler<AttachStream<S>> for T
where
    T: StreamGroup<S::Item> + Consumer<S::Item>,
    S: Stream + Unpin + Send + 'static,
    S::Item: Send,
{
    async fn handle(&mut self, msg: AttachStream<S>, ctx: &mut Context<Self>) -> Result<(), Error> {
        let stream = msg.stream;
        let group = self.stream_group();
        ctx.attach(stream, group);
        Ok(())
    }
}

pub(crate) struct InteractionForwarder<I: Interaction> {
    recipient: Box<dyn InteractionRecipient<I>>,
    event: Option<I>,
}

impl<I> InteractionForwarder<I>
where
    I: Interaction,
{
    pub fn new(recipient: impl InteractionRecipient<I>, event: I) -> Self {
        Self {
            recipient: Box::new(recipient),
            event: Some(event),
        }
    }
}

#[async_trait]
impl<I> LiteTask for InteractionForwarder<I>
where
    I: Interaction,
{
    type Output = I::Output;

    async fn interruptable_routine(mut self) -> Result<Self::Output, Error> {
        let request = self
            .event
            .take()
            .expect("InteractionForwarder called twice");
        self.recipient.interact_and_wait(request).await
    }
}
