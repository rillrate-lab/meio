//! Contains modules of different ways to communicate with `Actor`s.

mod address;
pub use address::Address;

pub mod bridge;
pub use bridge::Bridge;

mod link;
pub use link::Link;
