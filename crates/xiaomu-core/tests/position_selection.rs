//! P0.3 position and selection validation against document snapshots.

use xiaomu_core::Error;
use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, NodeGap, NodeSelection, TextPoint, TextSelection};
use xiaomu_core::text::{TextBuffer, TextOffset};

/// Builds `Document > [paragraph("中文😀abc"), heading, rule]` and returns the
/// snapshot plus the child node ids.
fn fixture_document() -> (
    XiaomuDocument,
    [xiaomu_core::document::NodeId; 3],
    xiaomu_core::document::NodeId,
) {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("中文😀abc", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let heading = builder
        .insert(
            NodeKind::Heading(xiaomu_core::document::HeadingLevel::new(2).unwrap()),
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("标题", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let rule = builder
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph, heading, rule]),
        )
        .unwrap();
    let missing = builder.peek_next_id();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    (document, [paragraph, heading, rule], missing)
}

fn offset(raw: usize) -> TextOffset {
    // Offsets carry no buffer identity; validation happens against the target
    // node's text. A scratch buffer only provides a public constructor path.
    const SCRATCH: &str = "00000000000000000000000000000000";
    TextBuffer::from(SCRATCH)
        .offset_at(raw)
        .expect("scratch fixture covers test offsets")
}

#[test]
fn chinese_and_emoji_boundaries_validate_consistently() {
    let (document, [paragraph, _, _], _missing) = fixture_document();

    // "中文😀abc": 中=3, 文=6, 😀=10, a=11, b=12, c=13 bytes.
    for valid in [0, 3, 6, 10, 11, 12, 13] {
        assert_eq!(
            TextPoint::new(paragraph, offset(valid), CursorAffinity::Before).validate(&document),
            Ok(()),
            "offset {valid} should be a valid boundary"
        );
    }

    for invalid in [1, 2, 4, 5, 7, 8, 9] {
        assert_eq!(
            TextPoint::new(paragraph, offset(invalid), CursorAffinity::Before).validate(&document),
            Err(Error::InvalidTextBoundary { offset: invalid }),
            "offset {invalid} splits a code point"
        );
    }

    assert_eq!(
        TextPoint::new(paragraph, offset(14), CursorAffinity::Before).validate(&document),
        Err(Error::TextOutOfBounds {
            offset: 14,
            len: 13
        })
    );
}

#[test]
fn deleted_and_unknown_nodes_are_reported() {
    let (document, [paragraph, _, _], missing) = fixture_document();
    let stale = TextPoint::new(missing, TextOffset::ZERO, CursorAffinity::Before);

    assert_eq!(stale.validate(&document), Err(Error::UnknownNode));
    assert_eq!(
        NodeSelection::new(missing).validate(&document),
        Err(Error::UnknownNode)
    );
    assert_eq!(
        NodeGap::new(missing, 0).validate(&document),
        Err(Error::UnknownNode)
    );
    let _ = paragraph;
}

#[test]
fn non_inline_nodes_reject_text_points_but_accept_node_selections() {
    let (document, [_, _, rule], _) = fixture_document();

    assert_eq!(
        TextPoint::at_start_of(rule).validate(&document),
        Err(Error::InvalidSelection)
    );
    assert_eq!(NodeSelection::new(rule).validate(&document), Ok(()));
    assert_eq!(
        TextPoint::at_start_of(document.root()).validate(&document),
        Err(Error::InvalidSelection)
    );
}

#[test]
fn text_selections_require_one_inline_node() {
    let (document, [paragraph, heading, _], _) = fixture_document();

    let inside = TextSelection::new(
        TextPoint::new(paragraph, offset(0), CursorAffinity::Before),
        TextPoint::new(paragraph, offset(6), CursorAffinity::Before),
    );
    assert_eq!(inside.validate(&document), Ok(()));
    assert!(!inside.is_collapsed());
    let range = inside.ordered_range().unwrap();
    assert_eq!(range.start().as_usize(), 0);
    assert_eq!(range.end().as_usize(), 6);
    assert_eq!(range.len_bytes(), 6);

    // Reversed anchor/focus keeps user intent but orders the range.
    let reversed = TextSelection::new(inside.focus(), inside.anchor());
    assert_eq!(reversed.validate(&document), Ok(()));
    assert_eq!(reversed.ordered_range(), Ok(range));

    // Cross-node selections are rejected at the Core layer for P0.
    let cross = TextSelection::new(
        TextPoint::new(paragraph, offset(0), CursorAffinity::Before),
        TextPoint::new(heading, offset(3), CursorAffinity::Before),
    );
    assert_eq!(cross.validate(&document), Err(Error::InvalidSelection));
    assert_eq!(cross.ordered_range(), Err(Error::InvalidSelection));

    // One endpoint on an invalid boundary invalidates the whole selection.
    let bad_end = TextSelection::new(
        TextPoint::new(paragraph, offset(0), CursorAffinity::Before),
        TextPoint::new(paragraph, offset(4), CursorAffinity::Before),
    );
    assert_eq!(
        bad_end.validate(&document),
        Err(Error::InvalidTextBoundary { offset: 4 })
    );
}

#[test]
fn collapsed_selections_and_affinity_round_trip() {
    let (document, [paragraph, _, _], _) = fixture_document();

    let point = TextPoint::new(paragraph, offset(6), CursorAffinity::After);
    let collapsed = TextSelection::collapsed(point);
    assert_eq!(collapsed.validate(&document), Ok(()));
    assert!(collapsed.is_collapsed());
    assert!(collapsed.ordered_range().unwrap().is_empty());

    let moved = point.with_affinity(CursorAffinity::Before);
    assert_eq!(moved.affinity(), CursorAffinity::Before);
    assert_eq!(moved.node_id(), point.node_id());
    assert_eq!(moved.offset(), point.offset());
    assert_eq!(moved.validate(&document), Ok(()));
}

#[test]
fn node_gaps_cover_child_boundaries() {
    let (document, [paragraph, heading, _], _) = fixture_document();

    for index in 0..=3 {
        assert_eq!(
            NodeGap::new(document.root(), index).validate(&document),
            Ok(()),
            "boundary {index} of three children should be valid"
        );
    }
    assert_eq!(
        NodeGap::new(document.root(), 4).validate(&document),
        Err(Error::InvalidSelection)
    );

    // Paragraph has inline content, so it has no structural gaps.
    assert_eq!(
        NodeGap::new(paragraph, 0).validate(&document),
        Err(Error::InvalidSelection)
    );
    let _ = heading;
}
