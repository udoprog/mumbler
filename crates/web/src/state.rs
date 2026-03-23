use core::fmt;
use core::mem;
use core::ops::{Deref, DerefMut};

/// A wrapper around a value that tracks whether it has changed.
///
/// The [`update`] method assigns a new value and returns `true` if the value
/// actually changed (i.e. `new != old`), allowing callers to skip unnecessary
/// re-renders or redraws when the value is already up to date.
///
/// All other accesses go through [`Deref`] / [`DerefMut`], so the wrapper is
/// transparent to code that just reads or directly mutates the inner value.
pub(crate) struct State<T>(T);

impl<T> State<T> {
    #[inline]
    pub(crate) const fn new(value: T) -> Self {
        Self(value)
    }
}

impl<T> State<T>
where
    T: PartialEq,
{
    /// Assign `new` to the inner value.
    ///
    /// Returns `true` if the value changed.
    #[inline]
    pub(crate) fn update(&mut self, new: T) -> bool {
        if self.0 == new {
            return false;
        }
        self.0 = new;
        true
    }

    /// Replace the inner value and return the old one.
    #[inline]
    pub(crate) fn replace(&mut self, new: T) -> Option<T> {
        if self.0 == new {
            return None;
        }

        Some(mem::replace(&mut self.0, new))
    }
}

impl<T> Deref for State<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for State<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T> Default for State<T>
where
    T: Default,
{
    #[inline]
    fn default() -> Self {
        Self(T::default())
    }
}

impl<T> Clone for State<T>
where
    T: Clone,
{
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> fmt::Debug for State<T>
where
    T: fmt::Debug,
{
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
