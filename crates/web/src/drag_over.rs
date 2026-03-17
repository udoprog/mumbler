use api::Id;

use crate::hierarchy::HierarchyRef;
use crate::objects::ObjectsRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drag {
    Above,
    Into,
    Below,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct DragOver {
    drag: Drag,
    pub(crate) group: Id,
    pub(crate) target: Id,
}

impl DragOver {
    #[inline]
    pub(crate) const fn above(group: Id, target: Id) -> Self {
        Self {
            drag: Drag::Above,
            group,
            target,
        }
    }

    #[inline]
    pub(crate) const fn below(group: Id, target: Id) -> Self {
        Self {
            drag: Drag::Below,
            group,
            target,
        }
    }

    #[inline]
    pub(crate) const fn into(group: Id, target: Id) -> Self {
        Self {
            drag: Drag::Into,
            group,
            target,
        }
    }

    #[inline]
    pub(crate) fn target_group(&self) -> Id {
        match self.drag {
            Drag::Into => self.target,
            _ => self.group,
        }
    }

    pub(crate) fn new_sort(self, objects: &ObjectsRef, order: &HierarchyRef) -> Option<Vec<u8>> {
        let Some(target) = objects.get(self.target) else {
            return None;
        };

        let new_sort = match self.drag {
            Drag::Into => {
                // When inserting into, we insert after the last element in the group.
                let last = order
                    .iter(self.target)
                    .last()
                    .and_then(|id| Some(objects.get(id)?.sort()));

                if let Some(last) = last {
                    sorting::after(last)
                } else {
                    target.id.as_bytes().to_vec()
                }
            }
            Drag::Above => {
                let prev = order
                    .iter(self.group)
                    .rev()
                    .skip_while(|id| *id != self.target)
                    .nth(1);

                let prev = prev.and_then(|id| Some(objects.get(id)?.sort()));

                if let Some(prev) = prev {
                    sorting::midpoint(prev, target.sort())
                } else {
                    sorting::before(target.sort())
                }
            }
            Drag::Below => {
                let next = order
                    .iter(self.group)
                    .skip_while(|id| *id != self.target)
                    .nth(1);
                let next = next.and_then(|id| Some(objects.get(id)?.sort()));

                if let Some(next) = next {
                    sorting::midpoint(target.sort(), next)
                } else {
                    sorting::after(target.sort())
                }
            }
        };

        Some(new_sort)
    }
}
