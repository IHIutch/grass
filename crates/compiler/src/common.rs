use std::{
    fmt::{self, Display, Write},
    mem,
};

use crate::interner::InternedString;

/// Small insertion-ordered map keyed by [`Identifier`] (named/keyword-argument
/// maps: `ArgumentInvocation::named`, `ArgumentResult::named`,
/// `ArgList::keywords`). These maps are almost always tiny (a handful of
/// entries at most), where a hash table's extra allocation (entries + index)
/// costs more than a linear scan saves -- measured as a consistent ~1-3%
/// instructions-retired regression on USWDS/Bootstrap versus a plain
/// `Vec`-backed scan. Named-argument lookups are keyed on the argument's
/// parameter name, whose identifier equality is a cheap interned-id compare,
/// so `contains_key`/`get`/`insert` staying O(n) is a non-issue at these
/// sizes.
#[derive(Debug, Clone)]
pub struct SmallOrderedMap<K, V> {
    entries: Vec<(K, V)>,
}

impl<K, V> Default for SmallOrderedMap<K, V> {
    fn default() -> Self {
        Self { entries: Vec::new() }
    }
}

impl<K: PartialEq + Copy, V> SmallOrderedMap<K, V> {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.entries.iter().any(|(k, _)| k == key)
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Insert a key/value pair. If the key already exists, its value is
    /// replaced in place (position preserved); otherwise the pair is appended.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if let Some((_, existing)) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            Some(mem::replace(existing, value))
        } else {
            self.entries.push((key, value));
            None
        }
    }

    /// Remove a key/value pair, preserving the relative order of the
    /// remaining entries (mirrors `IndexMap::shift_remove`).
    pub fn shift_remove(&mut self, key: &K) -> Option<V> {
        let idx = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(idx).1)
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.entries.iter().map(|(k, _)| k)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }
}

impl<'a, K, V> IntoIterator for &'a SmallOrderedMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = std::iter::Map<std::slice::Iter<'a, (K, V)>, fn(&'a (K, V)) -> (&'a K, &'a V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter().map(|(k, v)| (k, v))
    }
}

impl<K, V> IntoIterator for SmallOrderedMap<K, V> {
    type Item = (K, V);
    type IntoIter = std::vec::IntoIter<(K, V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

/// Read-only interface shared by [`SmallOrderedMap`] (used by
/// `ArgumentResult::named`, a runtime, mutable map) and a plain
/// `[(K, V)]` slice (used by `ArgumentInvocation::named`, an immutable
/// arena slice -- see Plan 091 / todo #276), so callers like
/// [`ArgumentDeclaration::verify`](crate::ast::ArgumentDeclaration::verify)
/// can accept either kind of "named arguments" container without caring
/// which backing storage the caller has. Iteration order matches
/// insertion order for both (dart `LinkedHashMap` parity).
pub trait NamedArgsView<K, V> {
    fn contains_key(&self, key: &K) -> bool;
    fn get(&self, key: &K) -> Option<&V>;
    fn is_empty(&self) -> bool;
    fn len(&self) -> usize;
    fn keys<'s>(&'s self) -> impl Iterator<Item = &'s K>
    where
        K: 's;
}

impl<K: PartialEq + Copy, V> NamedArgsView<K, V> for SmallOrderedMap<K, V> {
    fn contains_key(&self, key: &K) -> bool {
        SmallOrderedMap::contains_key(self, key)
    }

    fn get(&self, key: &K) -> Option<&V> {
        SmallOrderedMap::get(self, key)
    }

    fn is_empty(&self) -> bool {
        SmallOrderedMap::is_empty(self)
    }

    fn len(&self) -> usize {
        SmallOrderedMap::len(self)
    }

    fn keys<'s>(&'s self) -> impl Iterator<Item = &'s K>
    where
        K: 's,
    {
        SmallOrderedMap::keys(self)
    }
}

impl<K: PartialEq, V> NamedArgsView<K, V> for [(K, V)] {
    fn contains_key(&self, key: &K) -> bool {
        self.iter().any(|(k, _)| k == key)
    }

    fn get(&self, key: &K) -> Option<&V> {
        self.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    fn is_empty(&self) -> bool {
        <[(K, V)]>::is_empty(self)
    }

    fn len(&self) -> usize {
        <[(K, V)]>::len(self)
    }

    fn keys<'s>(&'s self) -> impl Iterator<Item = &'s K>
    where
        K: 's,
    {
        self.iter().map(|(k, _)| k)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum UnaryOp {
    Plus,
    Neg,
    Div,
    Not,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BinaryOp {
    SingleEq,
    Equal,
    NotEqual,
    GreaterThan,
    GreaterThanEqual,
    LessThan,
    LessThanEqual,
    Plus,
    Minus,
    Mul,
    Div,
    Rem,
    And,
    Or,
}

impl BinaryOp {
    pub fn precedence(self) -> u8 {
        match self {
            Self::SingleEq => 0,
            Self::Or => 1,
            Self::And => 2,
            Self::Equal | Self::NotEqual => 3,
            Self::GreaterThan | Self::GreaterThanEqual | Self::LessThan | Self::LessThanEqual => 4,
            Self::Plus | Self::Minus => 5,
            Self::Mul | Self::Div | Self::Rem => 6,
        }
    }
}

impl BinaryOp {
    pub fn as_bytes(self) -> &'static [u8] {
        match self {
            BinaryOp::SingleEq => b"=",
            BinaryOp::Equal => b"==",
            BinaryOp::NotEqual => b"!=",
            BinaryOp::GreaterThanEqual => b">=",
            BinaryOp::LessThanEqual => b"<=",
            BinaryOp::GreaterThan => b">",
            BinaryOp::LessThan => b"<",
            BinaryOp::Plus => b"+",
            BinaryOp::Minus => b"-",
            BinaryOp::Mul => b"*",
            BinaryOp::Div => b"/",
            BinaryOp::Rem => b"%",
            BinaryOp::And => b"and",
            BinaryOp::Or => b"or",
        }
    }
}

impl Display for BinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinaryOp::SingleEq => write!(f, "="),
            BinaryOp::Equal => write!(f, "=="),
            BinaryOp::NotEqual => write!(f, "!="),
            BinaryOp::GreaterThanEqual => write!(f, ">="),
            BinaryOp::LessThanEqual => write!(f, "<="),
            BinaryOp::GreaterThan => write!(f, ">"),
            BinaryOp::LessThan => write!(f, "<"),
            BinaryOp::Plus => write!(f, "+"),
            BinaryOp::Minus => write!(f, "-"),
            BinaryOp::Mul => write!(f, "*"),
            BinaryOp::Div => write!(f, "/"),
            BinaryOp::Rem => write!(f, "%"),
            BinaryOp::And => write!(f, "and"),
            BinaryOp::Or => write!(f, "or"),
        }
    }
}

/// Strings can either have quotes or not
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum QuoteKind {
    Quoted,
    None,
}

impl Display for QuoteKind {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Quoted => f.write_char('"'),
            Self::None => Ok(()),
        }
    }
}

/// Lists can either be bracketed or not
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Brackets {
    None,
    Bracketed,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ListSeparator {
    Space,
    Comma,
    Slash,
    Undecided,
}

impl ListSeparator {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Space | Self::Undecided => " ",
            Self::Comma => ", ",
            Self::Slash => " / ",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Space | Self::Undecided => "space",
            Self::Comma => "comma",
            Self::Slash => "slash",
        }
    }
}

/// In Sass, underscores and hyphens are considered equal when inside identifiers.
///
/// This struct protects that invariant by normalizing all underscores into hyphens.
#[derive(Clone, Eq, PartialEq, Hash, PartialOrd, Ord, Copy)]
pub struct Identifier(InternedString);

impl fmt::Debug for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Identifier")
            .field(&self.0.to_string())
            .finish()
    }
}

impl Identifier {
    fn from_str(s: &str) -> Self {
        if s.contains('_') {
            Identifier(InternedString::get_or_intern(s.replace('_', "-")))
        } else {
            Identifier(InternedString::get_or_intern(s))
        }
    }

    /// Create an identifier without normalizing underscores to dashes.
    /// Used for module namespace lookups, which are dash-sensitive.
    pub fn verbatim(s: &str) -> Self {
        Identifier(InternedString::get_or_intern(s))
    }

    pub fn is_public(&self) -> bool {
        !self.as_str().starts_with('-')
    }
}

impl From<String> for Identifier {
    fn from(s: String) -> Identifier {
        Self::from_str(&s)
    }
}

impl From<&String> for Identifier {
    fn from(s: &String) -> Identifier {
        Self::from_str(s)
    }
}

impl From<&str> for Identifier {
    fn from(s: &str) -> Identifier {
        Self::from_str(s)
    }
}

impl Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Identifier {
    pub fn as_str(&self) -> &str {
        self.0.resolve_ref()
    }
}

/// Returns `name` without a vendor prefix.
///
/// If `name` has no vendor prefix, it's returned as-is.
pub(crate) fn unvendor(name: &str) -> &str {
    let bytes = name.as_bytes();

    if bytes.len() < 2 {
        return name;
    }

    if bytes.first() != Some(&b'-') || bytes.get(1_usize) == Some(&b'-') {
        return name;
    }

    for i in 2..bytes.len() {
        if bytes.get(i) == Some(&b'-') {
            return &name[i + 1..];
        }
    }

    name
}
