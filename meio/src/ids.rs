//! Typed and generic id types.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::Arc;
use uuid::Uuid;

/// Generic ID of `Actor`'s runtime.
///
/// It used used to identify all senders for that actor.
#[derive(Clone)]
pub struct Id(Arc<Uuid>);

impl Id {
    /// Only the framework can instantiate it.
    pub fn unique() -> Self {
        let uuid = Uuid::new_v4();
        Self(Arc::new(uuid))
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.as_ref().fmt(f)
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({})", self.0)
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

/*
impl AsRef<str> for Id {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}
*/

/// Typed ID of the `LiteTask` or `Actor`.
///
/// It can be simply converted into a generic `Id`.
pub struct IdOf<T> {
    id: Id,
    _origin: PhantomData<T>,
}

unsafe impl<T> Sync for IdOf<T> {}

impl<T> Clone for IdOf<T> {
    fn clone(&self) -> Self {
        Self::new(self.id.clone())
    }
}

impl<T> fmt::Display for IdOf<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.id, f)
    }
}

impl<T> fmt::Debug for IdOf<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.id, f)
    }
}

impl<T> IdOf<T> {
    /// The instance can be created by the framework only.
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

impl<T> From<IdOf<T>> for Id {
    fn from(id_of: IdOf<T>) -> Self {
        id_of.id
    }
}

impl<T> AsRef<Id> for IdOf<T> {
    fn as_ref(&self) -> &Id {
        &self.id
    }
}
