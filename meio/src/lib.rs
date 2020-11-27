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
#![recursion_limit = "512"]
#![feature(generators)]
#![feature(option_expect_none)]

mod actor_runtime;
mod lite_runtime;

//pub mod channel;
pub mod handlers;
pub mod lifecycle;
pub mod linkage;
pub mod signal;
pub mod task;
//pub mod terminator;

pub use actor_runtime::{standalone, Actor, Context, System};
//pub use channel::{Controller, Status, Supervisor};
//use channel::{Operator, Signal};
use handlers::Envelope;
pub use handlers::{Action, ActionHandler, Interaction, InteractionHandler};
pub use linkage::address::Address;
pub use linkage::link::Link;
pub use linkage::notifier::Notifier;
pub use linkage::performers::{ActionPerformer, InteractionPerformer};
pub use linkage::recipients::{ActionRecipient, InteractionRecipient};
pub use lite_runtime::{LiteStatus, LiteTask, ShutdownReceiver};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::Arc;
//pub use terminator::{Stage, TerminationProgress, Terminator};

/// Unique Id of Actor's runtime that used to identify
/// all senders for that actor.
// TODO: Rename to `GenericId`
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

impl PartialEq<Self> for Id {
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

/// Typed if of the task or actor.
// TODO: Rename to Id
#[derive(Debug, Clone)]
pub struct TypedId<T> {
    id: Id,
    _origin: PhantomData<T>,
}

impl<T> TypedId<T> {
    fn new(id: Id) -> Self {
        Self {
            id,
            _origin: PhantomData,
        }
    }
}

impl<T> PartialEq<Self> for TypedId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

impl<T> Eq for TypedId<T> {}

impl<T> Hash for TypedId<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

// %%%%%%%%%%%%%%%%%%%%%% TESTS %%%%%%%%%%%%%%%%%%%%%

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Error;
    use async_trait::async_trait;
    use futures::stream;
    //use std::time::Duration;
    //use tokio::time::delay_for;

    #[derive(Debug)]
    pub struct MyActor;

    #[async_trait]
    impl ActionHandler<lifecycle::Awake<System>> for MyActor {
        async fn handle(
            &mut self,
            _event: lifecycle::Awake<System>,
            _ctx: &mut Context<Self>,
        ) -> Result<(), Error> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct MsgOne;

    impl Action for MsgOne {}

    #[derive(Debug)]
    struct MsgTwo;

    impl Interaction for MsgTwo {
        type Output = u8;
    }

    mod link {
        use super::*;
        use derive_more::{Deref, DerefMut, From};

        #[derive(Debug, From)]
        pub struct MyLink {
            address: Address<MyActor>,
        }

        pub(super) struct LinkSignal;

        impl Action for LinkSignal {}

        impl MyLink {
            pub async fn send_signal(&mut self) -> Result<(), Error> {
                self.address.act(LinkSignal).await
            }
        }

        /// Two important points about links (the example you can see in the test below):
        ///
        /// 1. You can have different links/views to the `Actor`.
        ///
        /// 2. And if `DerefMut` implemented you can use the `Link`
        /// as an ordinary `Address` instance.
        ///
        #[derive(Debug, From, Deref, DerefMut)]
        pub struct MyAlternativeLink {
            address: Address<MyActor>,
        }
    }

    #[async_trait]
    impl Actor for MyActor {
        /* TODO: Fix in the future releases.
        async fn terminate(&mut self) {
            delay_for(Duration::from_secs(3)).await;
        }
        */
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

    #[async_trait]
    impl ActionHandler<link::LinkSignal> for MyActor {
        async fn handle(
            &mut self,
            _: link::LinkSignal,
            _ctx: &mut Context<Self>,
        ) -> Result<(), Error> {
            log::info!("Received LinkSignal");
            Ok(())
        }
    }

    #[tokio::test]
    async fn start_and_terminate() -> Result<(), Error> {
        env_logger::try_init().ok();
        let mut address = standalone(MyActor);
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
        let mut address = standalone(MyActor);
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
        let mut address = standalone(MyActor);
        let stream = stream::iter(vec![MsgOne, MsgOne, MsgOne]);
        address.attach(stream).await?;
        // If you activeate this line the test will wait for the `Ctrl+C` signal.
        //address.attach(signal::CtrlC::stream()).await?;
        address.shutdown();
        Ok(())
    }

    #[tokio::test]
    async fn test_link() -> Result<(), Error> {
        env_logger::try_init().ok();
        let address = standalone(MyActor);
        let mut link: link::MyLink = address.link();
        link.send_signal().await?;
        let mut alternative_link: link::MyAlternativeLink = address.link();
        alternative_link.shutdown();
        alternative_link.join().await;
        Ok(())
    }
}
