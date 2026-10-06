//! Clipped CellRange export is explicit, lossless at retained origins and read-only.

use super::*;
use crate::session::{
    DocumentSelection, DocumentSession, PolicyError, SessionContext, SessionError, SessionPolicy,
};
use std::collections::BTreeMap;
use xiaomu_core::document::{
    AttrValue, InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;

mod boundaries;
mod geometry;
mod rich;
mod state_budget;

struct Policy(ClipboardExportSpec);

impl SessionPolicy for Policy {
    fn clipboard_export_spec(
        &self,
        _: SessionContext<'_>,
        _: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        Ok(Some(self.0.clone()))
    }
}

fn attrs(values: &[(&str, AttrValue)]) -> NodeAttrs {
    NodeAttrs::new(
        values
            .iter()
            .map(|(key, value)| ((*key).into(), value.clone()))
            .collect(),
    )
    .unwrap()
}

fn extended(original: &NodeAttrs, values: &[(&str, AttrValue)]) -> NodeAttrs {
    let mut result = original
        .iter()
        .map(|(key, value)| (key.to_owned(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    result.extend(
        values
            .iter()
            .map(|(key, value)| ((*key).into(), value.clone())),
    );
    NodeAttrs::new(result).unwrap()
}

fn widths(values: &[i64]) -> AttrValue {
    AttrValue::List(values.iter().copied().map(AttrValue::Integer).collect())
}

fn opaque() -> AttrValue {
    AttrValue::Object(
        [
            ("nullable".into(), AttrValue::Null),
            (
                "values".into(),
                AttrValue::List(vec![
                    AttrValue::Bool(false),
                    AttrValue::Integer(-9),
                    AttrValue::String("原样\r\n\t🙂".into()),
                ]),
            ),
        ]
        .into(),
    )
}

fn defaults() -> NodeAttrs {
    attrs(&[("hostParagraph", opaque()), ("align", AttrValue::Null)])
}

fn spec_with(defaults: NodeAttrs) -> ClipboardExportSpec {
    ClipboardExportSpec::new()
        .with_clipped_cell_ranges(defaults)
        .with_text_projection(ClipboardTextProjection::TextBetweenLfV1)
}

fn spec() -> ClipboardExportSpec {
    spec_with(defaults())
}

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    let inline = if text.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new([TextRun::new(text, Default::default()).unwrap()]).unwrap()
    };
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline),
        )
        .unwrap()
}

struct CellSpec {
    row: usize,
    column: usize,
    rowspan: usize,
    colspan: usize,
    attrs: NodeAttrs,
    rich: bool,
    header: bool,
}

impl CellSpec {
    fn new(row: usize, column: usize, rowspan: usize, colspan: usize) -> Self {
        let mut values = vec![("origin", AttrValue::String(format!("{row}:{column}")))];
        if rowspan != 1 {
            values.push(("rowspan", AttrValue::Integer(rowspan as i64)));
        }
        if colspan != 1 {
            values.push(("colspan", AttrValue::Integer(colspan as i64)));
        }
        Self {
            row,
            column,
            rowspan,
            colspan,
            attrs: attrs(&values),
            rich: false,
            header: false,
        }
    }

    fn with_attrs(mut self, values: &[(&str, AttrValue)]) -> Self {
        self.attrs = extended(&self.attrs, values);
        self
    }

    fn rich_header(mut self) -> Self {
        self.rich = true;
        self.header = true;
        self
    }
}

struct Fixture {
    document: XiaomuDocument,
    table: NodeId,
    intro: NodeId,
    cells: BTreeMap<(usize, usize), NodeId>,
    row_attrs: Vec<NodeAttrs>,
}

fn fixture(rows: usize, columns: usize, cells: Vec<CellSpec>) -> Fixture {
    fixture_with_intro(rows, columns, cells, "intro")
}

fn fixture_with_intro(
    row_count: usize,
    columns: usize,
    specs: Vec<CellSpec>,
    intro_text: &str,
) -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let intro = paragraph(&mut builder, intro_text);
    let mut covered = vec![vec![false; columns]; row_count];
    let mut cells = BTreeMap::new();
    let mut rows = Vec::new();
    let mut row_attrs = Vec::new();
    for row in 0..row_count {
        let mut children = Vec::new();
        for column in 0..columns {
            if covered[row][column] {
                continue;
            }
            let unit = CellSpec::new(row, column, 1, 1);
            let cell = specs
                .iter()
                .find(|spec| spec.row == row && spec.column == column)
                .unwrap_or(&unit);
            for covered_row in &mut covered[row..row + cell.rowspan] {
                for slot in &mut covered_row[column..column + cell.colspan] {
                    assert!(!*slot, "fixture spans must not overlap");
                    *slot = true;
                }
            }
            let content = if cell.rich {
                rich::children(&mut builder)
            } else {
                vec![paragraph(&mut builder, &format!("{row}:{column}"))]
            };
            let id = builder
                .insert(
                    if cell.header {
                        NodeKind::TableHeader
                    } else {
                        NodeKind::TableCell
                    },
                    cell.attrs.clone(),
                    NodeContent::children(content),
                )
                .unwrap();
            children.push(id);
            cells.insert((row, column), id);
        }
        let raw_attrs = attrs(&[
            ("row", AttrValue::Integer(row as i64)),
            ("opaque", opaque()),
        ]);
        rows.push(
            builder
                .insert(
                    NodeKind::TableRow,
                    raw_attrs.clone(),
                    NodeContent::children(children),
                )
                .unwrap(),
        );
        row_attrs.push(raw_attrs);
    }
    let table = builder
        .insert(
            NodeKind::Table,
            attrs(&[("table", AttrValue::Null), ("opaque", opaque())]),
            NodeContent::children(rows),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([intro, table]),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        table,
        intro,
        cells,
        row_attrs,
    }
}

fn session(f: &Fixture, options: Option<ClipboardExportSpec>) -> DocumentSession {
    let selection = DocumentSelection::collapsed(InlinePoint::at_start_of(f.intro));
    match options {
        Some(options) => DocumentSession::new_with_policy(
            f.document.clone(),
            selection,
            Box::new(Policy(options)),
        )
        .unwrap(),
        None => DocumentSession::new(f.document.clone(), selection).unwrap(),
    }
}

fn copied(
    f: &Fixture,
    anchor: (usize, usize),
    focus: (usize, usize),
    options: Option<ClipboardExportSpec>,
) -> Result<ClipboardSlice, SessionError> {
    let mut session = session(f, options);
    session
        .set_cell_range_selection(f.cells[&anchor], f.cells[&focus])
        .unwrap();
    session.clipboard_slice().map(Option::unwrap)
}

fn rows(slice: &ClipboardSlice) -> (&[Vec<ClipboardNode>], &[NodeAttrs]) {
    let ClipboardNodeContent::Table { rows, row_attrs } = slice.roots()[0].content() else {
        panic!("CellRange export must retain a table carrier")
    };
    (rows, row_attrs)
}

fn origin(cell: &ClipboardNode) -> &str {
    let Some(AttrValue::String(value)) = cell.attrs().get("origin") else {
        panic!("fixture origin attribute was lost")
    };
    value
}

fn find_cell<'a>(slice: &'a ClipboardSlice, label: &str) -> &'a ClipboardNode {
    rows(slice)
        .0
        .iter()
        .flatten()
        .find(|cell| origin(cell) == label)
        .unwrap()
}

fn assert_cleared(cell: &ClipboardNode, expected_attrs: &NodeAttrs) {
    let children = cell.content().as_children().unwrap();
    assert_eq!(children.len(), 1, "crossing origins replace all children");
    assert_eq!(children[0].kind(), &NodeKind::Paragraph);
    assert_eq!(children[0].attrs(), expected_attrs);
    assert!(children[0].content().as_inline().unwrap().is_empty());
}

fn roundtrip(slice: &ClipboardSlice) {
    let metadata = encode_metadata(slice).unwrap();
    assert!(metadata.starts_with("xiaomu.clipboard.v14\n"));
    assert_eq!(
        decode_metadata_checked(slice.plain_text(), &metadata),
        ClipboardMetadataDecode::Valid(slice.clone())
    );
}
