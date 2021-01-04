//! Contains modules of different ways to communicate with `Actor`s.

mod address;
pub use address::Address;

mod recipient;
pub use recipient::{ActionRecipient, InteractionRecipient};

mod multi_recipient;
pub use multi_recipient::MultiRecipient;
