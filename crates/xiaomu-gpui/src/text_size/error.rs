use std::fmt;
use std::ops::Range;

use xiaomu_core::document::NodeId;

use crate::font_size::FontSizeError;
use crate::mixed_size::{Reason, Unsupported};

/// Why the opt-in native size capability rejected a block.
///
/// These are rendering limitations, never canonical-data repair rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextSizeErrorKind {
    /// A canonical font-size value or inherited size could not be resolved.
    FontSize(FontSizeError),
    /// The host supplied invalid line-height or non-finite geometry.
    InvalidStyle,
    /// A batch provider omitted an inline node or supplied an extra node key.
    InvalidStyleMap,
    /// A canonical block or referenced atom could not be projected.
    InvalidProjection,
    /// Display spans or layout parameters violate the shaping contract.
    InvalidLayout,
    /// Mixed sizes require a script/control outside the conservative LTR gate.
    ComplexScriptOrControl,
    /// A size boundary splits one extended grapheme cluster.
    SizeBoundaryInsideGrapheme,
    /// A size boundary splits a native shaped glyph cluster.
    SizeBoundaryInsideShapedCluster,
    /// Shaping depends on context across a size boundary.
    ContextAtSizeBoundary,
    /// Native cluster positions cannot support reliable layout geometry.
    UnreliableClusterGeometry,
    /// Stock native shaping failed.
    NativeShapeFailed,
    /// Mixed-size admission exceeds the bounded synchronous work reservation.
    WorkBudgetExceeded,
}

/// A fail-closed font-size capability error tied to one canonical block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSizeError {
    node: NodeId,
    display_range: Range<usize>,
    kind: TextSizeErrorKind,
}

impl TextSizeError {
    /// Returns the inline-bearing block, or the unexpected map key when a
    /// batch provider supplies a non-inline/missing node (`InvalidStyleMap`).
    #[must_use]
    pub const fn node(&self) -> NodeId {
        self.node
    }

    /// Returns the affected half-open UTF-8 range in projected display text.
    /// These bytes include atom labels and are not canonical text offsets.
    #[must_use]
    pub const fn display_range(&self) -> &Range<usize> {
        &self.display_range
    }

    /// Returns the diagnostic rejection category.
    #[must_use]
    pub const fn kind(&self) -> TextSizeErrorKind {
        self.kind
    }

    pub(crate) fn new(node: NodeId, display_range: Range<usize>, kind: TextSizeErrorKind) -> Self {
        Self {
            node,
            display_range,
            kind,
        }
    }

    pub(crate) fn from_layout(node: NodeId, error: Unsupported) -> Self {
        let kind = match error.reason {
            Reason::InvalidInput => TextSizeErrorKind::InvalidLayout,
            Reason::ComplexScriptOrControl => TextSizeErrorKind::ComplexScriptOrControl,
            Reason::SizeBoundaryInsideGrapheme => TextSizeErrorKind::SizeBoundaryInsideGrapheme,
            Reason::SizeBoundaryInsideShapedCluster => {
                TextSizeErrorKind::SizeBoundaryInsideShapedCluster
            }
            Reason::ContextAtSizeBoundary => TextSizeErrorKind::ContextAtSizeBoundary,
            Reason::UnreliableClusterGeometry => TextSizeErrorKind::UnreliableClusterGeometry,
            Reason::NativeShapeFailed => TextSizeErrorKind::NativeShapeFailed,
            Reason::WorkBudgetExceeded => TextSizeErrorKind::WorkBudgetExceeded,
        };
        Self::new(node, error.range, kind)
    }
}

impl fmt::Display for TextSizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unsupported native text size in {:?} at display bytes {:?}: {:?}",
            self.node, self.display_range, self.kind
        )
    }
}

impl std::error::Error for TextSizeError {}
