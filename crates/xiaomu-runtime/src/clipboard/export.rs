//! Explicit, deterministic host clipboard export options and provenance.

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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClipboardExportSpec {
    closed_cell_ranges: bool,
    text_projection: Option<ClipboardTextProjection>,
}

impl ClipboardExportSpec {
    /// Creates options without broadening cell geometry or changing plain text.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            closed_cell_ranges: false,
            text_projection: None,
        }
    }

    /// Enables whole-origin copying of geometrically closed logical rectangles.
    ///
    /// Nonclosed rectangles still reject. CellRange Cut remains unsupported and
    /// is rejected before projection or any platform clipboard write.
    #[must_use]
    pub const fn with_closed_cell_ranges(mut self) -> Self {
        self.closed_cell_ranges = true;
        self
    }

    /// Selects a bounded, deterministic plain-text algorithm, not supplied text.
    #[must_use]
    pub const fn with_text_projection(mut self, projection: ClipboardTextProjection) -> Self {
        self.text_projection = Some(projection);
        self
    }

    pub(crate) const fn closed_cell_ranges(self) -> bool {
        self.closed_cell_ranges
    }

    /// Returns the independently selected plain-text algorithm.
    #[must_use]
    pub const fn text_projection(self) -> Option<ClipboardTextProjection> {
        self.text_projection
    }
}
