use std::collections::btree_set::Iter;
use std::collections::{BTreeSet, HashMap};

use api::Id;

use crate::objects::LocalObject;

#[derive(Default)]
pub(crate) struct Hierarchy {
    inner: HashMap<Id, BTreeSet<(Vec<u8>, Id)>>,
}

impl Hierarchy {
    /// Get the children of the given group, sorted by their sort key.
    pub(crate) fn iter(&self, group: Id) -> impl DoubleEndedIterator<Item = Id> {
        self.inner
            .get(&group)
            .into_iter()
            .flatten()
            .map(|(_, id)| *id)
    }

    pub(crate) fn iter_all(&self) -> impl DoubleEndedIterator<Item = Id> {
        Walk {
            inner: &self.inner,
            stack: self
                .inner
                .get(&Id::ZERO)
                .map(|s| s.iter())
                .into_iter()
                .collect(),
        }
    }

    /// Remove the given id from all groups.
    pub(crate) fn remove(&mut self, group: Id, sort: Vec<u8>, id: Id) {
        let key = (sort, id);

        if let Some(values) = self.inner.get_mut(&group) {
            values.remove(&key);
        }
    }

    /// Insert a child into the given group with the given sort key.
    pub(crate) fn insert(&mut self, group: Id, sort: Vec<u8>, id: Id) {
        self.inner.entry(group).or_default().insert((sort, id));
    }

    /// Extend the hierarchy with the given objects.
    pub(crate) fn extend<'a>(&mut self, objects: impl IntoIterator<Item = &'a LocalObject>) {
        for object in objects {
            self.inner
                .entry(*object.group)
                .or_default()
                .insert((object.sort().to_vec(), object.id));
        }
    }
}

pub(crate) struct Walk<'a> {
    inner: &'a HashMap<Id, BTreeSet<(Vec<u8>, Id)>>,
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

            if let Some(children) = self.inner.get(id) {
                self.stack.push(children.iter());
            }

            return Some(*id);
        }
    }
}

impl DoubleEndedIterator for Walk<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        tracing::warn!("here...");

        loop {
            let iter = self.stack.last_mut()?;

            let Some((_, id)) = iter.next_back() else {
                self.stack.pop();
                continue;
            };

            if let Some(children) = self.inner.get(id) {
                self.stack.push(children.iter());
            }

            return Some(*id);
        }
    }
}
