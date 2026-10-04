//! Canonical split/join consumes exact atom seams and keeps identities reversible.

use xiaomu_core::Error;
use xiaomu_core::document::{
    AtomKind, AttrValue, HeadingLevel, InlineAtomContent, InlineContent, LinkMark, Mark, MarkSet,
    NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::mapping::{MapBias, MappedPosition, StepMap};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, NodeGap, NodeSelection, TextPoint};
use xiaomu_core::text::{TextBuffer, TextOffset};
use xiaomu_core::transaction::{
    AppliedTransaction, Transaction, TransactionOrigin, TransactionStep,
};

fn offset(raw: usize) -> TextOffset {
    TextBuffer::from(" ".repeat(raw)).offset_at(raw).unwrap()
}

fn point(node: NodeId, byte: usize, ordinal: usize, affinity: CursorAffinity) -> InlinePoint {
    InlinePoint::new(node, offset(byte), ordinal, affinity)
}

fn before(node: NodeId, byte: usize, ordinal: usize) -> InlinePoint {
    point(node, byte, ordinal, CursorAffinity::Before)
}

fn inline(doc: &XiaomuDocument, node: NodeId) -> &InlineContent {
    doc.node(node).unwrap().content().as_inline().unwrap()
}

fn text(doc: &XiaomuDocument, node: NodeId) -> String {
    inline(doc, node)
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

fn attrs(label: &str) -> NodeAttrs {
    NodeAttrs::new(
        [
            ("label".into(), AttrValue::String(label.into())),
            ("unset".into(), AttrValue::Null),
        ]
        .into(),
    )
    .unwrap()
}

fn marks() -> MarkSet {
    MarkSet::new([
        Mark::Bold,
        Mark::Link(LinkMark::new("https://example.test", Some("title".into()))),
    ])
    .unwrap()
}

fn apply(doc: &XiaomuDocument, step: TransactionStep) -> AppliedTransaction {
    Transaction::new(TransactionOrigin::UserInput)
        .with_step(step)
        .apply_with_changes(doc)
        .unwrap()
}

fn fixture(parts: &[&str]) -> (XiaomuDocument, Vec<NodeId>) {
    let mut builder = NodeStoreBuilder::new();
    let nodes: Vec<_> = parts
        .iter()
        .enumerate()
        .map(|(i, value)| {
            // Distinct marks on Unicode scalars test cuts inside normalized runs
            // and at their boundaries, including literal LF and combining marks.
            let runs = value.chars().enumerate().map(|(j, c)| {
                TextRun::new(
                    c.to_string(),
                    if j % 2 == 0 {
                        marks()
                    } else {
                        MarkSet::empty()
                    },
                )
                .unwrap()
            });
            builder
                .insert(
                    if i == 0 {
                        NodeKind::Heading(HeadingLevel::new(2).unwrap())
                    } else {
                        NodeKind::Paragraph
                    },
                    attrs(&format!("block-{i}")),
                    NodeContent::Inline(InlineContent::new(runs).unwrap()),
                )
                .unwrap()
        })
        .collect();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(nodes.clone()),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), nodes)
}

fn insert(doc: XiaomuDocument, node: NodeId, byte: usize, extension: bool) -> XiaomuDocument {
    let ordinal = inline(&doc, node).atom_count_at(offset(byte));
    let (kind, payload, attrs) = if extension {
        // The extension uses the built-in's label and LF fallback, proving
        // migration depends on identity/order rather than text heuristics.
        (
            AtomKind::new("hardBreak").unwrap(),
            InlineAtomContent::new("\n").unwrap().with_marks(marks()),
            attrs("extension"),
        )
    } else {
        (
            AtomKind::hard_break(),
            InlineAtomContent::hard_break().with_marks(marks()),
            NodeAttrs::empty(),
        )
    };
    apply(
        &doc,
        TransactionStep::InsertInlineAtom {
            at: before(node, byte, ordinal),
            kind,
            attrs,
            content: payload,
        },
    )
    .into_document()
}

fn all_points(doc: &XiaomuDocument, node: NodeId) -> Vec<InlinePoint> {
    let text = text(doc, node);
    let mut points = Vec::new();
    for byte in text.char_indices().map(|(i, _)| i).chain([text.len()]) {
        for ordinal in 0..=inline(doc, node).atom_count_at(offset(byte)) {
            for affinity in [CursorAffinity::Before, CursorAffinity::After] {
                points.push(point(node, byte, ordinal, affinity));
            }
        }
    }
    points
}

fn assert_roundtrip(original: &XiaomuDocument, applied: &AppliedTransaction) {
    let undone = applied
        .inverse()
        .apply_with_changes(applied.document())
        .unwrap();
    assert_eq!(undone.document().store(), original.store());
    assert_eq!(undone.document().root(), original.root());
    let redone = undone
        .inverse()
        .apply_with_changes(undone.document())
        .unwrap();
    assert_eq!(redone.document().store(), applied.document().store());
    assert_eq!(redone.document().root(), applied.document().root());
    let undone_again = redone.inverse().apply(redone.document()).unwrap();
    assert_eq!(undone_again.store(), original.store());
}

#[test]
fn split_every_unicode_boundary_and_atom_seam_preserves_store_and_maps_both_biases() {
    for value in ["", "中\n🙂e\u{301}"] {
        let (mut doc, nodes) = fixture(&[value]);
        let node = nodes[0];
        // Multiple typed breaks surround a colliding extension at each seam.
        let boundaries: Vec<_> = value
            .char_indices()
            .map(|(i, _)| i)
            .chain([value.len()])
            .collect();
        for byte in &boundaries {
            for extension in [false, true, false] {
                doc = insert(doc, node, *byte, extension);
            }
        }
        let original_points = all_points(&doc, node);
        for at in &original_points {
            let applied = apply(&doc, TransactionStep::SplitInlineNode { at: *at });
            let next = applied.document();
            let tail = match applied.changes().steps() {
                [
                    StepMap::InlineNodeSplit {
                        at: actual,
                        inserted,
                        ..
                    },
                ] => {
                    assert_eq!(actual, at);
                    *inserted
                }
                other => panic!("unexpected maps: {other:?}"),
            };
            assert_eq!(text(next, node), value[..at.text_offset().as_usize()]);
            assert_eq!(text(next, tail), value[at.text_offset().as_usize()..]);
            assert_eq!(
                next.node(node).unwrap().kind(),
                next.node(tail).unwrap().kind()
            );
            assert_eq!(
                next.node(node).unwrap().attrs(),
                next.node(tail).unwrap().attrs()
            );
            let cut = inline(&doc, node)
                .atoms()
                .partition_point(|p| p.text_offset() < at.text_offset())
                + at.atom_index();
            assert_eq!(
                inline(next, node).atoms(),
                &inline(&doc, node).atoms()[..cut]
            );
            let moved = &inline(&doc, node).atoms()[cut..];
            assert_eq!(inline(next, tail).atoms().len(), moved.len());
            for (old, new) in moved.iter().zip(inline(next, tail).atoms()) {
                assert_eq!(old.atom(), new.atom());
                assert_eq!(
                    new.text_offset().as_usize(),
                    old.text_offset().as_usize() - at.text_offset().as_usize()
                );
            }
            for placement in inline(&doc, node).atoms() {
                assert_eq!(next.node(placement.atom()), doc.node(placement.atom()));
                assert_eq!(
                    applied
                        .changes()
                        .map_node_selection(NodeSelection::new(placement.atom())),
                    MappedPosition::Mapped(NodeSelection::new(placement.atom()))
                );
            }
            let undo = applied.inverse().apply_with_changes(next).unwrap();
            for old in &original_points {
                for bias in [MapBias::Start, MapBias::End] {
                    let old_key = (old.text_offset(), old.atom_index());
                    let seam_key = (at.text_offset(), at.atom_index());
                    let goes_right =
                        old_key > seam_key || (old_key == seam_key && bias == MapBias::End);
                    let expected = if goes_right {
                        point(
                            tail,
                            old.text_offset().as_usize() - at.text_offset().as_usize(),
                            if old.text_offset() == at.text_offset() {
                                old.atom_index() - at.atom_index()
                            } else {
                                old.atom_index()
                            },
                            old.affinity(),
                        )
                    } else {
                        *old
                    };
                    assert_eq!(
                        applied.changes().map_inline_point(*old, bias),
                        MappedPosition::Mapped(expected)
                    );
                    expected.validate(next).unwrap();
                    assert_eq!(
                        undo.changes().map_inline_point(expected, bias),
                        MappedPosition::Mapped(*old)
                    );
                }
            }
            assert_eq!(
                applied
                    .changes()
                    .map_node_gap(NodeGap::new(doc.root(), 1), MapBias::Start),
                MappedPosition::Mapped(NodeGap::new(doc.root(), 1))
            );
            assert_eq!(
                applied
                    .changes()
                    .map_node_gap(NodeGap::new(doc.root(), 1), MapBias::End),
                MappedPosition::Mapped(NodeGap::new(doc.root(), 2))
            );
            assert_roundtrip(&doc, &applied);
        }
    }
}

#[test]
fn join_preserves_atom_ids_and_offsets_across_empty_and_nonempty_siblings() {
    for left in ["", "中\n"] {
        for right in ["", "🙂e\u{301}"] {
            let (mut doc, nodes) = fixture(&[left, right]);
            let (first, second) = (nodes[0], nodes[1]);
            for node in [first, second] {
                let len = inline(&doc, node).len_bytes();
                for byte in [0, len] {
                    for extension in [false, true, false] {
                        doc = insert(doc, node, byte, extension);
                    }
                }
            }
            let applied = apply(&doc, TransactionStep::JoinNodes { first, second });
            let next = applied.document();
            assert_eq!(text(next, first), format!("{left}{right}"));
            assert_eq!(
                next.node(first).unwrap().kind(),
                doc.node(first).unwrap().kind()
            );
            assert_eq!(
                next.node(first).unwrap().attrs(),
                doc.node(first).unwrap().attrs()
            );
            assert!(next.node(second).is_none());
            let seam = inline(&doc, first).atom_count_at(offset(left.len()));
            let placements = inline(&doc, first)
                .atoms()
                .iter()
                .chain(inline(&doc, second).atoms());
            for (old, new) in placements.zip(inline(next, first).atoms()) {
                assert_eq!(old.atom(), new.atom());
                assert_eq!(next.node(new.atom()), doc.node(old.atom()));
                assert_eq!(next.parent_of(new.atom()), Some(first));
                let delta = if doc.parent_of(old.atom()) == Some(second) {
                    left.len()
                } else {
                    0
                };
                assert_eq!(
                    new.text_offset().as_usize(),
                    old.text_offset().as_usize() + delta
                );
                assert_eq!(
                    applied
                        .changes()
                        .map_node_selection(NodeSelection::new(old.atom())),
                    MappedPosition::Mapped(NodeSelection::new(old.atom()))
                );
            }
            assert_eq!(
                applied
                    .changes()
                    .map_node_selection(NodeSelection::new(second)),
                MappedPosition::Deleted
            );
            let undo = applied.inverse().apply_with_changes(next).unwrap();
            for node in [first, second] {
                for old in all_points(&doc, node) {
                    let expected = if node == second {
                        point(
                            first,
                            left.len() + old.text_offset().as_usize(),
                            old.atom_index()
                                + if old.text_offset() == TextOffset::ZERO {
                                    seam
                                } else {
                                    0
                                },
                            old.affinity(),
                        )
                    } else {
                        old
                    };
                    for bias in [MapBias::Start, MapBias::End] {
                        assert_eq!(
                            applied.changes().map_inline_point(old, bias),
                            MappedPosition::Mapped(expected)
                        );
                        expected.validate(next).unwrap();
                    }
                    // The one joined gap represents both old block edges;
                    // inverse bias chooses the original side explicitly.
                    let inverse_bias = if node == second {
                        MapBias::End
                    } else {
                        MapBias::Start
                    };
                    assert_eq!(
                        undo.changes().map_inline_point(expected, inverse_bias),
                        MappedPosition::Mapped(old)
                    );
                }
            }
            // Forward and reversed endpoint pairs spanning the join keep
            // both break-adjacent endpoints and their visual affinities.
            let a = point(first, left.len(), seam - 1, CursorAffinity::After);
            let b = before(second, 0, 1);
            for (anchor, focus, a_bias, f_bias) in [
                (a, b, MapBias::Start, MapBias::End),
                (b, a, MapBias::End, MapBias::Start),
            ] {
                let MappedPosition::Mapped(mapped_anchor) =
                    applied.changes().map_inline_point(anchor, a_bias)
                else {
                    panic!()
                };
                let MappedPosition::Mapped(mapped_focus) =
                    applied.changes().map_inline_point(focus, f_bias)
                else {
                    panic!()
                };
                assert_eq!(
                    undo.changes().map_inline_point(mapped_anchor, a_bias),
                    MappedPosition::Mapped(anchor)
                );
                assert_eq!(
                    undo.changes().map_inline_point(mapped_focus, f_bias),
                    MappedPosition::Mapped(focus)
                );
            }
            assert_eq!(
                applied
                    .changes()
                    .map_node_gap(NodeGap::new(doc.root(), 2), MapBias::Start),
                MappedPosition::Mapped(NodeGap::new(doc.root(), 1))
            );
            assert_roundtrip(&doc, &applied);
        }
    }
}

#[test]
fn legacy_split_stays_fail_closed_and_text_projection_respects_ordinal_zero() {
    let (doc, nodes) = fixture(&["中\n"]);
    let node = nodes[0];
    let doc = insert(insert(doc, node, 3, false), node, 3, true);
    for byte in [0, 3, 4] {
        let error = Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::SplitNode {
                node,
                at: offset(byte),
            })
            .apply(&doc)
            .unwrap_err();
        assert_eq!(error, Error::InvalidTransaction);
    }
    let text_point = TextPoint::new(node, offset(3), CursorAffinity::After);
    let applied = apply(
        &doc,
        TransactionStep::SplitInlineNode {
            at: before(node, 3, 1),
        },
    );
    for bias in [MapBias::Start, MapBias::End] {
        assert_eq!(
            applied.changes().map_text_point(text_point, bias),
            MappedPosition::Mapped(text_point)
        );
    }
}

#[test]
fn invalid_seams_and_structural_targets_never_publish_an_earlier_valid_edit() {
    let (doc, nodes) = fixture(&["中\n", "tail", "third"]);
    let (first, second, third) = (nodes[0], nodes[1], nodes[2]);
    let doc = insert(doc, first, 3, false);
    let atom = inline(&doc, first).atoms()[0].atom();
    let invalid_steps = [
        TransactionStep::SplitInlineNode {
            at: before(first, 3, 2),
        },
        TransactionStep::SplitInlineNode {
            at: before(first, 1, 0),
        },
        TransactionStep::SplitInlineNode {
            at: before(first, 99, 0),
        },
        TransactionStep::SplitInlineNode {
            at: before(doc.root(), 0, 0),
        },
        TransactionStep::SplitInlineNode {
            at: before(atom, 0, 0),
        },
        TransactionStep::JoinNodes {
            first,
            second: first,
        },
        TransactionStep::JoinNodes {
            first: second,
            second: first,
        },
        TransactionStep::JoinNodes {
            first,
            second: third,
        },
        TransactionStep::JoinNodes {
            first,
            second: atom,
        },
        TransactionStep::JoinNodes {
            first,
            second: doc.root(),
        },
    ];
    let original = doc.clone();
    for bad in invalid_steps {
        let result = Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::SetNodeAttrs {
                node: first,
                attrs: attrs("must-not-publish"),
            })
            .with_step(bad)
            .apply(&doc);
        assert!(result.is_err());
        assert_eq!(doc.store(), original.store());
        assert_eq!(doc.revision(), original.revision());
    }
}

#[test]
fn cross_parent_mixed_join_is_rejected() {
    let mut builder = NodeStoreBuilder::new();
    let a = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let b = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let quote = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([b]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([a, quote]),
        )
        .unwrap();
    let doc = XiaomuDocument::new(root, builder.finish()).unwrap();
    let doc = insert(insert(doc, a, 0, false), b, 0, true);
    let result = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::JoinNodes {
            first: a,
            second: b,
        })
        .apply(&doc);
    assert_eq!(result.unwrap_err(), Error::InvalidTransaction);
}

#[test]
fn restore_joined_node_refuses_live_ids_wrong_suffix_and_noninline_payloads() {
    let (doc, nodes) = fixture(&["left", "right"]);
    let (first, second) = (nodes[0], nodes[1]);
    let doc = insert(insert(doc, first, 4, false), second, 0, true);
    let saved = doc.node(second).unwrap().clone();
    let applied = apply(&doc, TransactionStep::JoinNodes { first, second });
    for (target, at, payload) in [
        (&doc, before(first, 4, 1), saved.clone()),
        (applied.document(), before(first, 4, 0), saved.clone()),
        (applied.document(), before(first, 3, 0), saved),
        (
            applied.document(),
            before(first, 4, 1),
            doc.node(doc.root()).unwrap().clone(),
        ),
    ] {
        let original = target.clone();
        let result = Transaction::new(TransactionOrigin::System)
            .with_step(TransactionStep::RestoreJoinedNode { at, node: payload })
            .apply(target);
        assert_eq!(result.unwrap_err(), Error::InvalidTransaction);
        assert_eq!(target.store(), original.store());
    }
}

#[test]
fn pure_text_join_inverse_maps_suffix_back_to_original_identity() {
    let (doc, nodes) = fixture(&["中\n", "🙂tail"]);
    let (first, second) = (nodes[0], nodes[1]);
    let applied = apply(&doc, TransactionStep::JoinNodes { first, second });
    assert!(matches!(
        applied.changes().steps(),
        [StepMap::NodeJoined { .. }]
    ));
    let undo = applied
        .inverse()
        .apply_with_changes(applied.document())
        .unwrap();
    assert!(matches!(
        undo.changes().steps(),
        [StepMap::NodeSplit { .. }]
    ));
    let old = before(second, 4, 0);
    let MappedPosition::Mapped(joined) = applied.changes().map_inline_point(old, MapBias::Start)
    else {
        panic!()
    };
    assert_eq!(
        undo.changes().map_inline_point(joined, MapBias::End),
        MappedPosition::Mapped(old)
    );
    assert_roundtrip(&doc, &applied);
}

#[test]
fn seam_aware_split_also_supports_completely_empty_and_plain_text_nodes() {
    for value in ["", "中\n🙂"] {
        let (doc, nodes) = fixture(&[value]);
        let node = nodes[0];
        for byte in value.char_indices().map(|(i, _)| i).chain([value.len()]) {
            let applied = apply(
                &doc,
                TransactionStep::SplitInlineNode {
                    at: before(node, byte, 0),
                },
            );
            let StepMap::InlineNodeSplit { inserted, .. } = &applied.changes().steps()[0] else {
                panic!()
            };
            assert_eq!(text(applied.document(), node), value[..byte]);
            assert_eq!(text(applied.document(), *inserted), value[byte..]);
            assert_roundtrip(&doc, &applied);
        }
    }
}

#[test]
fn composed_join_split_mapping_and_inverse_preserve_break_adjacent_ranges() {
    let (doc, nodes) = fixture(&["中\n", "🙂tail"]);
    let (first, second) = (nodes[0], nodes[1]);
    let doc = insert(
        insert(insert(doc, first, 4, false), second, 0, true),
        second,
        0,
        false,
    );
    // Join both seams, then split between the two atoms that originally
    // belonged to the second node. The first's retained end atom stays put.
    let applied = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::JoinNodes { first, second })
        .with_step(TransactionStep::SplitInlineNode {
            at: before(first, 4, 2),
        })
        .apply_with_changes(&doc)
        .unwrap();
    let StepMap::InlineNodeSplit { inserted: tail, .. } = &applied.changes().steps()[1] else {
        panic!()
    };
    let undo = applied
        .inverse()
        .apply_with_changes(applied.document())
        .unwrap();
    let before_break = before(second, 0, 0);
    let after_break = point(second, 0, 2, CursorAffinity::After);
    let a_expected = before(first, 4, 1);
    let b_expected = point(*tail, 0, 1, CursorAffinity::After);
    for (anchor, focus, expected_a, expected_b, a_bias, f_bias) in [
        (
            before_break,
            after_break,
            a_expected,
            b_expected,
            MapBias::Start,
            MapBias::End,
        ),
        (
            after_break,
            before_break,
            b_expected,
            a_expected,
            MapBias::End,
            MapBias::Start,
        ),
    ] {
        assert_eq!(
            applied.changes().map_inline_point(anchor, a_bias),
            MappedPosition::Mapped(expected_a)
        );
        assert_eq!(
            applied.changes().map_inline_point(focus, f_bias),
            MappedPosition::Mapped(expected_b)
        );
        // Start of the former second block needs End to disambiguate its
        // boundary from the old first block's final gap.
        assert_eq!(
            undo.changes().map_inline_point(expected_a, MapBias::End),
            MappedPosition::Mapped(anchor)
        );
        assert_eq!(
            undo.changes().map_inline_point(expected_b, MapBias::End),
            MappedPosition::Mapped(focus)
        );
    }
    assert_roundtrip(&doc, &applied);

    let failed = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::SplitInlineNode {
            at: before(first, 3, 0),
        })
        .with_step(TransactionStep::SplitInlineNode {
            at: before(first, 99, 0),
        })
        .apply(&doc);
    assert!(failed.is_err());
    assert_eq!(text(&doc, first), "中\n");
    assert_eq!(
        doc.parent_of(inline(&doc, first).atoms()[0].atom()),
        Some(first)
    );
}
