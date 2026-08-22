//! P0.5 position mapping across applied transactions.

use xiaomu_core::document::{
    AttrValue, InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::mapping::{MapBias, MappedPosition, StepMap};
use xiaomu_core::selection::{CursorAffinity, NodeGap, NodeSelection, TextPoint, TextSelection};
use xiaomu_core::text::{TextBuffer, TextOffset, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

fn offset_at(raw: usize) -> TextOffset {
    const SCRATCH: &str = "00000000000000000000000000000000";
    TextBuffer::from(SCRATCH).offset_at(raw).unwrap()
}

fn range(start: usize, end: usize) -> TextRange {
    TextRange::new(offset_at(start), offset_at(end)).unwrap()
}

fn point(node: NodeId, raw: usize) -> TextPoint {
    TextPoint::new(node, offset_at(raw), CursorAffinity::Before)
}

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}

fn document_with_children(texts: &[&str]) -> (XiaomuDocument, Vec<NodeId>) {
    let mut builder = NodeStoreBuilder::new();
    let paragraphs: Vec<NodeId> = texts
        .iter()
        .map(|text| paragraph(&mut builder, text))
        .collect();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(paragraphs.iter().copied()),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        paragraphs,
    )
}

fn assert_mapped(
    expected: Option<usize>,
    actual: MappedPosition<TextPoint>,
    document: &XiaomuDocument,
) {
    match (expected, actual) {
        (Some(raw), MappedPosition::Mapped(mapped)) => {
            assert_eq!(mapped.offset().as_usize(), raw);
            assert!(mapped.validate(document).is_ok());
        }
        (None, MappedPosition::Deleted) => {}
        (expected, actual) => panic!("unexpected mapping: {expected:?} vs {actual:?}"),
    }
}

/// `Document > [p("你好世界"), p("second")]`
fn fixture() -> (XiaomuDocument, [NodeId; 2]) {
    let (document, nodes) = document_with_children(&["你好世界", "second"]);
    (document, [nodes[0], nodes[1]])
}

fn replace_text(node: NodeId, start: usize, end: usize, replacement: &str) -> Transaction {
    Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::ReplaceText {
        node,
        range: range(start, end),
        replacement: replacement.to_owned(),
    })
}

#[test]
fn replacement_mapping_table_covers_all_regions() {
    let (document, [first, second]) = fixture();

    // Replace "世" [6,9) with "XY": length delta is -1. Valid boundaries of
    // the original text are 0, 3, 6, 9, 12.
    let applied = replace_text(first, 6, 9, "XY")
        .apply_with_changes(&document)
        .unwrap();
    let changes = applied.changes();
    assert_eq!(
        changes.steps(),
        &[StepMap::TextReplaced {
            node: first,
            range: range(6, 9),
            replacement_len: 2,
        }]
    );

    // Offsets before the replacement never move.
    for raw in [0, 3] {
        assert_mapped(
            Some(raw),
            changes.map_text_point(point(first, raw), MapBias::Start),
            applied.document(),
        );
    }

    // Offsets inside [6,9) resolve to the replacement boundaries.
    for raw in [6, 7, 8] {
        assert_mapped(
            Some(6),
            changes.map_text_point(point(first, raw), MapBias::Start),
            applied.document(),
        );
        assert_mapped(
            Some(8),
            changes.map_text_point(point(first, raw), MapBias::End),
            applied.document(),
        );
    }

    // Offsets at or after the end shift by the delta.
    for (raw, mapped) in [(9, 8), (12, 11)] {
        assert_mapped(
            Some(mapped),
            changes.map_text_point(point(first, raw), MapBias::Start),
            applied.document(),
        );
        assert_mapped(
            Some(mapped),
            changes.map_text_point(point(first, raw), MapBias::End),
            applied.document(),
        );
    }

    // Positions on untouched nodes map to themselves.
    assert_mapped(
        Some(3),
        changes.map_text_point(point(second, 3), MapBias::Start),
        applied.document(),
    );
}

#[test]
fn cjk_and_emoji_replacement_mapping() {
    // "a👍b中c": a=0..1, 👍=1..5, b=5..6, 中=6..9, c=9..10.
    let (document, nodes) = document_with_children(&["a👍b中c"]);
    let node = nodes[0];

    // Replace the emoji with "🎉!" (5 bytes).
    let applied = replace_text(node, 1, 5, "🎉!")
        .apply_with_changes(&document)
        .unwrap();
    let changes = applied.changes();

    let table = [
        (0usize, Some(0usize), Some(0usize)),
        (1, Some(1), Some(6)),
        (2, Some(1), Some(6)),
        (4, Some(1), Some(6)),
        (5, Some(6), Some(6)),
        (6, Some(7), Some(7)),
        (10, Some(11), Some(11)),
    ];
    for (raw, start_bias, end_bias) in table {
        assert_mapped(
            start_bias,
            changes.map_text_point(point(node, raw), MapBias::Start),
            applied.document(),
        );
        assert_mapped(
            end_bias,
            changes.map_text_point(point(node, raw), MapBias::End),
            applied.document(),
        );
    }

    // Deleting "世" [6,9) from "你好世界" collapses the region.
    let (document, nodes) = document_with_children(&["你好世界"]);
    let cjk = nodes[0];
    let applied = replace_text(cjk, 6, 9, "")
        .apply_with_changes(&document)
        .unwrap();
    let changes = applied.changes();

    for raw in [6, 7, 8] {
        assert_eq!(
            changes.map_text_point(point(cjk, raw), MapBias::Start),
            MappedPosition::Mapped(point(cjk, 6))
        );
        assert_eq!(
            changes.map_text_point(point(cjk, raw), MapBias::End),
            MappedPosition::Mapped(point(cjk, 6))
        );
    }
    assert_eq!(
        changes.map_text_point(point(cjk, 9), MapBias::Start),
        MappedPosition::Mapped(point(cjk, 6))
    );
    assert_eq!(
        changes.map_text_point(point(cjk, 12), MapBias::End),
        MappedPosition::Mapped(point(cjk, 9))
    );
}

#[test]
fn insertion_maps_gaps_and_reports_inserted_identity() {
    let (document, [first, second]) = fixture();

    let transaction =
        Transaction::new(TransactionOrigin::System).with_step(TransactionStep::InsertNode {
            parent: document.root(),
            index: 1,
            kind: NodeKind::Paragraph,
            attrs: NodeAttrs::empty(),
            content: NodeContent::empty_inline(),
        });
    let applied = transaction.apply_with_changes(&document).unwrap();
    let changes = applied.changes();

    // The allocated identity is visible and matches the new child list.
    let [
        StepMap::NodeInserted {
            parent,
            index,
            inserted,
        },
    ] = changes.steps()
    else {
        panic!("expected one NodeInserted step map");
    };
    assert_eq!((*parent, *index), (document.root(), 1));
    let children = applied
        .document()
        .node(document.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap();
    assert_eq!(children[1], *inserted);
    assert_eq!(children[0], first);
    assert_eq!(children[2], second);

    let gap = |index: usize| NodeGap::new(document.root(), index);
    // Gap 0 sits before the insertion point.
    assert_eq!(
        changes.map_node_gap(gap(0), MapBias::End),
        MappedPosition::Mapped(gap(0))
    );
    // Gap 1 is exactly the insertion boundary and resolves by bias.
    assert_eq!(
        changes.map_node_gap(gap(1), MapBias::Start),
        MappedPosition::Mapped(gap(1))
    );
    assert_eq!(
        changes.map_node_gap(gap(1), MapBias::End),
        MappedPosition::Mapped(gap(2))
    );
    // Gap 2 sits after the insertion point.
    assert_eq!(
        changes.map_node_gap(gap(2), MapBias::Start),
        MappedPosition::Mapped(gap(3))
    );

    // Text points are unaffected by structural insertion.
    assert_eq!(
        changes.map_text_point(point(first, 3), MapBias::Start),
        MappedPosition::Mapped(point(first, 3))
    );
    // Mapped gaps stay valid in the new snapshot.
    let MappedPosition::Mapped(mapped) = changes.map_node_gap(gap(1), MapBias::End) else {
        panic!("gap should survive");
    };
    assert!(mapped.validate(applied.document()).is_ok());
}

#[test]
fn removal_maps_gaps_and_deletes_positions() {
    let (document, nodes) = document_with_children(&["one", "two", "three"]);
    let [first, second, third] = [nodes[0], nodes[1], nodes[2]];

    let transaction = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveNode { node: second });
    let applied = transaction.apply_with_changes(&document).unwrap();
    let changes = applied.changes();

    // Gap arithmetic: only boundaries after the removed child shift.
    let gap = |index: usize| NodeGap::new(document.root(), index);
    assert_eq!(
        changes.map_node_gap(gap(0), MapBias::Start),
        MappedPosition::Mapped(gap(0))
    );
    assert_eq!(
        changes.map_node_gap(gap(1), MapBias::Start),
        MappedPosition::Mapped(gap(1))
    );
    assert_eq!(
        changes.map_node_gap(gap(2), MapBias::Start),
        MappedPosition::Mapped(gap(1))
    );
    assert_eq!(
        changes.map_node_gap(gap(3), MapBias::End),
        MappedPosition::Mapped(gap(2))
    );

    // Positions on the removed node report explicit deletion.
    assert_eq!(
        changes.map_text_point(point(second, 0), MapBias::Start),
        MappedPosition::Deleted
    );
    assert_eq!(
        changes.map_node_selection(NodeSelection::new(second)),
        MappedPosition::Deleted
    );
    assert_eq!(
        changes.map_text_selection(TextSelection::new(point(second, 0), point(second, 3))),
        MappedPosition::Deleted
    );

    // Surviving nodes keep their positions.
    assert_eq!(
        changes.map_text_point(point(first, 1), MapBias::Start),
        MappedPosition::Mapped(point(first, 1))
    );
    assert_eq!(
        changes.map_text_point(point(third, 1), MapBias::End),
        MappedPosition::Mapped(point(third, 1))
    );
}

#[test]
fn removal_of_a_subtree_deletes_positions_inside_it() {
    let mut builder = NodeStoreBuilder::new();
    let child = paragraph(&mut builder, "child");
    let list_item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([child]),
        )
        .unwrap();
    let list = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([list_item]),
        )
        .unwrap();
    let survivor = paragraph(&mut builder, "keep");
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([list, survivor]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();

    let applied = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveNode { node: list })
        .apply_with_changes(&document)
        .unwrap();
    let changes = applied.changes();

    assert_eq!(
        changes.map_text_point(point(child, 2), MapBias::Start),
        MappedPosition::Deleted
    );
    // A structural boundary inside the removed subtree is deleted too.
    assert_eq!(
        changes.map_node_gap(NodeGap::new(list_item, 0), MapBias::Start),
        MappedPosition::Deleted
    );
    // The boundary between the removed list and the survivor survives as the
    // boundary before the survivor.
    assert_eq!(
        changes.map_node_gap(NodeGap::new(root, 1), MapBias::Start),
        MappedPosition::Mapped(NodeGap::new(root, 0))
    );
    assert!(applied.document().node(survivor).is_some());
}

#[test]
fn mapping_composes_across_steps_in_one_transaction() {
    let (document, [first, _]) = fixture();

    // Step 1 deletes "你" [0,3); step 2 replaces "世" [3,6) of the
    // intermediate text "好世界" with "X". Final text: "好X界".
    let transaction = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::ReplaceText {
            node: first,
            range: range(0, 3),
            replacement: String::new(),
        })
        .with_step(TransactionStep::ReplaceText {
            node: first,
            range: range(3, 6),
            replacement: "X".to_owned(),
        });
    let applied = transaction.apply_with_changes(&document).unwrap();
    let changes = applied.changes();
    assert_eq!(changes.steps().len(), 2);

    // Original end offset 12 flows through both replacements to the new end.
    assert_mapped(
        Some(7),
        changes.map_text_point(point(first, 12), MapBias::Start),
        applied.document(),
    );
    // Original offset 9 ("界") ends up between "X" and "界".
    assert_mapped(
        Some(4),
        changes.map_text_point(point(first, 9), MapBias::Start),
        applied.document(),
    );
    // Original offset 6 ("世", replaced by step 2) resolves by bias to the
    // boundaries around "X".
    assert_mapped(
        Some(3),
        changes.map_text_point(point(first, 6), MapBias::Start),
        applied.document(),
    );
    assert_mapped(
        Some(4),
        changes.map_text_point(point(first, 6), MapBias::End),
        applied.document(),
    );
    // The offset at the start of the step-1 deleted region stays at zero.
    assert_eq!(
        changes.map_text_point(point(first, 0), MapBias::End),
        MappedPosition::Mapped(point(first, 0))
    );

    // Structural composition: insert before A, then remove B.
    let (document, nodes) = document_with_children(&["A", "B"]);
    let [a, b] = [nodes[0], nodes[1]];
    let root = document.root();
    let transaction = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::InsertNode {
            parent: root,
            index: 0,
            kind: NodeKind::Paragraph,
            attrs: NodeAttrs::empty(),
            content: NodeContent::empty_inline(),
        })
        .with_step(TransactionStep::RemoveNode { node: b });
    let applied = transaction.apply_with_changes(&document).unwrap();
    let changes = applied.changes();

    let gap = |index: usize| NodeGap::new(root, index);
    // Old gap 1 (between A and B): insertion pushes it to 2, removal of B
    // leaves it at the end of [new, A].
    assert_eq!(
        changes.map_node_gap(gap(1), MapBias::Start),
        MappedPosition::Mapped(gap(2))
    );
    // Old gap 0 (before A) with End bias moves past the inserted node.
    assert_eq!(
        changes.map_node_gap(gap(0), MapBias::End),
        MappedPosition::Mapped(gap(1))
    );
    assert!(applied.document().node(a).is_some());
    assert!(applied.document().node(b).is_none());
}

#[test]
fn text_selection_mapping_is_outward_and_collapsed_stays_collapsed() {
    let (document, [first, _]) = fixture();

    // Replace "好" [3,6) with "XY".
    let applied = replace_text(first, 3, 6, "XY")
        .apply_with_changes(&document)
        .unwrap();
    let changes = applied.changes();

    // A selection exactly over the replaced range still covers the
    // replacement.
    let selection = TextSelection::new(point(first, 3), point(first, 6));
    let MappedPosition::Mapped(mapped) = changes.map_text_selection(selection) else {
        panic!("selection should survive");
    };
    assert_eq!(mapped.ordered_range().unwrap(), range(3, 5));

    // Reversed anchor/focus keeps its direction and still covers.
    let reversed = TextSelection::new(point(first, 6), point(first, 3));
    let MappedPosition::Mapped(mapped) = changes.map_text_selection(reversed) else {
        panic!("selection should survive");
    };
    assert_eq!(mapped.anchor().offset().as_usize(), 5);
    assert_eq!(mapped.focus().offset().as_usize(), 3);

    // A collapsed selection stays collapsed at the replacement start.
    let collapsed = TextSelection::collapsed(point(first, 4));
    let MappedPosition::Mapped(mapped) = changes.map_text_selection(collapsed) else {
        panic!("selection should survive");
    };
    assert!(mapped.is_collapsed());
    assert_eq!(mapped.anchor().offset().as_usize(), 3);
}

#[test]
fn attrs_and_mark_steps_produce_no_mapping_entries() {
    let (document, [first, _]) = fixture();

    let mut values = std::collections::BTreeMap::new();
    values.insert("lang".to_owned(), AttrValue::String("zh".to_owned()));
    let transaction = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::SetNodeAttrs {
            node: first,
            attrs: NodeAttrs::new(values).unwrap(),
        })
        .with_step(TransactionStep::AddMark {
            node: first,
            range: range(0, 3),
            mark: Mark::Bold,
        });
    let applied = transaction.apply_with_changes(&document).unwrap();

    assert!(applied.changes().steps().is_empty());
    assert_eq!(
        applied
            .changes()
            .map_text_point(point(first, 9), MapBias::Start),
        MappedPosition::Mapped(point(first, 9))
    );
    assert_eq!(
        applied
            .changes()
            .map_node_gap(NodeGap::new(document.root(), 1), MapBias::End),
        MappedPosition::Mapped(NodeGap::new(document.root(), 1))
    );
}

#[test]
fn empty_transaction_yields_identity_mapping() {
    let (document, [first, _]) = fixture();
    let applied = Transaction::new(TransactionOrigin::System)
        .apply_with_changes(&document)
        .unwrap();

    assert!(applied.changes().steps().is_empty());
    assert_eq!(
        applied
            .changes()
            .map_text_point(point(first, 12), MapBias::End),
        MappedPosition::Mapped(point(first, 12))
    );
    assert_eq!(
        applied
            .changes()
            .map_node_selection(NodeSelection::new(first)),
        MappedPosition::Mapped(NodeSelection::new(first))
    );
    assert_eq!(
        applied.document().revision().as_u64(),
        document.revision().as_u64() + 1
    );
}
