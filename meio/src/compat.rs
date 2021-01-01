#[cfg(feature = "client")]
pub use futures_delay_queue::DelayQueue;
#[cfg(feature = "server")]
pub use tokio::time::DelayQueue;

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
        wasm_timer::Delay::new_at(deadline).await;
    }
}
