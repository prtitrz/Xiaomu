//! Hit only painted cell boundaries, never imaginary edges inside a colspan.

use std::rc::Rc;

use gpui::{Bounds, Pixels, Point, px};
use xiaomu_core::document::{CellPlacement, DocumentRevision, NodeId, XiaomuDocument};

use crate::table_capability::TableCapabilityKey;
use crate::table_column_resize::{TableColumnResizeConfig, TableColumnResizeIntent};
use crate::table_layout::TableGeometry;

pub(in crate::document_view) struct ResizeMeasurement {
    pub table: NodeId,
    pub revision: DocumentRevision,
    pub document: XiaomuDocument,
    pub key: Rc<TableCapabilityKey>,
    pub origin: Point<Pixels>,
    pub available: f32,
    pub clip: Bounds<Pixels>,
    pub geometry: TableGeometry,
    pub placements: Vec<CellPlacement>,
}

impl ResizeMeasurement {
    pub(super) fn contains(&self, position: Point<Pixels>, handle: f32) -> bool {
        self.clip.contains(&position)
            && position.y >= self.origin.y
            && position.y <= self.origin.y + px(self.geometry.height)
            && position.x >= self.origin.x - px(handle)
            && position.x <= self.origin.x + px(self.geometry.width + handle)
    }

    pub(super) fn hit(
        &self,
        position: Point<Pixels>,
        config: TableColumnResizeConfig,
    ) -> Option<TableColumnResizeIntent> {
        if !config.valid() || !self.contains(position, config.handle_width) {
            return None;
        }
        let x = f32::from(position.x - self.origin.x);
        let y = f32::from(position.y - self.origin.y);
        // Keep the existing seven-pixel cell-selection handles fully operable.
        if self
            .geometry
            .cells
            .iter()
            .any(|cell| (cell.x..cell.x + 7.0).contains(&x) && (cell.y..cell.y + 7.0).contains(&y))
        {
            return None;
        }
        let mut nearest: Option<(usize, f32)> = None;
        for (cell, placement) in self.geometry.cells.iter().zip(&self.placements) {
            if y < cell.y || y > cell.y + cell.height {
                continue;
            }
            for (edge, column) in [
                (cell.x, placement.column().checked_sub(1)),
                (
                    cell.x + cell.width,
                    Some(placement.column() + placement.colspan() - 1),
                ),
            ] {
                let Some(column) = column else { continue };
                let distance = (x - edge).abs();
                if distance <= config.handle_width
                    && (config.last_column_resizable
                        || column + 2 < self.geometry.column_edges.len())
                    && nearest.is_none_or(|(_, previous)| distance < previous)
                {
                    nearest = Some((column, distance));
                }
            }
        }
        let (column, _) = nearest?;
        let width = integral_width(
            self.geometry.column_edges[column + 1] - self.geometry.column_edges[column],
        )?;
        Some(TableColumnResizeIntent {
            table: self.table,
            column,
            initial_width: width,
            width,
            revision: self.revision,
        })
    }
}

pub(super) fn integral_width(width: f32) -> Option<u32> {
    (width.is_finite() && (1.0..=1_000_000.0).contains(&width) && width.fract() == 0.0)
        .then_some(width as u32)
}

pub(super) fn drag_width(initial: u32, start_x: f32, current_x: f32, minimum: u32) -> Option<u32> {
    if !start_x.is_finite() || !current_x.is_finite() {
        return None;
    }
    // GPUI supplies f32 logical coordinates, including subpixel native decoding.
    // Widen each operand before arithmetic; this cannot recover prior precision.
    // Subtract first so equal large coordinates preserve the initial width.
    let delta = f64::from(current_x) - f64::from(start_x);
    let width = (f64::from(initial) + delta).max(f64::from(minimum));
    // Refuse oversized raw requests before rounding; do not silently clamp them.
    // The positive target rounds to nearest integer, with exact halves upward.
    (1.0..=1_000_000.0)
        .contains(&width)
        .then_some(width.round() as u32)
}
