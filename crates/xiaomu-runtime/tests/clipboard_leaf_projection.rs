//! Public clipboard leaf descriptors remain independent of reading and editing.

use std::cell::Cell;

use xiaomu_core::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark, MarkSet,
    NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::clipboard::{
    ClipboardCellRangeRoot, ClipboardExportPurpose, ClipboardExportSpec, ClipboardMetadataDecode,
    ClipboardNodeContent, ClipboardSlice, ClipboardSourceBoundary, ClipboardTextProjection,
    decode_metadata_checked, encode_metadata,
};
use xiaomu_runtime::reading::{
    AtomText, ReadingProjection, ReadingProjectionLimits, ReadingProjectionOptions,
};
use xiaomu_runtime::session::{
    DocumentSelection, DocumentSession, EditIntent, EditPlan, IntentDisposition, PolicyError,
    SelectionUpdate, SessionContext, SessionError, SessionOutcome, SessionPolicy,
};

fn kind(key: &str) -> AtomKind {
    AtomKind::new(key).unwrap()
}

fn projection(keys: &[&str]) -> ClipboardTextProjection {
    ClipboardTextProjection::text_between_lf_with_atoms(keys.iter().map(|key| kind(key))).unwrap()
}

fn attrs(key: &str, value: AttrValue) -> NodeAttrs {
    NodeAttrs::new([(key.into(), value)].into()).unwrap()
}

fn paragraph(
    builder: &mut NodeStoreBuilder,
    runs: &[(&str, MarkSet)],
    atoms: &[(usize, AtomKind, &str)],
) -> NodeId {
    let runs = runs
        .iter()
        .map(|(text, marks)| TextRun::new(*text, marks.clone()).unwrap())
        .collect::<Vec<_>>();
    let text = InlineContent::new(runs.clone()).unwrap();
    let mut placements = Vec::new();
    for (offset, kind, fallback) in atoms {
        let attributes = if kind.is_hard_break() {
            NodeAttrs::empty()
        } else {
            attrs(
                "payload",
                AttrValue::List(vec![
                    AttrValue::Null,
                    AttrValue::String("https://example.invalid/opaque".into()),
                ]),
            )
        };
        let atom = builder
            .insert(
                NodeKind::InlineAtom(kind.clone()),
                attributes,
                NodeContent::InlineAtom(
                    InlineAtomContent::new(*fallback)
                        .unwrap()
                        .with_marks(MarkSet::new([Mark::Italic]).unwrap()),
                ),
            )
            .unwrap();
        placements.push(InlineAtomPlacement::new(
            atom,
            text.offset_at(*offset).unwrap(),
        ));
    }
    builder
        .insert(
            NodeKind::Paragraph,
            attrs("block", AttrValue::Null),
            NodeContent::Inline(InlineContent::with_atoms(runs, placements).unwrap()),
        )
        .unwrap()
}

fn plain(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    if text.is_empty() {
        paragraph(builder, &[], &[])
    } else {
        paragraph(builder, &[(text, MarkSet::empty())], &[])
    }
}

fn finish(mut builder: NodeStoreBuilder, children: &[NodeId]) -> XiaomuDocument {
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(children.iter().copied()),
        )
        .unwrap();
    XiaomuDocument::new(root, builder.finish()).unwrap()
}

struct ExportPolicy(ClipboardTextProjection);

impl SessionPolicy for ExportPolicy {
    fn clipboard_export_spec(
        &self,
        _: SessionContext<'_>,
        _: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        Ok(Some(
            ClipboardExportSpec::new()
                .with_closed_cell_ranges()
                .with_text_projection(self.0.clone()),
        ))
    }
}

fn session(
    document: &XiaomuDocument,
    selection: DocumentSelection,
    projection: ClipboardTextProjection,
) -> DocumentSession {
    DocumentSession::new_with_policy(
        document.clone(),
        selection,
        Box::new(ExportPolicy(projection)),
    )
    .unwrap()
}

fn roundtrip(slice: &ClipboardSlice) {
    let metadata = encode_metadata(slice).unwrap();
    assert!(metadata.starts_with("xiaomu.clipboard.v15\n"));
    assert_eq!(
        decode_metadata_checked(slice.plain_text(), &metadata),
        ClipboardMetadataDecode::Valid(slice.clone())
    );
}

#[test]
fn descriptor_canonicalizes_order_without_normalizing_exact_kind_identity() {
    let keys = [
        "leaf",
        "Leaf",
        " leaf ",
        "leaf-child",
        "é",
        "e\u{301}",
        "hardBreak",
    ];
    let value = projection(&keys);
    let mut expected = keys.into_iter().map(kind).collect::<Vec<_>>();
    expected.sort();
    let ClipboardTextProjection::TextBetweenLfWithAtomsV1(kinds) = &value else {
        panic!("expected bounded leaf descriptor")
    };
    assert_eq!(kinds.atom_kinds(), expected);
    assert!(kinds.atom_kinds().iter().all(|kind| !kind.is_hard_break()));
    assert_eq!(
        value,
        ClipboardTextProjection::text_between_lf_with_atoms(expected.into_iter().rev()).unwrap()
    );
    let cloned = kinds.clone();
    assert!(std::ptr::eq(kinds.atom_kinds(), cloned.atom_kinds()));
}

#[test]
fn descriptor_rejects_empty_duplicate_builtin_count_and_utf8_byte_overflow() {
    assert!(ClipboardTextProjection::text_between_lf_with_atoms([]).is_err());
    assert!(ClipboardTextProjection::text_between_lf_with_atoms([AtomKind::hard_break()]).is_err());
    assert!(
        ClipboardTextProjection::text_between_lf_with_atoms([kind("leaf"), kind("leaf")]).is_err()
    );
    assert!(
        ClipboardTextProjection::text_between_lf_with_atoms(
            (0..32).map(|index| kind(&format!("leaf-{index}")))
        )
        .is_ok()
    );
    assert!(
        ClipboardTextProjection::text_between_lf_with_atoms(
            (0..33).map(|index| kind(&format!("leaf-{index}")))
        )
        .is_err()
    );
    for accepted in ["x".repeat(128), format!("{}ab", "界".repeat(42))] {
        assert_eq!(accepted.len(), 128);
        assert!(ClipboardTextProjection::text_between_lf_with_atoms([kind(&accepted)]).is_ok());
        let rejected = format!("{accepted}x");
        assert!(ClipboardTextProjection::text_between_lf_with_atoms([kind(&rejected)]).is_err());
    }
}

#[test]
fn descriptor_stops_unbounded_input_after_the_thirty_third_entry() {
    let consumed = Cell::new(0);
    let kinds = std::iter::from_fn(|| {
        let index = consumed.get();
        consumed.set(index + 1);
        assert!(index < 33, "constructor traversed an unbounded iterator");
        Some(kind(&format!("leaf-{index}")))
    });
    assert!(ClipboardTextProjection::text_between_lf_with_atoms(kinds).is_err());
    assert_eq!(consumed.get(), 33);
}

#[test]
fn copy_changes_only_plaintext_and_keeps_reading_replacement_characters_and_rich_atoms() {
    let mut builder = NodeStoreBuilder::new();
    let node = paragraph(
        &mut builder,
        &[
            ("甲", MarkSet::new([Mark::Bold]).unwrap()),
            ("x\r\n\t😀", MarkSet::new([Mark::Strike]).unwrap()),
        ],
        &[
            (0, kind("leaf"), "<start>"),
            (3, AtomKind::hard_break(), "\n"),
            (3, kind("leaf"), "<middle>"),
            (11, kind("leaf"), "<end>"),
        ],
    );
    let document = finish(builder, &[node]);
    let all = DocumentSelection::all(&document);
    let options = ReadingProjectionOptions {
        hard_break: AtomText::Character('\n'),
        other_atoms: AtomText::Character('\u{fffc}'),
    };
    let reading =
        ReadingProjection::build(&document, options, ReadingProjectionLimits::default()).unwrap();
    assert_eq!(
        reading.block(node).unwrap().text(),
        "\u{fffc}甲\n\u{fffc}x\r\n\t😀\u{fffc}"
    );
    let default = DocumentSession::new(document.clone(), all).unwrap();
    assert_eq!(
        default.clipboard_slice().unwrap().unwrap().plain_text(),
        "<start>甲\n<middle>x\r\n\t😀<end>"
    );
    assert!(
        session(&document, all, ClipboardTextProjection::TextBetweenLfV1)
            .clipboard_slice()
            .is_err()
    );
    let selected = projection(&["leaf"]);
    let session = session(&document, all, selected.clone());
    let slice = session.clipboard_slice().unwrap().unwrap();
    assert_eq!(slice.plain_text(), "\n甲\n\nx\r\n\t😀\n");
    assert_eq!(slice.text_projection(), Some(selected));
    assert_eq!(
        slice.source_boundary(),
        Some(ClipboardSourceBoundary::WholeRoots)
    );
    let original = document.node(node).unwrap().content().as_inline().unwrap();
    let detached = slice.roots()[0].content().as_inline().unwrap();
    assert_eq!(detached.runs(), original.runs());
    assert_eq!(detached.atoms().len(), original.atoms().len());
    for (atom, placement) in detached.atoms().iter().zip(original.atoms()) {
        let source = document.node(placement.atom()).unwrap();
        assert_eq!(atom.anchor(), placement.text_offset());
        assert_eq!(source.kind(), &NodeKind::InlineAtom(atom.kind().clone()));
        assert_eq!(atom.attrs(), source.attrs());
        assert_eq!(Some(atom.content()), source.content().as_inline_atom());
    }
    roundtrip(&slice);
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.document().revision(), document.revision());
    assert_eq!(session.selection(), all);
    assert_eq!(session.history_depths(), (0, 0));
    let after = ReadingProjection::build(
        session.document(),
        options,
        ReadingProjectionLimits::default(),
    )
    .unwrap();
    assert_eq!(after.blocks(), reading.blocks());
}

#[test]
fn exact_allowlist_never_infers_builtin_semantics_or_accepts_nearby_keys() {
    for atom_kind in [
        kind("Leaf"),
        kind(" leaf "),
        kind("leaf-child"),
        kind("hardBreak"),
    ] {
        let mut builder = NodeStoreBuilder::new();
        let node = paragraph(&mut builder, &[], &[(0, atom_kind.clone(), "\n")]);
        let document = finish(builder, &[node]);
        let all = DocumentSelection::all(&document);
        assert!(
            session(&document, all, projection(&["leaf"]))
                .clipboard_slice()
                .is_err()
        );
        assert!(
            session(&document, all, ClipboardTextProjection::TextBetweenLfV1)
                .clipboard_slice()
                .is_err()
        );
        let slice = session(&document, all, projection(&[atom_kind.as_str()]))
            .clipboard_slice()
            .unwrap()
            .unwrap();
        assert_eq!(slice.plain_text(), "\n");
        roundtrip(&slice);
    }
    let mut builder = NodeStoreBuilder::new();
    let node = paragraph(&mut builder, &[], &[(0, AtomKind::hard_break(), "\n")]);
    let document = finish(builder, &[node]);
    let slice = session(
        &document,
        DocumentSelection::all(&document),
        projection(&["leaf"]),
    )
    .clipboard_slice()
    .unwrap()
    .unwrap();
    assert_eq!(slice.plain_text(), "\n");
    roundtrip(&slice);
}

fn table(builder: &mut NodeStoreBuilder, cells: &[Vec<NodeId>]) -> (NodeId, Vec<NodeId>) {
    let cells = cells
        .iter()
        .enumerate()
        .map(|(index, children)| {
            builder
                .insert(
                    if index == 0 {
                        NodeKind::TableHeader
                    } else {
                        NodeKind::TableCell
                    },
                    attrs("cell", AttrValue::Integer(index as i64)),
                    NodeContent::children(children.iter().copied()),
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    let row = builder
        .insert(
            NodeKind::TableRow,
            attrs("row", AttrValue::Bool(true)),
            NodeContent::children(cells.iter().copied()),
        )
        .unwrap();
    let table = builder
        .insert(
            NodeKind::Table,
            attrs("table", AttrValue::Null),
            NodeContent::children([row]),
        )
        .unwrap();
    (table, cells)
}

#[test]
fn cross_paragraph_and_nested_cell_exports_keep_provenance_and_leaf_separators() {
    let mut builder = NodeStoreBuilder::new();
    let intro = paragraph(
        &mut builder,
        &[("A", MarkSet::empty())],
        &[(1, kind("leaf"), "ignored")],
    );
    let empty = plain(&mut builder, "");
    let first = paragraph(
        &mut builder,
        &[("B", MarkSet::empty())],
        &[(0, kind("leaf"), "ignored")],
    );
    let cell_empty = plain(&mut builder, "");
    let nested = paragraph(
        &mut builder,
        &[("C", MarkSet::empty())],
        &[(1, kind("leaf"), "ignored")],
    );
    let (nested_table, _) = table(&mut builder, &[vec![nested]]);
    let (outer_table, cells) = table(&mut builder, &[vec![first], vec![cell_empty, nested_table]]);
    let rule = builder
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let document = finish(builder, &[intro, empty, outer_table, rule]);
    let mut session = session(
        &document,
        DocumentSelection::all(&document),
        projection(&["leaf"]),
    );
    let whole = session.clipboard_slice().unwrap().unwrap();
    assert_eq!(
        whole.plain_text(),
        ["A\n", "", "\nB", "", "C\n", "\n"].join("\n")
    );
    assert_eq!(whole.roots().len(), 4);
    roundtrip(&whole);
    session
        .set_cell_range_selection(cells[0], cells[1])
        .unwrap();
    let cell_selection = session.selection();
    let range = session.clipboard_slice().unwrap().unwrap();
    assert_eq!(range.plain_text(), ["\nB", "", "C\n"].join("\n"));
    assert_eq!(
        range.source_boundary(),
        Some(ClipboardSourceBoundary::CellRange {
            root_form: ClipboardCellRangeRoot::Table
        })
    );
    assert_eq!(range.source_boundary().unwrap().open_depths(), Some((1, 1)));
    assert!(!range.is_closed());
    assert!(!range.allows_default_fitting());
    assert_eq!(range.roots(), &whole.roots()[2..3]);
    let ClipboardNodeContent::Table { rows, row_attrs } = range.roots()[0].content() else {
        panic!("expected table carrier")
    };
    assert_eq!(rows[0][0].kind(), &NodeKind::TableHeader);
    assert_eq!(row_attrs, &[attrs("row", AttrValue::Bool(true))]);
    roundtrip(&range);
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), cell_selection);
    assert_eq!(session.history_depths(), (0, 0));
}

#[test]
fn clipped_cell_range_preserves_retained_atoms_and_clears_the_entering_cell_forest() {
    struct ClippedPolicy;
    impl SessionPolicy for ClippedPolicy {
        fn clipboard_export_spec(
            &self,
            _: SessionContext<'_>,
            _: ClipboardExportPurpose,
        ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
            Ok(Some(
                ClipboardExportSpec::new()
                    .with_clipped_cell_ranges(attrs("fill", AttrValue::Bool(true)))
                    .with_text_projection(projection(&["leaf"])),
            ))
        }
    }

    let mut builder = NodeStoreBuilder::new();
    let top_left = plain(&mut builder, "top-left");
    let top_right = plain(&mut builder, "top-right");
    let entering = paragraph(
        &mut builder,
        &[("cleared", MarkSet::new([Mark::Bold]).unwrap())],
        &[(0, kind("leaf"), "entering-label")],
    );
    let retained_left = paragraph(
        &mut builder,
        &[("L", MarkSet::new([Mark::Bold]).unwrap())],
        &[(1, kind("leaf"), "left-label")],
    );
    let retained_right = paragraph(
        &mut builder,
        &[("R", MarkSet::new([Mark::Strike]).unwrap())],
        &[(0, kind("leaf"), "right-label")],
    );
    let entering_attrs = NodeAttrs::new(
        [
            ("rowspan".into(), AttrValue::Integer(2)),
            ("opaque".into(), AttrValue::String("preserved".into())),
        ]
        .into(),
    )
    .unwrap();
    let mut cells = Vec::new();
    for (index, paragraph) in [top_left, entering, top_right, retained_left, retained_right]
        .into_iter()
        .enumerate()
    {
        cells.push(
            builder
                .insert(
                    if index == 1 {
                        NodeKind::TableHeader
                    } else {
                        NodeKind::TableCell
                    },
                    if index == 1 {
                        entering_attrs.clone()
                    } else {
                        attrs("origin", AttrValue::Integer(index as i64))
                    },
                    NodeContent::children([paragraph]),
                )
                .unwrap(),
        );
    }
    let mut rows = Vec::new();
    for (index, row) in [&cells[..3], &cells[3..]].into_iter().enumerate() {
        rows.push(
            builder
                .insert(
                    NodeKind::TableRow,
                    attrs("row", AttrValue::Integer(index as i64)),
                    NodeContent::children(row.iter().copied()),
                )
                .unwrap(),
        );
    }
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children(rows),
        )
        .unwrap();
    let document = finish(builder, &[table]);
    let mut session = DocumentSession::new_with_policy(
        document.clone(),
        DocumentSelection::all(&document),
        Box::new(ClippedPolicy),
    )
    .unwrap();
    session
        .set_cell_range_selection(cells[3], cells[4])
        .unwrap();
    let selection = session.selection();
    let slice = session.clipboard_slice().unwrap().unwrap();
    assert_eq!(slice.plain_text(), ["L\n", "", "\nR"].join("\n"));
    assert_eq!(
        slice.source_boundary(),
        Some(ClipboardSourceBoundary::CellRange {
            root_form: ClipboardCellRangeRoot::Rows,
        })
    );
    assert!(!slice.is_closed());
    assert!(!slice.allows_default_fitting());
    let ClipboardNodeContent::Table { rows, row_attrs } = slice.roots()[0].content() else {
        panic!("expected clipped table carrier")
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].len(), 3);
    assert_eq!(row_attrs, &[attrs("row", AttrValue::Integer(1))]);
    let cleared = &rows[0][1];
    assert_eq!(cleared.kind(), &NodeKind::TableHeader);
    assert_eq!(cleared.attrs().get("rowspan"), Some(&AttrValue::Integer(1)));
    assert_eq!(cleared.attrs().get("opaque"), entering_attrs.get("opaque"));
    let cleared_children = cleared.content().as_children().unwrap();
    assert_eq!(cleared_children.len(), 1);
    assert_eq!(cleared_children[0].kind(), &NodeKind::Paragraph);
    assert_eq!(
        cleared_children[0].attrs(),
        &attrs("fill", AttrValue::Bool(true))
    );
    let cleared_inline = cleared_children[0].content().as_inline().unwrap();
    assert!(cleared_inline.runs().is_empty());
    assert!(cleared_inline.atoms().is_empty());
    for (index, original) in [(0, retained_left), (2, retained_right)] {
        let source = document.node(original).unwrap();
        let source_inline = source.content().as_inline().unwrap();
        let copied = &rows[0][index].content().as_children().unwrap()[0];
        let copied_inline = copied.content().as_inline().unwrap();
        assert_eq!(copied.attrs(), source.attrs());
        assert_eq!(copied_inline.runs(), source_inline.runs());
        assert_eq!(copied_inline.atoms().len(), 1);
        let original_atom = document.node(source_inline.atoms()[0].atom()).unwrap();
        let copied_atom = &copied_inline.atoms()[0];
        assert_eq!(copied_atom.anchor(), source_inline.atoms()[0].text_offset());
        assert_eq!(copied_atom.attrs(), original_atom.attrs());
        assert_eq!(
            Some(copied_atom.content()),
            original_atom.content().as_inline_atom()
        );
    }
    roundtrip(&slice);
    assert!(
        session
            .clipboard_slice_for(ClipboardExportPurpose::Cut)
            .is_err()
    );
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.document().revision(), document.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), (0, 0));
}

fn cut_fixture() -> (XiaomuDocument, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let selected = paragraph(
        &mut builder,
        &[("A", MarkSet::empty())],
        &[(1, kind("leaf"), "opaque")],
    );
    let tail = plain(&mut builder, "tail");
    (finish(builder, &[selected, tail]), selected, tail)
}

#[test]
fn ordinary_cut_rejects_new_descriptor_without_changing_state_or_redo_history() {
    let (document, selected, tail) = cut_fixture();
    let mut session = session(
        &document,
        DocumentSelection::collapsed(InlinePoint::at_start_of(tail)),
        projection(&["leaf"]),
    );
    session
        .apply_intent(&EditIntent::InsertText {
            text: "history".into(),
        })
        .unwrap();
    session.undo().unwrap();
    let end = document
        .node(selected)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(1)
        .unwrap();
    for selection in [
        DocumentSelection::all(&document),
        DocumentSelection::node(&document, selected).unwrap(),
        DocumentSelection::new(
            InlinePoint::at_start_of(selected),
            InlinePoint::new(selected, end, 1, CursorAffinity::After),
        ),
    ] {
        session.set_document_selection(selection).unwrap();
        let before = session.document().clone();
        let history = session.history_depths();
        let marks = session.stored_marks().cloned();
        assert!(session.clipboard_slice().unwrap().is_some());
        assert!(session.prepare_cut().unwrap().is_none());
        assert!(matches!(
            session.clipboard_slice_for(ClipboardExportPurpose::Cut),
            Err(SessionError::Policy(_))
        ));
        assert_eq!(session.document().store(), before.store());
        assert_eq!(session.document().revision(), before.revision());
        assert_eq!(session.selection(), selection);
        assert_eq!(session.history_depths(), history);
        assert_eq!(session.stored_marks(), marks.as_ref());
    }
    assert_eq!(session.history_depths(), (0, 1));
    session.redo().unwrap();
    assert_eq!(session.history_depths(), (1, 0));
}

struct CutPolicy {
    selected: NodeId,
    tail: NodeId,
    reject_candidate: bool,
}

impl SessionPolicy for CutPolicy {
    fn clipboard_export_spec(
        &self,
        _: SessionContext<'_>,
        purpose: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        assert_eq!(purpose, ClipboardExportPurpose::Cut);
        Ok(Some(
            ClipboardExportSpec::new().with_text_projection(projection(&["leaf"])),
        ))
    }

    fn prepare_cut(&self, _: SessionContext<'_>) -> Result<Option<EditPlan>, PolicyError> {
        Ok(Some(EditPlan::new(
            Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::RemoveNode {
                node: self.selected,
            }),
            SelectionUpdate::Exact {
                selection: DocumentSelection::collapsed(InlinePoint::at_start_of(self.tail)),
            },
            None,
        )))
    }

    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        assert!(
            !matches!(intent, EditIntent::Delete),
            "prepared Cut must not dispatch generic Delete"
        );
        Ok(IntentDisposition::Continue)
    }

    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if self.reject_candidate && document.node(self.selected).is_none() {
            return Err(PolicyError::new("synthetic candidate rejection"));
        }
        Ok(())
    }
}

#[test]
fn explicit_prepared_node_cut_roundtrips_and_restores_exact_rich_subtree_on_undo() {
    let (document, selected, tail) = cut_fixture();
    let selection = DocumentSelection::node(&document, selected).unwrap();
    let mut session = DocumentSession::new_with_policy(
        document.clone(),
        selection,
        Box::new(CutPolicy {
            selected,
            tail,
            reject_candidate: false,
        }),
    )
    .unwrap();
    let prepared = session.prepare_cut().unwrap().unwrap();
    assert_eq!(prepared.clipboard_slice().plain_text(), "A\n");
    roundtrip(prepared.clipboard_slice());
    drop(prepared);
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), (0, 0));
    let prepared = session.prepare_cut().unwrap().unwrap();
    let slice = prepared.clipboard_slice().clone();
    roundtrip(&slice);
    assert_eq!(prepared.publish(), SessionOutcome::DocumentChanged);
    assert!(session.document().node(selected).is_none());
    assert_eq!(session.document().node(tail), document.node(tail));
    assert_eq!(
        session.selection(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(tail))
    );
    assert_eq!(session.history_depths(), (1, 0));
    let after = session.document().clone();
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), selection);
    session.redo().unwrap();
    assert_eq!(session.document().store(), after.store());
    roundtrip(&slice);
}

#[test]
fn leaf_descriptor_cannot_override_prepared_cut_candidate_admission() {
    let (document, selected, tail) = cut_fixture();
    let selection = DocumentSelection::node(&document, selected).unwrap();
    let mut session = DocumentSession::new_with_policy(
        document.clone(),
        selection,
        Box::new(CutPolicy {
            selected,
            tail,
            reject_candidate: true,
        }),
    )
    .unwrap();
    assert!(matches!(
        session.prepare_cut(),
        Err(SessionError::Policy(_))
    ));
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.document().revision(), document.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), (0, 0));
}
