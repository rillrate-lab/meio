use crate::actor_runtime::{Actor, Context};
use crate::handlers::{
    ActionHandler, Consumer, InstantAction, InstantActionHandler, Interaction, StreamItem,
};
use crate::linkage::{Address, InteractionRecipient};
use crate::lite_runtime::LiteTask;
use anyhow::Error;
use async_trait::async_trait;
use futures::{stream::ReadyChunks, Stream, StreamExt};

// TODO: Change it to `LiteTask`
// TODO: Await results of `LiteTask`s inside a main `actor's routine loop`
/// This worker receives items from a stream and send them as actions
/// into an `Actor`.
pub(crate) struct StreamForwarder<S: Stream, A: Actor> {
    stream: ReadyChunks<S>,
    address: Address<A>,
}

impl<S, A> StreamForwarder<S, A>
where
    S: Stream,
    A: Actor,
{
    pub fn new(stream: S, address: Address<A>) -> Self {
        Self {
            stream: stream.ready_chunks(16),
            address,
        }
    }
}

#[async_trait]
impl<S, A> LiteTask for StreamForwarder<S, A>
where
    S: Stream + Unpin + Send + 'static,
    S::Item: Send + 'static,
    A: Actor + ActionHandler<StreamItem<S::Item>>,
{
    type Output = ();

    async fn interruptable_routine(mut self) -> Result<Self::Output, Error> {
        while let Some(item) = self.stream.next().await {
            let action = StreamItem::Chunk(item);
            if let Err(err) = self.address.act(action).await {
                log::error!(
                    "Can't send an event to {:?} form a background stream: {}. Breaking...",
                    self.address.id(),
                    err
                );
                break;
            }
        }
        Ok(())
    }
}

/// Allows to attach the specific termination group to an attached stream.
pub trait StreamGroup<S>: Actor {
    /// The group of the task that works for gathering a stream.
    fn stream_group(&self, stream: &S) -> Self::GroupBy;
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
    T: StreamGroup<S> + Consumer<S::Item>,
    S: Stream + Unpin + Send + 'static,
    S::Item: Send,
{
    async fn handle(&mut self, msg: AttachStream<S>, ctx: &mut Context<Self>) -> Result<(), Error> {
        let stream = msg.stream;
        let group = self.stream_group(&stream);
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
