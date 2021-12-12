use crate::actor_runtime::Context;
use crate::handlers::{Consumer, InstantAction, InstantActionHandler, StreamAcceptor, StreamItem};
use crate::linkage::ActionRecipient;
use crate::lite_runtime::{LiteTask, Tag};
use anyhow::Error;
use async_trait::async_trait;
use futures::{Stream, StreamExt};

/// This worker receives items from a stream and send them as actions
/// into an `Actor`.
pub(crate) struct StreamForwarder<S: Stream> {
    stream: S,
    recipient: Box<dyn ActionRecipient<StreamItem<S::Item>>>,
}

/// If you need chunks use `ready_chunk` on the stream before.
impl<S> StreamForwarder<S>
where
    S: Stream,
    S::Item: Send + 'static,
{
    pub fn new(stream: S, recipient: impl ActionRecipient<StreamItem<S::Item>>) -> Self {
        Self {
            stream,
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

    fn log_target(&self) -> &str {
        "StreamForwarder"
    }

    async fn interruptable_routine(mut self) -> Result<Self::Output, Error> {
        while let Some(item) = self.stream.next().await {
            let action = StreamItem::Item(item);
            self.recipient.act(action)?;
        }
        let action = StreamItem::Done;
        self.recipient.act(action)?;
        Ok(())
    }
}

pub(crate) struct AttachStream<S, M> {
    stream: S,
    tag: M,
}

impl<S, M> AttachStream<S, M> {
    pub fn new(stream: S, tag: M) -> Self {
        Self { stream, tag }
    }
}

impl<S, M> InstantAction for AttachStream<S, M>
where
    S: Stream + Send + 'static,
    M: Tag,
{
}

#[async_trait]
impl<T, S, M> InstantActionHandler<AttachStream<S, M>> for T
where
    T: Consumer<S::Item> + StreamAcceptor<S::Item>,
    S: Stream + Unpin + Send + 'static,
    S::Item: Send,
    M: Tag,
{
    async fn handle(
        &mut self,
        msg: AttachStream<S, M>,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        let stream = msg.stream;
        let group = self.stream_group();
        ctx.attach(stream, msg.tag, group);
        Ok(())
    }
}
