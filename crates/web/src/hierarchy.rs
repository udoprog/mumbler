use core::cell::{Cell, Ref, RefCell, RefMut};

use std::collections::btree_set::Iter;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use api::{PeerId, RemoteId};

use crate::objects::LocalObject;

#[derive(Default)]
struct Inner {
    mutable: RefCell<HierarchyRef>,
    version: Cell<u64>,
}

#[derive(Default)]
pub(crate) struct Hierarchy {
    inner: Rc<Inner>,
    // The version this instance saw when it was cloned.
    version: u64,
}

impl PartialEq for Hierarchy {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner) && self.version == other.inner.version.get()
    }
}

impl Clone for Hierarchy {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
            version: self.inner.version.get(),
        }
    }
}

impl Hierarchy {
    /// Borrow hiearchy read-only.
    #[inline]
    pub(crate) fn borrow(&self) -> Ref<'_, HierarchyRef> {
        self.inner.mutable.borrow()
    }

    /// Borrow hiearchy mutably.
    #[inline]
    pub(crate) fn borrow_mut(&self) -> RefMut<'_, HierarchyRef> {
        self.inner
            .version
            .set(self.inner.version.get().wrapping_add(1));
        self.inner.mutable.borrow_mut()
    }
}

#[derive(PartialOrd, Ord, PartialEq, Eq)]
pub(crate) struct Key {
    sort: Vec<u8>,
    pub(crate) id: RemoteId,
}

/// Reference to the mutable data of a hierarchy.
#[derive(Default)]
pub(crate) struct HierarchyRef {
    children: HashMap<RemoteId, BTreeSet<Key>>,
    len: usize,
}

impl HierarchyRef {
    /// Test if the given group is empty.
    #[inline]
    pub(crate) fn is_empty(&self, group: RemoteId) -> bool {
        self.children
            .get(&group)
            .map(|s| s.is_empty())
            .unwrap_or(true)
    }

    /// Get the children of the given group, sorted by their sort key.
    pub(crate) fn iter(&self, group: RemoteId) -> impl DoubleEndedIterator<Item = RemoteId> {
        self.children
            .get(&group)
            .into_iter()
            .flatten()
            .map(|node| node.id)
    }

    /// Get all objects in the hierarchy.
    pub(crate) fn walk(&self) -> impl DoubleEndedIterator<Item = RemoteId> {
        self.walk_from(RemoteId::ZERO)
    }

    /// Get all objects in the hierarchy.
    pub(crate) fn walk_from(&self, id: RemoteId) -> impl DoubleEndedIterator<Item = RemoteId> {
        Walk {
            mutable: self,
            visited: HashSet::with_capacity(self.len),
            stack: self
                .children
                .get(&id)
                .map(|s| s.iter())
                .into_iter()
                .collect(),
        }
    }

    /// Remove the given id from all groups.
    pub(crate) fn remove(&mut self, group: RemoteId, sort: &[u8], id: RemoteId) {
        let key = Key {
            sort: sort.to_vec(),
            id,
        };

        if let Some(values) = self.children.get_mut(&group) {
            values.remove(&key);
            self.len = self.len.saturating_sub(1);
        }
    }

    /// Insert an object into the hierarchy. Does nothing if the object has no sort key.
    pub(crate) fn insert(&mut self, object: &LocalObject) {
        if object.sort().is_empty() {
            return;
        }

        let group = as_group(*object.group);
        let key = Key {
            sort: object.sort().to_vec(),
            id: object.id,
        };

        if self.children.entry(group).or_default().insert(key) {
            self.len = self.len.saturating_add(1);
        }
    }

    /// Move an object from one position to another.
    pub(crate) fn reorder(
        &mut self,
        old_group: RemoteId,
        old_sort: &[u8],
        new_group: RemoteId,
        new_sort: &[u8],
        id: RemoteId,
    ) {
        self.remove(old_group, old_sort, id);

        let new_group = as_group(new_group);

        let key = Key {
            sort: new_sort.to_vec(),
            id,
        };

        if self.children.entry(new_group).or_default().insert(key) {
            self.len = self.len.saturating_add(1);
        }
    }

    /// Extend the hierarchy with the given objects.
    pub(crate) fn extend<'a>(&mut self, objects: impl IntoIterator<Item = &'a LocalObject>) {
        for object in objects {
            self.insert(object);
        }
    }

    pub(crate) fn retain(&mut self, mut f: impl FnMut(PeerId) -> bool + Copy) {
        self.children.retain(|group, children| {
            if !f(group.peer_id) {
                self.len -= children.len();
                return false;
            }

            let before = children.len();
            children.retain(move |id| f(id.id.peer_id));
            self.len -= before - children.len();

            !children.is_empty()
        });
    }
}

fn as_group(id: RemoteId) -> RemoteId {
    if id.id.is_zero() {
        return RemoteId::ZERO;
    }

    id
}

pub(crate) struct Walk<'a> {
    mutable: &'a HierarchyRef,
    visited: HashSet<RemoteId>,
    stack: Vec<Iter<'a, Key>>,
}

impl Iterator for Walk<'_> {
    type Item = RemoteId;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let iter = self.stack.last_mut()?;

            let Some(node) = iter.next() else {
                self.stack.pop();
                continue;
            };

            if let Some(children) = self.mutable.children.get(&node.id) {
                self.stack.push(children.iter());
            }

            if self.visited.insert(node.id) {
                return Some(node.id);
            }
        }
    }
}

impl DoubleEndedIterator for Walk<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        loop {
            let iter = self.stack.last_mut()?;

            let Some(node) = iter.next_back() else {
                self.stack.pop();
                continue;
            };

            if let Some(children) = self.mutable.children.get(&node.id) {
                self.stack.push(children.iter());
            }

            if self.visited.insert(node.id) {
                return Some(node.id);
            }
        }
    }
}
