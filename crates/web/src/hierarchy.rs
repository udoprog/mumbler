use core::cell::{Cell, Ref, RefCell, RefMut};

use std::collections::btree_set::Iter;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use api::Id;

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
    id: Id,
}

/// Reference to the mutable data of a hierarchy.
#[derive(Default)]
pub(crate) struct HierarchyRef {
    children: HashMap<Id, BTreeSet<Key>>,
    len: usize,
}

impl HierarchyRef {
    /// Test if the given group is empty.
    #[inline]
    pub(crate) fn is_empty(&self, group: Id) -> bool {
        self.children
            .get(&group)
            .map(|s| s.is_empty())
            .unwrap_or(true)
    }

    /// Get the children of the given group, sorted by their sort key.
    pub(crate) fn iter(&self, group: Id) -> impl DoubleEndedIterator<Item = Id> {
        self.children
            .get(&group)
            .into_iter()
            .flatten()
            .map(|node| node.id)
    }

    /// Get all objects in the hierarchy.
    pub(crate) fn walk(&self) -> impl DoubleEndedIterator<Item = Id> {
        self.walk_from(Id::ZERO)
    }

    /// Get all objects in the hierarchy.
    pub(crate) fn walk_from(&self, id: Id) -> impl DoubleEndedIterator<Item = Id> {
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
    pub(crate) fn remove(&mut self, group: Id, sort: Vec<u8>, id: Id) {
        let key = Key { id, sort };

        if let Some(values) = self.children.get_mut(&group) {
            values.remove(&key);
            self.len = self.len.saturating_sub(1);
        }
    }

    /// Insert a child into the given group with the given sort key.
    pub(crate) fn insert(&mut self, group: Id, sort: Vec<u8>, id: Id) {
        let key = Key { id, sort };

        if self.children.entry(group).or_default().insert(key) {
            self.len = self.len.saturating_add(1);
        }
    }

    /// Extend the hierarchy with the given objects.
    pub(crate) fn extend<'a>(&mut self, objects: impl IntoIterator<Item = &'a LocalObject>) {
        for object in objects {
            let key = Key {
                id: object.id,
                sort: object.sort().to_vec(),
            };

            if self.children.entry(*object.group).or_default().insert(key) {
                self.len = self.len.saturating_add(1);
            }
        }
    }
}

pub(crate) struct Walk<'a> {
    mutable: &'a HierarchyRef,
    visited: HashSet<Id>,
    stack: Vec<Iter<'a, Key>>,
}

impl Iterator for Walk<'_> {
    type Item = Id;

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
