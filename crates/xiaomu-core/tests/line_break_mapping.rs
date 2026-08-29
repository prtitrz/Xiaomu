//! ADR 0004 regression: canonical LF uses ordinary text coordinates.

use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::mapping::{MapBias, MappedPosition};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_core::text::{TextBuffer, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

fn document_with(kind: NodeKind, text: &str) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let block = builder
        .insert(
            kind,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([block]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), block)
}

fn point(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextPoint {
    let inline = document.node(node).unwrap().content().as_inline().unwrap();
    TextPoint::new(node, inline.offset_at(raw).unwrap(), CursorAffinity::Before)
}

fn collapsed_range(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextRange {
    let offset = document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(raw)
        .unwrap();
    TextRange::new(offset, offset).unwrap()
}

fn text(document: &XiaomuDocument, node: NodeId) -> String {
    document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

fn inserted_lf_transaction(document: &XiaomuDocument, node: NodeId) -> Transaction {
    Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::ReplaceText {
        node,
        range: collapsed_range(document, node, 1),
        replacement: "\n".to_owned(),
    })
}

#[test]
fn lf_insertion_maps_before_after_and_following_positions_as_one_byte() {
    let (document, paragraph) = document_with(NodeKind::Paragraph, "ab");
    let applied = inserted_lf_transaction(&document, paragraph)
        .apply_with_changes(&document)
        .unwrap();

    assert_eq!(text(applied.document(), paragraph), "a\nb");

    let at_seam = point(&document, paragraph, 1);
    let before = applied.changes().map_text_point(at_seam, MapBias::Start);
    let after = applied.changes().map_text_point(at_seam, MapBias::End);
    match before {
        MappedPosition::Mapped(point) => assert_eq!(point.offset().as_usize(), 1),
        MappedPosition::Deleted => panic!("insertion seam must remain mappable"),
    }
    match after {
        MappedPosition::Mapped(point) => assert_eq!(point.offset().as_usize(), 2),
        MappedPosition::Deleted => panic!("insertion seam must remain mappable"),
    }

    let following = applied
        .changes()
        .map_text_point(point(&document, paragraph, 2), MapBias::Start);
    match following {
        MappedPosition::Mapped(point) => assert_eq!(point.offset().as_usize(), 3),
        MappedPosition::Deleted => panic!("following text must shift by one LF byte"),
    }
}

#[test]
fn lf_replace_round_trips_exact_store_for_paragraph_and_code_block() {
    for kind in [NodeKind::Paragraph, NodeKind::CodeBlock] {
        let (document, node) = document_with(kind, "ab");
        let applied = inserted_lf_transaction(&document, node)
            .apply_with_changes(&document)
            .unwrap();
        assert_eq!(text(applied.document(), node), "a\nb");
        assert!(applied.document().validate().is_ok());

        let undone = applied.inverse().apply(applied.document()).unwrap();
        assert_eq!(undone.store(), document.store());
        assert_eq!(undone.root(), document.root());
        assert_eq!(text(&undone, node), "ab");
    }
}

#[test]
fn lf_is_a_valid_single_byte_text_boundary() {
    let buffer = TextBuffer::from("a\nb");
    assert!(buffer.offset_at(1).is_ok());
    assert!(buffer.offset_at(2).is_ok());
    assert_eq!(
        buffer.offset_at(2).unwrap().as_usize() - buffer.offset_at(1).unwrap().as_usize(),
        1
    );
}
