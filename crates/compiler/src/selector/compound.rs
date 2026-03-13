use std::fmt::{self, Write};

use codemap::Span;

use crate::error::SassResult;

use super::{
    ComplexSelector, ComplexSelectorComponent, Namespace, Pseudo, SelectorList, SimpleSelector,
    Specificity,
};

/// A compound selector is composed of several
/// simple selectors
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct CompoundSelector {
    pub components: Vec<SimpleSelector>,
}

impl fmt::Display for CompoundSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut did_write = false;
        for simple in &self.components {
            if did_write {
                write!(f, "{}", simple)?;
            } else {
                let s = simple.to_string();
                if !s.is_empty() {
                    did_write = true;
                }
                write!(f, "{}", s)?;
            }
        }

        // If we emit an empty compound, it's because all of the components got
        // optimized out because they match all selectors, so we just emit the
        // universal selector.
        if !did_write {
            f.write_char('*')?;
        }

        Ok(())
    }
}

impl CompoundSelector {
    pub fn max_specificity(&self) -> i32 {
        self.specificity().max
    }

    pub fn min_specificity(&self) -> i32 {
        self.specificity().min
    }

    /// Returns tuple of (min, max) specificity
    pub fn specificity(&self) -> Specificity {
        let mut min = 0;
        let mut max = 0;
        for simple in &self.components {
            min += simple.min_specificity();
            max += simple.max_specificity();
        }
        Specificity::new(min, max)
    }

    /// Returns the index and reference of the first pseudo-element in this compound.
    fn find_pseudo_element_indexed(&self) -> Option<(usize, &SimpleSelector)> {
        self.components.iter().enumerate().find(|(_, s)| {
            matches!(s, SimpleSelector::Pseudo(Pseudo { is_class: false, .. }))
        })
    }

    pub fn is_invisible(&self) -> bool {
        self.components.iter().any(SimpleSelector::is_invisible)
    }

    pub fn is_super_selector(
        &self,
        other: &Self,
        parents: &Option<Vec<ComplexSelectorComponent>>,
    ) -> bool {
        // Pseudo-elements effectively change the target of a compound selector.
        // If either has a pseudo-element, they both must have the same one,
        // and components before/after must be checked separately.
        let pseudo1 = self.find_pseudo_element_indexed();
        let pseudo2 = other.find_pseudo_element_indexed();
        match (pseudo1, pseudo2) {
            (Some((idx1, pe1)), Some((idx2, pe2))) => {
                // The pseudo-elements themselves must match.
                // For pseudo-elements with selector args (e.g. ::slotted),
                // use proper superselector logic; otherwise use equality.
                match (pe1, pe2) {
                    (
                        SimpleSelector::Pseudo(
                            pseudo @ Pseudo {
                                selector: Some(..), ..
                            },
                        ),
                        _,
                    ) => {
                        // Build a 1-element compound from pe2 for the check
                        let pe2_compound = CompoundSelector {
                            components: vec![pe2.clone()],
                        };
                        if !pseudo.is_super_selector(&pe2_compound, parents.clone()) {
                            return false;
                        }
                    }
                    _ => {
                        if pe1 != pe2 {
                            return false;
                        }
                    }
                }

                // Check components before the pseudo-element
                let before1 = &self.components[..idx1];
                let before2 = &other.components[..idx2];
                if !compound_components_is_superselector(before1, before2, parents) {
                    return false;
                }

                // Check components after the pseudo-element
                let after1 = &self.components[idx1 + 1..];
                let after2 = &other.components[idx2 + 1..];
                return compound_components_is_superselector(after1, after2, parents);
            }
            (Some(_), None) | (None, Some(_)) => return false,
            (None, None) => {}
        }

        for simple1 in &self.components {
            if let SimpleSelector::Pseudo(
                pseudo @ Pseudo {
                    selector: Some(..), ..
                },
            ) = simple1
            {
                if !pseudo.is_super_selector(other, parents.clone()) {
                    return false;
                }
            } else if !simple1.is_super_selector_of_compound(other) {
                return false;
            }
        }

        for simple2 in &other.components {
            if let SimpleSelector::Pseudo(Pseudo {
                is_class: false,
                selector: None,
                ..
            }) = simple2
            {
                if !simple2.is_super_selector_of_compound(self) {
                    return false;
                }
            }
        }

        true
    }

    /// Returns a new `CompoundSelector` based on `compound` with all
    /// `SimpleSelector::Parent`s replaced with `parent`.
    ///
    /// Returns `None` if `compound` doesn't contain any `SimpleSelector::Parent`s.
    pub fn resolve_parent_selectors(
        self,
        span: Span,
        parent: SelectorList,
    ) -> SassResult<Option<Vec<ComplexSelector>>> {
        let contains_selector_pseudo = self.components.iter().any(|simple| {
            if let SimpleSelector::Pseudo(Pseudo {
                selector: Some(sel),
                ..
            }) = simple
            {
                sel.contains_parent_selector()
            } else {
                false
            }
        });

        if !contains_selector_pseudo && !self.components[0].is_parent() {
            return Ok(None);
        }

        let resolved_members: Vec<SimpleSelector> = if contains_selector_pseudo {
            self.components
                .clone()
                .into_iter()
                .map(|simple| {
                    if let SimpleSelector::Pseudo(mut pseudo) = simple {
                        if let Some(sel) = pseudo.selector.clone() {
                            if !sel.contains_parent_selector() {
                                return Ok(SimpleSelector::Pseudo(pseudo));
                            }

                            pseudo.selector = Some(Box::new(
                                sel.resolve_parent_selectors(Some(parent.clone()), false)?,
                            ));
                        }

                        Ok(SimpleSelector::Pseudo(pseudo))
                    } else {
                        Ok(simple)
                    }
                })
                .collect::<SassResult<Vec<SimpleSelector>>>()?
        } else {
            self.components.clone()
        };

        if let Some(SimpleSelector::Parent(suffix)) = self.components.first() {
            if self.components.len() == 1 && suffix.is_none() {
                return Ok(Some(parent.components));
            }
        } else {
            return Ok(Some(vec![ComplexSelector::new(
                vec![ComplexSelectorComponent::Compound(CompoundSelector {
                    components: resolved_members,
                })],
                false,
            )]));
        }

        let parent_span = parent.span;

        Ok(Some(
            parent
                .components
                .into_iter()
                .map(move |mut complex| {
                    let last_component = complex.components.last();
                    let last = if let Some(ComplexSelectorComponent::Compound(c)) = last_component {
                        c.clone()
                    } else {
                        return Err((
                            format!("Parent \"{}\" is incompatible with this selector.", complex),
                            span,
                        )
                            .into());
                    };

                    let mut components = last.components;

                    if let Some(SimpleSelector::Parent(Some(suffix))) = self.components.first() {
                        let mut end = components.pop().unwrap();
                        end.add_suffix(suffix, parent_span)?;
                        components.push(end);
                    }

                    components.extend(resolved_members.clone().into_iter().skip(1));

                    let last = CompoundSelector { components };

                    complex.components.pop();

                    let mut components = complex.components;
                    components.push(ComplexSelectorComponent::Compound(last));

                    Ok(ComplexSelector::new(components, complex.line_break))
                })
                .collect::<SassResult<Vec<ComplexSelector>>>()?,
        ))
    }

    /// Returns a `CompoundSelector` that matches only elements that are matched by
    /// both `compound1` and `compound2`.
    ///
    /// If no such selector can be produced, returns `None`.
    pub fn unify(self, other: Self) -> Option<Self> {
        let mut result = self.components;
        let mut pseudo_result: Vec<SimpleSelector> = Vec::new();
        let mut pseudo_element_found = false;

        for simple in other.components {
            if pseudo_element_found && matches!(simple, SimpleSelector::Pseudo(..)) {
                // Once we've seen a pseudo-element, subsequent pseudo selectors
                // go into a separate list to preserve their position after the
                // pseudo-element in the final result.
                pseudo_result = simple.unify(std::mem::take(&mut pseudo_result))?;
            } else {
                if matches!(
                    simple,
                    SimpleSelector::Pseudo(Pseudo {
                        is_class: false,
                        ..
                    })
                ) {
                    pseudo_element_found = true;
                }
                result = simple.unify(std::mem::take(&mut result))?;
            }
        }

        result.extend(pseudo_result);
        Some(Self {
            components: result,
        })
    }

    /// Adds a `SimpleSelector::Parent` to the beginning of `compound`, or returns `None` if
    /// that wouldn't produce a valid selector.
    pub fn prepend_parent(mut self) -> Option<Self> {
        Some(match self.components.first()? {
            SimpleSelector::Universal(..) => return None,
            SimpleSelector::Type(name) => {
                if name.namespace != Namespace::None {
                    return None;
                }
                let mut components = vec![SimpleSelector::Parent(Some(name.ident.clone()))];
                components.extend(self.components.into_iter().skip(1));

                Self { components }
            }
            _ => {
                let mut components = vec![SimpleSelector::Parent(None)];
                components.append(&mut self.components);
                Self { components }
            }
        })
    }
}

/// Like `CompoundSelector::is_super_selector` but operates on slices of components.
/// An empty `compound1` is treated as a universal selector (matches everything).
fn compound_components_is_superselector(
    compound1: &[SimpleSelector],
    compound2: &[SimpleSelector],
    parents: &Option<Vec<ComplexSelectorComponent>>,
) -> bool {
    if compound1.is_empty() {
        return true;
    }
    let c1 = CompoundSelector {
        components: compound1.to_vec(),
    };
    let c2 = if compound2.is_empty() {
        CompoundSelector {
            components: vec![SimpleSelector::Universal(Namespace::Asterisk)],
        }
    } else {
        CompoundSelector {
            components: compound2.to_vec(),
        }
    };
    c1.is_super_selector(&c2, parents)
}
