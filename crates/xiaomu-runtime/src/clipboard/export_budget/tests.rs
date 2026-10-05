use super::*;
use xiaomu_core::document::{InlineContent, NodeStoreBuilder};

fn attrs(value: AttrValue) -> NodeAttrs {
    NodeAttrs::new([("opaque".into(), value)].into()).unwrap()
}

fn source(attrs: NodeAttrs) -> XiaomuDocument {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            attrs,
            NodeContent::Inline(InlineContent::empty()),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    XiaomuDocument::new(root, builder.finish()).unwrap()
}

fn nested_attrs(levels: usize) -> NodeAttrs {
    let mut value = AttrValue::Null;
    for _ in 0..levels {
        value = AttrValue::List(vec![value]);
    }
    attrs(value)
}

fn counts(budget: &ClippedBudget) -> (usize, usize, usize) {
    (
        budget.budget.nodes,
        budget.budget.values,
        budget.budget.bytes,
    )
}

#[test]
fn clipped_source_reserves_roots_envelope_without_changing_legacy_threshold() {
    let small = source(NodeAttrs::empty());
    let mut original = Budget::default();
    original.document_node(&small, small.root(), 0).unwrap();
    let clipped = clipped_document(&small).unwrap();
    assert_eq!(clipped.budget.nodes, original.nodes);
    assert_eq!(clipped.budget.values, original.values + 32);
    assert_eq!(clipped.budget.bytes, original.bytes + 1024);

    let near_limit = source(attrs(AttrValue::List(vec![AttrValue::Null; 24_990])));
    assert_eq!(document(&near_limit), Ok(()));
    assert!(clipped_document(&near_limit).is_err());
}

#[test]
fn zero_count_skips_invalid_depth_and_preserves_budget() {
    let mut budget = clipped_document(&source(NodeAttrs::empty())).unwrap();
    let before = counts(&budget);
    let too_deep = nested_attrs(MAX_DEPTH + 1);
    assert_eq!(budget.reserve_empty_paragraphs(&too_deep, 0), Ok(()));
    assert_eq!(counts(&budget), before);
    assert!(budget.reserve_empty_paragraphs(&too_deep, 1).is_err());
    assert_eq!(counts(&budget), before);
}

#[test]
fn paragraph_attrs_use_exported_depth_four() {
    let mut budget = clipped_document(&source(NodeAttrs::empty())).unwrap();
    assert_eq!(
        budget.reserve_empty_paragraphs(&nested_attrs(MAX_DEPTH - 4), 1),
        Ok(())
    );
    assert!(
        budget
            .reserve_empty_paragraphs(&nested_attrs(MAX_DEPTH - 3), 1)
            .is_err()
    );
}

#[test]
fn reservation_multiplies_full_paragraph_skeleton_and_all_attrs() {
    let attrs = attrs(AttrValue::Object(
        [(
            "unknown".into(),
            AttrValue::List(vec![
                AttrValue::Null,
                AttrValue::String("preserved\nvalue".into()),
                AttrValue::Integer(80),
            ]),
        )]
        .into(),
    ));
    let mut cost = Budget::default();
    cost.node(4).unwrap();
    cost.attrs(&attrs, 4).unwrap();
    let mut budget = clipped_document(&source(NodeAttrs::empty())).unwrap();
    let before = counts(&budget);
    budget.reserve_empty_paragraphs(&attrs, 7).unwrap();
    assert_eq!(
        counts(&budget),
        (
            before.0 + cost.nodes * 7,
            before.1 + cost.values * 7,
            before.2 + cost.bytes * 7
        )
    );
}

#[test]
fn overlarge_counts_and_multiplication_overflow_fail_without_reserving() {
    let attrs = attrs(AttrValue::Null);
    let mut cost = Budget::default();
    cost.node(4).unwrap();
    cost.attrs(&attrs, 4).unwrap();
    let mut budget = clipped_document(&source(NodeAttrs::empty())).unwrap();
    let before = counts(&budget);
    let byte_overflow = usize::MAX / cost.bytes + 1;
    assert!(cost.values.checked_mul(byte_overflow).is_some());
    for count in [MAX_VALUES, usize::MAX, byte_overflow] {
        assert!(budget.reserve_empty_paragraphs(&attrs, count).is_err());
        assert_eq!(counts(&budget), before);
    }
}

#[test]
fn reservation_addition_overflow_fails_without_changing_budget() {
    let attrs = attrs(AttrValue::Null);
    for (nodes, values, bytes) in [(usize::MAX, 0, 0), (1, usize::MAX, 0), (1, 0, usize::MAX)] {
        let mut budget = ClippedBudget {
            budget: Budget {
                nodes,
                values,
                bytes,
            },
        };
        let before = counts(&budget);
        assert!(budget.reserve_empty_paragraphs(&attrs, 1).is_err());
        assert_eq!(counts(&budget), before);
    }
}

#[test]
fn repeated_paragraphs_respect_total_value_and_byte_limits() {
    for attrs in [
        attrs(AttrValue::List(vec![AttrValue::Null; 64])),
        attrs(AttrValue::String("x".repeat(2048))),
    ] {
        let mut cost = Budget::default();
        cost.node(4).unwrap();
        cost.attrs(&attrs, 4).unwrap();
        let mut budget = clipped_document(&source(NodeAttrs::empty())).unwrap();
        let remaining_values = MAX_VALUES - budget.budget.values;
        let remaining_bytes = MAX_BYTES - budget.budget.bytes;
        let remaining_nodes = MAX_NODES - budget.budget.nodes;
        let admitted = (remaining_values / cost.values)
            .min(remaining_bytes / cost.bytes)
            .min(remaining_nodes / cost.nodes);
        budget.reserve_empty_paragraphs(&attrs, admitted).unwrap();
        let before_failure = counts(&budget);
        assert!(budget.reserve_empty_paragraphs(&attrs, 1).is_err());
        assert_eq!(counts(&budget), before_failure);
    }
}

#[test]
fn empty_attrs_still_reserve_the_full_paragraph_skeleton() {
    let mut budget = clipped_document(&source(NodeAttrs::empty())).unwrap();
    let before = counts(&budget);
    budget
        .reserve_empty_paragraphs(&NodeAttrs::empty(), 3)
        .unwrap();
    assert_eq!(
        counts(&budget),
        (before.0 + 3, before.1 + 16 * 3, before.2 + 512 * 3)
    );
}

#[test]
fn paragraph_reservations_enforce_the_total_node_limit() {
    let mut budget = ClippedBudget {
        budget: Budget {
            nodes: MAX_NODES - 1,
            values: 0,
            bytes: 0,
        },
    };
    let before = counts(&budget);
    assert!(
        budget
            .reserve_empty_paragraphs(&NodeAttrs::empty(), 2)
            .is_err()
    );
    assert_eq!(counts(&budget), before);
    budget
        .reserve_empty_paragraphs(&NodeAttrs::empty(), 1)
        .unwrap();
    assert_eq!(counts(&budget), (MAX_NODES, 16, 512));
    assert!(
        budget
            .reserve_empty_paragraphs(&NodeAttrs::empty(), 1)
            .is_err()
    );
    assert_eq!(counts(&budget), (MAX_NODES, 16, 512));
}
