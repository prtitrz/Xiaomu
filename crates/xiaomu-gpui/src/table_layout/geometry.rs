//! Snapshot geometry and deterministic, measured row constraints.

use gpui::{Bounds, Pixels, Point, point, px, size};
use xiaomu_core::document::{
    CellPlacement, NodeId, NodeKind, TableAttribute, TableCellAttrs, XiaomuDocument,
};

// A renderer capability bound, not a new canonical-document constraint. It
// keeps enormous persisted integers out of f32 coordinate arithmetic.
const MAX_EXTENT: f64 = 1_000_000.0;

/// Explicit native sizing policy, independent of the persisted attributes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TableLayoutOptions {
    /// Minimum width of an automatic column; must be positive and finite.
    pub min_auto_column_width: f32,
    /// Minimum height of each logical row; must be nonnegative and finite.
    pub min_row_height: f32,
}

impl Default for TableLayoutOptions {
    fn default() -> Self {
        Self {
            min_auto_column_width: 40.0,
            min_row_height: 28.0,
        }
    }
}

/// A rejected prototype layout; no error changes the canonical document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableLayoutError {
    /// Core refused the table or its attributes.
    InvalidTable(xiaomu_core::Error),
    /// Positive width hints disagree for this logical column.
    ConflictingColumnWidth {
        /// Zero-based logical column.
        column: usize,
    },
    /// A dimension is negative, nonfinite, or outside this renderer's bound.
    InvalidDimension,
    /// Measured cells do not match the plan's unique origins in order.
    CellMismatch,
    /// An element instance received a second layout request instead of being
    /// rebuilt for the next frame, contrary to GPUI's element lifecycle.
    RepeatedLayoutRequest,
}

impl std::fmt::Display for TableLayoutError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "table layout refused: {self:?}")
    }
}

impl std::error::Error for TableLayoutError {}

impl From<xiaomu_core::Error> for TableLayoutError {
    fn from(error: xiaomu_core::Error) -> Self {
        Self::InvalidTable(error)
    }
}

/// One unique physical cell with its checked logical placement and kind.
#[derive(Clone, Debug)]
pub struct TableCellPlan {
    /// Core-validated origin and spans.
    pub placement: CellPlacement,
    /// True for the distinct canonical `TableHeader` kind.
    pub is_header: bool,
}

/// Immutable width/placement input for one document snapshot.
///
/// Rebuild after a revision, attribute or host sizing-policy change. This
/// object does not retain or modify the document, nor normalize raw attrs.
#[derive(Clone, Debug)]
pub struct TableLayoutPlan {
    table: NodeId,
    rows: usize,
    cells: Vec<TableCellPlan>,
    fixed_columns: Vec<Option<usize>>,
    options: TableLayoutOptions,
}

impl TableLayoutPlan {
    /// Reads checked geometry and reconciles positive hints without repairing.
    pub fn from_document(
        document: &XiaomuDocument,
        table: NodeId,
        options: TableLayoutOptions,
    ) -> Result<Self, TableLayoutError> {
        dimension(f64::from(options.min_auto_column_width))?;
        dimension(f64::from(options.min_row_height))?;
        if options.min_auto_column_width == 0.0 {
            return Err(TableLayoutError::InvalidDimension);
        }
        let grid = document.table_grid(table)?;
        let mut fixed_columns = vec![None; grid.columns()];
        let mut cells = Vec::with_capacity(grid.origins().len());
        for placement in grid.origins() {
            let node = document
                .node(placement.cell())
                .expect("checked grid has existing cell identities");
            let attrs = TableCellAttrs::read(node.attrs())?;
            if let TableAttribute::Value(widths) = attrs.colwidth() {
                for (offset, width) in widths.iter().enumerate() {
                    if width == 0 {
                        continue;
                    }
                    dimension(width as f64)?;
                    let column = placement.column() + offset;
                    match fixed_columns[column] {
                        Some(existing) if existing != width => {
                            return Err(TableLayoutError::ConflictingColumnWidth { column });
                        }
                        _ => fixed_columns[column] = Some(width),
                    }
                }
            }
            cells.push(TableCellPlan {
                placement: *placement,
                is_header: matches!(node.kind(), NodeKind::TableHeader),
            });
        }
        Ok(Self {
            table,
            rows: grid.rows(),
            cells,
            fixed_columns,
            options,
        })
    }

    /// Overrides one track for a renderer-only preview. Canonical attributes
    /// are neither modified nor interpreted as an edit. Other automatic tracks
    /// keep the plan's ordinary remaining-space policy and minimum. Rebuild the
    /// plan from the document to discard this override.
    pub(crate) fn override_column_width(
        &mut self,
        column: usize,
        width: u32,
    ) -> Result<(), TableLayoutError> {
        dimension(f64::from(width))?;
        if width == 0 {
            return Err(TableLayoutError::InvalidDimension);
        }
        let track = self
            .fixed_columns
            .get_mut(column)
            .ok_or(TableLayoutError::InvalidDimension)?;
        *track = Some(width as usize);
        Ok(())
    }

    /// Returns the table's stable identity.
    #[must_use]
    pub const fn table(&self) -> NodeId {
        self.table
    }

    /// Returns unique origins in Core's logical row-major order.
    #[must_use]
    pub fn cells(&self) -> &[TableCellPlan] {
        &self.cells
    }

    /// Resolves shared tracks from the containing content width.
    ///
    /// Positive hints are exact; automatic columns split unused width equally.
    /// Minimum automatic widths may make the table wider than its container.
    pub fn column_widths(&self, available_width: f32) -> Result<Vec<f32>, TableLayoutError> {
        dimension(f64::from(available_width))?;
        let fixed: f64 = self.fixed_columns.iter().flatten().map(|w| *w as f64).sum();
        dimension(fixed)?;
        let automatic = self.fixed_columns.iter().filter(|w| w.is_none()).count();
        let auto_width = if automatic == 0 {
            0.0
        } else {
            ((f64::from(available_width) - fixed) / automatic as f64)
                .max(f64::from(self.options.min_auto_column_width))
        };
        dimension(fixed + auto_width * automatic as f64)?;
        Ok(self
            .fixed_columns
            .iter()
            .map(|hint| hint.map_or(auto_width as f32, |value| value as f32))
            .collect())
    }

    /// Solves row heights using actual cell-root measurements at shared widths.
    ///
    /// `cell_heights` must contain one natural height per [`Self::cells`] entry,
    /// including borders/padding implemented by the caller's child subtree.
    /// Unit cells establish row minima first; increasing rowspan order then
    /// shares each remaining deficit among its covered rows. This guarantees
    /// every cell fits without using fixed text-height estimates.
    pub fn layout(
        &self,
        available_width: f32,
        cell_heights: &[f32],
    ) -> Result<TableGeometry, TableLayoutError> {
        if cell_heights.len() != self.cells.len() {
            return Err(TableLayoutError::CellMismatch);
        }
        let columns = self.column_widths(available_width)?;
        let column_edges = edges(columns.iter().copied().map(f64::from))?;
        let mut row_heights = vec![f64::from(self.options.min_row_height); self.rows];
        for (cell, height) in self.cells.iter().zip(cell_heights) {
            dimension(f64::from(*height))?;
            if cell.placement.rowspan() == 1 {
                let row = &mut row_heights[cell.placement.row()];
                *row = row.max(f64::from(*height));
            }
        }
        let mut spanning: Vec<_> = self
            .cells
            .iter()
            .zip(cell_heights)
            .filter(|(cell, _)| cell.placement.rowspan() > 1)
            .collect();
        spanning.sort_by_key(|(cell, _)| cell.placement.rowspan());
        for (cell, measured) in spanning {
            let start = cell.placement.row();
            let rows = &mut row_heights[start..start + cell.placement.rowspan()];
            let current: f64 = rows.iter().sum();
            let deficit = (f64::from(*measured) - current).max(0.0) / rows.len() as f64;
            for row in rows {
                *row += deficit;
            }
        }
        let row_edges = edges(row_heights.iter().copied())?;
        let cells = self
            .cells
            .iter()
            .zip(cell_heights)
            .map(|(cell, height)| {
                let placement = cell.placement;
                let left = column_edges[placement.column()];
                let top = row_edges[placement.row()];
                TableCellGeometry {
                    cell: placement.cell(),
                    is_header: cell.is_header,
                    x: left,
                    y: top,
                    width: column_edges[placement.column() + placement.colspan()] - left,
                    height: row_edges[placement.row() + placement.rowspan()] - top,
                    measured_height: *height,
                }
            })
            .collect();
        Ok(TableGeometry {
            width: *column_edges.last().unwrap_or(&0.0),
            height: *row_edges.last().unwrap_or(&0.0),
            column_edges,
            row_edges,
            cells,
        })
    }
}

fn dimension(value: f64) -> Result<(), TableLayoutError> {
    if value.is_finite() && (0.0..=MAX_EXTENT).contains(&value) {
        Ok(())
    } else {
        Err(TableLayoutError::InvalidDimension)
    }
}

fn edges(sizes: impl IntoIterator<Item = f64>) -> Result<Vec<f32>, TableLayoutError> {
    let mut result = vec![0.0];
    let mut end = 0.0;
    for size in sizes {
        end += size;
        dimension(end)?;
        result.push(end as f32);
    }
    Ok(result)
}

/// Full table and cell rectangles in one shared local coordinate system.
#[derive(Clone, Debug, PartialEq)]
pub struct TableGeometry {
    /// Actual table width, which may overflow the available width.
    pub width: f32,
    /// Height from real child measurements and row constraints.
    pub height: f32,
    /// Cumulative column boundaries, starting at zero.
    pub column_edges: Vec<f32>,
    /// Cumulative row boundaries, starting at zero.
    pub row_edges: Vec<f32>,
    /// Unique full-cell rectangles in the plan's origin order.
    pub cells: Vec<TableCellGeometry>,
}

/// A cell's full selectable area and its natural measured content height.
#[derive(Clone, Debug, PartialEq)]
pub struct TableCellGeometry {
    /// Stable identity; covered logical slots never create duplicate children.
    pub cell: NodeId,
    /// Canonical Header identity, for caller-owned presentation.
    pub is_header: bool,
    /// Left offset within the table.
    pub x: f32,
    /// Top offset within the table.
    pub y: f32,
    /// Shared-track width, including all spanned columns.
    pub width: f32,
    /// Full height, including all spanned rows.
    pub height: f32,
    /// Natural child-root height at this exact cell width.
    pub measured_height: f32,
}

impl TableCellGeometry {
    /// Translates the full-cell rectangle to the same absolute origin used by
    /// `prepaint_at`; hosts can register blank-space hits with these bounds.
    #[must_use]
    pub fn bounds_at(&self, origin: Point<Pixels>) -> Bounds<Pixels> {
        Bounds::new(
            origin + point(px(self.x), px(self.y)),
            size(px(self.width), px(self.height)),
        )
    }
}
