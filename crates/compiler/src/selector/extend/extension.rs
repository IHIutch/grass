use codemap::Span;

use crate::{ast::CssMediaQuery, error::SassResult};

use super::{ComplexSelector, SimpleSelector};

#[derive(Clone, Debug)]
pub(crate) struct Extension {
    /// The selector in which the `@extend` appeared.
    pub extender: ComplexSelector,

    /// The selector that's being extended.
    ///
    /// `None` for one-off extensions.
    pub target: Option<SimpleSelector>,

    /// The minimum specificity required for any selector generated from this
    /// extender.
    pub specificity: i32,

    /// Whether this extension is optional.
    pub is_optional: bool,

    /// Whether this is a one-off extender representing a selector that was
    /// originally in the document, rather than one defined with `@extend`.
    pub is_original: bool,

    /// The media query context to which this extend is restricted, or `None` if
    /// it can apply within any context.
    pub media_context: Option<Vec<CssMediaQuery>>,

    /// The span in which `extender` was defined.
    pub span: Span,

    #[allow(dead_code)]
    pub left: Option<Box<Extension>>,

    #[allow(dead_code)]
    pub right: Option<Box<Extension>>,
}

impl Extension {
    pub fn one_off(
        extender: ComplexSelector,
        specificity: Option<i32>,
        is_original: bool,
        span: Span,
    ) -> Self {
        Self {
            specificity: specificity.unwrap_or_else(|| extender.max_specificity()),
            extender,
            target: None,
            span,
            is_optional: true,
            is_original,
            media_context: None,
            left: None,
            right: None,
        }
    }

    /// Asserts that the `media_context` for a selector is compatible with the
    /// query context for this extender. An extension defined outside any @media
    /// can extend selectors in any context; one defined inside @media can only
    /// extend selectors in the same context.
    pub fn assert_compatible_media_context(
        &self,
        media_context: &Option<Vec<CssMediaQuery>>,
    ) -> SassResult<()> {
        // If this extension has no media context, it can extend anything.
        let expected = match &self.media_context {
            Some(ctx) => ctx,
            None => return Ok(()),
        };

        // If the target selector's media context matches, it's compatible.
        if let Some(ctx) = media_context {
            if expected == ctx {
                return Ok(());
            }
        }

        Err((
            "You may not @extend selectors across media queries.",
            self.span,
        )
            .into())
    }

    #[allow(clippy::missing_const_for_fn)]
    pub fn with_extender(mut self, extender: ComplexSelector) -> Self {
        self.extender = extender;
        self
    }
}
