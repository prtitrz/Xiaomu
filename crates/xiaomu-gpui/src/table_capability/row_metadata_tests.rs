//! Exact, opt-in row metadata remains separate from measured admission.

use super::*;
use xiaomu_core::document::{NodeContent, NodeStoreBuilder};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

const MARKER: &str = "test:empty-row";

struct Fixture {
    document: XiaomuDocument,
    table: NodeId,
    row: NodeId,
    cell: NodeId,
    text: NodeId,
}

fn marker(value: AttrValue) -> NodeAttrs {
    NodeAttrs::new([(MARKER.to_owned(), value)].into()).unwrap()
}

fn fixture(row_attrs: NodeAttrs) -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let text = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let cell = builder
        .insert(
            NodeKind::TableHeader,
            NodeAttrs::empty(),
            NodeContent::children([text]),
        )
        .unwrap();
    let row = builder
        .insert(NodeKind::TableRow, row_attrs, NodeContent::children([cell]))
        .unwrap();
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([table]),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        table,
        row,
        cell,
        text,
    }
}

fn with_attrs(document: &XiaomuDocument, node: NodeId, attrs: NodeAttrs) -> XiaomuDocument {
    Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::SetNodeAttrs { node, attrs })
        .apply(document)
        .unwrap()
}

fn enabled() -> TableCapability {
    let mut cap = TableCapability::default();
    cap.set_enabled(true);
    cap
}

#[test]
fn default_row_metadata_rejects_marker_and_explicit_value_still_requires_measurement() {
    let row_attrs = marker(AttrValue::Bool(true));
    let f = fixture(row_attrs.clone());
    let mut cap = enabled();
    assert!(cap.key(&f.document, f.table).is_err());
    assert!(!cap.can_build(&f.document, f.table));
    assert!(!cap.permits(&f.document, f.table));
    assert_eq!(
        cap.hidden_ancestor_for_build(&f.document, f.text),
        Some(f.table)
    );

    // The same snapshot's cached rejection must be discarded by configuration.
    cap.set_row_metadata(row_attrs.clone());
    let key = cap.key(&f.document, f.table).unwrap();
    assert!(cap.can_build(&f.document, f.table));
    assert_eq!(cap.hidden_ancestor_for_build(&f.document, f.text), None);
    assert!(!cap.permits(&f.document, f.table));
    assert!(!cap.permits_document(&f.document));
    cap.record(f.table, key, true);
    assert!(cap.permits_document(&f.document));
    assert_eq!(cap.hidden_ancestor(&f.document, f.text), None);
    assert_eq!(f.document.node(f.row).unwrap().attrs(), &row_attrs);
}

#[test]
fn row_whitelist_requires_exact_values_and_rejects_every_unlisted_attribute() {
    let f = fixture(marker(AttrValue::Bool(true)));
    let mut cap = enabled();
    cap.set_row_metadata(marker(AttrValue::Bool(true)));
    for value in [
        AttrValue::Bool(false),
        AttrValue::Null,
        AttrValue::String("true".into()),
        AttrValue::Integer(1),
    ] {
        let raw = marker(value);
        let changed = with_attrs(&f.document, f.row, raw.clone());
        assert!(!cap.can_build(&changed, f.table));
        assert!(cap.key(&changed, f.table).is_err());
        assert_eq!(changed.node(f.row).unwrap().attrs(), &raw);
    }
    let raw = NodeAttrs::new(
        [
            (MARKER.to_owned(), AttrValue::Bool(true)),
            ("test:unknown-row-field".into(), AttrValue::Null),
        ]
        .into(),
    )
    .unwrap();
    let changed = with_attrs(&f.document, f.row, raw.clone());
    assert!(!cap.can_build(&changed, f.table));
    assert_eq!(changed.node(f.row).unwrap().attrs(), &raw);

    // A whitelist permits exact present values; it does not require defaults.
    let absent = with_attrs(&f.document, f.row, NodeAttrs::empty());
    assert!(cap.can_build(&absent, f.table));
    assert!(absent.node(f.row).unwrap().attrs().is_empty());
}

#[test]
fn row_metadata_permission_never_extends_to_table_or_cell_attributes() {
    let f = fixture(marker(AttrValue::Bool(true)));
    let mut cap = enabled();
    cap.set_row_metadata(marker(AttrValue::Bool(true)));
    assert!(cap.can_build(&f.document, f.table));
    for node in [f.table, f.cell] {
        let raw = marker(AttrValue::Bool(true));
        let changed = with_attrs(&f.document, node, raw.clone());
        assert!(cap.key(&changed, f.table).is_err());
        assert!(!cap.can_build(&changed, f.table));
        assert!(!cap.permits_document(&changed));
        assert_eq!(changed.node(node).unwrap().attrs(), &raw);
    }
}

#[test]
fn actual_row_metadata_changes_structural_key_without_changing_configuration() {
    let f = fixture(NodeAttrs::empty());
    let mut cap = enabled();
    cap.set_row_metadata(marker(AttrValue::Bool(true)));
    let plain = cap.key(&f.document, f.table).unwrap();
    cap.record(f.table, plain.clone(), true);
    assert!(cap.permits(&f.document, f.table));

    let marked = with_attrs(&f.document, f.row, marker(AttrValue::Bool(true)));
    let marked_key = cap.key(&marked, f.table).unwrap();
    assert_ne!(plain, marked_key);
    assert!(cap.can_build(&marked, f.table));
    assert!(!cap.permits(&marked, f.table));
    cap.record(f.table, plain, true);
    assert!(!cap.permits(&marked, f.table));
    cap.record(f.table, marked_key.clone(), true);
    assert!(cap.permits(&marked, f.table));

    let restored = with_attrs(&marked, f.row, NodeAttrs::empty());
    let restored_key = cap.key(&restored, f.table).unwrap();
    assert_ne!(marked_key, restored_key);
    assert!(cap.can_build(&restored, f.table));
    assert!(!cap.permits(&restored, f.table));
}

#[test]
fn configuration_change_and_restore_revoke_success_and_refuse_stale_epoch_keys() {
    let allowed = marker(AttrValue::Bool(true));
    let f = fixture(allowed.clone());
    let mut cap = enabled();
    cap.set_row_metadata(allowed.clone());
    let original_key = cap.key(&f.document, f.table).unwrap();
    cap.record(f.table, original_key.clone(), true);
    assert!(cap.permits(&f.document, f.table));

    // A wider whitelist still changes the epoch, even though the row's raw
    // attrs and the document snapshot remain byte-for-byte unchanged.
    let wider = NodeAttrs::new(
        [
            (MARKER.to_owned(), AttrValue::Bool(true)),
            ("test:other-row-marker".into(), AttrValue::Bool(true)),
        ]
        .into(),
    )
    .unwrap();
    cap.set_row_metadata(wider);
    assert!(!cap.permits_document(&f.document));
    let changed_key = cap.key(&f.document, f.table).unwrap();
    assert_ne!(original_key, changed_key);
    cap.record(f.table, original_key.clone(), true);
    assert!(!cap.permits(&f.document, f.table));
    cap.record(f.table, changed_key.clone(), true);
    assert!(cap.permits(&f.document, f.table));

    // Returning to the exact original whitelist must not resurrect a layout
    // callback issued before either configuration change (the ABA case).
    cap.set_row_metadata(allowed);
    let restored_key = cap.key(&f.document, f.table).unwrap();
    assert_ne!(original_key, restored_key);
    assert_ne!(changed_key, restored_key);
    assert!(!cap.permits(&f.document, f.table));
    cap.record(f.table, original_key, true);
    cap.record(f.table, changed_key, true);
    assert!(!cap.permits(&f.document, f.table));
    cap.record(f.table, restored_key, true);
    assert!(cap.permits_document(&f.document));
}

#[test]
fn row_metadata_configuration_and_measured_success_are_per_instance() {
    let f = fixture(marker(AttrValue::Bool(true)));
    let mut first = enabled();
    let mut second = enabled();
    first.set_row_metadata(marker(AttrValue::Bool(true)));
    let first_key = first.key(&f.document, f.table).unwrap();
    first.record(f.table, first_key, true);
    assert!(first.permits_document(&f.document));
    assert!(!second.can_build(&f.document, f.table));
    assert!(!second.permits_document(&f.document));

    second.set_row_metadata(marker(AttrValue::Bool(true)));
    let second_key = second.key(&f.document, f.table).unwrap();
    assert!(second.can_build(&f.document, f.table));
    assert!(!second.permits(&f.document, f.table));
    second.record(f.table, second_key, true);
    assert!(second.permits_document(&f.document));

    first.set_row_metadata(NodeAttrs::empty());
    assert!(!first.can_build(&f.document, f.table));
    assert!(!first.permits_document(&f.document));
    assert!(second.permits_document(&f.document));
}
