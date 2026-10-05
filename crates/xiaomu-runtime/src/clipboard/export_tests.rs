//! Opt-in producer, provenance, text binding and historical-default regressions.

use super::*;
use crate::session::{
    DocumentSelection, DocumentSession, PolicyError, SessionContext, SessionError, SessionPolicy,
};
use xiaomu_core::document::{
    AttrValue, InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;

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

fn spec() -> ClipboardExportSpec {
    ClipboardExportSpec::new()
        .with_closed_cell_ranges()
        .with_text_projection(ClipboardTextProjection::TextBetweenLfV1)
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

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, Default::default()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}

struct Fixture {
    document: XiaomuDocument,
    table: NodeId,
    cells: Vec<NodeId>,
    intro: NodeId,
}
fn fixture(spans: bool) -> Fixture {
    let mut b = NodeStoreBuilder::new();
    let intro = paragraph(&mut b, "intro");
    let mut cells = Vec::new();
    let mut rows = Vec::new();
    let shape: &[&[&str]] = if spans {
        &[&["A", "B"], &["C"], &["D", "E", "F"]]
    } else {
        &[&["A", "B"], &["C", "D"]]
    };
    for (r, values) in shape.iter().enumerate() {
        let mut row = Vec::new();
        for (c, text) in values.iter().enumerate() {
            let leaf = paragraph(&mut b, text);
            let cell_attrs = if spans && r == 0 && c == 0 {
                attrs(&[
                    ("colspan", AttrValue::Integer(2)),
                    ("rowspan", AttrValue::Integer(2)),
                    (
                        "colwidth",
                        AttrValue::List(vec![AttrValue::Integer(80), AttrValue::Integer(120)]),
                    ),
                    (
                        "opaque",
                        AttrValue::Object([("null".into(), AttrValue::Null)].into()),
                    ),
                ])
            } else {
                NodeAttrs::empty()
            };
            let kind = if spans && r == 0 && c == 0 {
                NodeKind::TableHeader
            } else {
                NodeKind::TableCell
            };
            let cell = b
                .insert(kind, cell_attrs, NodeContent::children([leaf]))
                .unwrap();
            cells.push(cell);
            row.push(cell);
        }
        rows.push(
            b.insert(
                NodeKind::TableRow,
                attrs(&[("row", AttrValue::Integer(r as i64))]),
                NodeContent::children(row),
            )
            .unwrap(),
        );
    }
    let table = b
        .insert(
            NodeKind::Table,
            attrs(&[("table", AttrValue::Null)]),
            NodeContent::children(rows),
        )
        .unwrap();
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([intro, table]),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, b.finish()).unwrap(),
        table,
        cells,
        intro,
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

fn copied_range(
    f: &Fixture,
    anchor: usize,
    focus: usize,
    options: Option<ClipboardExportSpec>,
) -> Result<ClipboardSlice, SessionError> {
    let mut session = session(f, options);
    session
        .set_cell_range_selection(f.cells[anchor], f.cells[focus])
        .unwrap();
    let before = session.document().clone();
    let selection = session.selection();
    let result = session.clipboard_slice();
    assert_eq!(session.document().store(), before.store());
    assert_eq!(session.document().revision(), before.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), (0, 0));
    result.map(Option::unwrap)
}

fn roundtrip(slice: &ClipboardSlice) {
    let metadata = encode_metadata(slice).unwrap();
    assert!(metadata.starts_with("xiaomu.clipboard.v14\n"));
    assert_eq!(
        decode_metadata_checked(slice.plain_text(), &metadata),
        ClipboardMetadataDecode::Valid(slice.clone())
    );
}

#[test]
fn closed_span_rectangle_preserves_empty_physical_row_and_direction() {
    let f = fixture(true);
    let slice = copied_range(&f, 0, 0, Some(spec())).unwrap();
    assert_eq!(slice.plain_text(), "A");
    assert_eq!(
        slice.source_boundary(),
        Some(ClipboardSourceBoundary::CellRange {
            root_form: ClipboardCellRangeRoot::Rows
        })
    );
    assert_eq!(slice.source_boundary().unwrap().open_depths(), Some((1, 1)));
    assert!(!slice.is_closed());
    let ClipboardNodeContent::Table { rows, row_attrs } = slice.roots()[0].content() else {
        panic!()
    };
    assert_eq!(rows.iter().map(Vec::len).collect::<Vec<_>>(), [1, 0]);
    assert_eq!(rows[0][0].kind(), &NodeKind::TableHeader);
    assert_eq!(
        rows[0][0].attrs(),
        f.document.node(f.cells[0]).unwrap().attrs()
    );
    assert_eq!(
        row_attrs,
        &vec![
            attrs(&[("row", AttrValue::Integer(0))]),
            attrs(&[("row", AttrValue::Integer(1))])
        ]
    );
    roundtrip(&slice);
    let forward = copied_range(&f, 0, 2, Some(spec())).unwrap();
    assert_eq!(forward, copied_range(&f, 2, 0, Some(spec())).unwrap());
    assert_eq!(forward.plain_text(), "A\nB\nC");
    roundtrip(&forward);
}

#[test]
fn full_table_cell_range_and_explicit_whole_roots_stay_distinct() {
    let f = fixture(true);
    let cell_slice = copied_range(&f, 0, 5, Some(spec())).unwrap();
    assert_eq!(
        cell_slice.source_boundary(),
        Some(ClipboardSourceBoundary::CellRange {
            root_form: ClipboardCellRangeRoot::Table
        })
    );
    assert!(!cell_slice.is_closed());
    let mut s = session(&f, Some(spec()));
    s.set_document_selection(DocumentSelection::node(s.document(), f.table).unwrap())
        .unwrap();
    let whole = s.clipboard_slice().unwrap().unwrap();
    assert_eq!(whole.roots(), cell_slice.roots());
    assert_eq!(whole.plain_text(), cell_slice.plain_text());
    assert_eq!(
        whole.source_boundary(),
        Some(ClipboardSourceBoundary::WholeRoots)
    );
    assert_eq!(whole.source_boundary().unwrap().open_depths(), Some((0, 0)));
    assert!(whole.is_closed());
    roundtrip(&whole);
    roundtrip(&cell_slice);
    s.set_document_selection(DocumentSelection::all(s.document()))
        .unwrap();
    let all = s.clipboard_slice().unwrap().unwrap();
    assert_eq!(all.plain_text(), "intro\nA\nB\nC\nD\nE\nF");
    roundtrip(&all);
}

#[test]
fn nonclosed_geometry_and_cut_reject_before_any_state_change() {
    let f = fixture(true);
    assert_eq!(
        copied_range(&f, 1, 4, Some(spec())),
        Err(SessionError::UnsupportedTableOperation)
    );
    for spans in [false, true] {
        let f = fixture(spans);
        let mut s = session(&f, Some(spec()));
        s.set_cell_range_selection(f.cells[0], *f.cells.last().unwrap())
            .unwrap();
        let before = s.document().clone();
        let selection = s.selection();
        assert_eq!(
            s.clipboard_slice_for(ClipboardExportPurpose::Cut),
            Err(SessionError::UnsupportedTableOperation)
        );
        assert_eq!(s.document().store(), before.store());
        assert_eq!(s.selection(), selection);
        assert_eq!(s.history_depths(), (0, 0));
    }
}

#[test]
fn legacy_defaults_and_independent_projection_options_are_preserved() {
    let unit = fixture(false);
    let old = copied_range(&unit, 0, 3, None).unwrap();
    assert_eq!(old.plain_text(), "A\tB\nC\tD");
    assert_eq!(old.source_boundary(), None);
    assert_eq!(old.text_projection(), None);
    assert!(
        !encode_metadata(&old)
            .unwrap()
            .starts_with("xiaomu.clipboard.")
    );
    assert_eq!(
        decode_metadata(old.plain_text(), &encode_metadata(&old).unwrap()),
        Some(old)
    );
    let spanning = fixture(true);
    assert_eq!(
        copied_range(&spanning, 0, 5, None),
        Err(SessionError::UnsupportedTableOperation)
    );
    let text_only =
        ClipboardExportSpec::new().with_text_projection(ClipboardTextProjection::TextBetweenLfV1);
    assert_eq!(
        copied_range(&spanning, 0, 5, Some(text_only.clone())),
        Err(SessionError::UnsupportedTableOperation)
    );
    let structure_only = copied_range(
        &spanning,
        0,
        0,
        Some(ClipboardExportSpec::new().with_closed_cell_ranges()),
    )
    .unwrap();
    assert_eq!(structure_only.plain_text(), "A\t\n\t");
    assert_eq!(structure_only.text_projection(), None);
    roundtrip(&structure_only);
    let projected_unit = copied_range(&unit, 0, 3, Some(text_only)).unwrap();
    assert_eq!(projected_unit.plain_text(), "A\nB\nC\nD");
    roundtrip(&projected_unit);
}

#[test]
fn purpose_policy_rejection_precedes_projection() {
    struct Reject;
    impl SessionPolicy for Reject {
        fn clipboard_export_spec(
            &self,
            _: SessionContext<'_>,
            purpose: ClipboardExportPurpose,
        ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
            match purpose {
                ClipboardExportPurpose::Copy => Ok(Some(spec())),
                ClipboardExportPurpose::Cut => Err(PolicyError::new("cut not supported")),
            }
        }
    }
    let f = fixture(false);
    let s = DocumentSession::new_with_policy(
        f.document.clone(),
        DocumentSelection::all(&f.document),
        Box::new(Reject),
    )
    .unwrap();
    assert!(s.clipboard_slice().unwrap().is_some());
    assert!(matches!(
        s.clipboard_slice_for(ClipboardExportPurpose::Cut),
        Err(SessionError::Policy(_))
    ));
}

#[test]
fn borrowed_budget_rejects_before_fragment_expansion() {
    let mut b = NodeStoreBuilder::new();
    let p = paragraph(&mut b, &"x".repeat(3 * 1024 * 1024));
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([p]),
        )
        .unwrap();
    let doc = XiaomuDocument::new(root, b.finish()).unwrap();
    let s = DocumentSession::new_with_policy(
        doc.clone(),
        DocumentSelection::all(&doc),
        Box::new(Policy(spec())),
    )
    .unwrap();
    assert!(matches!(s.clipboard_slice(), Err(SessionError::Policy(_))));
    assert_eq!(s.document().store(), doc.store());
    assert_eq!(s.history_depths(), (0, 0));
}
