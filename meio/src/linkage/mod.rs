//! Contains modules of different ways to communicate with `Actor`s.

mod address;
pub use address::Address;

pub mod bridge;

mod link;
pub use link::Link;

mod performers;
pub use performers::{ActionPerformer, InteractionPerformer};

mod recipients;
pub use recipients::{ActionRecipient, InteractionRecipient};
