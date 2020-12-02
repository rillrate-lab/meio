//! Contains typed and generic id types.

use crate::actor_runtime::Actor;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::Arc;

/// Unique Id of Actor's runtime that used to identify
/// all senders for that actor.
#[derive(Clone)]
pub struct Id(Arc<String>);

impl Id {
    /// Generated new `Id` for `Actor`.
    pub(crate) fn of_actor<T: Actor>(entity: &T) -> Self {
        let name = entity.name();
        Self(Arc::new(name))
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.as_ref().fmt(f)
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple(self.0.as_ref()).finish()
    }
}

impl PartialEq<Self> for Id {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Id {}

impl Hash for Id {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.as_ref().hash(state);
    }
}

impl AsRef<str> for Id {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

/// Typed if of the task or actor.
#[derive(Debug, Clone)]
pub struct IdOf<T> {
    id: Id,
    _origin: PhantomData<T>,
}

impl<T> IdOf<T> {
    pub(crate) fn new(id: Id) -> Self {
        Self {
            id,
            _origin: PhantomData,
        }
    }
}

impl<T> PartialEq<Self> for IdOf<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

impl<T> Eq for IdOf<T> {}

impl<T> Hash for IdOf<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T> Into<Id> for IdOf<T> {
    fn into(self) -> Id {
        self.id
    }
}

impl<T> AsRef<Id> for IdOf<T> {
    fn as_ref(&self) -> &Id {
        &self.id
    }
}
