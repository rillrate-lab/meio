pub fn spawn_async<F>(future: F)
where
    F: futures::Future<Output = ()> + Send + 'static,
{
    /*
    let fut = future.map(|res| {
        if let Err(err) = res {
            log::error!("Async future failed: {}", err);
        }
    });
    */
    #[cfg(feature = "server")]
    {
        tokio::spawn(future);
    }
    #[cfg(feature = "client")]
    {
        wasm_bindgen_futures::spawn_local(future);
    }
}

pub async fn delay_until(deadline: std::time::Instant) {
    #[cfg(feature = "server")]
    {
        tokio::time::delay_until(deadline.into()).await;
    }
    #[cfg(feature = "client")]
    {
        use std::time::Instant;
        let duration = deadline.duration_since(Instant::now());
        futures_timer::Delay::new(duration).await;
    }
}

pub use delay_queue::DelayQueue;

#[cfg(feature = "server")]
mod delay_queue {
    use futures::task::{Context, Poll};
    use futures::Stream;
    use std::pin::Pin;
    use std::time::Instant;
    pub use tokio::time::delay_queue::Expired;
    use tokio::time::DelayQueue as TokioDelayQueue;
    use tokio::time::Error;

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

#[cfg(feature = "client")]
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
            todo!();
        }
    }

    impl<T> Stream for DelayQueue<T> {
        type Item = Result<Expired<T>, Error>;

        fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            todo!()
        }
    }
}

#[cfg(feature = "server")]
pub mod watch {
    pub use tokio::sync::watch::*;
}

#[cfg(feature = "client")]
pub mod watch {
    use anyhow::Error;
    use std::marker::PhantomData;

    #[derive(Debug)]
    pub struct Sender<T> {
        _type: PhantomData<T>,
    }

    impl<T> Sender<T> {
        pub fn broadcast(&self, value: T) -> Result<(), Error> {
            todo!();
        }
    }

    #[derive(Debug, Clone)]
    pub struct Receiver<T> {
        _type: PhantomData<T>,
    }

    impl<T> Receiver<T> {
        pub fn borrow(&self) -> &T {
            todo!();
        }

        pub async fn recv(&mut self) -> Option<&T> {
            todo!();
        }
    }

    pub fn channel<T>(value: T) -> (Sender<T>, Receiver<T>) {
        todo!();
    }
}
