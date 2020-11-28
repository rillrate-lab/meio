//! `Link` provides convenient ways of interaction with `Actors` using methods
//! by wrapping `Address` of `Recipient`.

use crate::{Action, ActionRecipient, Actor, Address, Interaction, InteractionRecipient};

/// Returns a `Link` to an `Actor`.
/// `Link` is a convenient concept for creating wrappers for
/// `Address` that provides methods instead of using message types
/// directly. It allows also to use private message types opaquely.
pub trait Link<T> {
    /// Gets a `Link` to an `Actor`.
    fn link(&self) -> T;
}

impl<T, A> Link<T> for Address<A>
where
    T: From<Address<A>>,
    A: Actor,
{
    fn link(&self) -> T {
        T::from(self.clone())
    }
}

impl<T, I> Link<T> for ActionRecipient<I>
where
    T: From<ActionRecipient<I>>,
    I: Action,
{
    fn link(&self) -> T {
        T::from(self.clone())
    }
}

impl<T, I> Link<T> for InteractionRecipient<I>
where
    T: From<InteractionRecipient<I>>,
    I: Interaction,
{
    fn link(&self) -> T {
        T::from(self.clone())
    }
}
