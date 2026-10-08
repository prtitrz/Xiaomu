use std::cmp::Ordering;

use xiaomu_core::{
    document::{
        AtomKind, HeadingLevel, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark,
        MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
        XiaomuDocument,
    },
    selection::{CursorAffinity, InlinePoint, NodeGap},
    text::{TextBuffer, TextOffset},
};
use xiaomu_runtime::{
    reading::{
        AtomText, BoundarySide, ReadingBudget, ReadingError, ReadingEvent, ReadingProjection,
        ReadingProjectionLimits, ReadingProjectionOptions, ReadingSpanKind,
    },
    session::{DocumentPosition, DocumentSelection, DocumentSession, EditIntent},
};

fn text(builder: &mut NodeStoreBuilder, kind: NodeKind, value: &str) -> NodeId {
    let runs = if value.is_empty() {
        vec![]
    } else {
        vec![TextRun::new(value, MarkSet::empty()).unwrap()]
    };
    builder
        .insert(
            kind,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::new(runs).unwrap()),
        )
        .unwrap()
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

fn project(document: &XiaomuDocument) -> ReadingProjection {
    ReadingProjection::build(
        document,
        ReadingProjectionOptions {
            hard_break: AtomText::Character('\n'),
            other_atoms: AtomText::Character('\u{fffc}'),
        },
        ReadingProjectionLimits::default(),
    )
    .unwrap()
}

fn offset(raw: usize) -> TextOffset {
    TextBuffer::from_string("x".repeat(raw))
        .offset_at(raw)
        .unwrap()
}

fn point(node: NodeId, raw: usize, ordinal: usize) -> InlinePoint {
    InlinePoint::new(node, offset(raw), ordinal, CursorAffinity::Before)
}

fn mixed() -> (XiaomuDocument, NodeId) {
    let mut b = NodeStoreBuilder::new();
    let break_id = b
        .insert(
            NodeKind::InlineAtom(AtomKind::hard_break()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::hard_break()),
        )
        .unwrap();
    let extension = b
        .insert(
            NodeKind::InlineAtom(AtomKind::new("hardBreak").unwrap()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::new("ignored fallback").unwrap()),
        )
        .unwrap();
    let end_atom = b
        .insert(
            NodeKind::InlineAtom(AtomKind::new("ref").unwrap()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::new("ref").unwrap()),
        )
        .unwrap();
    let node = b
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [
                        TextRun::new("a中", MarkSet::empty()).unwrap(),
                        TextRun::new("😀e\u{301}", MarkSet::new([Mark::Bold]).unwrap()).unwrap(),
                    ],
                    [
                        InlineAtomPlacement::new(break_id, offset(4)),
                        InlineAtomPlacement::new(extension, offset(4)),
                        InlineAtomPlacement::new(end_atom, offset(11)),
                    ],
                )
                .unwrap(),
            ),
        )
        .unwrap();
    (finish(b, &[node]), node)
}

#[test]
fn preserves_typed_atoms_marks_seams_and_real_source_points() {
    let (doc, node) = mixed();
    let projection = project(&doc);
    let block = projection.block(node).unwrap();
    assert_eq!(block.text(), "a中\n\u{fffc}😀e\u{301}\u{fffc}");
    assert_eq!(block.text_fragments().collect::<String>(), "a中😀e\u{301}");
    assert_eq!(block.spans().len(), 5);
    let span = &block.spans()[1];
    assert!(
        matches!(span.kind(), ReadingSpanKind::InlineAtom { kind, .. } if kind.is_hard_break())
    );
    assert_eq!(
        (
            span.start().text_offset().as_usize(),
            span.start().atom_index()
        ),
        (4, 0)
    );
    assert_eq!(
        (span.end().text_offset().as_usize(), span.end().atom_index()),
        (4, 1)
    );
    assert!(
        matches!(block.spans()[2].kind(), ReadingSpanKind::InlineAtom { kind, .. } if !kind.is_hard_break())
    );
    for raw in 0..=block.text().len() {
        if !block.text().is_char_boundary(raw) {
            continue;
        }
        for side in [BoundarySide::BeforeAtoms, BoundarySide::AfterAtoms] {
            let source = block.point_at(raw, side).unwrap();
            source.validate(&doc).unwrap();
            assert_eq!(block.projected_offset(source), Ok(raw));
        }
    }
    assert!(block.point_at(2, BoundarySide::BeforeAtoms).is_err());
    assert!(
        block
            .point_at(block.text().len() + 1, BoundarySide::AfterAtoms)
            .is_err()
    );
    for invalid in [
        point(node, 2, 0),
        point(node, 4, 3),
        point(node, 8, 1),
        point(node, 99, 0),
    ] {
        assert_eq!(
            block.projected_offset(invalid),
            Err(ReadingError::InvalidPoint)
        );
    }
}

#[test]
fn omitted_atoms_keep_all_ordinals_and_prefix_is_text_only() {
    let (doc, node) = mixed();
    let projection = ReadingProjection::build(
        &doc,
        ReadingProjectionOptions::default(),
        ReadingProjectionLimits::default(),
    )
    .unwrap();
    let block = projection.block(node).unwrap();
    assert_eq!(block.text(), "a中😀e\u{301}");
    assert_eq!(
        block
            .point_at(4, BoundarySide::BeforeAtoms)
            .unwrap()
            .atom_index(),
        0
    );
    assert_eq!(
        block
            .point_at(4, BoundarySide::AfterAtoms)
            .unwrap()
            .atom_index(),
        2
    );
    for ordinal in 0..=2 {
        let p = point(node, 4, ordinal);
        assert_eq!(block.projected_offset(p), Ok(4));
        assert_eq!(
            block.text_fragments_before(p).unwrap().collect::<String>(),
            "a中"
        );
    }
    assert_eq!(
        projection.compare_points(point(node, 4, 0), point(node, 4, 2)),
        Ok(Ordering::Less)
    );
    assert_eq!(
        block
            .point_at(11, BoundarySide::AfterAtoms)
            .unwrap()
            .atom_index(),
        1
    );
}

#[test]
fn nested_blocks_and_empty_headings_remain_in_structural_order() {
    let mut b = NodeStoreBuilder::new();
    let first = text(
        &mut b,
        NodeKind::Heading(HeadingLevel::new(1).unwrap()),
        "  same ",
    );
    let empty = text(&mut b, NodeKind::Paragraph, "");
    let nested = text(
        &mut b,
        NodeKind::Heading(HeadingLevel::new(4).unwrap()),
        "same",
    );
    let quote = b
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([empty, nested]),
        )
        .unwrap();
    let cell_heading = text(&mut b, NodeKind::Heading(HeadingLevel::new(2).unwrap()), "");
    let cell = b
        .insert(
            NodeKind::TableCell,
            NodeAttrs::empty(),
            NodeContent::children([cell_heading]),
        )
        .unwrap();
    let row = b
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([cell]),
        )
        .unwrap();
    let table = b
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap();
    let rule = b
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let doc = finish(b, &[first, quote, table, rule]);
    let projection = project(&doc);
    assert_eq!(projection.root(), doc.root());
    assert_eq!(
        projection.events(),
        [
            ReadingEvent::EnterContainer(doc.root()),
            ReadingEvent::TextBlock(first),
            ReadingEvent::EnterContainer(quote),
            ReadingEvent::TextBlock(empty),
            ReadingEvent::TextBlock(nested),
            ReadingEvent::LeaveContainer(quote),
            ReadingEvent::EnterContainer(table),
            ReadingEvent::EnterContainer(row),
            ReadingEvent::EnterContainer(cell),
            ReadingEvent::TextBlock(cell_heading),
            ReadingEvent::LeaveContainer(cell),
            ReadingEvent::LeaveContainer(row),
            ReadingEvent::LeaveContainer(table),
            ReadingEvent::AtomicBlock(rule),
            ReadingEvent::LeaveContainer(doc.root()),
        ]
    );
    assert_eq!(
        projection
            .blocks()
            .iter()
            .map(|b| b.node_id())
            .collect::<Vec<_>>(),
        [first, empty, nested, cell_heading]
    );
    assert_eq!(projection.blocks()[0].text(), "  same ");
    assert!(matches!(
        projection.blocks()[3].kind(),
        NodeKind::Heading(_)
    ));
    let positions = [
        DocumentPosition::Gap(NodeGap::new(doc.root(), 0)),
        DocumentPosition::Inline(point(first, 0, 0)),
        DocumentPosition::Inline(point(first, 7, 0)),
        DocumentPosition::Gap(NodeGap::new(quote, 0)),
        DocumentPosition::Inline(point(empty, 0, 0)),
        DocumentPosition::Gap(NodeGap::new(quote, 1)),
        DocumentPosition::Inline(point(nested, 2, 0)),
        DocumentPosition::Gap(NodeGap::new(table, 0)),
        DocumentPosition::Inline(point(cell_heading, 0, 0)),
        DocumentPosition::Atomic(rule),
        DocumentPosition::Gap(NodeGap::new(doc.root(), 4)),
    ];
    for (i, left) in positions.iter().enumerate() {
        for (j, right) in positions.iter().enumerate() {
            assert_eq!(projection.compare_positions(*left, *right), Ok(i.cmp(&j)));
            let ordered = DocumentSelection::new(*left, *right).ordered(&doc).unwrap();
            assert_eq!(
                ordered,
                if i <= j {
                    (*left, *right)
                } else {
                    (*right, *left)
                }
            );
        }
    }
    let counts = [0, 1, 1, 1, 2, 2, 3, 3, 4, 4, 4];
    for (position, count) in positions.iter().zip(counts) {
        assert_eq!(
            projection.prefix(*position).unwrap().blocks().count(),
            count
        );
    }
    let prefix = projection
        .prefix(DocumentPosition::Inline(point(nested, 2, 0)))
        .unwrap();
    let text: Vec<String> = prefix
        .blocks()
        .map(|b| b.text_fragments().collect())
        .collect();
    assert_eq!(text, ["  same ", "", "sa"]);
    assert_eq!(
        projection
            .prefix(DocumentPosition::Gap(NodeGap::new(first, 0)))
            .unwrap_err(),
        ReadingError::InvalidPoint
    );
    assert_eq!(
        projection
            .prefix(DocumentPosition::Atomic(first))
            .unwrap_err(),
        ReadingError::InvalidPoint
    );
}

#[test]
fn all_atom_and_empty_documents_have_unambiguous_boundaries() {
    let mut b = NodeStoreBuilder::new();
    let empty = text(&mut b, NodeKind::Paragraph, "");
    let doc = finish(b, &[empty]);
    let projection = project(&doc);
    assert_eq!(
        projection.blocks()[0]
            .point_at(0, BoundarySide::BeforeAtoms)
            .unwrap(),
        point(empty, 0, 0)
    );
    let doc = finish(NodeStoreBuilder::new(), &[]);
    let projection = project(&doc);
    assert!(projection.blocks().is_empty());
    assert_eq!(
        projection
            .prefix(DocumentPosition::Gap(NodeGap::new(doc.root(), 0)))
            .unwrap()
            .blocks()
            .count(),
        0
    );
}

#[test]
fn projection_limits_fail_whole_build_without_mutation() {
    let (doc, _) = mixed();
    let baseline = doc.clone();
    let exact = ReadingProjectionLimits {
        max_nodes: 5,
        max_bytes: 41,
        max_spans: 5,
    };
    ReadingProjection::build(
        &doc,
        ReadingProjectionOptions {
            hard_break: AtomText::Character('\n'),
            other_atoms: AtomText::Character('\u{fffc}'),
        },
        exact,
    )
    .unwrap();
    assert_eq!(
        ReadingProjection::build(
            &doc,
            ReadingProjectionOptions {
                hard_break: AtomText::Character('\n'),
                other_atoms: AtomText::Character('\u{fffc}'),
            },
            ReadingProjectionLimits {
                max_bytes: 40,
                ..exact
            }
        )
        .unwrap_err(),
        ReadingError::BudgetExceeded(ReadingBudget::ProjectionBytes)
    );
    for (limits, resource) in [
        (
            ReadingProjectionLimits {
                max_nodes: 1,
                ..Default::default()
            },
            ReadingBudget::Nodes,
        ),
        (
            ReadingProjectionLimits {
                max_bytes: 1,
                ..Default::default()
            },
            ReadingBudget::ProjectionBytes,
        ),
        (
            ReadingProjectionLimits {
                max_spans: 1,
                ..Default::default()
            },
            ReadingBudget::Spans,
        ),
    ] {
        assert_eq!(
            ReadingProjection::build(&doc, ReadingProjectionOptions::default(), limits)
                .unwrap_err(),
            ReadingError::BudgetExceeded(resource)
        );
        assert_eq!(doc.revision(), baseline.revision());
        assert_eq!(doc.store(), baseline.store());
    }
}

#[test]
fn projection_is_revision_bound_and_reading_preserves_typing_group() {
    let mut b = NodeStoreBuilder::new();
    let id = text(&mut b, NodeKind::Paragraph, "");
    let doc = finish(b, &[id]);
    let mut session =
        DocumentSession::new(doc, DocumentSelection::collapsed(point(id, 0, 0))).unwrap();
    let identity = session.identity();
    session
        .apply_intent(&EditIntent::InsertText { text: "a".into() })
        .unwrap();
    let selection = session.selection();
    let depth = session.history_depths();
    let projection = project(session.document());
    projection.prefix(selection.anchor()).unwrap();
    assert_eq!(session.identity(), identity.clone());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), depth);
    session
        .apply_intent(&EditIntent::InsertText { text: "b".into() })
        .unwrap();
    assert_ne!(session.document().revision(), projection.revision());
    assert_eq!(projection.blocks()[0].text(), "a");
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    session.redo().unwrap();
    assert_eq!(session.identity(), identity);
    let next = DocumentSession::new(session.document().clone(), session.selection()).unwrap();
    assert_ne!(next.identity(), session.identity());
    assert_eq!(next.document().revision(), session.document().revision());
}

#[test]
fn opaque_custom_leaf_is_not_a_valid_atomic_position_or_search_text() {
    let mut builder = NodeStoreBuilder::new();
    let leaf = builder
        .insert(
            NodeKind::custom("opaque").unwrap(),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::new("fallback").unwrap()),
        )
        .unwrap();
    let doc = finish(builder, &[leaf]);
    let projection = project(&doc);
    assert!(projection.blocks().is_empty());
    assert!(
        projection
            .events()
            .contains(&ReadingEvent::OpaqueLeaf(leaf))
    );
    assert_eq!(
        projection
            .prefix(DocumentPosition::Atomic(leaf))
            .unwrap_err(),
        ReadingError::InvalidPoint
    );
}
