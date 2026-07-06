//! Prototype for Plan 019 (SassMap indexed backing store): a hash function
//! over [`Value`] that's consistent with [`Value`]'s `PartialEq`, i.e.
//! `a == b` implies `value_key_hash(a) == value_key_hash(b)`.
//!
//! This is a SPIKE deliverable (see solo scratchpad #68 / todo #167): it
//! proves the invariant holds for the hard cases dart-sass itself has to
//! solve (fuzzy numerics, unit-comparable Dimensions, quoted/unquoted
//! strings, list/map keys) by porting dart-sass's approach
//! (`lib/src/util/number.dart` `fuzzyHashCode`, `lib/src/value/number/
//! single_unit.dart` `canonicalMultiplierForUnit`) rather than inventing a
//! new one. See the module-level tests for dart-sass 1.97.3 ground-truth
//! provenance on every hard case.
//!
//! # Design notes (why this is safe even where it's approximate)
//!
//! Hash collisions are always safe: two unequal values sharing a hash bucket
//! just costs a linear scan within that bucket (resolved by the real
//! `Value::eq`), it never produces a wrong answer. That safety valve is used
//! deliberately in a few places below:
//!
//! - **All empty containers hash identically** (`List` regardless of
//!   separator/brackets, `Map`, empty `ArgList`). `Value::eq` already treats
//!   many of these as cross-equal (`() == []`, `[] == (a: 1)` after removing
//!   `a`, etc.) in ways that are not even transitive with each other (an
//!   empty `List(Comma)` equals an empty `Map` but not an empty
//!   `List(Space)`, yet both equal the same `Map`). Rather than replicate
//!   that exact non-transitive web, every empty container gets one sentinel
//!   hash; this is a superset of the required merges and is always
//!   collision-safe.
//! - **`List` hashing ignores `Brackets`.** As of Plan 035, `Value::eq`
//!   requires an `ArgList` (always unbracketed) to match brackets with a
//!   `List` it's compared against (i.e. only `Brackets::None` lists can be
//!   eq to an `ArgList`), so this is no longer strictly required for
//!   cross-type consistency. It's kept anyway: folding brackets into plain
//!   `List` hashing (rather than giving `List` a brackets-aware hash and a
//!   brackets-blind one just for cross-comparison with `ArgList`) is
//!   simpler, and only ever causes extra (safe) collisions between e.g. `(1,
//!   2)` and `[1, 2]`, which were never equal to begin with.
//! - **`ArgList`'s `keywords` are not hashed**, matching `Value::eq`
//!   (`ArgList == ArgList` and the `ArgList == List(..)` / `List(..) ==
//!   ArgList` arms all ignore keywords, per dart-sass: `SassArgumentList`
//!   doesn't override `SassList::==`, so keywords never factor into
//!   equality). As of Plan 035, `Value::eq` is symmetric for ArgList/List
//!   comparisons (a prior asymmetry — `list == arglist` always `false` while
//!   `arglist == list` could be `true` — was a genuine bug, now fixed). This
//!   doesn't threaten hash consistency either way: consistency only
//!   requires `eq ⟹ same hash`, never the converse.
//! - **`Color`, `Calculation`, `FunctionRef`, `MixinRef` fall back to a
//!   single hash bucket per variant.** These were not in the plan's list of
//!   hard cases, and doing them justice is substantially more work: `Color`
//!   equality spans legacy vs. modern color spaces with fuzzy per-channel
//!   comparison and clamped-alpha edge cases; `Calculation` equality is a
//!   recursive AST comparison with fuzzy numeric leaves nested arbitrarily
//!   deep. Both a valid design (a recursive extension of the same fuzzy-hash
//!   approach) but are out of scope for this spike's time budget. Falling
//!   back to one bucket is always correct, just not O(1) for maps keyed
//!   heavily by colors/calculations, which is expected to be rare relative
//!   to string/number/list/map keys.

use std::hash::{Hash, Hasher};

use rustc_hash::FxHasher;

use crate::{
    common::ListSeparator,
    unit::{Unit, UnitKind},
    value::{number::fuzzy_hash_component, SassNumber, Value},
};

use super::sass_number::conversion_factor;

/// Tags used as the first thing hashed for each `Value` shape, so that e.g. a
/// number and a string never collide even if their payload hashes happen to
/// match. Kept as plain bytes rather than deriving `Hash` on an enum so nesting
/// (recursing into elements) can freely embed sub-hashes without dragging enum
/// discriminant instability into the mix.
const TAG_TRUE: u8 = 0;
const TAG_FALSE: u8 = 1;
const TAG_NULL: u8 = 2;
const TAG_STRING: u8 = 3;
const TAG_DIMENSION: u8 = 4;
const TAG_COLOR: u8 = 5;
const TAG_CALCULATION: u8 = 6;
const TAG_FUNCTION_REF: u8 = 7;
const TAG_MIXIN_REF: u8 = 8;
const TAG_LIST: u8 = 9;
const TAG_MAP: u8 = 10;
/// Shared by every empty container (List of any separator/brackets, Map,
/// empty ArgList) -- see module docs for why this is safe.
const TAG_EMPTY_CONTAINER: u8 = 11;

pub(crate) fn value_key_hash(value: &Value) -> u64 {
    let mut hasher = FxHasher::default();
    hash_value_into(value, &mut hasher);
    hasher.finish()
}

fn hash_value_into(value: &Value, hasher: &mut FxHasher) {
    match value {
        Value::True => TAG_TRUE.hash(hasher),
        Value::False => TAG_FALSE.hash(hasher),
        Value::Null => TAG_NULL.hash(hasher),
        // QuoteKind is deliberately not hashed: Value::eq compares String
        // contents only ("a" == a as map keys).
        Value::String(s, ..) => {
            TAG_STRING.hash(hasher);
            s.hash(hasher);
        }
        Value::Dimension(n) => {
            TAG_DIMENSION.hash(hasher);
            hash_dimension(n, hasher);
        }
        Value::Color(..) => TAG_COLOR.hash(hasher),
        Value::Calculation(..) => TAG_CALCULATION.hash(hasher),
        Value::FunctionRef(..) => TAG_FUNCTION_REF.hash(hasher),
        Value::MixinRef(..) => TAG_MIXIN_REF.hash(hasher),
        Value::List(elems, sep, ..) => {
            if elems.is_empty() {
                TAG_EMPTY_CONTAINER.hash(hasher);
            } else {
                TAG_LIST.hash(hasher);
                hash_separator(*sep, hasher);
                for elem in elems.iter() {
                    hash_value_into(elem, hasher);
                }
            }
        }
        Value::Map(map) => {
            if map.is_empty() {
                TAG_EMPTY_CONTAINER.hash(hasher);
            } else {
                TAG_MAP.hash(hasher);
                // Map equality (SassMap::eq) is order-independent (an
                // any-match search per key), so the hash must be too: fold
                // per-entry hashes with a commutative combiner instead of
                // hashing entries in iteration order.
                let mut acc: u64 = 0;
                for (k, v) in map.iter() {
                    let mut entry_hasher = FxHasher::default();
                    hash_value_into(&k.node, &mut entry_hasher);
                    hash_value_into(v, &mut entry_hasher);
                    acc = acc.wrapping_add(entry_hasher.finish());
                }
                acc.hash(hasher);
                map.iter().count().hash(hasher);
            }
        }
        Value::ArgList(list) => {
            if list.elems.is_empty() {
                TAG_EMPTY_CONTAINER.hash(hasher);
            } else {
                // Matches the List(Comma, ..) arm Value::eq uses to compare
                // an ArgList against a List: same tag/separator, elements
                // only (keywords are not part of that comparison, and
                // brackets are not checked on the List side either -- see
                // module docs).
                TAG_LIST.hash(hasher);
                hash_separator(ListSeparator::Comma, hasher);
                for elem in list.elems.iter() {
                    hash_value_into(elem, hasher);
                }
            }
        }
    }
}

fn hash_separator(sep: ListSeparator, hasher: &mut FxHasher) {
    let tag: u8 = match sep {
        ListSeparator::Space => 0,
        ListSeparator::Comma => 1,
        ListSeparator::Slash => 2,
        ListSeparator::Undecided => 3,
    };
    tag.hash(hasher);
}

/// Hashes a `SassNumber`, consistent with `SassNumber::eq`: comparable units
/// are converted to a canonical per-kind representative before hashing (e.g.
/// `1cm` and `10mm` both hash via a canonicalized "in" value), mirroring
/// dart-sass's `canonicalMultiplierForUnit`. `as_slash` is intentionally
/// ignored -- `SassNumber::eq` doesn't compare it either.
fn hash_dimension(n: &SassNumber, hasher: &mut FxHasher) {
    let (numer, denom) = n.unit.clone().numer_and_denom();

    // Shape must be hashed independently of the canonical value: e.g. a
    // unitless number and a `deg` number must never collide into a shape
    // that makes them look comparable. Lengths alone already separate
    // Unit::None ([], []) from any single unit ([u], []).
    numer.len().hash(hasher);
    denom.len().hash(hasher);

    let mut canonical_value = n.num.0;

    for u in &numer {
        canonical_value *= canonical_multiplier(u);
        hash_unit_shape(u, hasher);
    }
    for u in &denom {
        canonical_value /= canonical_multiplier(u);
        hash_unit_shape(u, hasher);
    }

    fuzzy_hash_component(canonical_value).hash(hasher);
}

/// Returns the factor that converts a value in `unit` into the fixed
/// per-kind canonical unit (`In` for absolute lengths, `Deg` for angles, `S`
/// for time, `Hz` for frequency, `Dpi` for resolution). Units outside those
/// five comparable kinds (font-relative, viewport-relative, `%`, `fr`,
/// unknown units, `None`) are only ever comparable to themselves, so no
/// conversion applies (multiplier 1.0); their exact identity is instead
/// captured by `hash_unit_shape`.
fn canonical_multiplier(unit: &Unit) -> f64 {
    let canonical = match unit.kind() {
        UnitKind::Absolute => &Unit::In,
        UnitKind::Angle => &Unit::Deg,
        UnitKind::Time => &Unit::S,
        UnitKind::Frequency => &Unit::Hz,
        UnitKind::Resolution => &Unit::Dpi,
        UnitKind::FontRelative
        | UnitKind::ViewportRelative
        | UnitKind::Other
        | UnitKind::None => return 1.0,
    };

    conversion_factor(unit, canonical).unwrap_or(1.0)
}

/// Hashes the part of a unit's identity that ISN'T captured by
/// `canonical_multiplier`'s value scaling: for the five comparable kinds,
/// that's just the kind tag (since e.g. `Px` and `Cm` must hash the same
/// shape so that only their canonicalized values are compared); for
/// everything else, it's the exact unit (since e.g. `Em` and `Rem` are never
/// comparable despite both being font-relative).
fn hash_unit_shape(unit: &Unit, hasher: &mut FxHasher) {
    let kind_tag: u8 = match unit.kind() {
        UnitKind::Absolute => 0,
        UnitKind::Angle => 1,
        UnitKind::Time => 2,
        UnitKind::Frequency => 3,
        UnitKind::Resolution => 4,
        UnitKind::FontRelative
        | UnitKind::ViewportRelative
        | UnitKind::Other
        | UnitKind::None => {
            // Not one of the convertible kinds: hash the exact unit so e.g.
            // Em vs Rem, or Unknown("foo") vs Unknown("bar"), never collide
            // into a shape that implies comparability.
            5u8.hash(hasher);
            unit.hash(hasher);
            return;
        }
    };
    kind_tag.hash(hasher);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        common::{Brackets, SmallOrderedMap, ListSeparator, QuoteKind},
        value::{ArgList, Number, SassMap},
    };
    use codemap::{CodeMap, Spanned};
    use compact_str::CompactString;
    use std::{cell::Cell, rc::Rc};

    fn span() -> codemap::Span {
        let mut codemap = CodeMap::new();
        let file = codemap.add_file("test".into(), "test".into());
        file.span.subspan(0, 0)
    }

    fn spanned(v: Value) -> Spanned<Value> {
        Spanned { node: v, span: span() }
    }

    fn dim<N: Into<Number>>(n: N, unit: Unit) -> Value {
        Value::Dimension(SassNumber {
            num: n.into(),
            unit,
            as_slash: None,
        })
    }

    fn string(s: &str, quotes: QuoteKind) -> Value {
        Value::String(CompactString::from(s), quotes)
    }

    /// Asserts the core invariant this whole module exists to prove: if
    /// `Value::eq` says two values are equal, `value_key_hash` must agree.
    fn assert_hash_consistent(a: &Value, b: &Value) {
        assert_eq!(a, b, "test bug: values are not actually Value::eq-equal");
        assert_eq!(
            value_key_hash(a),
            value_key_hash(b),
            "eq holds but hashes differ for {a:?} vs {b:?}"
        );
    }

    // --- Fuzzy numerics ---
    // dart-sass 1.97.3: `@debug map.get((1: a), 1.0);` -> `a` (found)
    #[test]
    fn int_and_float_dimension_hash_equal() {
        assert_hash_consistent(&dim(1, Unit::None), &dim(1.0, Unit::None));
    }

    // dart-sass 1.97.3: `@debug map.get((1: a), 1.00000000001);` -> `null`
    // (NOT fuzzy-equal: outside both the epsilon band and the rounding
    // grid). Not an eq pair, so nothing to assert_hash_consistent on: this
    // just documents that the boundary is a real boundary, not fuzzy-equal
    // everywhere nearby.
    #[test]
    fn near_boundary_dimension_is_not_fuzzy_equal() {
        assert_ne!(dim(1, Unit::None), dim(1.000_000_000_01, Unit::None));
    }

    // --- Unit-comparable Dimensions ---
    // dart-sass 1.97.3: `@debug map.get((1px: a), 1.0px);` -> `a`
    #[test]
    fn same_unit_dimension_hash_equal() {
        assert_hash_consistent(&dim(1, Unit::Px), &dim(1.0, Unit::Px));
    }

    // dart-sass 1.97.3: `@debug map.get((1cm: a), 10mm);` -> `a`
    #[test]
    fn comparable_absolute_units_hash_equal() {
        assert_hash_consistent(&dim(1, Unit::Cm), &dim(10, Unit::Mm));
    }

    // dart-sass 1.97.3: `@debug map.get((1in: a), 96px);` -> `a`
    #[test]
    fn in_and_px_hash_equal() {
        assert_hash_consistent(&dim(1, Unit::In), &dim(96, Unit::Px));
    }

    // dart-sass 1.97.3: `@debug map.get((1em: a), 1px);` -> `null` (not
    // comparable: FontRelative vs Absolute)
    #[test]
    fn font_relative_and_absolute_are_not_comparable() {
        assert_ne!(dim(1, Unit::Em), dim(1, Unit::Px));
    }

    // dart-sass 1.97.3: `@debug map.get((1em: a), 1rem);` -> `null` (both
    // FontRelative, but not comparable to each other -- only to themselves)
    #[test]
    fn em_and_rem_are_not_comparable_despite_same_kind() {
        assert_ne!(dim(1, Unit::Em), dim(1, Unit::Rem));
        // Confirm the hash design doesn't accidentally merge them either
        // (this isn't required by the invariant since they're not eq, but
        // it's cheap insurance against a design that's "consistent" only by
        // being a constant function).
        assert_ne!(
            value_key_hash(&dim(1, Unit::Em)),
            value_key_hash(&dim(1, Unit::Rem))
        );
    }

    // dart-sass 1.97.3: `@debug map.get((1: a), 1px);` -> `null` (None is
    // only comparable to None, despite Unit::comparable(None, _) => true)
    #[test]
    fn unitless_and_unit_are_not_comparable() {
        assert_ne!(dim(1, Unit::None), dim(1, Unit::Px));
    }

    // --- Quoted vs unquoted strings ---
    // dart-sass 1.97.3: `@debug map.get(("a": 1), a);` -> `1`;
    // `@debug map.get((a: 1), "a");` -> `1`
    #[test]
    fn quoted_and_unquoted_string_hash_equal() {
        assert_hash_consistent(
            &string("a", QuoteKind::Quoted),
            &string("a", QuoteKind::None),
        );
    }

    // --- List/map keys ---
    // dart-sass 1.97.3: `@debug map.get(((1 2): a), (1 2));` -> `a`
    #[test]
    fn list_key_with_same_elements_and_separator_hash_equal() {
        let a = Value::List(
            Rc::new(vec![dim(1, Unit::None), dim(2, Unit::None)]),
            ListSeparator::Space,
            Brackets::None,
        );
        let b = Value::List(
            Rc::new(vec![dim(1, Unit::None), dim(2.0, Unit::None)]),
            ListSeparator::Space,
            Brackets::None,
        );
        assert_hash_consistent(&a, &b);
    }

    // dart-sass 1.97.3: `@debug map.get(((a: 1): a), (a: 1));` -> `a`
    #[test]
    fn map_key_with_same_entries_hash_equal() {
        let a = Value::Map(SassMap::new_with(vec![(
            spanned(string("a", QuoteKind::None)),
            dim(1, Unit::None),
        )]));
        let b = Value::Map(SassMap::new_with(vec![(
            spanned(string("a", QuoteKind::Quoted)),
            dim(1.0, Unit::None),
        )]));
        assert_hash_consistent(&a, &b);
    }

    /// Map equality (and therefore hashing) is order-independent.
    #[test]
    fn map_key_entry_order_does_not_affect_hash() {
        let a = Value::Map(SassMap::new_with(vec![
            (spanned(string("a", QuoteKind::None)), dim(1, Unit::None)),
            (spanned(string("b", QuoteKind::None)), dim(2, Unit::None)),
        ]));
        let b = Value::Map(SassMap::new_with(vec![
            (spanned(string("b", QuoteKind::None)), dim(2, Unit::None)),
            (spanned(string("a", QuoteKind::None)), dim(1, Unit::None)),
        ]));
        assert_hash_consistent(&a, &b);
    }

    // dart-sass 1.97.3: `@debug map.get(([1, 2]: a), (1, 2));` -> `null`
    // (bracketed list is not equal to unbracketed/differently-separated
    // list, even with identical elements)
    #[test]
    fn bracketed_list_is_not_equal_to_differently_shaped_list() {
        let bracketed = Value::List(
            Rc::new(vec![dim(1, Unit::None), dim(2, Unit::None)]),
            ListSeparator::Comma,
            Brackets::Bracketed,
        );
        let comma_unbracketed = Value::List(
            Rc::new(vec![dim(1, Unit::None), dim(2, Unit::None)]),
            ListSeparator::Comma,
            Brackets::None,
        );
        assert_ne!(bracketed, comma_unbracketed);
        // Not required by the invariant (they're not eq), but documents the
        // deliberate simplification: List hashing ignores Brackets (see
        // module docs), so these DO collide into the same bucket. That's a
        // safe collision, not a correctness bug -- assert it here so a
        // future reader isn't surprised by it.
        assert_eq!(
            value_key_hash(&bracketed),
            value_key_hash(&comma_unbracketed)
        );
    }

    // --- Empty containers ---
    // dart-sass 1.97.3: `@debug map.remove((a: 1), a) == [];` -> `true`;
    // `@debug map.remove((a: 1), a) == ();` -> `true`
    #[test]
    fn empty_list_and_empty_map_hash_equal() {
        let empty_list = Value::List(Rc::new(vec![]), ListSeparator::Comma, Brackets::Bracketed);
        let empty_map = Value::Map(SassMap::new());
        assert_hash_consistent(&empty_list, &empty_map);
    }

    // dart-sass 1.97.3: `@debug [] == [];` -> `true` (but note: `() == []` is
    // `false` -- a bracketed empty list is not equal to an unbracketed one.
    // We don't assert that pair here since our hash deliberately merges ALL
    // empty containers, which is a safe superset, not an exact match, of
    // Value::eq's actual (non-transitive) empty-equality web -- see module
    // docs.)
    #[test]
    fn empty_lists_with_different_brackets_hash_equal_to_each_other_too() {
        let a = Value::List(Rc::new(vec![]), ListSeparator::Space, Brackets::None);
        let b = Value::List(Rc::new(vec![]), ListSeparator::Comma, Brackets::Bracketed);
        // Not eq per Value::eq (different sep/brackets), but both equal the
        // same empty Map (per the previous test), so a shared hash bucket is
        // required transitively through the hash (not through eq itself).
        assert_ne!(a, b);
        assert_eq!(value_key_hash(&a), value_key_hash(&b));
    }

    // --- ArgList / List(Comma) cross-equality ---
    #[test]
    fn arglist_and_comma_list_with_same_elements_hash_equal() {
        let arglist = Value::ArgList(ArgList::new(
            vec![dim(1, Unit::None), dim(2, Unit::None)],
            Rc::new(Cell::new(false)),
            SmallOrderedMap::default(),
            ListSeparator::Comma,
        ));
        // Brackets::None: an ArgList is never bracketed, and Value::eq (fixed
        // in Plan 035 to match dart-sass's SassList::== exactly) now requires
        // matching brackets, so a Bracketed list here would no longer be eq.
        let list = Value::List(
            Rc::new(vec![dim(1.0, Unit::None), dim(2.0, Unit::None)]),
            ListSeparator::Comma,
            Brackets::None,
        );
        // Value::eq is symmetric here as of Plan 035 (both ArgList == List
        // and List == ArgList hold); assert_hash_consistent only checks one
        // direction (arglist first), which remains sufficient for the eq ⟹
        // same hash invariant this module cares about.
        assert_hash_consistent(&arglist, &list);
    }

    #[test]
    fn empty_arglist_hashes_into_empty_container_bucket() {
        let arglist = Value::ArgList(ArgList::new(
            vec![],
            Rc::new(Cell::new(false)),
            SmallOrderedMap::default(),
            ListSeparator::Comma,
        ));
        let empty_map = Value::Map(SassMap::new());
        // Empty ArgList vs empty Map is not actually Value::eq (ArgList's
        // match arm has no Map case), unlike empty List vs empty Map. This
        // just confirms the deliberate over-merge (see module docs: ALL
        // empty containers share one bucket) rather than asserting eq.
        assert_eq!(value_key_hash(&arglist), value_key_hash(&empty_map));
    }

    // --- Colors / calculations: conservative single-bucket fallback ---
    #[test]
    fn distinct_colors_share_a_bucket() {
        use crate::color::{Color, ColorFormat};
        let red = Value::Color(Rc::new(Color::new_rgba(
            Number(255.0),
            Number(0.0),
            Number(0.0),
            Number(1.0),
            ColorFormat::Rgb,
        )));
        let blue = Value::Color(Rc::new(Color::new_rgba(
            Number(0.0),
            Number(0.0),
            Number(255.0),
            Number(1.0),
            ColorFormat::Rgb,
        )));
        // Not eq, but both hash into the single Color fallback bucket --
        // documents the conservative (always-correct, not-always-fast)
        // design choice rather than asserting it's a coincidence.
        assert_ne!(red, blue);
        assert_eq!(value_key_hash(&red), value_key_hash(&blue));
    }
}
