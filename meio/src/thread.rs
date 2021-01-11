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
            if let Err(_) = sender.notifier_tx.send(()) {
                log::error!("Can't send termination signal to the {}", self.name);
                return;
            }
            if let Err(_) = sender.blocker.lock() {
                log::error!("Can't wait for termination of the {}", self.name);
            }
        }
    }
}

/// Spawns supervisor actor in a separate runtime and returns a handle
/// to terminate it on `Drop`.
pub fn spawn<T>(actor: T) -> Result<ScopedRuntime, Error>
where
    T: Actor + StartedBy<System> + InterruptedBy<System>,
{
    let name = format!("thread-{}", actor.name());
    let (term_tx, term_rx) = term::channel();
    thread::Builder::new()
        .name(name.clone())
        .spawn(move || entrypoint(actor, term_rx))?;
    Ok(ScopedRuntime {
        name,
        sender: Some(term_tx),
    })
}

#[tokio::main]
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
