//! Special module to run a supervisor in a separate thread.

use crate::actor_runtime::Actor;
use crate::handlers::{InterruptedBy, StartedBy};
use crate::system::System;
use anyhow::Error;
use std::thread;

/// Keeps the control handle to the spawned runtime.
#[derive(Debug)]
pub struct ScopedRuntime {
    name: String,
    sender: Option<term::Sender>,
}

impl ScopedRuntime {
    /*
    /// Don't wait for the `Worker` termination on drop of this instance.
    pub fn no_wait(&mut self) {
        self.sender.take();
    }
    */
}

impl Drop for ScopedRuntime {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            if sender.notifier_tx.send(()).is_err() {
                log::error!("Can't send termination signal to the {}", self.name);
                return;
            }
            if sender.blocker.lock().is_err() {
                log::error!("Can't wait for termination of the {}", self.name);
            }
        }
    }
}

/// Spawns supervisor actor in a separate runtime and returns a handle
/// to terminate it on `Drop`.
///
/// It started with no working threads, because if you used this separate-threaded
/// `meio` instance that means you wanted to spawn a background runtime.
///
/// If you use `meio` in your main `Runtime` than it will use all properties
/// of the existent runtime. That's why this `spawn` function consider it used
/// for background tasks only.
pub fn spawn<T>(actor: T) -> Result<ScopedRuntime, Error>
where
    T: Actor + StartedBy<System> + InterruptedBy<System>,
{
    let name = format!("thread-{}", actor.name());
    let (term_tx, term_rx) = term::channel();
    thread::Builder::new().name(name.clone()).spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .thread_name("meio-pool")
            .worker_threads(1)
            .on_thread_start(|| {
                log::info!("New meio worker thread spawned");
            })
            .on_thread_stop(|| {
                log::info!("The meio worker thread retired");
            })
            .enable_all()
            .build()?;
        let routine = entrypoint(actor, term_rx);
        runtime.block_on(routine)
    })?;
    Ok(ScopedRuntime {
        name,
        sender: Some(term_tx),
    })
}

// TODO: Consider to deny and refactor
#[allow(clippy::await_holding_lock)]
async fn entrypoint<T>(actor: T, term_rx: term::Receiver) -> Result<(), Error>
where
    T: Actor + StartedBy<System> + InterruptedBy<System>,
{
    let blocker = term_rx
        .blocker
        .lock()
        .map_err(|_| Error::msg("can't take termination blocker"))?;
    let mut handle = System::spawn(actor);
    term_rx.notifier_rx.await?;
    System::interrupt(&mut handle)?;
    handle.join().await;
    drop(blocker);
    Ok(())
}

/// Contains termination interaction for the runtime.
mod term {
    use std::sync::{Arc, Mutex};
    use tokio::sync::oneshot;

    // TODO: Hide fields and give methods...
    #[derive(Debug)]
    pub struct Receiver {
        pub notifier_rx: oneshot::Receiver<()>,
        pub blocker: Arc<Mutex<()>>,
    }

    // TODO: Hide fields and give methods...
    #[derive(Debug)]
    pub struct Sender {
        pub notifier_tx: oneshot::Sender<()>,
        pub blocker: Arc<Mutex<()>>,
    }

    pub fn channel() -> (Sender, Receiver) {
        let (tx, rx) = oneshot::channel();
        let blocker = Arc::new(Mutex::new(()));
        let sender = Sender {
            notifier_tx: tx,
            blocker: blocker.clone(),
        };
        let receiver = Receiver {
            notifier_rx: rx,
            blocker,
        };
        (sender, receiver)
    }
}
