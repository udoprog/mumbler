use core::cell::{Cell, Ref, RefCell, RefMut};

use std::collections::btree_set::Iter;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use api::{Key, PeerId, RemoteId, Value};

use crate::objects::Object;
use crate::state::State;

#[derive(Default)]
struct Inner {
    mutable: RefCell<OrderRef>,
    version: Cell<u64>,
}

#[derive(Default)]
pub(crate) struct Order {
    inner: Rc<Inner>,
    // The version this instance saw when it was cloned.
    version: u64,
}

impl PartialEq for Order {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner) && self.version == other.inner.version.get()
    }
}

impl Clone for Order {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
            version: self.inner.version.get(),
        }
    }
}

impl Order {
    /// Borrow hiearchy read-only.
    #[inline]
    pub(crate) fn borrow(&self) -> Ref<'_, OrderRef> {
        self.inner.mutable.borrow()
    }

    /// Borrow hiearchy mutably.
    #[inline]
    pub(crate) fn borrow_mut(&self) -> RefMut<'_, OrderRef> {
        self.inner
            .version
            .set(self.inner.version.get().wrapping_add(1));
        self.inner.mutable.borrow_mut()
    }
}

#[derive(PartialOrd, Ord, PartialEq, Eq)]
pub(crate) struct SortKey {
    sort: Vec<u8>,
    pub(crate) id: RemoteId,
}

struct ObjectData {
    pub(crate) group: State<RemoteId>,
    pub(crate) sort: State<Vec<u8>>,
}

#[derive(Default)]
struct Data {
    children: HashMap<RemoteId, BTreeSet<SortKey>>,
}

impl Data {
    /// Insert an object into the hierarchy. Does nothing if the object has no sort key.
    pub(crate) fn insert(&mut self, object: &Object) -> bool {
        let group = as_group(*object.group);

        let key = SortKey {
            sort: object.sort.to_vec(),
            id: object.id,
        };

        if self.children.entry(group).or_default().insert(key) {
            return true;
        }

        false
    }

    /// Remove the given id from all groups.
    fn remove(&mut self, group: RemoteId, sort: &[u8], id: RemoteId) -> bool {
        let key = SortKey {
            sort: sort.to_vec(),
            id,
        };

        let group = as_group(group);

        let Some(values) = self.children.get_mut(&group) else {
            return false;
        };

        if !values.remove(&key) {
            return false;
        }

        let is_empty = values.is_empty();

        if is_empty {
            self.children.remove(&group);
        }

        true
    }

    /// Move an object from one position to another.
    fn reorder(
        &mut self,
        old_group: RemoteId,
        old_sort: &[u8],
        new_group: RemoteId,
        new_sort: &[u8],
        id: RemoteId,
    ) -> bool {
        if !self.remove(old_group, old_sort, id) {
            return false;
        }

        let new_group = as_group(new_group);

        let key = SortKey {
            sort: new_sort.to_vec(),
            id,
        };

        if self.children.entry(new_group).or_default().insert(key) {
            return true;
        }

        false
    }
}

/// Reference to the mutable data of a hierarchy.
#[derive(Default)]
pub(crate) struct OrderRef {
    objects: HashMap<RemoteId, ObjectData>,
    data: Data,
}

impl OrderRef {
    pub(crate) fn update(&mut self, id: RemoteId, key: Key, value: &Value) -> bool {
        let Some(o) = self.objects.get_mut(&id) else {
            return false;
        };

        match key {
            Key::SORT => 'done: {
                let new = value.as_bytes().to_vec();

                let Some(old_sort) = o.sort.replace(new) else {
                    break 'done false;
                };

                self.data
                    .reorder(*o.group, &old_sort, *o.group, &o.sort, id)
            }
            Key::GROUP => 'done: {
                let new = RemoteId::new(id.peer_id, value.as_id());

                let Some(old_group) = o.group.replace(new) else {
                    break 'done false;
                };

                self.data.reorder(old_group, &o.sort, *o.group, &o.sort, id)
            }
            _ => false,
        }
    }

    /// Move an object from one position to another.
    pub(crate) fn reorder(&mut self, new_group: RemoteId, new_sort: &[u8], id: RemoteId) -> bool {
        let Some(o) = self.objects.get_mut(&id) else {
            return false;
        };

        let group = o.group.replace(new_group);
        let sort = o.sort.replace(new_sort.to_vec());

        let group = group.unwrap_or(*o.group);
        let sort = sort.as_deref().unwrap_or(&o.sort[..]);

        self.data.reorder(group, sort, *o.group, &o.sort[..], id)
    }

    /// Test if the given group is empty.
    #[inline]
    pub(crate) fn is_empty(&self, group: RemoteId) -> bool {
        let group = as_group(group);
        self.data
            .children
            .get(&group)
            .map(|s| s.is_empty())
            .unwrap_or(true)
    }

    /// Get the number of children in the given group.
    pub(crate) fn child_count(&self, group: RemoteId) -> usize {
        let group = as_group(group);
        self.data
            .children
            .get(&group)
            .map(|s| s.len())
            .unwrap_or_default()
    }

    /// Get the children of the given group, sorted by their sort key.
    pub(crate) fn iter(&self, group: RemoteId) -> impl DoubleEndedIterator<Item = RemoteId> {
        let group = as_group(group);

        self.data
            .children
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
            order: self,
            visited: HashSet::with_capacity(self.data.children.len()),
            stack: self
                .data
                .children
                .get(&id)
                .map(|s| s.iter())
                .into_iter()
                .collect(),
        }
    }

    pub(crate) fn remove(&mut self, id: RemoteId) -> bool {
        let Some(o) = self.objects.remove(&id) else {
            return false;
        };

        self.data.remove(*o.group, &o.sort, id)
    }

    /// Insert an object into the hierarchy. Does nothing if the object has no sort key.
    pub(crate) fn insert(&mut self, object: &Object) -> bool {
        if object.is_global() {
            return false;
        }

        let data = ObjectData {
            group: object.group.clone(),
            sort: object.sort.clone(),
        };

        let mut update = false;
        update |= self.data.insert(object);

        if self.objects.insert(object.id, data).is_none() {
            update = true;
        }

        update
    }

    /// Extend the hierarchy with the given objects.
    pub(crate) fn extend<'a>(&mut self, objects: impl IntoIterator<Item = &'a Object>) {
        for object in objects {
            self.insert(object);
        }
    }

    pub(crate) fn retain(&mut self, mut f: impl FnMut(PeerId) -> bool + Copy) {
        self.data.children.retain(move |group, children| {
            if !f(group.peer_id) {
                return false;
            }

            children.retain(move |id| f(id.id.peer_id));
            !children.is_empty()
        });

        self.objects.retain(move |id, _| f(id.peer_id));
    }
}

fn as_group(id: RemoteId) -> RemoteId {
    if id.id.is_zero() {
        return RemoteId::ZERO;
    }

    id
}

pub(crate) struct Walk<'a> {
    order: &'a OrderRef,
    visited: HashSet<RemoteId>,
    stack: Vec<Iter<'a, SortKey>>,
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

            if let Some(children) = self.order.data.children.get(&node.id) {
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

            if let Some(children) = self.order.data.children.get(&node.id) {
                self.stack.push(children.iter());
            }

            if self.visited.insert(node.id) {
                return Some(node.id);
            }
        }
    }
}
