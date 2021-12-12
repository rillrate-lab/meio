//! `meio` - is the lightweight asynchronous actor framework for Rust.
//! It extends `tokio` runtime to bring actor benefits to it.
//!
//! The framework is designed for business applications where
//! developers need a flexible code base and pluggable actors.
//!
//! It also has a fast runtime that calls async methods of plain
//! structs in an isolated asynchronous task.

#![warn(missing_docs)]
#![recursion_limit = "512"]

mod actor_runtime;
mod compat;
mod forwarders;
pub mod handlers;
pub mod ids;
mod lifecycle;
pub mod linkage;
mod lite_runtime;
#[cfg(not(feature = "wasm"))]
pub mod signal;
pub mod system;
pub mod tasks;
#[cfg(not(feature = "wasm"))]
pub mod thread;

pub mod prelude;

// %%%%%%%%%%%%%%%%%%%%%% TESTS %%%%%%%%%%%%%%%%%%%%%

#[cfg(test)]
mod tests {
    use super::handlers::Interact;
    use super::prelude::*;
    use super::signal;
    use anyhow::Error;
    use async_trait::async_trait;
    use futures::stream;
    use std::time::{Duration, Instant};
    use tokio::time::{sleep, timeout};

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

    #[derive(Debug)]
    struct MsgThree;

    impl Interaction for MsgThree {
        type Output = u8;
    }

    struct ScheduledStop(bool);

    mod link {
        use super::*;
        use derive_more::{Deref, DerefMut, From, Into};

        #[derive(Debug, From)]
        pub struct MyLink {
            address: Address<MyActor>,
        }

        pub(super) struct LinkSignal;

        impl Action for LinkSignal {}

        impl MyLink {
            pub fn send_signal(&mut self) -> Result<(), Error> {
                self.address.act(LinkSignal)
            }
        }

        /// Two important points about links (the example you can see in the test below):
        ///
        /// 1. You can have different links/views to the `Actor`.
        ///
        /// 2. And if `DerefMut` implemented you can use the `Link`
        /// as an ordinary `Address` instance.
        ///
        #[derive(Debug, From, Deref, DerefMut, Into)]
        pub struct MyAlternativeLink {
            address: Address<MyActor>,
        }
    }

    #[async_trait]
    impl Actor for MyActor {
        type GroupBy = ();

        fn log_target(&self) -> &str {
            "MyActor"
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
    impl Consumer<MsgOne> for MyActor {
        async fn handle(&mut self, _: MsgOne, _ctx: &mut Context<Self>) -> Result<(), Error> {
            log::info!("Received MsgOne (from the stream)");
            Ok(())
        }

        async fn finished(&mut self, ctx: &mut Context<Self>) -> Result<(), Error> {
            ctx.shutdown();
            Ok(())
        }
    }

    impl StreamAcceptor<MsgOne> for MyActor {
        fn stream_group(&self) -> Self::GroupBy {}
    }

    #[async_trait]
    impl InteractionHandler<MsgTwo> for MyActor {
        async fn handle(&mut self, _: MsgTwo, _ctx: &mut Context<Self>) -> Result<u8, Error> {
            log::info!("Received MsgTwo");
            Ok(1)
        }
    }

    #[async_trait]
    impl ActionHandler<Interact<MsgThree>> for MyActor {
        async fn handle(
            &mut self,
            interact: Interact<MsgThree>,
            _ctx: &mut Context<Self>,
        ) -> Result<(), Error> {
            log::info!("Received MsgThree");
            interact
                .responder
                .send(Ok(123))
                .map_err(|_| Error::msg("can't send a response"))
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
            stop: ScheduledStop,
            ctx: &mut Context<Self>,
        ) -> Result<(), Error> {
            if stop.0 {
                ctx.shutdown();
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn start_and_terminate() -> Result<(), Error> {
        env_logger::try_init().ok();
        let mut address = System::spawn(MyActor);
        address.act(MsgOne)?;
        let res = address.interact(MsgTwo).recv().await?;
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
        action_recipient.clone().act(MsgOne)?;
        let interaction_recipient = address.interaction_recipient();
        let res = interaction_recipient
            .clone()
            .interact(MsgTwo)
            .recv()
            .await?;
        assert_eq!(res, 1);
        System::interrupt(&mut address)?;
        address.join().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_custom_interaction() -> Result<(), Error> {
        env_logger::try_init().ok();
        let mut address = System::spawn(MyActor);
        let res = address.clone().interact(MsgThree).recv().await?;
        assert_eq!(res, 123);
        System::interrupt(&mut address)?;
        address.join().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_attach() -> Result<(), Error> {
        env_logger::try_init().ok();
        let mut address = System::spawn(MyActor);
        let stream = stream::iter(vec![MsgOne, MsgOne, MsgOne]);
        address.attach(stream, ())?;
        // If you acivate this line the test will wait for the `Ctrl+C` signal.
        //address.attach(signal::CtrlC::stream()).await?;
        //System::interrupt(&mut address)?;
        address.join().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_link() -> Result<(), Error> {
        env_logger::try_init().ok();
        let address = System::spawn(MyActor);
        let mut link: link::MyLink = address.link();
        link.send_signal()?;
        let mut alternative_link: link::MyAlternativeLink = address.link();
        System::interrupt(&mut alternative_link)?;
        address.join().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_schedule() -> Result<(), Error> {
        env_logger::try_init().ok();
        let address = System::spawn(MyActor);
        // To check the `DelayedQueue` won't closed.
        sleep(Duration::from_secs(3)).await;
        address.schedule(
            ScheduledStop(false),
            Instant::now() + Duration::from_secs(3),
        )?;
        address.schedule(
            ScheduledStop(false),
            Instant::now() + Duration::from_secs(3),
        )?;
        address.schedule(ScheduledStop(true), Instant::now() + Duration::from_secs(3))?;
        timeout(Duration::from_secs(5), address.join()).await?;
        Ok(())
    }

    struct ActorSingle(usize);

    impl Actor for ActorSingle {
        type GroupBy = ();

        fn log_target(&self) -> &str {
            "ActorSingle"
        }
    }

    #[async_trait]
    impl StartedBy<ActorMany> for ActorSingle {
        async fn handle(&mut self, ctx: &mut Context<Self>) -> Result<(), Error> {
            sleep(Duration::from_secs(10)).await;
            ctx.shutdown();
            Ok(())
        }
    }

    #[async_trait]
    impl InterruptedBy<ActorMany> for ActorSingle {
        async fn handle(&mut self, _ctx: &mut Context<Self>) -> Result<(), Error> {
            Ok(())
        }
    }

    use std::collections::HashSet;

    #[derive(Default)]
    struct ActorMany {
        actors: HashSet<IdOf<ActorSingle>>,
    }

    #[async_trait]
    impl StartedBy<System> for ActorMany {
        async fn handle(&mut self, ctx: &mut Context<Self>) -> Result<(), Error> {
            for id in 1..=1_000_000 {
                let addr = ctx.spawn_actor(ActorSingle(id), ());
                self.actors.insert(addr.id());
            }
            Ok(())
        }
    }

    #[async_trait]
    impl Eliminated<ActorSingle> for ActorMany {
        async fn handle(
            &mut self,
            id: IdOf<ActorSingle>,
            ctx: &mut Context<Self>,
        ) -> Result<(), Error> {
            self.actors.remove(&id);
            if self.actors.is_empty() {
                ctx.shutdown();
            }
            Ok(())
        }
    }

    impl Actor for ActorMany {
        type GroupBy = ();

        fn log_target(&self) -> &str {
            "ActorMany"
        }
    }

    // To run use: `cargo test -- --include-ignored`
    #[ignore] // Expensive!
    #[tokio::test]
    async fn test_million_actors() -> Result<(), Error> {
        let address = System::spawn(ActorMany::default());
        address.join().await;
        Ok(())
    }

    struct TaskSpawner;

    impl Actor for TaskSpawner {
        type GroupBy = ();

        fn log_target(&self) -> &str {
            "TaskSpawner"
        }
    }

    #[tokio::test]
    async fn test_async_closure() -> Result<(), Error> {
        let address = System::spawn(TaskSpawner);
        address.join().await;
        Ok(())
    }

    #[async_trait]
    impl StartedBy<System> for TaskSpawner {
        async fn handle(&mut self, ctx: &mut Context<Self>) -> Result<(), Error> {
            ctx.spawn_task(FnTask(async move { Ok(8) }), (), ());
            Ok(())
        }
    }

    #[async_trait]
    impl FnTaskEliminated<u8, ()> for TaskSpawner {
        async fn handle(
            &mut self,
            _id: Id,
            _tag: (),
            result: Result<u8, TaskError>,
            ctx: &mut Context<Self>,
        ) -> Result<(), Error> {
            assert_eq!(result?, 8);
            ctx.shutdown();
            Ok(())
        }
    }

    /* TODO: Not ready yet
     * It required to use a `schedule` queue to add a delayed event
    struct DrainedActor;

    #[async_trait]
    impl Actor for DrainedActor {
        type GroupBy = ();

        async fn queue_drained(&mut self, ctx: &mut Context<Self>) -> Result<(), Error> {
            ctx.shutdown();
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_draining() -> Result<(), Error> {
        env_logger::try_init().ok();
        let address = System::spawn(MyActor);
        address.join().await;
        Ok(())
    }
    */
}
