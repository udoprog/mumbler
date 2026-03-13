use core::cell::{Cell, Ref, RefCell, RefMut};

use std::collections::btree_set::Iter;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use api::Id;

use crate::objects::LocalObject;

#[derive(Default)]
struct Mutable {
    values: HashMap<Id, BTreeSet<(Vec<u8>, Id)>>,
}

#[derive(Default)]
struct Inner {
    mutable: RefCell<Mutable>,
    version: Cell<u64>,
}

#[derive(Default, Clone)]
pub(crate) struct Hierarchy {
    inner: Rc<Inner>,
}

impl PartialEq for Hierarchy {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
            && self.inner.version.get() == other.inner.version.get()
    }
}

impl Hierarchy {
    /// Borrow hiearchy read-only.
    #[inline]
    pub(crate) fn borrow(&self) -> HierarchyRef<'_> {
        HierarchyRef {
            mutable: self.inner.mutable.borrow(),
        }
    }

    /// Borrow hiearchy mutably.
    #[inline]
    pub(crate) fn borrow_mut(&self) -> HierarchyRefMut<'_> {
        HierarchyRefMut {
            mutable: self.inner.mutable.borrow_mut(),
            version: &self.inner.version,
        }
    }
}

/// A borrowed hierarchy reference.
#[repr(transparent)]
pub(crate) struct HierarchyRef<'a> {
    mutable: Ref<'a, Mutable>,
}

impl HierarchyRef<'_> {
    /// Get the children of the given group, sorted by their sort key.
    pub(crate) fn iter(&self, group: Id) -> impl DoubleEndedIterator<Item = Id> {
        self.mutable
            .values
            .get(&group)
            .into_iter()
            .flatten()
            .map(|(_, id)| *id)
    }

    /// Get all objects in the hierarchy.
    pub(crate) fn iter_all(&self) -> impl DoubleEndedIterator<Item = Id> {
        Walk {
            mutable: &self.mutable,
            stack: self
                .mutable
                .values
                .get(&Id::ZERO)
                .map(|s| s.iter())
                .into_iter()
                .collect(),
        }
    }
}

/// A borrowed hierarchy reference.
pub(crate) struct HierarchyRefMut<'a> {
    mutable: RefMut<'a, Mutable>,
    version: &'a Cell<u64>,
}

impl HierarchyRefMut<'_> {
    /// Remove the given id from all groups.
    pub(crate) fn remove(&mut self, group: Id, sort: Vec<u8>, id: Id) {
        let key = (sort, id);

        if let Some(values) = self.mutable.values.get_mut(&group) {
            values.remove(&key);
        }
    }

    /// Insert a child into the given group with the given sort key.
    pub(crate) fn insert(&mut self, group: Id, sort: Vec<u8>, id: Id) {
        self.mutable
            .values
            .entry(group)
            .or_default()
            .insert((sort, id));
    }

    /// Extend the hierarchy with the given objects.
    pub(crate) fn extend<'a>(&mut self, objects: impl IntoIterator<Item = &'a LocalObject>) {
        for object in objects {
            self.mutable
                .values
                .entry(*object.group)
                .or_default()
                .insert((object.sort().to_vec(), object.id));
        }
    }

    /// Get the children of the given group, sorted by their sort key.
    pub(crate) fn iter(&self, group: Id) -> impl DoubleEndedIterator<Item = Id> {
        self.mutable
            .values
            .get(&group)
            .into_iter()
            .flatten()
            .map(|(_, id)| *id)
    }
}

impl Drop for HierarchyRefMut<'_> {
    #[inline]
    fn drop(&mut self) {
        let version = self.version.get().wrapping_add(1);
        self.version.set(version);
    }
}

pub(crate) struct Walk<'a> {
    mutable: &'a Mutable,
    stack: Vec<Iter<'a, (Vec<u8>, Id)>>,
}

impl Iterator for Walk<'_> {
    type Item = Id;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let iter = self.stack.last_mut()?;

            let Some((_, id)) = iter.next() else {
                self.stack.pop();
                continue;
            };

            if let Some(children) = self.mutable.values.get(id) {
                self.stack.push(children.iter());
            }

            return Some(*id);
        }
    }
}

impl DoubleEndedIterator for Walk<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        loop {
            let iter = self.stack.last_mut()?;

            let Some((_, id)) = iter.next_back() else {
                self.stack.pop();
                continue;
            };

            if let Some(children) = self.mutable.values.get(id) {
                self.stack.push(children.iter());
            }

            return Some(*id);
        }
    }
}
