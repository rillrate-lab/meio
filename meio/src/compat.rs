pub fn spawn_async<F>(future: F)
where
    F: futures::Future<Output = ()> + Send + 'static,
{
    #[cfg(not(feature = "wasm"))]
    {
        tokio::spawn(future);
    }
    #[cfg(feature = "wasm")]
    {
        wasm_bindgen_futures::spawn_local(future);
    }
}

pub async fn delay_until(deadline: std::time::Instant) {
    #[cfg(not(feature = "wasm"))]
    {
        tokio::time::sleep_until(deadline.into()).await;
    }
    #[cfg(feature = "wasm")]
    {
        use std::time::Instant;
        let duration = deadline.duration_since(Instant::now());
        futures_timer::Delay::new(duration).await;
    }
}

pub use delay_queue::DelayQueue;

#[cfg(not(feature = "wasm"))]
mod delay_queue {
    use futures::task::{Context, Poll};
    use futures::Stream;
    use std::pin::Pin;
    use std::time::Instant;
    use tokio::time::error::Error;
    use tokio_util::time::delay_queue::DelayQueue as TokioDelayQueue;
    pub use tokio_util::time::delay_queue::Expired;

    pub struct DelayQueue<T> {
        queue: TokioDelayQueue<T>,
    }

    impl<T> DelayQueue<T> {
        pub fn new() -> Self {
            Self {
                queue: TokioDelayQueue::new(),
            }
        }

        pub fn insert_at(&mut self, value: T, deadline: Instant) {
            self.queue.insert_at(value, deadline.into());
        }
    }

    impl<T> Stream for DelayQueue<T> {
        type Item = Result<Expired<T>, Error>;

        fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            let queue = unsafe { self.map_unchecked_mut(|this| &mut this.queue) };
            queue.poll_next(cx)
        }
    }
}

#[cfg(feature = "wasm")]
mod delay_queue {
    use anyhow::Error;
    use futures::task::{Context, Poll};
    use futures::Stream;
    //use futures_delay_queue::DelayQueue as WasmDelayQueue;
    use std::marker::PhantomData;
    use std::pin::Pin;
    use std::time::Instant;

    pub struct Expired<T> {
        value: T,
    }

    impl<T> Expired<T> {
        pub fn into_inner(self) -> T {
            self.value
        }
    }

    pub struct DelayQueue<T> {
        _type: PhantomData<T>,
    }

    impl<T> DelayQueue<T> {
        pub fn new() -> Self {
            Self { _type: PhantomData }
        }

        pub fn insert_at<Z>(&mut self, _: Z, deadline: Instant) {
            // TODO: Implement
        }
    }

    impl<T> Stream for DelayQueue<T> {
        type Item = Result<Expired<T>, Error>;

        fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            // TODO: Implement
            Poll::Pending
        }
    }
}

#[cfg(not(feature = "wasm"))]
pub mod watch {
    pub use tokio::sync::watch::*;
}

#[cfg(feature = "wasm")]
pub mod watch {
    use anyhow::Error;
    use std::marker::PhantomData;
    use std::time::Duration;

    #[derive(Debug)]
    pub struct Sender<T> {
        _type: PhantomData<T>,
    }

    impl<T> Sender<T> {
        pub fn broadcast(&self, value: T) -> Result<(), Error> {
            // TODO: Implement
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    pub struct Receiver<T> {
        value: T,
    }

    impl<T> Receiver<T> {
        pub fn borrow(&self) -> &T {
            &self.value
        }

        pub async fn recv(&mut self) -> Option<&T> {
            // TODO: Implement
            let duration = Duration::from_secs(5);
            futures_timer::Delay::new(duration).await;
            Some(&self.value)
        }
    }

    pub fn channel<T>(value: T) -> (Sender<T>, Receiver<T>) {
        let sender = Sender { _type: PhantomData };
        let receiver = Receiver { value };
        (sender, receiver)
    }
}
