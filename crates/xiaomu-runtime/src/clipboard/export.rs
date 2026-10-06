//! Explicit, deterministic host clipboard export options and provenance.

use std::sync::Arc;

use xiaomu_core::document::NodeAttrs;

/// The platform action requesting a clipboard export.
///
/// Cut must be rejected here when the host cannot safely remove the selected
/// source. A successful Copy projection alone never authorizes a Cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardExportPurpose {
    /// Read-only copy, without source deletion.
    Copy,
    /// Copy followed by a separately supported source deletion.
    Cut,
}

/// A known, reproducible plain-text projection over canonical fragments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardTextProjection {
    /// Full-fragment text-between semantics with LF block and leaf separators.
    ///
    /// Empty textblocks participate in block separation. HardBreak, Image and
    /// HorizontalRule contribute LF; unknown custom block/atom semantics reject.
    /// Text bytes, including CRLF and tabs, are preserved exactly.
    TextBetweenLfV1,
}

/// The actual source fragment wrapper of a cell-range copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardCellRangeRoot {
    /// Selected row fragments; the stored Table is only a validated carrier.
    Rows,
    /// The original Table wrapper, when the selection covered the entire table.
    Table,
}

/// Independently verified source boundaries, never inferred from text coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardSourceBoundary {
    /// Ordinary open selection; no arbitrary host open depth is claimed.
    Open,
    /// Explicit complete block subtrees, with open depths 0/0.
    WholeRoots,
    /// Cell selection content, always open 1/1 even for an entire table.
    CellRange {
        /// Whether the source actually contains row roots or a Table root.
        root_form: ClipboardCellRangeRoot,
    },
}

impl ClipboardSourceBoundary {
    /// Returns proven source open depths, or `None` for ordinary open ranges.
    #[must_use]
    pub const fn open_depths(self) -> Option<(u8, u8)> {
        match self {
            Self::Open => None,
            Self::WholeRoots => Some((0, 0)),
            Self::CellRange { .. } => Some((1, 1)),
        }
    }
}

/// Per-export opt-in rules returned by a session's read-only host policy.
///
/// Structure and plain-text projection are independent. The default options
/// retain unit-only cell projection and the historical plain text. Returning
/// `None` from the policy also retains the historical wire format.
/// Cloning these options shares the immutable fill attributes. Their payload
/// is not copied until the export's borrowed resource preflight succeeds.
/// Unlike the original options, this type is `Clone`, not `Copy`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClipboardExportSpec {
    cell_ranges: CellRangeExport,
    text_projection: Option<ClipboardTextProjection>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) enum CellRangeExport {
    #[default]
    Unit,
    Closed,
    Clipped(Arc<NodeAttrs>),
}

impl ClipboardExportSpec {
    /// Creates options without broadening cell geometry or changing plain text.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            cell_ranges: CellRangeExport::Unit,
            text_projection: None,
        }
    }

    /// Enables whole-origin copying of geometrically closed logical rectangles.
    ///
    /// Nonclosed rectangles still reject. CellRange Cut remains unsupported and
    /// is rejected before projection or any platform clipboard write. This
    /// replaces a previous clipped mode and drops its unused fill attributes.
    /// The builder is no longer `const`, because replacing that owned mode may
    /// release its shared attributes.
    #[must_use]
    pub fn with_closed_cell_ranges(mut self) -> Self {
        self.cell_ranges = CellRangeExport::Closed;
        self
    }

    /// Enables clipping at the exact logical cell-selection rectangle.
    ///
    /// Each intersecting origin is captured once. Cells entering from above or
    /// the left retain their kind and cropped attributes but replace all their
    /// children with one empty Paragraph using `empty_paragraph_attrs`. Cropping
    /// only the right or bottom retains the complete original children. Width
    /// arrays are cropped with columns; a horizontally cropped all-zero array
    /// becomes null. Other attributes and selected physical rows are retained.
    ///
    /// The host supplies only its schema's default Paragraph attributes, never
    /// arbitrary clipboard text or children. Their repeated cost is checked
    /// before cloning. A later geometry builder replaces this mode. Copy keeps
    /// CellRange open 1/1 provenance; this grants neither Paste fitting nor Cut.
    #[must_use]
    pub fn with_clipped_cell_ranges(mut self, empty_paragraph_attrs: NodeAttrs) -> Self {
        self.cell_ranges = CellRangeExport::Clipped(Arc::new(empty_paragraph_attrs));
        self
    }

    /// Selects a bounded, deterministic plain-text algorithm, not supplied text.
    #[must_use]
    pub const fn with_text_projection(mut self, projection: ClipboardTextProjection) -> Self {
        self.text_projection = Some(projection);
        self
    }

    pub(super) const fn cell_ranges(&self) -> &CellRangeExport {
        &self.cell_ranges
    }

    /// Returns the independently selected plain-text algorithm.
    #[must_use]
    pub const fn text_projection(&self) -> Option<ClipboardTextProjection> {
        self.text_projection
    }
}
