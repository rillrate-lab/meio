use crate::actor_runtime::Actor;
use crate::handlers::{ActionHandler, Interact, Interaction, StreamItem};
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
    pub stream: ReadyChunks<S>,
    pub address: Address<A>,
}

impl<S, A> StreamForwarder<S, A>
where
    S: Stream + Unpin,
    S::Item: Send + 'static,
    A: Actor + ActionHandler<StreamItem<S::Item>>,
{
    // TODO: Change to `LiteTask`!
    pub async fn entrypoint(mut self) {
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
