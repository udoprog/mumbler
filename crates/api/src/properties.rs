use core::fmt;

use std::collections::HashMap;
use std::collections::hash_map::{Entry, IntoIter};

use musli_core::{Decode, Encode};

use crate::{Key, Value, ValueKind};

#[derive(Default, Clone, Encode, Decode)]
#[musli(crate = musli_core, transparent)]
pub struct Properties {
    /// Global values.
    values: HashMap<Key, Value>,
}

impl Properties {
    /// Construct a new empty set of properties.
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
        }
    }

    /// Iterate over properties.
    pub fn iter(&self) -> impl Iterator<Item = (Key, &Value)> {
        self.values.iter().map(|(&k, v)| (k, v))
    }

    /// Get the value of a property by key.
    #[inline]
    pub fn get(&self, key: Key) -> &Value {
        static DEFAULT: Value = Value::empty();
        self.values.get(&key).unwrap_or(&DEFAULT)
    }

    /// Get the mutable value of a property by key.
    #[inline]
    pub fn into_mut(&mut self, key: Key) -> &mut Value {
        self.values.entry(key).or_default()
    }

    /// Test if the set of properties contains the given key.
    #[inline]
    pub fn contains(&self, key: Key) -> bool {
        self.values.contains_key(&key)
    }

    /// Insert or update a property value by key.
    ///
    /// Inserting an [`Value::empty`] value is the equivalent of removing it.
    #[inline]
    pub fn insert(&mut self, key: Key, value: impl Into<Value>) -> Value {
        self._insert(key, value.into())
    }

    fn _insert(&mut self, key: Key, value: Value) -> Value {
        if value.is_empty() {
            return self.remove(key);
        }

        let Some(value) = self.values.insert(key, value) else {
            return Value::empty();
        };

        value
    }

    /// Update a property if it is different from the given value.
    #[inline]
    pub fn update(&mut self, key: Key, other: impl Into<Value>) -> bool {
        let other = other.into();
        self._update(key, other)
    }

    fn _update(&mut self, key: Key, other: Value) -> bool {
        match self.values.entry(key) {
            Entry::Vacant(e) => {
                if other.is_empty() {
                    return false;
                }

                e.insert(other);
                true
            }
            Entry::Occupied(mut e) => {
                let this = e.get_mut();

                match (this.as_kind(), other.as_kind()) {
                    (ValueKind::Float(a), ValueKind::Float(b)) => {
                        if (*a - b).abs() < f64::EPSILON {
                            return false;
                        }
                    }
                    (this, other) => {
                        if other.is_empty() {
                            e.remove();
                            return true;
                        }

                        if *this == *other {
                            return false;
                        }
                    }
                }

                *this = other;
                true
            }
        }
    }

    /// Remove a property by key.
    #[inline]
    pub fn remove(&mut self, key: Key) -> Value {
        let Some(value) = self.values.remove(&key) else {
            return Value::empty();
        };

        value
    }
}

impl fmt::Debug for Properties {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.values.iter()).finish()
    }
}

impl IntoIterator for Properties {
    type Item = (Key, Value);
    type IntoIter = IntoIter<Key, Value>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.values.into_iter()
    }
}

impl FromIterator<(Key, Value)> for Properties {
    #[inline]
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (Key, Value)>,
    {
        let mut properties = Properties::new();

        for (key, value) in iter {
            properties.insert(key, value);
        }

        properties
    }
}

impl<const N: usize> From<[(Key, Value); N]> for Properties {
    #[inline]
    fn from(values: [(Key, Value); N]) -> Self {
        Self::from_iter(values)
    }
}
