//! Snapshot-cache regressions; these exercise no window or native platform.

use super::*;
use xiaomu_core::document::{InlineContent, MarkSet, NodeContent, NodeStoreBuilder, TextRun};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{DocumentSession, EditIntent};

struct Fixture {
    document: XiaomuDocument,
    table: NodeId,
    outside: NodeId,
    rows: Vec<NodeId>,
    cells: Vec<NodeId>,
    blocks: Vec<NodeId>,
}

fn attrs(key: &str, value: AttrValue) -> NodeAttrs {
    NodeAttrs::new([(key.to_owned(), value)].into()).unwrap()
}

fn fixture(rows: usize, columns: usize, cell_attrs: NodeAttrs, text: &str) -> Fixture {
    let mut b = NodeStoreBuilder::new();
    let inline = if text.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
    };
    let outside = b
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let mut row_ids = Vec::new();
    let mut cells = Vec::new();
    let mut blocks = Vec::new();
    for _ in 0..rows {
        let mut row_cells = Vec::new();
        for _ in 0..columns {
            let block = b
                .insert(
                    NodeKind::Paragraph,
                    NodeAttrs::empty(),
                    NodeContent::Inline(inline.clone()),
                )
                .unwrap();
            let cell = b
                .insert(
                    NodeKind::TableCell,
                    cell_attrs.clone(),
                    NodeContent::children([block]),
                )
                .unwrap();
            blocks.push(block);
            cells.push(cell);
            row_cells.push(cell);
        }
        row_ids.push(
            b.insert(
                NodeKind::TableRow,
                NodeAttrs::empty(),
                NodeContent::children(row_cells),
            )
            .unwrap(),
        );
    }
    let table = b
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children(row_ids.iter().copied()),
        )
        .unwrap();
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([outside, table]),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, b.finish()).unwrap(),
        table,
        outside,
        rows: row_ids,
        cells,
        blocks,
    }
}

fn caret(node: NodeId) -> DocumentSelection {
    DocumentSelection::collapsed(DocumentPosition::Inline(InlinePoint::at_start_of(node)))
}

fn enabled() -> TableCapability {
    let mut cap = TableCapability::default();
    cap.set_enabled(true);
    cap
}

#[test]
fn large_table_repeated_ancestry_and_handlers_reuse_one_snapshot_key_and_selection() {
    let f = fixture(64, 8, NodeAttrs::empty(), "a");
    let mut cap = enabled();
    let measured = cap.key(&f.document, f.table).unwrap();
    cap.record(f.table, measured.clone(), true);
    let selection = caret(f.blocks[0]);
    assert!(cap.selection_is_valid(&f.document, selection));
    let counts = cap.cache_counts();
    assert_eq!(counts, (1, 1, 1));

    for index in 0..4096 {
        let cell_index = index % f.blocks.len();
        let block = f.blocks[cell_index];
        assert_eq!(cap.parent_of(&f.document, block), Some(f.cells[cell_index]));
        assert_eq!(cap.hidden_ancestor(&f.document, block), None);
        assert_eq!(cap.hidden_ancestor_for_build(&f.document, block), None);
        assert!(cap.can_build(&f.document, f.table));
        assert!(cap.permits(&f.document, f.table));
        assert!(!handler_is_hidden_by_table(
            &f.document,
            selection,
            block,
            false,
            &cap
        ));
        assert!(Rc::ptr_eq(
            &measured,
            &cap.key(&f.document, f.table).unwrap()
        ));
    }

    assert_eq!(cap.cache_counts(), counts);
    assert_eq!(cap.cache_sizes(), (1, 1, f.document.node_count() - 1));
}

#[test]
fn successive_typing_and_undo_rebuild_once_per_snapshot_and_reuse_measured_key() {
    let f = fixture(1, 1, attrs("colspan", AttrValue::Integer(2)), "a");
    let mut cap = enabled();
    let measured = cap.key(&f.document, f.table).unwrap();
    cap.record(f.table, measured.clone(), true);
    let mut session = DocumentSession::new(f.document.clone(), caret(f.blocks[0])).unwrap();

    for text in ["中", "文"] {
        let before = cap.cache_counts();
        session
            .apply_intent(&EditIntent::InsertText { text: text.into() })
            .unwrap();
        assert!(cap.permits(session.document(), f.table));
        assert!(Rc::ptr_eq(
            &measured,
            &cap.key(session.document(), f.table).unwrap()
        ));
        assert!(!handler_is_hidden_by_table(
            session.document(),
            session.selection(),
            f.blocks[0],
            false,
            &cap
        ));
        let after = cap.cache_counts();
        assert_eq!(after.0, before.0 + 1);
        assert_eq!(after.1, before.1 + 1);
        assert_eq!(after.2, before.2 + 1);
        for _ in 0..32 {
            assert!(cap.permits(session.document(), f.table));
            assert!(cap.selection_is_valid(session.document(), session.selection()));
        }
        assert_eq!(cap.cache_counts(), after);
    }

    let before = cap.cache_counts();
    let revision = session.document().revision();
    session.undo().unwrap();
    assert_ne!(session.document().revision(), revision);
    assert!(cap.permits(session.document(), f.table));
    assert!(Rc::ptr_eq(
        &measured,
        &cap.key(session.document(), f.table).unwrap()
    ));
    let after = cap.cache_counts();
    assert_eq!(after.0, before.0 + 1);
    assert_eq!(after.1, before.1 + 1);
}

#[test]
fn same_revision_and_root_with_different_store_revalidate_and_old_record_cannot_admit() {
    let unit = fixture(1, 1, NodeAttrs::empty(), "a");
    let spanning = fixture(1, 1, attrs("colspan", AttrValue::Integer(2)), "a");
    let huge = fixture(
        1,
        1,
        attrs(
            "colwidth",
            AttrValue::List(vec![AttrValue::Integer(1_000_001)]),
        ),
        "a",
    );
    assert_eq!(unit.document.root(), spanning.document.root());
    assert_eq!(unit.document.root(), huge.document.root());
    assert_eq!(unit.document.revision(), spanning.document.revision());
    assert_eq!(unit.document.revision(), huge.document.revision());
    assert_ne!(unit.document.store(), spanning.document.store());

    let mut cap = enabled();
    let unit_key = cap.key(&unit.document, unit.table).unwrap();
    cap.record(unit.table, unit_key.clone(), true);
    assert!(cap.permits(&unit.document, unit.table));
    let before = cap.cache_counts();
    let spanning_key = cap.key(&spanning.document, spanning.table).unwrap();
    assert!(!Rc::ptr_eq(&unit_key, &spanning_key));
    assert!(!cap.permits(&spanning.document, spanning.table));
    assert_eq!(cap.cache_counts().0, before.0 + 1);
    assert_eq!(cap.cache_counts().1, before.1 + 1);
    cap.record(spanning.table, unit_key.clone(), true);
    assert!(!cap.permits(&spanning.document, spanning.table));
    cap.record(spanning.table, spanning_key.clone(), true);
    assert!(cap.permits(&spanning.document, spanning.table));

    assert!(cap.key(&huge.document, huge.table).is_err());
    let rejected_counts = cap.cache_counts();
    for _ in 0..128 {
        assert!(!cap.can_build(&huge.document, huge.table));
        assert!(!cap.permits(&huge.document, huge.table));
        assert!(cap.key(&huge.document, huge.table).is_err());
    }
    assert_eq!(cap.cache_counts(), rejected_counts);
    cap.record(huge.table, spanning_key, true);
    assert!(!cap.permits(&huge.document, huge.table));

    // Restoring a saved snapshot must replace both cached errors and admission.
    assert!(cap.can_build(&unit.document, unit.table));
    assert!(!cap.permits(&unit.document, unit.table));
    cap.record(unit.table, unit_key, true);
    assert!(cap.permits(&unit.document, unit.table));
}

#[test]
fn revoke_is_immediate_and_deleted_tables_cannot_accumulate_cache_or_measurements() {
    let f = fixture(3, 4, NodeAttrs::empty(), "a");
    let mut cap = enabled();
    let key = cap.key(&f.document, f.table).unwrap();
    cap.record(f.table, key.clone(), true);
    assert!(cap.permits(&f.document, f.table));
    let counts = cap.cache_counts();
    cap.revoke(f.table);
    assert!(!cap.permits(&f.document, f.table));
    assert_eq!(cap.cache_counts(), counts);
    cap.record(f.table, key.clone(), true);
    assert!(cap.permits(&f.document, f.table));

    let removed = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::RemoveNode { node: f.table })
        .apply(&f.document)
        .unwrap();
    assert_eq!(cap.parent_of(&removed, f.outside), Some(removed.root()));
    assert_eq!(cap.cache_sizes(), (0, 0, 1));
    for _ in 0..128 {
        assert!(cap.key(&removed, f.table).is_err());
        assert!(!cap.permits(&removed, f.table));
        assert!(handler_is_hidden_by_table(
            &removed,
            caret(f.outside),
            f.blocks[0],
            false,
            &cap
        ));
    }
    assert_eq!(cap.cache_sizes(), (0, 0, 1));

    // An old layout callback must not repopulate a deleted table's success.
    cap.record(f.table, key, true);
    assert_eq!(cap.cache_sizes(), (0, 0, 1));
    assert!(cap.can_build(&f.document, f.table));
    assert!(!cap.permits(&f.document, f.table));
    assert_eq!(cap.cache_sizes(), (1, 0, f.document.node_count() - 1));
}

#[test]
fn selection_validation_memo_keeps_full_selection_identity_and_snapshot_checks() {
    let f = fixture(1, 1, NodeAttrs::empty(), "a");
    let empty = fixture(1, 1, NodeAttrs::empty(), "");
    let cap = enabled();
    let parked = caret(f.outside);
    assert!(cap.selection_is_valid(&f.document, parked));
    let initial = cap.cache_counts();
    for _ in 0..128 {
        assert!(cap.selection_is_valid(&f.document, parked));
    }
    assert_eq!(cap.cache_counts(), initial);

    // Same parked endpoints, but a TableRow is not a valid CellRange anchor.
    let invalid = DocumentSelection::cell_range(f.rows[0], f.cells[0], parked.focus());
    assert_eq!(parked.anchor(), invalid.anchor());
    assert_eq!(parked.focus(), invalid.focus());
    assert!(!cap.selection_is_valid(&f.document, invalid));
    let invalid_counts = cap.cache_counts();
    assert_eq!(invalid_counts.2, initial.2 + 1);
    assert!(!cap.selection_is_valid(&f.document, invalid));
    assert_eq!(cap.cache_counts(), invalid_counts);

    let inline = f
        .document
        .node(f.blocks[0])
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    let end = DocumentSelection::collapsed(DocumentPosition::Inline(InlinePoint::new(
        f.blocks[0],
        inline.offset_at(1).unwrap(),
        0,
        CursorAffinity::Before,
    )));
    assert!(cap.selection_is_valid(&f.document, end));
    let before = cap.cache_counts();
    assert_eq!(f.document.root(), empty.document.root());
    assert_eq!(f.document.revision(), empty.document.revision());
    assert!(!cap.selection_is_valid(&empty.document, end));
    let after = cap.cache_counts();
    assert_eq!(after.0, before.0 + 1);
    assert_eq!(after.2, before.2 + 1);
}
