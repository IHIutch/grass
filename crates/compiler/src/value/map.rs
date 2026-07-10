use std::{cell::RefCell, rc::Rc, slice::Iter, vec::IntoIter};

use codemap::Spanned;
use rustc_hash::FxHashMap;

use crate::{
    common::{Brackets, ListSeparator},
    value::Value,
};

use super::key_hash::value_key_hash;

/// Below this many entries, a linear scan is as fast as (and simpler and
/// smaller than) hashing, so no index is built. Chosen to match the spike's
/// synthetic bench threshold recommendation (Plan 019 / solo scratchpad #68).
const INDEX_THRESHOLD: usize = 8;

type PositionIndex = FxHashMap<u64, Vec<u32>>;

/// A Sass map type. The inner Vec is Rc-wrapped so that cloning a SassMap
/// (which happens on every variable lookup) is O(1) instead of deep-copying
/// all keys and values. Mutations use Rc::make_mut for copy-on-write.
///
/// `index` is a lazily-built, per-instance cache mapping
/// [`value_key_hash`] of a key to the positions in `entries` that hash to it
/// (usually exactly one position, absent hash collisions). It accelerates
/// `get_ref`/`contains`/`insert` from O(n) to O(1) amortized for maps at or
/// above [`INDEX_THRESHOLD`] entries. It is only ever consulted against
/// THIS instance's `entries`, so sharing it (cheaply, via the inner `Rc`)
/// with another `SassMap` clone is safe: as soon as either clone performs a
/// structural mutation (insert-of-new-key or remove), that instance resets
/// its OWN `index` field to `None` (see `invalidate_index`) without
/// affecting the other clone's copy of the `Option<Rc<..>>`, which remains
/// valid for the entries it still points to.
#[derive(Debug, Clone, Default)]
pub struct SassMap {
    entries: Rc<Vec<(Spanned<Value>, Value)>>,
    index: RefCell<Option<Rc<PositionIndex>>>,
}

impl PartialEq for SassMap {
    fn eq(&self, other: &Self) -> bool {
        // Fast path: same Rc pointer means same data
        if Rc::ptr_eq(&self.entries, &other.entries) {
            return true;
        }
        if self.entries.len() != other.entries.len() {
            return false;
        }
        for (key, value) in self.entries.iter() {
            match other.get_ref(&key.node) {
                Some(value2) if value == value2 => {}
                _ => return false,
            }
        }
        true
    }
}

impl Eq for SassMap {}

impl SassMap {
    pub fn new() -> SassMap {
        SassMap {
            entries: Rc::new(Vec::new()),
            index: RefCell::new(None),
        }
    }

    pub fn new_with(elements: Vec<(Spanned<Value>, Value)>) -> SassMap {
        SassMap {
            entries: Rc::new(elements),
            index: RefCell::new(None),
        }
    }

    /// Builds `self.index` if `entries` is large enough to warrant one and
    /// it isn't already built. A no-op otherwise.
    fn ensure_index(&self) {
        if self.entries.len() < INDEX_THRESHOLD || self.index.borrow().is_some() {
            return;
        }

        let mut positions: PositionIndex = FxHashMap::default();
        for (i, (k, ..)) in self.entries.iter().enumerate() {
            positions
                .entry(value_key_hash(&k.node))
                .or_default()
                .push(i as u32);
        }
        *self.index.borrow_mut() = Some(Rc::new(positions));
    }

    /// Structural changes (a new key appearing, or any key disappearing)
    /// move entries around in ways the index doesn't track; drop it so it's
    /// rebuilt lazily on next lookup. Overwriting an EXISTING key's value
    /// does NOT need this, since neither positions nor keys move.
    fn invalidate_index(&mut self) {
        *self.index.get_mut() = None;
    }

    /// Returns the position of `key` in `entries`, using the index if one is
    /// built (building it first if `entries` is large enough to warrant it),
    /// falling back to a linear scan otherwise.
    fn find(&self, key: &Value) -> Option<usize> {
        self.ensure_index();

        let index_guard = self.index.borrow();
        if let Some(index) = index_guard.as_ref() {
            let hash = value_key_hash(key);
            return index
                .get(&hash)
                .into_iter()
                .flatten()
                .map(|&pos| pos as usize)
                .find(|&pos| &self.entries[pos].0.node == key);
        }
        drop(index_guard);

        self.entries.iter().position(|(k, ..)| &k.node == key)
    }

    pub fn get_ref(&self, key: &Value) -> Option<&Value> {
        self.find(key).map(|pos| &self.entries[pos].1)
    }

    pub fn remove(&mut self, key: &Value) {
        Rc::make_mut(&mut self.entries).retain(|(ref k, ..)| k.not_equals(key));
        self.invalidate_index();
    }

    pub fn merge(&mut self, other: SassMap) {
        for (key, value) in other {
            self.insert(key, value);
        }
    }

    pub fn iter(&self) -> Iter<'_, (Spanned<Value>, Value)> {
        self.entries.iter()
    }

    pub fn keys(self) -> Vec<Value> {
        self.into_vec().into_iter().map(|(k, ..)| k.node).collect()
    }

    pub fn values(self) -> Vec<Value> {
        self.into_vec().into_iter().map(|(.., v)| v).collect()
    }

    pub fn contains(&self, key: &Value) -> bool {
        self.find(key).is_some()
    }

    pub fn as_list(self) -> Vec<Value> {
        self.into_vec()
            .into_iter()
            .map(|(k, v)| {
                Value::List(
                    Rc::new(vec![k.node, v]),
                    ListSeparator::Space,
                    Brackets::None,
                )
            })
            .collect()
    }

    /// Returns true if the key already exists
    pub fn insert(&mut self, key: Spanned<Value>, value: Value) -> bool {
        if let Some(pos) = self.find(&key.node) {
            // Overwrite in place: original key Spanned and position are
            // preserved, and since neither moved, the index (if any) stays
            // valid -- no invalidation needed.
            Rc::make_mut(&mut self.entries)[pos].1 = value;
            return true;
        }

        let hash = value_key_hash(&key.node);
        let new_pos = self.entries.len() as u32;
        Rc::make_mut(&mut self.entries).push((key, value));

        // Extend the index in place rather than invalidating it, if one is
        // already built: appending a new entry doesn't move any existing
        // position, so the cheaper update is correct.
        let mut index_guard = self.index.borrow_mut();
        if let Some(index) = index_guard.as_mut() {
            Rc::make_mut(index).entry(hash).or_default().push(new_pos);
        }

        false
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Unwrap the Arc, cloning the inner Vec only if there are other references.
    fn into_vec(self) -> Vec<(Spanned<Value>, Value)> {
        Rc::try_unwrap(self.entries).unwrap_or_else(|arc| (*arc).clone())
    }
}

impl IntoIterator for SassMap {
    type Item = (Spanned<Value>, Value);
    type IntoIter = IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}
