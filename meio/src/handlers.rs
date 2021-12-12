//! Handler traits for incoming messages.
//!
//! This module contains the `Envelope` that allows
//! calling methods of actors related to a specific
//! incoming message.

use crate::actor_runtime::{Actor, Context};
use crate::forwarders::StreamForwarder;
use crate::ids::{Id, IdOf};
use crate::lifecycle;
use crate::linkage::{ActionRecipient, Address};
use crate::lite_runtime::{LiteTask, Tag, TaskError};
use anyhow::Error;
use async_trait::async_trait;
use futures::channel::oneshot;
use futures::Stream;
use std::convert::identity;
use std::fmt;
use std::marker::PhantomData;
use std::time::Instant;

/// `Parcel` packs any message for an `Actor`
/// for further processing that can be done later.
///
/// Also it useful to send multi-typed actions using ordinary channels.
pub struct Parcel<A: Actor> {
    pub(crate) operation: Operation,
    pub(crate) envelope: Envelope<A>,
}

impl<A: Actor> Parcel<A> {
    /// Creates a new `Parcel`.
    pub fn pack<I>(input: I) -> Self
    where
        A: InstantActionHandler<I>,
        I: InstantAction,
    {
        Self::new(Operation::Forward, input)
    }

    /// Internal method.
    pub(crate) fn new<I>(operation: Operation, input: I) -> Self
    where
        A: InstantActionHandler<I>,
        I: InstantAction,
    {
        Self {
            operation,
            envelope: Envelope::instant(input),
        }
    }

    pub(crate) fn from_envelope(envelope: Envelope<A>) -> Self {
        Self {
            operation: Operation::Forward,
            envelope,
        }
    }
}

pub(crate) struct Envelope<A: Actor> {
    handler: Box<dyn Handler<A>>,
}

impl<A: Actor> fmt::Debug for Envelope<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Envelope").finish()
    }
}

impl<A: Actor> Envelope<A> {
    pub(crate) async fn handle(
        &mut self,
        actor: &mut A,
        ctx: &mut Context<A>,
    ) -> Result<(), Error> {
        self.handler.handle(actor, ctx).await
    }

    pub(crate) fn from_handler(handler: impl Handler<A>) -> Self {
        Self {
            handler: Box::new(handler),
        }
    }

    // TODO: Is it posiible to use `handle` method directly and drop this one?
    /// Creates an `Envelope` for `Action`.
    pub(crate) fn new<I>(input: I) -> Self
    where
        A: ActionHandler<I>,
        I: Action,
    {
        let handler = ActionHandlerImpl { input: Some(input) };
        Self {
            handler: Box::new(handler),
        }
    }

    /// Creates an `Envelope` for `InstantAction`.
    pub(crate) fn instant<I>(input: I) -> Self
    where
        A: InstantActionHandler<I>,
        I: InstantAction,
    {
        let handler = InstantActionHandlerImpl { input: Some(input) };
        Self {
            handler: Box::new(handler),
        }
    }
}

// TODO: Consider renaming to attached action
#[derive(Clone)]
pub(crate) enum Operation {
    // TODO: Awake, Interrupt, also can be added here!
    Done {
        id: Id,
    },
    /// Just process it with high-priority.
    Forward,
    /// The operation to schedule en action handling at the specific time.
    ///
    /// `Instant` used to avoid delays for sending and processing this `Operation` message.
    ///
    /// It can't be sent as normal priority, because the message has to be scheduled as
    /// soon as possible to reduce influence of the ordinary processing queue to execution time.
    Schedule {
        deadline: Instant,
    },
}

/// The priority of the sendig event.
#[derive(Debug, Clone, Copy)]
pub enum Priority {
    /// Normal priority queue
    Normal,
    /// High priority queue
    Instant,
}

impl Default for Priority {
    fn default() -> Self {
        Self::Normal
    }
}

/// Internal `Handler` type that used by `Actor`'s routine to execute
/// `ActionHandler` or `InteractionHandler`.
///
/// It has `'static` lifetime, because every handler is boxed when sent to an `Actor`.
#[async_trait]
pub trait Handler<A: Actor>: Send + 'static {
    /// Returns priority of a handler.
    fn priority(&self) -> Priority {
        Priority::Normal
    }

    /// Main method that expects a mutable reference to `Actor` that
    /// will be used by implementations to handle messages.
    async fn handle(&mut self, actor: &mut A, _ctx: &mut Context<A>) -> Result<(), Error>;
}

/// `Action` type can be sent to an `Actor` that implements
/// `ActionHandler` for that message type.
pub trait Action: Send + 'static {}

/// Type of `Handler` to process incoming messages in one-shot style.
#[async_trait]
pub trait ActionHandler<I: Action>: Actor {
    /// Asyncronous method that receives incoming message.
    async fn handle(&mut self, input: I, _ctx: &mut Context<Self>) -> Result<(), Error>;
}

struct ActionHandlerImpl<I> {
    input: Option<I>,
}

#[async_trait]
impl<A, I> Handler<A> for ActionHandlerImpl<I>
where
    A: ActionHandler<I>,
    I: Action,
{
    fn priority(&self) -> Priority {
        Priority::Normal
    }

    async fn handle(&mut self, actor: &mut A, ctx: &mut Context<A>) -> Result<(), Error> {
        let input = self.input.take().expect("action handler called twice");
        actor.handle(input, ctx).await
    }
}

/// The high-priority action.
pub trait InstantAction: Send + 'static {}

/// Type of `Handler` to process high-priority messages.
#[async_trait]
pub trait InstantActionHandler<I: InstantAction>: Actor {
    /// Asyncronous method that receives incoming message.
    async fn handle(&mut self, input: I, _ctx: &mut Context<Self>) -> Result<(), Error>;
}

struct InstantActionHandlerImpl<I> {
    input: Option<I>,
}

#[async_trait]
impl<A, I> Handler<A> for InstantActionHandlerImpl<I>
where
    A: InstantActionHandler<I>,
    I: InstantAction,
{
    fn priority(&self) -> Priority {
        Priority::Instant
    }

    async fn handle(&mut self, actor: &mut A, ctx: &mut Context<A>) -> Result<(), Error> {
        let input = self
            .input
            .take()
            .expect("instant action handler called twice");
        actor.handle(input, ctx).await
    }
}

/// The synchronous action (useful for rendering routines).
pub trait SyncAction {}

/// Handler of sync actions.
pub trait SyncActionHandler<I: SyncAction>: Actor {
    /// The method called in synchronous context.
    fn handle(&self) -> Result<(), Error>;
}

/// Implements an interaction with an `Actor`.
#[async_trait]
pub trait InteractionHandler<I: Interaction>: Actor {
    /// Asyncronous method that receives incoming message.
    async fn handle(&mut self, input: I, _ctx: &mut Context<Self>) -> Result<I::Output, Error>;
}

#[async_trait]
impl<T, I> ActionHandler<Interact<I>> for T
where
    T: InteractionHandler<I>,
    I: Interaction,
{
    async fn handle(&mut self, input: Interact<I>, ctx: &mut Context<Self>) -> Result<(), Error> {
        let res = InteractionHandler::handle(self, input.request, ctx).await;
        let send_res = input.responder.send(res);
        // TODO: How to improve that???
        match send_res {
            Ok(()) => Ok(()),
            Err(Ok(_)) => Err(Error::msg(
                "Can't send the successful result of interaction",
            )),
            Err(Err(err)) => Err(err),
        }
    }
}

/// The alias to sender of an interaction result.
pub type InteractionResponder<T> = oneshot::Sender<Result<T, Error>>;

impl<T> Tag for InteractionResponder<T> where T: Send + 'static {}

/// The wrapper for an interaction request that keeps a request and the
/// channel for sending a response.
pub struct Interact<T: Interaction> {
    /// Interaction request.
    pub request: T,
    /// The responder to send a result of an interaction.
    pub responder: InteractionResponder<T::Output>,
}

impl<T: Interaction> Action for Interact<T> {}

/// Interaction message to an `Actor`.
/// Interactions can't be high-priority (instant), because it can block vital runtime handlers.
///
/// Long running interaction will block the actor's routine for a long time and the app can
/// be blocked by `Address::interact` method call. To avoid this issue you have:
///
/// 1. Use `ActionHandler` with `Interact` wrapper as a message to control manually
/// when a response will be send to avoid blocking of an `Actor` that performs long running
/// interaction.
///
/// 2. Use `interaction` method and send a response from a `LiteTask` to an `InteractionResponse`
/// handler of a caller.
///
pub trait Interaction: Send + 'static {
    /// The result of the `Interaction` that will be returned by `InteractionHandler`.
    type Output: Send + 'static;
}

/// Interaction task that can be awaited or aattached to a `Context`.
pub struct InteractionTask<I: Interaction> {
    recipient: Box<dyn ActionRecipient<Interact<I>>>,
    request: I,
}

impl<I: Interaction> InteractionTask<I> {
    pub(crate) fn new<T>(address: &Address<T>, request: I) -> Self
    where
        T: ActionHandler<Interact<I>>,
    {
        Self {
            recipient: Box::new(address.clone()),
            request,
        }
    }

    // TODO: impl `Future` instead of this
    /// Receive a value
    pub async fn recv(mut self) -> Result<I::Output, Error> {
        let (responder, rx) = oneshot::channel();
        let input = Interact {
            request: self.request,
            responder,
        };
        self.recipient.act(input)?;
        rx.await.map_err(Error::from).and_then(identity)
    }
}

#[async_trait]
impl<I> LiteTask for InteractionTask<I>
where
    I: Interaction,
{
    type Output = I::Output;

    fn log_target(&self) -> &str {
        "InteractionTask"
    }

    async fn interruptable_routine(mut self) -> Result<Self::Output, Error> {
        self.recv().await
    }
}

/// Represents initialization routine of an `Actor`.
#[async_trait]
pub trait StartedBy<A: Actor>: Actor {
    /// It's an initialization method of the `Actor`.
    async fn handle(&mut self, ctx: &mut Context<Self>) -> Result<(), Error>;
}

#[async_trait]
impl<T, S> InstantActionHandler<lifecycle::Awake<S>> for T
where
    T: StartedBy<S>,
    S: Actor,
{
    async fn handle(
        &mut self,
        _input: lifecycle::Awake<S>,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        StartedBy::handle(self, ctx).await
    }
}

/// The listener to an interruption signal.
#[async_trait]
pub trait InterruptedBy<A: Actor>: Actor {
    /// Called when the `Actor` terminated by another actor.
    ///
    /// In many cases you should prefer to call `ctx.shutdown()` here.
    async fn handle(&mut self, ctx: &mut Context<Self>) -> Result<(), Error>;
    // IMPORTANT! It has to be explicit! Don't add automatic implementation with shuttdown call.
}

#[async_trait]
impl<T, S> InstantActionHandler<lifecycle::Interrupt<S>> for T
where
    T: InterruptedBy<S>,
    S: Actor,
{
    async fn handle(
        &mut self,
        _input: lifecycle::Interrupt<S>,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        InterruptedBy::handle(self, ctx).await
    }
}

/// Termination signal handler for handling signals to terminatate the actor.
#[async_trait]
pub trait TerminatedBy<T>: Actor {
    /// The termination handling method.
    async fn handle(&mut self, ctx: &mut Context<Self>) -> Result<(), Error>;
}

/// An event for termination.
pub struct TerminateBy<T> {
    _ref: PhantomData<T>,
}

impl<T: 'static> Action for TerminateBy<T> {}

unsafe impl<T> Send for TerminateBy<T> {}

impl<T> TerminateBy<T> {
    pub(crate) fn new() -> Self {
        Self { _ref: PhantomData }
    }
}

#[async_trait]
impl<A, T> ActionHandler<TerminateBy<T>> for A
where
    A: TerminatedBy<T>,
    T: 'static,
{
    async fn handle(
        &mut self,
        _input: TerminateBy<T>,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        TerminatedBy::handle(self, ctx).await
    }
}

/// Listens for spawned actors finished.
#[async_trait]
pub trait Eliminated<A: Actor>: Actor {
    /// Called when the `Actor` finished.
    async fn handle(&mut self, id: IdOf<A>, ctx: &mut Context<Self>) -> Result<(), Error>;
}

#[async_trait]
impl<T, C> InstantActionHandler<lifecycle::Done<C>> for T
where
    T: Eliminated<C>,
    C: Actor,
{
    async fn handle(
        &mut self,
        done: lifecycle::Done<C>,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        Eliminated::handle(self, done.id, ctx).await
    }
}

/// Listens for spawned tasks finished.
#[async_trait]
pub trait TaskEliminated<T: LiteTask, M: Tag>: Actor {
    /// Called when the `Task` finished.
    async fn handle(
        &mut self,
        id: IdOf<T>,
        tag: M,
        result: Result<T::Output, TaskError>,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error>;
}

#[async_trait]
impl<T, C, M> InstantActionHandler<lifecycle::TaskDone<C, M>> for T
where
    T: TaskEliminated<C, M>,
    C: LiteTask,
    M: Tag,
{
    async fn handle(
        &mut self,
        done: lifecycle::TaskDone<C, M>,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        TaskEliminated::handle(self, done.id, done.tag, done.result, ctx).await
    }
}

/// Independent interaction results listener. It necessary to avoid blocking.
#[async_trait]
pub trait InteractionDone<I: Interaction, M: Tag>: Actor {
    /// Handling of the interaction result.
    async fn handle(
        &mut self,
        tag: M,
        output: I::Output,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error>;

    /// Called when interaction failed.
    async fn failed(
        &mut self,
        _tag: M,
        err: TaskError,
        _ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        if let Some(err) = err.into_other() {
            log::error!("Interaction failed: {}", err);
        }
        Ok(())
    }
}

#[async_trait]
impl<T, I, M> TaskEliminated<InteractionTask<I>, M> for T
where
    T: InteractionDone<I, M>,
    I: Interaction,
    M: Tag,
{
    async fn handle(
        &mut self,
        _id: IdOf<InteractionTask<I>>,
        tag: M,
        result: Result<I::Output, TaskError>,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        match result {
            Ok(output) => InteractionDone::handle(self, tag, output, ctx).await,
            Err(err) => InteractionDone::failed(self, tag, err, ctx).await,
        }
    }
}

pub(crate) enum StreamItem<T> {
    Item(T),
    Done,
}

impl<T: Send + 'static> Action for StreamItem<T> {}

/// Represents a capability to receive message from a `Stream`.
#[async_trait]
pub trait Consumer<T: 'static>: Actor {
    /// The method called when the next item received from a `Stream`.
    ///
    /// If you need chunks here (group multiple items into chunks) than
    /// wrap your stream with `ready_chunks` method.
    async fn handle(&mut self, item: T, ctx: &mut Context<Self>) -> Result<(), Error>;

    /// When the stream consumed completely.
    ///
    /// Called after the last item in the stream only.
    async fn finished(&mut self, _ctx: &mut Context<Self>) -> Result<(), Error> {
        log::info!("Stream finished");
        Ok(())
    }

    /// The stream was failed.
    async fn task_failed(&mut self, err: TaskError, _ctx: &mut Context<Self>) -> Result<(), Error> {
        if let Some(err) = err.into_other() {
            log::error!("Consumer task failed: {}", err);
        }
        Ok(())
    }

    /// When the stream task was finished sucessfully.
    ///
    /// You should prefer to use `finished` instead of this, because it's service
    /// event with the high-priority and it can overtake ordinary stream items.
    async fn task_finished(&mut self, _ctx: &mut Context<Self>) -> Result<(), Error> {
        log::info!("Stream task finished");
        Ok(())
    }
}

#[async_trait]
impl<T, I> ActionHandler<StreamItem<I>> for T
where
    T: Consumer<I>,
    I: Send + 'static,
{
    async fn handle(&mut self, msg: StreamItem<I>, ctx: &mut Context<Self>) -> Result<(), Error> {
        match msg {
            StreamItem::Item(item) => Consumer::handle(self, item, ctx).await,
            StreamItem::Done => Consumer::finished(self, ctx).await,
        }
    }
}

#[async_trait]
impl<T, S, M> TaskEliminated<StreamForwarder<S>, M> for T
where
    T: Consumer<S::Item>,
    S: Stream + Unpin + Send + 'static,
    S::Item: Send,
    M: Tag,
{
    async fn handle(
        &mut self,
        _id: IdOf<StreamForwarder<S>>,
        // TODO: Support tag
        _tag: M,
        result: Result<(), TaskError>,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        match result {
            Ok(()) => Consumer::task_finished(self, ctx).await,
            Err(err) => Consumer::task_failed(self, err, ctx).await,
        }
    }
}

/// Controls where stream can be accepted to an `Actor` using `Address`.
pub trait StreamAcceptor<T>: Actor {
    /// The termination group used by `Address::attach` method.
    fn stream_group(&self) -> Self::GroupBy;
}

/* TODO: Delete. Not smart thing since `Consumer` stared to group items inito chunks.
/// Represents a capability to receive message from a `TryStream`.
#[async_trait]
pub trait TryConsumer<T>: Actor {
    /// `Error` value that can happen in a stream.
    type Error;
    /// The method called when the next item received from a `Stream`.
    async fn handle(&mut self, item: T, ctx: &mut Context<Self>) -> Result<(), Error>;
    /// The method called when the stream received an `Error`.
    async fn error(&mut self, error: Self::Error, ctx: &mut Context<Self>) -> Result<(), Error>;
}

#[async_trait]
impl<T, I> Consumer<Result<I, T::Error>> for T
where
    T: TryConsumer<I>,
    T::Error: Send,
    I: Send + 'static,
{
    async fn handle(
        &mut self,
        result: Result<I, T::Error>,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        match result {
            Ok(item) => TryConsumer::handle(self, item, ctx).await,
            Err(err) => TryConsumer::error(self, err, ctx).await,
        }
    }
}
*/

/// Used to wrap scheduled event.
pub(crate) struct ScheduledItem<T> {
    pub timestamp: Instant,
    pub item: T,
}

/// Priority never taken into account for `Scheduled` message,
/// but it has high-priority to show that it will be called as
/// soon as the deadline has reached.
impl<T: Send + 'static> InstantAction for ScheduledItem<T> {}

/// Represents reaction to a scheduled activity.
#[async_trait]
pub trait Scheduled<T>: Actor {
    /// The method called when the deadline has reached.
    async fn handle(
        &mut self,
        timestamp: Instant,
        item: T,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error>;
}

#[async_trait]
impl<T, I> InstantActionHandler<ScheduledItem<I>> for T
where
    T: Scheduled<I>,
    I: Send + 'static,
{
    async fn handle(
        &mut self,
        msg: ScheduledItem<I>,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        Scheduled::handle(self, msg.timestamp, msg.item, ctx).await
    }
}
