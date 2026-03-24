use core::num::NonZeroU32;

use std::collections::VecDeque;

use crate::hash;

/// A generator for random-look identifiers.
pub struct Ids {
    /// Random seed used for mapping sequential peer IDs to random-looking ones.
    seed: u32,
    /// The last peer identifier used.
    last: u32,
    /// Queue of freed peer identifiers that can be reused.
    free: VecDeque<NonZeroU32>,
}

impl Ids {
    /// Construct a new id allocator.
    #[inline]
    pub(crate) fn new(seed: u32) -> Self {
        Self {
            seed,
            last: 0,
            free: VecDeque::new(),
        }
    }

    /// Get the next identifier.
    #[inline]
    pub(crate) fn next(&mut self) -> Option<NonZeroU32> {
        if let Some(free) = self.free.pop_front() {
            return Some(free);
        }

        let next = NonZeroU32::new(self.last.wrapping_add(1))?;
        self.last = next.get();
        NonZeroU32::new(hash::map(next.get(), self.seed))
    }

    /// Free the specified identifier.
    #[inline]
    pub(crate) fn free(&mut self, id: u32) {
        if let Some(id) = NonZeroU32::new(id) {
            self.free.push_back(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::Ids;

    #[test]
    fn test_ids() {
        let mut ids = Ids::new(0xdeadbeef);
        let mut seen = HashSet::new();

        for _ in 0..1000 {
            let id = ids.next().unwrap();
            assert!(seen.insert(id));
        }
    }
}
