use core::cell::{Cell, Ref, RefCell, RefMut};

use std::collections::btree_set::Iter;
use std::collections::{BTreeSet, HashMap};
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
pub(crate) struct Node {
    id: Id,
    sort: Vec<u8>,
}

/// Reference to the mutable data of a hierarchy.
#[derive(Default)]
pub(crate) struct HierarchyRef {
    values: HashMap<Id, BTreeSet<Node>>,
}

impl HierarchyRef {
    /// Get the children of the given group, sorted by their sort key.
    pub(crate) fn iter(&self, group: Id) -> impl DoubleEndedIterator<Item = Id> {
        self.values
            .get(&group)
            .into_iter()
            .flatten()
            .map(|node| node.id)
    }

    /// Get all objects in the hierarchy.
    pub(crate) fn iter_all(&self) -> impl DoubleEndedIterator<Item = Id> {
        Walk {
            mutable: self,
            stack: self
                .values
                .get(&Id::ZERO)
                .map(|s| s.iter())
                .into_iter()
                .collect(),
        }
    }

    /// Remove the given id from all groups.
    pub(crate) fn remove(&mut self, group: Id, sort: Vec<u8>, id: Id) {
        let key = Node { id, sort };

        if let Some(values) = self.values.get_mut(&group) {
            values.remove(&key);
        }
    }

    /// Insert a child into the given group with the given sort key.
    pub(crate) fn insert(&mut self, group: Id, sort: Vec<u8>, id: Id) {
        let key = Node { id, sort };
        self.values.entry(group).or_default().insert(key);
    }

    /// Extend the hierarchy with the given objects.
    pub(crate) fn extend<'a>(&mut self, objects: impl IntoIterator<Item = &'a LocalObject>) {
        for object in objects {
            let key = Node {
                id: object.id,
                sort: object.sort().to_vec(),
            };

            self.values.entry(*object.group).or_default().insert(key);
        }
    }
}

pub(crate) struct Walk<'a> {
    mutable: &'a HierarchyRef,
    stack: Vec<Iter<'a, Node>>,
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

            if let Some(children) = self.mutable.values.get(&node.id) {
                self.stack.push(children.iter());
            }

            return Some(node.id);
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

            if let Some(children) = self.mutable.values.get(&node.id) {
                self.stack.push(children.iter());
            }

            return Some(node.id);
        }
    }
}
