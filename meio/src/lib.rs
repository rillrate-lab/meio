//! `meio` - lightweight async actor framework for Rust.
//! The main benefit of this framework is that gives you
//! tiny extension over `tokio` with full control.
//!
//! It's designed for professional applications where you
//! can't have strong restrictions and have keep flexibility.
//!
//! Also this crate has zero-cost runtime. It just calls
//! `async` method of your type.

#![warn(missing_docs)]
#![recursion_limit = "256"]
#![feature(generators)]
#![feature(option_expect_none)]

mod actor_runtime;
mod lite_runtime;

pub mod address;
pub mod channel;
pub mod handlers;
pub mod performers;
pub mod recipients;
pub mod signal;
pub mod task;
pub mod terminator;

pub use actor_runtime::{Actor, Context};
pub use address::Address;
pub use channel::{Controller, Status, Supervisor};
use channel::{Operator, Signal};
use handlers::Envelope;
pub use handlers::{Action, ActionHandler, Interaction, InteractionHandler};
pub use lite_runtime::{LiteStatus, LiteTask, ShutdownReceiver};
pub use performers::{ActionPerformer, InteractionPerformer};
pub use recipients::{ActionRecipient, InteractionRecipient};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
pub use terminator::{Stage, TerminationProgress, Terminator};

/// Unique Id of Actor's runtime that used to identify
/// all senders for that actor.
#[derive(Clone)]
pub struct Id(Arc<String>);

impl Id {
    /// Generated new `Id` for `Actor`.
    fn of_actor<T: Actor>(entity: &T) -> Self {
        let name = entity.name();
        Self(Arc::new(name))
    }

    /// Generated new `Id` for `Actor`.
    fn of_task<T: LiteTask>(entity: &T) -> Self {
        let name = entity.name();
        Self(Arc::new(name))
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.as_ref().fmt(f)
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple(self.0.as_ref()).finish()
    }
}

impl PartialEq<Id> for Id {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Id {}

impl Hash for Id {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.as_ref().hash(state);
    }
}

impl AsRef<str> for Id {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

/// Returns a `Link` to an `Actor`.
/// `Link` is a convenient concept for creating wrappers for
/// `Address` that provides methods instead of using message types
/// directly. It allows also to use private message types opaquely.
pub trait Link<T> {
    /// Gets a `Link` to an `Actor`.
    fn link(&self) -> T;
}

impl<T, A> Link<T> for Address<A>
where
    T: From<Address<A>>,
    A: Actor,
{
    fn link(&self) -> T {
        T::from(self.clone())
    }
}

impl<T, I> Link<T> for ActionRecipient<I>
where
    T: From<ActionRecipient<I>>,
    I: Action,
{
    fn link(&self) -> T {
        T::from(self.clone())
    }
}

impl<T, I> Link<T> for InteractionRecipient<I>
where
    T: From<InteractionRecipient<I>>,
    I: Interaction,
{
    fn link(&self) -> T {
        T::from(self.clone())
    }
}

// %%%%%%%%%%%%%%%%%%%%%% TESTS %%%%%%%%%%%%%%%%%%%%%

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Error;
    use async_trait::async_trait;
    use futures::stream;
    use std::time::Duration;
    use tokio::time::delay_for;

    struct MyActor;

    struct MsgOne;

    impl Action for MsgOne {}

    struct MsgTwo;

    impl Interaction for MsgTwo {
        type Output = u8;
    }

    #[async_trait]
    impl Actor for MyActor {
        async fn terminate(&mut self) {
            delay_for(Duration::from_secs(3)).await;
        }
    }

    #[async_trait]
    impl ActionHandler<MsgOne> for MyActor {
        async fn handle(&mut self, _: MsgOne, _ctx: &mut Context<Self>) -> Result<(), Error> {
            log::info!("Received MsgOne");
            Ok(())
        }
    }

    #[async_trait]
    impl InteractionHandler<MsgTwo> for MyActor {
        async fn handle(&mut self, _: MsgTwo, _ctx: &mut Context<Self>) -> Result<u8, Error> {
            log::info!("Received MsgTwo");
            Ok(1)
        }
    }

    #[async_trait]
    impl ActionHandler<signal::CtrlC> for MyActor {
        async fn handle(
            &mut self,
            _: signal::CtrlC,
            _ctx: &mut Context<Self>,
        ) -> Result<(), Error> {
            log::info!("Received CtrlC");
            Ok(())
        }
    }

    #[tokio::test]
    async fn start_and_terminate() -> Result<(), Error> {
        env_logger::try_init().ok();
        let mut address = MyActor.start(Supervisor::None);
        address.act(MsgOne).await?;
        let res = address.interact(MsgTwo).await?;
        assert_eq!(res, 1);
        address.shutdown();
        address.join().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_recipient() -> Result<(), Error> {
        env_logger::try_init().ok();
        let mut address = MyActor.start(Supervisor::None);
        let action_recipient = address.action_recipient();
        action_recipient.clone().act(MsgOne).await?;
        let interaction_recipient = address.interaction_recipient();
        let res = interaction_recipient.clone().interact(MsgTwo).await?;
        assert_eq!(res, 1);
        address.shutdown();
        address.join().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_attach() -> Result<(), Error> {
        env_logger::try_init().ok();
        let mut address = MyActor.start(Supervisor::None);
        let stream = stream::iter(vec![MsgOne, MsgOne, MsgOne]);
        address.attach(stream).await?;
        // If you activeate this line the test will wait for the `Ctrl+C` signal.
        //address.attach(signal::CtrlC::stream()).await?;
        address.shutdown();
        Ok(())
    }
}
