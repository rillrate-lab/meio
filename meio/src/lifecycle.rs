//! Contains message of the `Actor`'s lifecycle.

use crate::Action;

/// This message sent by a `Supervisor` to a spawned child actor.
pub struct Awake /* TODO: Add `Supervisor` type parameter to support different spawners */ {
    // TODO: Add `Supervisor`
}

impl Awake {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

impl Action for Awake {
    fn is_high_priority(&self) -> bool {
        true
    }
}

/*
 * struct Supervisor {
 *   address?
 * }
 *
 * impl Supervisor {
 *   /// The method that allow a child to ask the supervisor to shutdown.
 *   /// It sends `Shutdown` message, the supervisor can ignore it.
 *   fn shutdown();
 * }
*/
