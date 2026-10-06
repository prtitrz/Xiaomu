/// Read-only logical-pixel inputs for a host's optional caret presentation.
///
/// Effective size follows normal typing-mark inheritance. The optional probes
/// retain the distinction between inherited size and a nonempty explicit
/// canonical `font_size` string resolved in the same block context. Their
/// priority and any thresholds belong to the host; the engine does not invent
/// a product-specific rule. Atom-aware adjacent probes never change selection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextSizeCaretContext {
    effective_size: f32,
    stored_size: Option<f32>,
    before_size: Option<f32>,
    after_size: Option<f32>,
}

impl TextSizeCaretContext {
    pub(crate) const fn new(
        effective_size: f32,
        stored_size: Option<f32>,
        before_size: Option<f32>,
        after_size: Option<f32>,
    ) -> Self {
        Self {
            effective_size,
            stored_size,
            before_size,
            after_size,
        }
    }

    /// Returns the resolved current typing size, including inherited defaults.
    #[must_use]
    pub const fn effective_size(&self) -> f32 {
        self.effective_size
    }

    /// Returns an explicit stored typing size, if present and nonempty.
    #[must_use]
    pub const fn stored_size(&self) -> Option<f32> {
        self.stored_size
    }

    /// Returns the explicit size immediately before the mixed-inline caret.
    #[must_use]
    pub const fn before_size(&self) -> Option<f32> {
        self.before_size
    }

    /// Returns the explicit size immediately after the mixed-inline caret.
    #[must_use]
    pub const fn after_size(&self) -> Option<f32> {
        self.after_size
    }
}
