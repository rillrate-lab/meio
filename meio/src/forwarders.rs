use crate::actor_runtime::Actor;
use crate::handlers::{ActionHandler, Interact, Interaction, StreamItem};
use crate::linkage::Address;
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

pub(crate) struct InteractionForwarder<I: Interaction, A: Actor> {
    // TODO: Use `ActionPerformer` here
    address: Address<A>,
    event: Option<I>,
}

impl<I, A> InteractionForwarder<I, A>
where
    I: Interaction,
    A: ActionHandler<Interact<I>>,
{
    pub fn new(address: Address<A>, event: I) -> Self {
        Self {
            address,
            event: Some(event),
        }
    }
}

#[async_trait]
impl<I, A> LiteTask for InteractionForwarder<I, A>
where
    I: Interaction,
    A: ActionHandler<Interact<I>>,
{
    type Output = I::Output;

    async fn interruptable_routine(mut self) -> Result<Self::Output, Error> {
        let request = self
            .event
            .take()
            .expect("InteractionForwarder called twice");
        self.address.interact_and_wait(request).await
    }
}
