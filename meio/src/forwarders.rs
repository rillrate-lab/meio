use crate::actor_runtime::Actor;
use crate::handlers::{ActionHandler, StreamItem};
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

pub(crate) struct InteractionForwarder<A: Actor> {
    pub address: Address<A>,
}

#[async_trait]
impl<A: Actor> LiteTask for InteractionForwarder<A> {
    type Output = ();

    async fn interruptable_routine(mut self) -> Result<Self::Output, Error> {
        todo!();
    }
}
