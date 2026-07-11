use std::{cell::RefCell, fmt, rc::Rc};

use rustc_hash::{FxHashMap, FxHashSet};

use crate::common::Identifier;

pub(crate) trait MapView: fmt::Debug {
    type Value;
    fn get(&self, name: Identifier) -> Option<Self::Value>;
    fn remove(&self, name: Identifier) -> Option<Self::Value>;
    fn insert(&self, name: Identifier, value: Self::Value) -> Option<Self::Value>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn contains_key(&self, k: Identifier) -> bool;
    // todo: wildly ineffecient to return vec here, because of the arbitrary nesting of Self
    fn keys(&self) -> Vec<Identifier>;
    fn iter(&self) -> Vec<(Identifier, Self::Value)>;
}

impl<T> MapView for Rc<dyn MapView<Value = T>> {
    type Value = T;
    fn get(&self, name: Identifier) -> Option<Self::Value> {
        (**self).get(name)
    }
    fn remove(&self, name: Identifier) -> Option<Self::Value> {
        (**self).remove(name)
    }
    fn insert(&self, name: Identifier, value: Self::Value) -> Option<Self::Value> {
        (**self).insert(name, value)
    }
    fn len(&self) -> usize {
        (**self).len()
    }
    fn contains_key(&self, name: Identifier) -> bool {
        (**self).contains_key(name)
    }
    fn keys(&self) -> Vec<Identifier> {
        (**self).keys()
    }

    fn iter(&self) -> Vec<(Identifier, Self::Value)> {
        (**self).iter()
    }
}

#[derive(Debug)]
pub(crate) struct BaseMapView<T>(pub Rc<RefCell<FxHashMap<Identifier, T>>>);

impl<T> Clone for BaseMapView<T> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct UnprefixedMapView<V: fmt::Debug + Clone, T: MapView<Value = V> + Clone>(
    pub T,
    pub String,
);

#[derive(Debug, Clone)]
pub(crate) struct PrefixedMapView<V: fmt::Debug + Clone, T: MapView<Value = V> + Clone>(
    pub T,
    pub String,
);

impl<T: fmt::Debug + Clone> MapView for BaseMapView<T> {
    type Value = T;
    fn get(&self, name: Identifier) -> Option<Self::Value> {
        (*self.0).borrow().get(&name).cloned()
    }

    fn contains_key(&self, name: Identifier) -> bool {
        (*self.0).borrow().contains_key(&name)
    }

    fn len(&self) -> usize {
        (*self.0).borrow().len()
    }

    fn remove(&self, name: Identifier) -> Option<Self::Value> {
        (*self.0).borrow_mut().remove(&name)
    }

    fn insert(&self, name: Identifier, value: Self::Value) -> Option<Self::Value> {
        (*self.0).borrow_mut().insert(name, value)
    }

    fn keys(&self) -> Vec<Identifier> {
        let mut keys: Vec<_> = (*self.0).borrow().keys().copied().collect();
        keys.sort();
        keys
    }

    fn iter(&self) -> Vec<(Identifier, Self::Value)> {
        let mut entries: Vec<_> = (*self.0).borrow().clone().into_iter().collect();
        entries.sort_by_key(|(k, _)| *k);
        entries
    }
}

impl<V: fmt::Debug + Clone, T: MapView<Value = V> + Clone> MapView for UnprefixedMapView<V, T> {
    type Value = V;
    fn get(&self, name: Identifier) -> Option<Self::Value> {
        let name = Identifier::from(format!("{}{}", self.1, name));
        self.0.get(name)
    }

    fn remove(&self, name: Identifier) -> Option<Self::Value> {
        let name = Identifier::from(format!("{}{}", self.1, name));
        self.0.remove(name)
    }

    fn insert(&self, name: Identifier, value: Self::Value) -> Option<Self::Value> {
        let name = Identifier::from(format!("{}{}", self.1, name));
        self.0.insert(name, value)
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn contains_key(&self, name: Identifier) -> bool {
        let name = Identifier::from(format!("{}{}", self.1, name));
        self.0.contains_key(name)
    }

    fn keys(&self) -> Vec<Identifier> {
        self.0
            .keys()
            .into_iter()
            .filter(|key| key.as_str().starts_with(&self.1))
            .map(|key| Identifier::from(key.as_str().strip_prefix(&self.1).unwrap()))
            .collect()
    }

    fn iter(&self) -> Vec<(Identifier, Self::Value)> {
        unimplemented!()
    }
}

impl<V: fmt::Debug + Clone, T: MapView<Value = V> + Clone> MapView for PrefixedMapView<V, T> {
    type Value = V;
    fn get(&self, name: Identifier) -> Option<Self::Value> {
        if !name.as_str().starts_with(&self.1) {
            return None;
        }

        let name = Identifier::from(name.as_str().strip_prefix(&self.1).unwrap());

        self.0.get(name)
    }

    fn remove(&self, name: Identifier) -> Option<Self::Value> {
        if !name.as_str().starts_with(&self.1) {
            return None;
        }

        let name = Identifier::from(name.as_str().strip_prefix(&self.1).unwrap());

        self.0.remove(name)
    }

    fn insert(&self, name: Identifier, value: Self::Value) -> Option<Self::Value> {
        if !name.as_str().starts_with(&self.1) {
            return None;
        }

        let name = Identifier::from(name.as_str().strip_prefix(&self.1).unwrap());

        self.0.insert(name, value)
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn contains_key(&self, name: Identifier) -> bool {
        if !name.as_str().starts_with(&self.1) {
            return false;
        }

        let name = Identifier::from(name.as_str().strip_prefix(&self.1).unwrap());

        self.0.contains_key(name)
    }

    fn keys(&self) -> Vec<Identifier> {
        self.0
            .keys()
            .into_iter()
            .map(|key| Identifier::from(format!("{}{}", self.1, key)))
            .collect()
    }

    fn iter(&self) -> Vec<(Identifier, Self::Value)> {
        unimplemented!()
    }
}

/// A mostly-unmodifiable view of a map that only allows certain keys to be
/// accessed.
///
/// Whether or not the underlying map contains keys that aren't allowed, this
/// view will behave as though it doesn't contain them.
///
/// The underlying map's values may change independently of this view, but its
/// set of keys may not.
///
/// This is unmodifiable *except for the [remove] method*, which is used for
/// `@used with` to mark configured variables as used.
#[derive(Debug, Clone)]
pub(crate) struct LimitedMapView<V: fmt::Debug + Clone, T: MapView<Value = V> + Clone>(
    pub T,
    pub FxHashSet<Identifier>,
);

impl<V: fmt::Debug + Clone, T: MapView<Value = V> + Clone> LimitedMapView<V, T> {
    pub fn safelist(map: T, keys: &FxHashSet<Identifier>) -> Self {
        let keys = keys
            .iter()
            .copied()
            .filter(|key| map.contains_key(*key))
            .collect();

        Self(map, keys)
    }

    pub fn blocklist(map: T, blocklist: &FxHashSet<Identifier>) -> Self {
        let keys = map
            .keys()
            .into_iter()
            .filter(|key| !blocklist.contains(key))
            .collect();

        Self(map, keys)
    }
}

impl<V: fmt::Debug + Clone, T: MapView<Value = V> + Clone> MapView for LimitedMapView<V, T> {
    type Value = V;
    fn get(&self, name: Identifier) -> Option<Self::Value> {
        if !self.1.contains(&name) {
            return None;
        }

        self.0.get(name)
    }

    fn remove(&self, name: Identifier) -> Option<Self::Value> {
        if !self.1.contains(&name) {
            return None;
        }

        self.0.remove(name)
    }

    fn insert(&self, name: Identifier, value: Self::Value) -> Option<Self::Value> {
        if !self.1.contains(&name) {
            return None;
        }

        self.0.insert(name, value)
    }

    fn len(&self) -> usize {
        self.1.len()
    }

    fn contains_key(&self, name: Identifier) -> bool {
        if !self.1.contains(&name) {
            return false;
        }

        self.0.contains_key(name)
    }

    fn keys(&self) -> Vec<Identifier> {
        self.1.iter().copied().collect()
    }

    fn iter(&self) -> Vec<(Identifier, Self::Value)> {
        unimplemented!()
    }
}

/// A view over several forwarded module maps, resolved in reverse order so
/// that later entries (and ultimately the local module, always last) shadow
/// earlier ones.
///
/// The key SET of a `MergedMapView` is frozen once constructed: `insert`
/// only ever overwrites the value for a key that already exists in one of
/// the wrapped submaps (it panics otherwise, see below), and `remove` is
/// never called on this type (module members can't be removed through it —
/// `@use ... with` configuration removal goes through a separate
/// `Configuration` map, not through `ModuleScope`'s `MapView`s). This makes
/// it safe to cache a name -> submap-index lookup table with no
/// invalidation logic: the table is built once, lazily, on first use.
#[derive(Debug)]
pub(crate) struct MergedMapView<V: fmt::Debug + Clone> {
    maps: Vec<Rc<dyn MapView<Value = V>>>,
    unique_keys: FxHashSet<Identifier>,
    /// name -> index into `maps`, built lazily in forward iteration order
    /// (last writer wins) so it agrees with `.iter().rev().find_map(..)`.
    index: RefCell<Option<FxHashMap<Identifier, usize>>>,
}

impl<V: fmt::Debug + Clone> MergedMapView<V> {
    pub fn new(maps: Vec<Rc<dyn MapView<Value = V>>>) -> Self {
        let unique_keys: FxHashSet<Identifier> =
            maps.iter().fold(FxHashSet::default(), |mut keys, map| {
                keys.extend(&map.keys());
                keys
            });

        Self {
            maps,
            unique_keys,
            index: RefCell::new(None),
        }
    }

    fn submap_index(&self, name: Identifier) -> Option<usize> {
        if self.index.borrow().is_none() {
            let mut built = FxHashMap::default();
            for (idx, map) in self.maps.iter().enumerate() {
                for key in map.keys() {
                    built.insert(key, idx);
                }
            }
            *self.index.borrow_mut() = Some(built);
        }

        self.index.borrow().as_ref().unwrap().get(&name).copied()
    }
}

impl<V: fmt::Debug + Clone> MapView for MergedMapView<V> {
    type Value = V;
    fn get(&self, name: Identifier) -> Option<Self::Value> {
        let idx = self.submap_index(name)?;
        self.maps[idx].get(name)
    }

    fn remove(&self, _name: Identifier) -> Option<Self::Value> {
        unimplemented!()
    }

    fn len(&self) -> usize {
        self.unique_keys.len()
    }

    fn contains_key(&self, name: Identifier) -> bool {
        self.submap_index(name).is_some()
    }

    fn insert(&self, name: Identifier, value: Self::Value) -> Option<Self::Value> {
        if let Some(idx) = self.submap_index(name) {
            return self.maps[idx].insert(name, value);
        }

        unreachable!("New entries may not be added to MergedMapView")
    }

    fn keys(&self) -> Vec<Identifier> {
        let mut keys: Vec<_> = self.unique_keys.iter().copied().collect();
        keys.sort();
        keys
    }

    fn iter(&self) -> Vec<(Identifier, Self::Value)> {
        let mut keys: Vec<_> = self.unique_keys.iter().copied().collect();
        keys.sort();
        keys.into_iter()
            .map(|name| (name, self.get(name).unwrap()))
            .collect()
    }
}

#[cfg(test)]
mod merged_map_view_tests {
    use super::*;

    fn stub(entries: &[(&str, i32)]) -> Rc<dyn MapView<Value = i32>> {
        let map: FxHashMap<Identifier, i32> = entries
            .iter()
            .map(|(k, v)| (Identifier::from(*k), *v))
            .collect();
        Rc::new(BaseMapView(Rc::new(RefCell::new(map))))
    }

    #[test]
    fn precedence_matches_reverse_scan() {
        // Two stub maps sharing a key: the later map in the vec (index 1)
        // must win, matching `.iter().rev().find_map(..)` on the old path.
        let first = stub(&[("shared", 1), ("only-first", 10)]);
        let second = stub(&[("shared", 2), ("only-second", 20)]);

        let merged = MergedMapView::new(vec![first, second]);

        assert_eq!(merged.get(Identifier::from("shared")), Some(2));
        assert_eq!(merged.get(Identifier::from("only-first")), Some(10));
        assert_eq!(merged.get(Identifier::from("only-second")), Some(20));
        assert_eq!(merged.get(Identifier::from("missing")), None);
    }

    #[test]
    fn contains_key_matches_get() {
        let first = stub(&[("shared", 1)]);
        let second = stub(&[("shared", 2), ("only-second", 20)]);

        let merged = MergedMapView::new(vec![first, second]);

        assert!(merged.contains_key(Identifier::from("shared")));
        assert!(merged.contains_key(Identifier::from("only-second")));
        assert!(!merged.contains_key(Identifier::from("missing")));
    }

    #[test]
    fn live_value_mutation_through_submap_is_visible() {
        let first = stub(&[("only-first", 10)]);
        let second = stub(&[("shared", 2)]);

        let merged = MergedMapView::new(vec![Rc::clone(&first), Rc::clone(&second)]);

        // Build the index via a first get, then mutate the winning submap
        // directly (as `MergedMapView::insert` does) and confirm the change
        // is visible through the already-built index.
        assert_eq!(merged.get(Identifier::from("shared")), Some(2));
        assert_eq!(second.insert(Identifier::from("shared"), 99), Some(2));
        assert_eq!(merged.get(Identifier::from("shared")), Some(99));
    }

    #[test]
    fn insert_overwrites_existing_key_via_index() {
        let first = stub(&[("only-first", 10)]);
        let second = stub(&[("shared", 2)]);

        let merged = MergedMapView::new(vec![first, second]);

        assert_eq!(merged.insert(Identifier::from("shared"), 42), Some(2));
        assert_eq!(merged.get(Identifier::from("shared")), Some(42));
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PublicMemberMapView<V: fmt::Debug + Clone, T: MapView<Value = V> + Clone>(pub T);

impl<V: fmt::Debug + Clone, T: MapView<Value = V> + Clone> MapView for PublicMemberMapView<V, T> {
    type Value = V;
    fn get(&self, name: Identifier) -> Option<Self::Value> {
        if !name.is_public() {
            return None;
        }

        self.0.get(name)
    }

    fn remove(&self, name: Identifier) -> Option<Self::Value> {
        if !name.is_public() {
            return None;
        }

        self.0.remove(name)
    }

    fn insert(&self, name: Identifier, value: Self::Value) -> Option<Self::Value> {
        if !name.is_public() {
            return None;
        }

        self.0.insert(name, value)
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn contains_key(&self, name: Identifier) -> bool {
        if !name.is_public() {
            return false;
        }

        self.0.contains_key(name)
    }

    fn keys(&self) -> Vec<Identifier> {
        self.0
            .keys()
            .iter()
            .copied()
            .filter(Identifier::is_public)
            .collect()
    }

    fn iter(&self) -> Vec<(Identifier, Self::Value)> {
        self.0
            .iter()
            .into_iter()
            .filter(|(name, _)| Identifier::is_public(name))
            .collect()
    }
}
