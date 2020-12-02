//! Meio prelude module.

pub use crate::actor_runtime::{Actor, Context, Status};
pub use crate::handlers::{
    Action, ActionHandler, Consumer, Eliminated, Interaction, InteractionHandler, InterruptedBy,
    StartedBy,
};
pub use crate::ids::{Id, IdOf};
pub use crate::linkage::{
    ActionPerformer, ActionRecipient, Address, InteractionPerformer, InteractionRecipient, Link,
};
pub use crate::lite_runtime::{LiteTask, ShutdownReceiver, Task};
pub use crate::system::System;

/// Spawns a standalone `Actor` that has no `Supervisor`.
pub fn spawn<A>(actor: A) -> Address<A>
where
    A: Actor + StartedBy<System>,
{
    crate::actor_runtime::spawn(actor, Option::<Address<System>>::None)
}
