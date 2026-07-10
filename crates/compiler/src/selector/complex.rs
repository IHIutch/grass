use std::{
    fmt::{self, Display, Write},
    hash::{BuildHasher, Hash, Hasher},
    rc::Rc,
};

use codemap::Span;
use rustc_hash::{FxBuildHasher, FxHashSet};

use crate::error::SassResult;

use super::{CompoundSelector, Pseudo, SelectorList, SimpleSelector, Specificity};

#[derive(Clone, Debug)]
pub(crate) struct ComplexSelectorHashSet(FxHashSet<ComplexSelector>);

impl ComplexSelectorHashSet {
    pub fn new() -> Self {
        Self(FxHashSet::default())
    }

    pub fn insert(&mut self, complex: &ComplexSelector) -> bool {
        self.0.insert(complex.clone())
    }

    pub fn contains(&self, complex: &ComplexSelector) -> bool {
        self.0.contains(complex)
    }

    pub fn extend<'a>(&mut self, complexes: impl Iterator<Item = &'a ComplexSelector>) {
        self.0.extend(complexes.cloned());
    }
}

/// A complex selector.
///
/// A complex selector is composed of `CompoundSelector`s separated by
/// `Combinator`s. It selects elements based on their parent selectors.
#[derive(Clone, Debug)]
pub(crate) struct ComplexSelector {
    /// The components of this selector.
    ///
    /// This is never empty.
    ///
    /// Descendant combinators aren't explicitly represented here. If two
    /// `CompoundSelector`s are adjacent to one another, there's an implicit
    /// descendant combinator between them.
    ///
    /// It's possible for multiple `Combinator`s to be adjacent to one another.
    /// This isn't valid CSS, but Sass supports it for CSS hack purposes.
    pub components: Vec<ComplexSelectorComponent>,

    /// Whether a line break should be emitted *before* this selector.
    pub line_break: bool,

    /// Pre-computed hash of components (computed at construction time), unless
    /// this is a transient selector (see `new_transient`), in which case it's
    /// meaningless and must never be read.
    /// Since components are never mutated after construction, this is always valid.
    cached_hash: u64,

    /// Pre-computed specificity (computed at construction time).
    /// Since components are never mutated after construction, this is always valid.
    cached_specificity: Specificity,

    /// True for selectors built via `new_transient`, which skip computing
    /// `cached_hash` because they exist only to be compared (e.g. via
    /// `is_super_selector`) and are discarded immediately after. Checked by a
    /// debug assertion in `Hash::hash` — transient selectors must never be
    /// inserted into a `ComplexSelectorHashSet` or used as a hash map/set key.
    is_transient: bool,
}

impl PartialEq for ComplexSelector {
    fn eq(&self, other: &Self) -> bool {
        self.components == other.components
    }
}

impl Eq for ComplexSelector {}

impl Hash for ComplexSelector {
    fn hash<H: Hasher>(&self, state: &mut H) {
        debug_assert!(
            !self.is_transient,
            "attempted to hash a transient ComplexSelector (built via new_transient); \
             transient selectors must never be inserted into a hash map/set"
        );
        state.write_u64(self.cached_hash);
    }
}

impl fmt::Display for ComplexSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut last_component = None;

        for component in &self.components {
            if let Some(c) = last_component {
                if !omit_spaces_around(c) && !omit_spaces_around(component) {
                    f.write_char(' ')?;
                }
            }
            write!(f, "{}", component)?;
            last_component = Some(component);
        }
        Ok(())
    }
}

/// Checks whether `intermediates` (the components of complex2 between
/// two matched compounds) are compatible with the `previous` combinator.
///
/// - Descendant (None): any intermediates are OK
/// - FollowingSibling (~): intermediates must all use ~ or +
/// - Child (>) or NextSibling (+): no intermediates allowed
fn compatible_with_previous_combinator(
    previous: Option<Combinator>,
    intermediates: &[ComplexSelectorComponent],
) -> bool {
    if intermediates.is_empty() {
        return true;
    }
    match previous {
        None => true,
        Some(Combinator::FollowingSibling) => {
            // Every compound must be followed by ~ or +. Adjacent compounds
            // (implicit descendant) are not OK. Ending with a compound (descendant
            // to the matched element) is also not OK.
            let mut prev_was_compound = false;
            for component in intermediates {
                match component {
                    ComplexSelectorComponent::Compound(_) => {
                        if prev_was_compound {
                            return false;
                        }
                        prev_was_compound = true;
                    }
                    ComplexSelectorComponent::Combinator(c) => {
                        if *c != Combinator::FollowingSibling && *c != Combinator::NextSibling {
                            return false;
                        }
                        prev_was_compound = false;
                    }
                }
            }
            // If intermediates end with a compound, that means descendant
            // between the last intermediate and the matched element — not OK for ~
            !prev_was_compound
        }
        Some(Combinator::Child) | Some(Combinator::NextSibling) => false,
    }
}

/// When `style` is `OutputStyle::compressed`, omit spaces around combinators.
fn omit_spaces_around(component: &ComplexSelectorComponent) -> bool {
    // todo: compressed
    let is_compressed = false;
    is_compressed && matches!(component, ComplexSelectorComponent::Combinator(..))
}

impl ComplexSelector {
    pub fn new(components: Vec<ComplexSelectorComponent>, line_break: bool) -> Self {
        let cached_hash = FxBuildHasher.hash_one(&components);
        let cached_specificity = Self::compute_specificity(&components);
        Self {
            components,
            line_break,
            cached_hash,
            cached_specificity,
            is_transient: false,
        }
    }

    /// Like `new`, but skips computing the hash. Only use this for
    /// comparison-only selectors (e.g. built solely to call
    /// `is_super_selector` on) that are discarded immediately and never
    /// inserted into a `ComplexSelectorHashSet` or used as a hash map/set key
    /// — doing so trips the debug assertion in `Hash::hash`.
    pub fn new_transient(components: Vec<ComplexSelectorComponent>, line_break: bool) -> Self {
        let cached_specificity = Self::compute_specificity(&components);
        Self {
            components,
            line_break,
            cached_hash: 0,
            cached_specificity,
            is_transient: true,
        }
    }

    fn compute_specificity(components: &[ComplexSelectorComponent]) -> Specificity {
        let mut min = 0;
        let mut max = 0;
        for component in components {
            if let ComplexSelectorComponent::Compound(compound) = component {
                min += compound.min_specificity();
                max += compound.max_specificity();
            }
        }
        Specificity::new(min, max)
    }

    // NOTE (Plan 028): these accessors were historically crossed (max read
    // `.min` and vice versa). All call sites were verbatim-preserving that
    // crossing, so this normalization also flips every call site's method
    // name to keep behavior byte-identical. See Solo scratchpad #77 / todo
    // #174 for the empirical fixture battery that ruled out a real semantic
    // bug here (grass's old min/max pseudo-range model happens to agree with
    // dart-sass's modern single-specificity model on every reachable case).
    pub fn max_specificity(&self) -> i32 {
        self.specificity().max
    }

    pub fn min_specificity(&self) -> i32 {
        self.specificity().min
    }

    pub fn specificity(&self) -> Specificity {
        self.cached_specificity
    }

    pub fn is_invisible(&self) -> bool {
        self.components
            .iter()
            .any(ComplexSelectorComponent::is_invisible)
    }

    /// Whether this selector is "bogus" — contains invalid combinator patterns
    /// that should be omitted from CSS output.
    ///
    /// A selector is bogus if it has:
    /// - Trailing combinators (e.g., `a >`)
    /// - Multiple adjacent combinators (e.g., `a > + b`)
    /// - Leading combinators ONLY inside pseudo selectors like :is()/:where()
    ///   (not at the top level, where they're a CSS hack)
    pub fn is_bogus(&self, in_pseudo: bool) -> bool {
        // Trailing combinators
        if let Some(last) = self.components.last() {
            if last.is_combinator() {
                return true;
            }
        }

        // Leading combinators are bogus inside pseudo selectors (except :has())
        if in_pseudo {
            if let Some(first) = self.components.first() {
                if first.is_combinator() {
                    return true;
                }
            }
        }

        self.has_adjacent_combinators()
    }

    /// Whether this selector has two or more consecutive combinator
    /// components anywhere (e.g. `a > + b`, or a doubled leading combinator
    /// `+ + a`). Matches dart-sass's `leadingCombinators.length > 1 ||
    /// component.combinators.length > 1` check (dart groups a compound with
    /// its *trailing* combinators; grass's flat component list makes both
    /// cases the same "two adjacent combinator components" scan).
    fn has_adjacent_combinators(&self) -> bool {
        let mut prev_was_combinator = false;
        for component in &self.components {
            let is_combinator = component.is_combinator();
            if is_combinator && prev_was_combinator {
                return true;
            }
            prev_was_combinator = is_combinator;
        }

        false
    }

    /// Whether this selector has exactly one leading combinator (e.g. `+ a`).
    /// Doubled leading combinators are covered separately by
    /// [`ComplexSelector::is_useless`], matching dart-sass's
    /// `leadingCombinators.isNotEmpty` check used by the `bogus-combinators`
    /// deprecation warning.
    pub fn has_leading_combinator(&self) -> bool {
        matches!(self.components.first(), Some(c) if c.is_combinator())
    }

    /// Whether this selector is bogus *and* can't be transformed into valid
    /// CSS by `@extend` or nesting — i.e. it has a doubled/adjacent
    /// combinator run anywhere (leading or not). Matches dart-sass's
    /// `Selector.isUseless` (recursion into nested pseudo-selector arguments
    /// is not replicated here — this only checks this selector's own
    /// top-level component list).
    pub fn is_useless(&self) -> bool {
        self.has_adjacent_combinators()
    }

    /// Returns whether `self` is a superselector of `other`.
    ///
    /// That is, whether `self` matches every element that `other` matches, as well
    /// as possibly additional elements.
    pub fn is_super_selector(&self, other: &Self) -> bool {
        if let Some(ComplexSelectorComponent::Combinator(..)) = self.components.last() {
            return false;
        }
        if let Some(ComplexSelectorComponent::Combinator(..)) = other.components.last() {
            return false;
        }

        // Bogus sub-selector check: if other has adjacent combinators, return false
        {
            let mut prev_was_combinator = false;
            for component in &other.components {
                let is_comb = component.is_combinator();
                if is_comb && prev_was_combinator {
                    return false;
                }
                prev_was_combinator = is_comb;
            }
        }

        let mut i1 = 0;
        let mut i2 = 0;
        let mut previous_combinator: Option<Combinator> = None;

        loop {
            let remaining1 = self.components.len() - i1;
            let remaining2 = other.components.len() - i2;

            if remaining1 == 0 || remaining2 == 0 || remaining1 > remaining2 {
                return false;
            }

            let compound1 = match self.components.get(i1) {
                Some(ComplexSelectorComponent::Compound(c)) => c,
                Some(ComplexSelectorComponent::Combinator(..)) => return false,
                None => unreachable!(),
            };

            if let ComplexSelectorComponent::Combinator(..) = other.components[i2] {
                return false;
            }

            if remaining1 == 1 {
                // Check intermediates compatibility with previous combinator
                let intermediates = &other.components[i2..other.components.len() - 1];
                if !compatible_with_previous_combinator(previous_combinator, intermediates) {
                    return false;
                }

                let parents = &other.components[i2..other.components.len() - 1];
                return compound1.is_super_selector(
                    other.components.last().unwrap().as_compound(),
                    Some(parents),
                );
            }

            let mut after_super_selector = i2 + 1;
            while after_super_selector < other.components.len() {
                if let Some(ComplexSelectorComponent::Compound(compound2)) =
                    other.components.get(after_super_selector - 1)
                {
                    let parents = other
                        .components
                        .get(i2 + 1..after_super_selector - 1)
                        .unwrap_or(&[]);
                    if compound1.is_super_selector(compound2, Some(parents)) {
                        break;
                    }
                }

                after_super_selector += 1;
            }

            if after_super_selector == other.components.len() {
                return false;
            }

            // Check intermediates compatibility with previous combinator
            let intermediates = &other.components[i2..after_super_selector - 1];
            if !compatible_with_previous_combinator(previous_combinator, intermediates) {
                return false;
            }

            if let Some(ComplexSelectorComponent::Combinator(combinator1)) =
                self.components.get(i1 + 1)
            {
                let combinator2 = match other.components.get(after_super_selector) {
                    Some(ComplexSelectorComponent::Combinator(c)) => c,
                    Some(ComplexSelectorComponent::Compound(..)) => return false,
                    None => unreachable!(),
                };

                if combinator1 == &Combinator::FollowingSibling {
                    if combinator2 == &Combinator::Child {
                        return false;
                    }
                } else if combinator1 != combinator2 {
                    return false;
                }

                previous_combinator = Some(*combinator1);
                i1 += 2;
                i2 = after_super_selector + 1;
            } else if let Some(ComplexSelectorComponent::Combinator(combinator2)) =
                other.components.get(after_super_selector)
            {
                if combinator2 != &Combinator::Child {
                    return false;
                }
                previous_combinator = None;
                i1 += 1;
                i2 = after_super_selector + 1;
            } else {
                previous_combinator = None;
                i1 += 1;
                i2 = after_super_selector;
            }
        }
    }

    pub fn contains_parent_selector(&self) -> bool {
        self.components.iter().any(|c| {
            if let ComplexSelectorComponent::Compound(compound) = c {
                compound.components.iter().any(|simple| {
                    if simple.is_parent() {
                        return true;
                    }
                    if let SimpleSelector::Pseudo(Pseudo {
                        selector: Some(sel),
                        ..
                    }) = simple
                    {
                        return sel.contains_parent_selector();
                    }
                    false
                })
            } else {
                false
            }
        })
    }

    pub fn contains_parent_selector_with_suffix(&self) -> bool {
        self.components.iter().any(|c| {
            if let ComplexSelectorComponent::Compound(compound) = c {
                compound.components.iter().any(|simple| match simple {
                    SimpleSelector::Parent(Some(_)) => true,
                    SimpleSelector::Pseudo(Pseudo {
                        selector: Some(sel), ..
                    }) => sel.contains_parent_selector_with_suffix(),
                    _ => false,
                })
            } else {
                false
            }
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Copy, Hash)]
pub(crate) enum Combinator {
    /// Matches the right-hand selector if it's immediately adjacent to the
    /// left-hand selector in the DOM tree.
    ///
    /// `'+'`
    NextSibling,

    /// Matches the right-hand selector if it's a direct child of the left-hand
    /// selector in the DOM tree.
    ///
    /// `'>'`
    Child,

    /// Matches the right-hand selector if it comes after the left-hand selector
    /// in the DOM tree.
    ///
    /// `'~'`
    FollowingSibling,
}

impl Display for Combinator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_char(match self {
            Self::NextSibling => '+',
            Self::Child => '>',
            Self::FollowingSibling => '~',
        })
    }
}

/// The `Compound` variant is `Rc`-wrapped so that weave/extend's pervasive
/// per-prefix and per-parent cloning of component chains (see
/// `selector/extend/functions.rs`) is a refcount bump instead of a deep
/// clone of the `CompoundSelector` (and its `Vec<SimpleSelector>`). `Rc<T>`'s
/// `PartialEq`/`Eq`/`Hash` impls compare/hash through to `T`'s content, not
/// the pointer, so equality and hashing semantics are unchanged.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) enum ComplexSelectorComponent {
    Combinator(Combinator),
    Compound(Rc<CompoundSelector>),
}

impl ComplexSelectorComponent {
    pub fn is_invisible(&self) -> bool {
        match self {
            Self::Combinator(..) => false,
            Self::Compound(c) => c.is_invisible(),
        }
    }

    pub fn is_compound(&self) -> bool {
        matches!(self, Self::Compound(..))
    }

    pub fn is_combinator(&self) -> bool {
        matches!(self, Self::Combinator(..))
    }

    pub fn resolve_parent_selectors(
        self,
        span: Span,
        parent: SelectorList,
    ) -> SassResult<Option<Vec<ComplexSelector>>> {
        match self {
            Self::Compound(c) => Rc::unwrap_or_clone(c).resolve_parent_selectors(span, parent),
            Self::Combinator(..) => todo!(),
        }
    }

    pub fn as_compound(&self) -> &CompoundSelector {
        match self {
            Self::Compound(c) => c.as_ref(),
            Self::Combinator(..) => unreachable!(),
        }
    }
}

impl Display for ComplexSelectorComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compound(c) => write!(f, "{}", c),
            Self::Combinator(c) => write!(f, "{}", c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Combinator, ComplexSelectorComponent};

    fn dummy_components(n: usize) -> Vec<ComplexSelectorComponent> {
        (0..n)
            .map(|i| {
                if i % 2 == 0 {
                    ComplexSelectorComponent::Combinator(Combinator::Child)
                } else {
                    ComplexSelectorComponent::Combinator(Combinator::NextSibling)
                }
            })
            .collect()
    }

    /// Range-equivalence check for the slice conversion in `is_super_selector`:
    /// `take(n - 1).skip(m + 1)` must always agree with `[m + 1..n - 1]`.
    #[test]
    fn range_equivalence_take_skip_vs_slice() {
        let components = dummy_components(8);

        // (n, m) combos matching real usage shapes: remaining1 == 1 case (n ==
        // components.len()), the inner-scan case (n == after_super_selector), and
        // the first-loop-iteration edge case where m + 1 > n - 1 (empty range).
        for &(n, m) in &[(8usize, 0usize), (6, 2), (5, 1), (4, 3)] {
            let via_take_skip: Vec<ComplexSelectorComponent> = components
                .iter()
                .take(n - 1)
                .skip(m + 1)
                .cloned()
                .collect();
            // Mirrors the real call site's `.get(range).unwrap_or(&[])` guard,
            // since a plain slice index panics when m + 1 > n - 1.
            let via_slice: &[ComplexSelectorComponent] =
                components.get(m + 1..n - 1).unwrap_or(&[]);

            assert_eq!(
                via_take_skip.as_slice(),
                via_slice,
                "n={n}, m={m}: take/skip and slice disagree"
            );
        }
    }
}
