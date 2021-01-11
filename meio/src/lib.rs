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

mod actor_runtime;
mod compat;
pub mod handlers;
pub mod ids;
mod lifecycle;
pub mod linkage;
mod lite_runtime;
pub mod prelude;
#[cfg(not(feature = "wasm"))]
pub mod signal;
pub mod system;
pub mod task;
#[cfg(not(feature = "wasm"))]
pub mod thread;

// %%%%%%%%%%%%%%%%%%%%%% TESTS %%%%%%%%%%%%%%%%%%%%%

#[cfg(test)]
mod tests {
    use super::prelude::*;
    use super::signal;
    use anyhow::Error;
    use async_trait::async_trait;
    use futures::stream;
    use std::time::{Duration, Instant};
    use tokio::time::sleep;

    #[derive(Debug)]
    pub struct MyActor;

    #[async_trait]
    impl StartedBy<System> for MyActor {
        async fn handle(&mut self, _ctx: &mut Context<Self>) -> Result<(), Error> {
            Ok(())
        }
    }

    #[async_trait]
    impl InterruptedBy<System> for MyActor {
        async fn handle(&mut self, ctx: &mut Context<Self>) -> Result<(), Error> {
            sleep(Duration::from_secs(3)).await;
            ctx.shutdown();
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

    struct ScheduledStop;

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
        type GroupBy = ();
    }

    #[async_trait]
    impl ActionHandler<MsgOne> for MyActor {
        async fn handle(&mut self, _: MsgOne, _ctx: &mut Context<Self>) -> Result<(), Error> {
            log::info!("Received MsgOne");
            Ok(())
        }
    }

    #[async_trait]
    impl Consumer<MsgOne> for MyActor {
        async fn handle(&mut self, _: MsgOne, _ctx: &mut Context<Self>) -> Result<(), Error> {
            log::info!("Received MsgOne (from the stream)");
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
    impl Consumer<signal::CtrlC> for MyActor {
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

    #[async_trait]
    impl Scheduled<ScheduledStop> for MyActor {
        async fn handle(
            &mut self,
            _: Instant,
            _: ScheduledStop,
            ctx: &mut Context<Self>,
        ) -> Result<(), Error> {
            ctx.shutdown();
            Ok(())
        }
    }

    #[tokio::test]
    async fn start_and_terminate() -> Result<(), Error> {
        env_logger::try_init().ok();
        let mut address = System::spawn(MyActor);
        address.act(MsgOne).await?;
        let res = address.interact(MsgTwo).await?;
        assert_eq!(res, 1);
        System::interrupt(&mut address)?;
        address.join().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_recipient() -> Result<(), Error> {
        env_logger::try_init().ok();
        let mut address = System::spawn(MyActor);
        let action_recipient = address.action_recipient();
        action_recipient.clone().act(MsgOne).await?;
        let interaction_recipient = address.interaction_recipient();
        let res = interaction_recipient.clone().interact(MsgTwo).await?;
        assert_eq!(res, 1);
        System::interrupt(&mut address)?;
        address.join().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_attach() -> Result<(), Error> {
        env_logger::try_init().ok();
        let mut address = System::spawn(MyActor);
        let stream = stream::iter(vec![MsgOne, MsgOne, MsgOne]);
        address.attach(stream);
        // If you acivate this line the test will wait for the `Ctrl+C` signal.
        //address.attach(signal::CtrlC::stream()).await?;
        System::interrupt(&mut address)?;
        Ok(())
    }

    #[tokio::test]
    async fn test_link() -> Result<(), Error> {
        env_logger::try_init().ok();
        let address = System::spawn(MyActor);
        let mut link: link::MyLink = address.link();
        link.send_signal().await?;
        let mut alternative_link: link::MyAlternativeLink = address.link();
        System::interrupt(&mut alternative_link)?;
        alternative_link.join().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_schedule() -> Result<(), Error> {
        env_logger::try_init().ok();
        let mut address = System::spawn(MyActor);
        address.schedule(ScheduledStop, Instant::now() + Duration::from_secs(3))?;
        address.join().await;
        Ok(())
    }
}
