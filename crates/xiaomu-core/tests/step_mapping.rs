//! StepMap mapping semantics over declarative step data.
//!
//! These tests exercise the pure coordinate arithmetic of `StepMap` with
//! identities allocated by a real document builder; no snapshot is involved.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use xiaomu_core::document::{NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder};
use xiaomu_core::mapping::{MapBias, MappedPosition, StepMap};
use xiaomu_core::selection::{CursorAffinity, NodeGap, NodeSelection, TextPoint};
use xiaomu_core::text::{TextBuffer, TextOffset, TextRange};

/// A pool of distinct, real node identities so tests can address arbitrary
/// "nodes" without minting raw ids (which is not possible outside Core).
fn node_ids() -> &'static [NodeId] {
    static IDS: OnceLock<Vec<NodeId>> = OnceLock::new();
    IDS.get_or_init(|| {
        let mut builder = NodeStoreBuilder::new();
        let mut ids = Vec::new();
        for _ in 0..16 {
            ids.push(
                builder
                    .insert(
                        NodeKind::Paragraph,
                        NodeAttrs::empty(),
                        NodeContent::empty_inline(),
                    )
                    .unwrap(),
            );
        }
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children(ids.iter().copied()),
            )
            .unwrap();
        let document = xiaomu_core::document::XiaomuDocument::new(root, builder.finish()).unwrap();
        assert!(document.validate().is_ok());
        ids
    })
}

fn node(raw: usize) -> NodeId {
    node_ids()[raw]
}

fn offset(raw: usize) -> TextOffset {
    const SCRATCH: &str = "00000000000000000000000000000000";
    TextBuffer::from(SCRATCH).offset_at(raw).unwrap()
}

fn point(raw_node: usize, raw_offset: usize) -> TextPoint {
    TextPoint::new(node(raw_node), offset(raw_offset), CursorAffinity::Before)
}

fn range(start: usize, end: usize) -> TextRange {
    TextRange::new(offset(start), offset(end)).unwrap()
}

fn replaced(start: usize, end: usize, len: usize) -> StepMap {
    StepMap::TextReplaced {
        node: node(1),
        range: range(start, end),
        replacement_len: len,
    }
}

#[test]
fn empty_range_insertion_resolves_the_boundary_by_bias() {
    let step = replaced(3, 3, 4);

    // A pure insertion at byte 3: the caret sitting exactly there is the
    // insertion boundary and resolves by bias.
    assert_eq!(
        step.map_text_point(point(1, 3), MapBias::Start),
        MappedPosition::Mapped(point(1, 3))
    );
    assert_eq!(
        step.map_text_point(point(1, 3), MapBias::End),
        MappedPosition::Mapped(point(1, 7))
    );
    // Earlier and later positions shift as usual.
    assert_eq!(
        step.map_text_point(point(1, 1), MapBias::End),
        MappedPosition::Mapped(point(1, 1))
    );
    assert_eq!(
        step.map_text_point(point(1, 6), MapBias::Start),
        MappedPosition::Mapped(point(1, 10))
    );
}

#[test]
fn text_replacement_moves_only_later_offsets() {
    let step = replaced(3, 6, 2);

    assert_eq!(
        step.map_text_point(point(1, 0), MapBias::Start),
        MappedPosition::Mapped(point(1, 0))
    );
    assert_eq!(
        step.map_text_point(point(1, 6), MapBias::Start),
        MappedPosition::Mapped(point(1, 5))
    );
    assert_eq!(
        step.map_text_point(point(1, 12), MapBias::End),
        MappedPosition::Mapped(point(1, 11))
    );
    // Unrelated nodes are untouched.
    assert_eq!(
        step.map_text_point(point(2, 9), MapBias::Start),
        MappedPosition::Mapped(point(2, 9))
    );
}

#[test]
fn inside_replacement_resolves_by_bias() {
    let step = replaced(3, 6, 2);

    for raw in 3..6 {
        assert_eq!(
            step.map_text_point(point(1, raw), MapBias::Start),
            MappedPosition::Mapped(point(1, 3))
        );
        assert_eq!(
            step.map_text_point(point(1, raw), MapBias::End),
            MappedPosition::Mapped(point(1, 5))
        );
    }
}

#[test]
fn insertion_bias_resolves_the_exact_gap() {
    let step = StepMap::NodeInserted {
        parent: node(0),
        index: 1,
        inserted: node(7),
    };

    assert_eq!(
        step.map_node_gap(NodeGap::new(node(0), 0), MapBias::Start),
        MappedPosition::Mapped(NodeGap::new(node(0), 0))
    );
    assert_eq!(
        step.map_node_gap(NodeGap::new(node(0), 1), MapBias::Start),
        MappedPosition::Mapped(NodeGap::new(node(0), 1))
    );
    assert_eq!(
        step.map_node_gap(NodeGap::new(node(0), 1), MapBias::End),
        MappedPosition::Mapped(NodeGap::new(node(0), 2))
    );
    assert_eq!(
        step.map_node_gap(NodeGap::new(node(0), 2), MapBias::Start),
        MappedPosition::Mapped(NodeGap::new(node(0), 3))
    );
    // Gaps of other parents are untouched.
    assert_eq!(
        step.map_node_gap(NodeGap::new(node(9), 1), MapBias::End),
        MappedPosition::Mapped(NodeGap::new(node(9), 1))
    );
}

#[test]
fn removal_shifts_only_later_gaps() {
    let step = StepMap::NodeRemoved {
        parent: node(0),
        index: 1,
        removed: BTreeSet::from([node(2), node(3)]),
    };

    assert_eq!(
        step.map_node_gap(NodeGap::new(node(0), 0), MapBias::Start),
        MappedPosition::Mapped(NodeGap::new(node(0), 0))
    );
    // The boundary that pointed at the removed child survives.
    assert_eq!(
        step.map_node_gap(NodeGap::new(node(0), 1), MapBias::Start),
        MappedPosition::Mapped(NodeGap::new(node(0), 1))
    );
    assert_eq!(
        step.map_node_gap(NodeGap::new(node(0), 2), MapBias::Start),
        MappedPosition::Mapped(NodeGap::new(node(0), 1))
    );
    assert_eq!(
        step.map_node_gap(NodeGap::new(node(0), 3), MapBias::Start),
        MappedPosition::Mapped(NodeGap::new(node(0), 2))
    );
}

#[test]
fn removed_subtrees_delete_positions_and_selections() {
    let step = StepMap::NodeRemoved {
        parent: node(0),
        index: 1,
        removed: BTreeSet::from([node(2), node(3)]),
    };

    assert_eq!(
        step.map_text_point(point(2, 0), MapBias::Start),
        MappedPosition::Deleted
    );
    assert_eq!(
        step.map_text_point(point(3, 4), MapBias::End),
        MappedPosition::Deleted
    );
    assert_eq!(
        step.map_node_gap(NodeGap::new(node(3), 0), MapBias::Start),
        MappedPosition::Deleted
    );
    assert_eq!(
        step.map_node_selection(NodeSelection::new(node(2))),
        MappedPosition::Deleted
    );

    assert_eq!(
        step.map_text_point(point(1, 4), MapBias::Start),
        MappedPosition::Mapped(point(1, 4))
    );
    assert_eq!(
        step.map_node_selection(NodeSelection::new(node(1))),
        MappedPosition::Mapped(NodeSelection::new(node(1)))
    );
}

#[test]
fn node_split_moves_tail_offsets_into_the_inserted_sibling() {
    // Paragraph 1 was split at byte 4; the tail entered parent 0 at
    // index 1 as node 7.
    let step = StepMap::NodeSplit {
        parent: node(0),
        index: 1,
        node: node(1),
        at: offset(4),
        inserted: node(7),
    };

    // Head offsets stay in the original node.
    assert_eq!(
        step.map_text_point(point(1, 0), MapBias::End),
        MappedPosition::Mapped(point(1, 0))
    );
    assert_eq!(
        step.map_text_point(point(1, 4), MapBias::Start),
        MappedPosition::Mapped(point(1, 4))
    );
    // The exact split point resolves by bias between the two boundaries.
    assert_eq!(
        step.map_text_point(point(1, 4), MapBias::End),
        MappedPosition::Mapped(point(7, 0))
    );
    // Tail offsets shift into the tail sibling.
    assert_eq!(
        step.map_text_point(point(1, 9), MapBias::Start),
        MappedPosition::Mapped(point(7, 5))
    );
    // Other nodes are untouched.
    assert_eq!(
        step.map_text_point(point(2, 4), MapBias::End),
        MappedPosition::Mapped(point(2, 4))
    );
}

#[test]
fn node_split_shifts_only_later_sibling_gaps() {
    let step = StepMap::NodeSplit {
        parent: node(0),
        index: 1,
        node: node(1),
        at: offset(0),
        inserted: node(7),
    };

    assert_eq!(
        step.map_node_gap(NodeGap::new(node(0), 0), MapBias::Start),
        MappedPosition::Mapped(NodeGap::new(node(0), 0))
    );
    // The boundary that pointed at the tail sibling resolves by bias.
    assert_eq!(
        step.map_node_gap(NodeGap::new(node(0), 1), MapBias::Start),
        MappedPosition::Mapped(NodeGap::new(node(0), 1))
    );
    assert_eq!(
        step.map_node_gap(NodeGap::new(node(0), 1), MapBias::End),
        MappedPosition::Mapped(NodeGap::new(node(0), 2))
    );
    assert_eq!(
        step.map_node_gap(NodeGap::new(node(0), 3), MapBias::Start),
        MappedPosition::Mapped(NodeGap::new(node(0), 4))
    );
    assert_eq!(
        step.map_node_gap(NodeGap::new(node(9), 1), MapBias::End),
        MappedPosition::Mapped(NodeGap::new(node(9), 1))
    );
}

#[test]
fn node_join_moves_absorbed_offsets_into_the_survivor() {
    // Node 2 was absorbed into node 1, whose text was 6 bytes before
    // the join; the child list of parent 0 lost index 1.
    let step = StepMap::NodeJoined {
        parent: node(0),
        index: 1,
        first: node(1),
        second: node(2),
        first_len: 6,
        removed: BTreeSet::from([node(2)]),
    };

    // Offsets of the survivor stay put.
    assert_eq!(
        step.map_text_point(point(1, 3), MapBias::Start),
        MappedPosition::Mapped(point(1, 3))
    );
    // Absorbed offsets translate past the survivor's text.
    assert_eq!(
        step.map_text_point(point(2, 0), MapBias::Start),
        MappedPosition::Mapped(point(1, 6))
    );
    assert_eq!(
        step.map_text_point(point(2, 4), MapBias::Start),
        MappedPosition::Mapped(point(1, 10))
    );
    // Unrelated nodes are untouched.
    assert_eq!(
        step.map_text_point(point(5, 1), MapBias::Start),
        MappedPosition::Mapped(point(5, 1))
    );
}

#[test]
fn node_join_deletes_positions_in_the_removed_subtree() {
    let step = StepMap::NodeJoined {
        parent: node(0),
        index: 1,
        first: node(1),
        second: node(2),
        first_len: 6,
        removed: BTreeSet::from([node(2), node(3)]),
    };

    assert_eq!(
        step.map_node_gap(NodeGap::new(node(0), 0), MapBias::Start),
        MappedPosition::Mapped(NodeGap::new(node(0), 0))
    );
    // The gap pointing at the joined pair survives between the former
    // neighbors.
    assert_eq!(
        step.map_node_gap(NodeGap::new(node(0), 1), MapBias::Start),
        MappedPosition::Mapped(NodeGap::new(node(0), 1))
    );
    assert_eq!(
        step.map_node_gap(NodeGap::new(node(0), 2), MapBias::Start),
        MappedPosition::Mapped(NodeGap::new(node(0), 1))
    );
    assert_eq!(
        step.map_node_gap(NodeGap::new(node(2), 0), MapBias::Start),
        MappedPosition::Deleted
    );
    assert_eq!(
        step.map_node_gap(NodeGap::new(node(3), 0), MapBias::Start),
        MappedPosition::Deleted
    );
    assert_eq!(
        step.map_node_selection(NodeSelection::new(node(3))),
        MappedPosition::Deleted
    );
    assert_eq!(
        step.map_node_selection(NodeSelection::new(node(1))),
        MappedPosition::Mapped(NodeSelection::new(node(1)))
    );
}
