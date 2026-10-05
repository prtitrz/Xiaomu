//! Detached clipping at the exact source rectangle, without unit-cell expansion.

use std::collections::BTreeMap;

use xiaomu_core::document::{
    AttrValue, CellPlacement, NodeAttrs, NodeId, NodeKind, TableRect, XiaomuDocument,
};

use crate::clipboard::export_budget::ClippedBudget;
use crate::clipboard::projection::whole_fragment;
use crate::clipboard::{
    ClipboardCellRangeRoot, ClipboardInline, ClipboardNode, ClipboardNodeContent,
    ClipboardSourceBoundary,
};
use crate::session::{CellRange, PolicyError, SessionError};

struct Part {
    source: NodeId,
    row: usize,
    column: usize,
    rowspan: usize,
    colspan: usize,
    left_trim: usize,
    crop_columns: bool,
    crop_rows: bool,
    clear_content: bool,
}

impl Part {
    fn intersect(source: &CellPlacement, rect: TableRect) -> Option<Self> {
        // Grid construction has already checked both source extent additions.
        let top = source.row().max(rect.top());
        let left = source.column().max(rect.left());
        let bottom = (source.row() + source.rowspan()).min(rect.bottom());
        let right = (source.column() + source.colspan()).min(rect.right());
        if top >= bottom || left >= right {
            return None;
        }
        let rowspan = bottom - top;
        let colspan = right - left;
        Some(Self {
            source: source.cell(),
            row: top - rect.top(),
            column: left - rect.left(),
            rowspan,
            colspan,
            left_trim: left - source.column(),
            crop_columns: colspan != source.colspan(),
            crop_rows: rowspan != source.rowspan(),
            clear_content: source.row() < rect.top() || source.column() < rect.left(),
        })
    }
}

pub(super) fn capture(
    document: &XiaomuDocument,
    range: CellRange,
    empty_paragraph_attrs: &NodeAttrs,
    mut budget: ClippedBudget,
) -> Result<(Vec<ClipboardNode>, ClipboardSourceBoundary), SessionError> {
    let table_id = document
        .parent_of(range.anchor())
        .and_then(|row| document.parent_of(row))
        .ok_or(SessionError::SelectionInvalid)?;
    let grid = document.table_grid(table_id).map_err(SessionError::Core)?;
    let rect = grid
        .rect_between(range.anchor(), range.focus())
        .map_err(SessionError::Core)?;
    // The borrowed source preflight bounds this small geometry-only vector.
    // Capture every intersecting origin, including those entering from above
    // or the left. Selection ranges/unit-only projection omit those origins.
    let mut parts = Vec::new();
    parts
        .try_reserve_exact(grid.origins().len())
        .map_err(|_| PolicyError::new("clipboard clipping exceeds geometry budget"))?;
    parts.extend(
        grid.origins()
            .filter_map(|origin| Part::intersect(origin, rect)),
    );
    parts.sort_unstable_by_key(|part| (part.row, part.column));
    let cleared = parts.iter().filter(|part| part.clear_content).count();
    budget
        .reserve_empty_paragraphs(empty_paragraph_attrs, cleared)
        .map_err(|()| PolicyError::new("clipboard clipping exceeds fill paragraph budget"))?;

    // No source/default attrs or tree payload has been cloned before this point.
    let table = document
        .node(table_id)
        .ok_or(SessionError::SelectionInvalid)?;
    let source_rows = table
        .content()
        .as_children()
        .ok_or(SessionError::SelectionInvalid)?;
    let mut rows: Vec<Vec<ClipboardNode>> =
        (rect.top()..rect.bottom()).map(|_| Vec::new()).collect();
    for part in parts {
        let source = document
            .node(part.source)
            .ok_or(SessionError::SelectionInvalid)?;
        let attrs = cropped_attrs(source.attrs(), &part)?;
        let children = if part.clear_content {
            vec![ClipboardNode::new(
                NodeKind::Paragraph,
                empty_paragraph_attrs.clone(),
                ClipboardNodeContent::Inline(ClipboardInline::default()),
            )]
        } else {
            source
                .content()
                .as_children()
                .ok_or(SessionError::SelectionInvalid)?
                .iter()
                .map(|child| whole_fragment(document, *child))
                .collect::<Result<Vec<_>, _>>()?
        };
        rows[part.row].push(ClipboardNode::new(
            source.kind().clone(),
            attrs,
            ClipboardNodeContent::Children(children),
        ));
    }
    // A cell moved down from an earlier origin row belongs to this selected
    // row's wrapper; source origin-row metadata must not move with the cell.
    let mut row_attrs = source_rows[rect.top()..rect.bottom()]
        .iter()
        .map(|row| {
            document
                .node(*row)
                .map(|node| node.attrs().clone())
                .ok_or(SessionError::SelectionInvalid)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if row_attrs.iter().all(NodeAttrs::is_empty) {
        row_attrs.clear();
    }
    let root_form = if rect.top() == 0
        && rect.left() == 0
        && rect.bottom() == grid.rows()
        && rect.right() == grid.columns()
    {
        ClipboardCellRangeRoot::Table
    } else {
        ClipboardCellRangeRoot::Rows
    };
    Ok((
        vec![ClipboardNode::new(
            NodeKind::Table,
            table.attrs().clone(),
            ClipboardNodeContent::Table { rows, row_attrs },
        )],
        ClipboardSourceBoundary::CellRange { root_form },
    ))
}

fn cropped_attrs(source: &NodeAttrs, part: &Part) -> Result<NodeAttrs, SessionError> {
    if !part.crop_columns && !part.crop_rows {
        return Ok(source.clone());
    }
    let mut attrs: BTreeMap<String, AttrValue> = source
        .iter()
        .map(|(key, value)| (key.to_owned(), value.clone()))
        .collect();
    if part.crop_columns {
        attrs.insert("colspan".into(), span(part.colspan)?);
        if let Some(AttrValue::List(widths)) = source.get("colwidth") {
            let end = part
                .left_trim
                .checked_add(part.colspan)
                .ok_or(SessionError::SelectionInvalid)?;
            let widths = widths
                .get(part.left_trim..end)
                .ok_or(SessionError::SelectionInvalid)?;
            let cropped = if widths
                .iter()
                .any(|value| matches!(value, AttrValue::Integer(width) if *width > 0))
            {
                AttrValue::List(widths.to_vec())
            } else {
                AttrValue::Null
            };
            attrs.insert("colwidth".into(), cropped);
        }
    }
    if part.crop_rows {
        attrs.insert("rowspan".into(), span(part.rowspan)?);
    }
    NodeAttrs::new(attrs).map_err(SessionError::Core)
}

fn span(value: usize) -> Result<AttrValue, SessionError> {
    i64::try_from(value)
        .map(AttrValue::Integer)
        .map_err(|_| SessionError::SelectionInvalid)
}
