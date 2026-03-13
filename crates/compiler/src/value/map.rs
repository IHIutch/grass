use std::{slice::Iter, sync::Arc, vec::IntoIter};

use codemap::Spanned;

use crate::{
    common::{Brackets, ListSeparator},
    value::Value,
};

/// A Sass map type. The inner Vec is Arc-wrapped so that cloning a SassMap
/// (which happens on every variable lookup) is O(1) instead of deep-copying
/// all keys and values. Mutations use Arc::make_mut for copy-on-write.
#[derive(Debug, Clone, Default)]
pub struct SassMap(Arc<Vec<(Spanned<Value>, Value)>>);

impl PartialEq for SassMap {
    fn eq(&self, other: &Self) -> bool {
        // Fast path: same Arc pointer means same data
        if Arc::ptr_eq(&self.0, &other.0) {
            return true;
        }
        if self.0.len() != other.0.len() {
            return false;
        }
        for (key, value) in self.0.iter() {
            if !other
                .0
                .iter()
                .any(|(key2, value2)| key.node == key2.node && value == value2)
            {
                return false;
            }
        }
        true
    }
}

impl Eq for SassMap {}

impl SassMap {
    pub fn new() -> SassMap {
        SassMap(Arc::new(Vec::new()))
    }

    pub fn new_with(elements: Vec<(Spanned<Value>, Value)>) -> SassMap {
        SassMap(Arc::new(elements))
    }

    pub fn get(self, key: &Value) -> Option<Value> {
        for (k, v) in self.into_vec() {
            if &k.node == key {
                return Some(v);
            }
        }

        None
    }

    pub fn get_ref(&self, key: &Value) -> Option<&Value> {
        for (k, v) in self.0.iter() {
            if &k.node == key {
                return Some(v);
            }
        }

        None
    }

    pub fn remove(&mut self, key: &Value) {
        Arc::make_mut(&mut self.0).retain(|(ref k, ..)| k.not_equals(key));
    }

    pub fn merge(&mut self, other: SassMap) {
        for (key, value) in other {
            self.insert(key, value);
        }
    }

    pub fn iter(&self) -> Iter<'_, (Spanned<Value>, Value)> {
        self.0.iter()
    }

    pub fn keys(self) -> Vec<Value> {
        self.into_vec()
            .into_iter()
            .map(|(k, ..)| k.node)
            .collect()
    }

    pub fn values(self) -> Vec<Value> {
        self.into_vec()
            .into_iter()
            .map(|(.., v)| v)
            .collect()
    }

    pub fn contains(&self, key: &Value) -> bool {
        self.0.iter().any(|(k, ..)| &k.node == key)
    }

    pub fn as_list(self) -> Vec<Value> {
        self.into_vec()
            .into_iter()
            .map(|(k, v)| Value::List(Arc::new(vec![k.node, v]), ListSeparator::Space, Brackets::None))
            .collect()
    }

    /// Returns true if the key already exists
    pub fn insert(&mut self, key: Spanned<Value>, value: Value) -> bool {
        let inner = Arc::make_mut(&mut self.0);
        for (ref k, ref mut v) in inner.iter_mut() {
            if k.node == key.node {
                *v = value;
                return true;
            }
        }
        inner.push((key, value));
        false
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Unwrap the Arc, cloning the inner Vec only if there are other references.
    fn into_vec(self) -> Vec<(Spanned<Value>, Value)> {
        Arc::try_unwrap(self.0).unwrap_or_else(|arc| (*arc).clone())
    }
}

impl IntoIterator for SassMap {
    type Item = (Spanned<Value>, Value);
    type IntoIter = IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}
