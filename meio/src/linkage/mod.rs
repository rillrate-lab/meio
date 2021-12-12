//! Contains modules of different ways to communicate with `Actor`s.

// TODO: Improve imports here (use them directly and prelude only)

mod address;
pub use address::Address;
pub(crate) use address::{AddressJoint, AddressPair};

mod recipient;
pub use recipient::{ActionRecipient, InteractionRecipient};

mod distributor;
pub use distributor::Distributor;

mod task_distributor;
pub use task_distributor::TaskDistributor;
