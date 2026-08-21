//! Caret affinity for positions that admit multiple visual interpretations.

/// Visual affinity of a text position.
///
/// One canonical logical position can correspond to multiple visual caret
/// locations after soft wrapping or in BiDi text. `CursorAffinity` keeps that
/// disambiguation inside the canonical position type so the selection contract
/// does not need to change when visual resolution is introduced.
///
/// P0 stores affinity but performs no visual resolution; resolving affinity to
/// a pixel caret belongs to the frontend layout layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CursorAffinity {
    /// Prefer the caret visually before / above the character at this
    /// position, for example at a soft-wrap line end.
    #[default]
    Before,
    /// Prefer the caret visually after / below the character at this
    /// position, for example at a soft-wrap line start.
    After,
}

impl CursorAffinity {
    /// Returns whether this is [`CursorAffinity::Before`].
    #[must_use]
    pub const fn is_before(self) -> bool {
        matches!(self, Self::Before)
    }

    /// Returns whether this is [`CursorAffinity::After`].
    #[must_use]
    pub const fn is_after(self) -> bool {
        matches!(self, Self::After)
    }
}
