//! The module contains the action to interrupt `Actor`s grecefully.

use crate::Action;

/// The event that send by `Address::interrupt()` method call.
pub struct Interrupted;

impl Action for Interrupted {
    fn is_high_priority(&self) -> bool {
        true
    }
}
