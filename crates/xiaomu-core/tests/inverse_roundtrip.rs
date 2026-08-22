//! P0.6 inverse round-trips and randomized invariant tests.

use std::collections::BTreeMap;

use xiaomu_core::document::{
    AttrValue, HeadingLevel, InlineContent, LinkMark, Mark, MarkKind, MarkSet, NodeAttrs,
    NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::mapping::{ChangeMap, MapBias, MappedPosition};
use xiaomu_core::selection::{CursorAffinity, NodeGap, NodeSelection, TextPoint};
use xiaomu_core::text::{TextBuffer, TextOffset, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

fn offset_at(raw: usize) -> TextOffset {
    const SCRATCH: &str =
        "0000000000000000000000000000000000000000000000000000000000000000000000000000000000";
    TextBuffer::from(SCRATCH).offset_at(raw).unwrap()
}

fn range(start: usize, end: usize) -> TextRange {
    TextRange::new(offset_at(start), offset_at(end)).unwrap()
}

fn bold() -> MarkSet {
    MarkSet::new([Mark::Bold]).unwrap()
}

fn italic() -> MarkSet {
    MarkSet::new([Mark::Italic]).unwrap()
}

fn link(href: &str) -> MarkSet {
    MarkSet::new([Mark::Link(LinkMark::new(href, None))]).unwrap()
}

fn inline(parts: &[(&str, MarkSet)]) -> NodeContent {
    let runs = parts
        .iter()
        .map(|(text, marks)| TextRun::new(*text, marks.clone()).unwrap())
        .collect::<Vec<_>>();
    NodeContent::Inline(InlineContent::new(runs).unwrap())
}

/// `Document > [p(bold "你好", link-a "世界", plain "tail")]`
fn marked_fixture() -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            inline(&[
                ("你好", bold()),
                ("世界", link("https://a.example")),
                ("tail", MarkSet::empty()),
            ]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        paragraph,
    )
}

fn replace_text(node: NodeId, start: usize, end: usize, replacement: &str) -> TransactionStep {
    TransactionStep::ReplaceText {
        node,
        range: range(start, end),
        replacement: replacement.to_owned(),
    }
}

/// Applies the transaction, then its inverse, and asserts the exact store and
/// root of the original snapshot are restored.
fn assert_round_trips(document: &XiaomuDocument, transaction: Transaction) {
    let applied = transaction.apply_with_changes(document).unwrap();
    assert!(applied.document().validate().is_ok());

    let undone = applied.inverse().apply(applied.document()).unwrap();
    assert_eq!(undone.store(), document.store());
    assert_eq!(undone.root(), document.root());
    assert!(
        undone.revision().as_u64() > document.revision().as_u64(),
        "undo still moves the revision forward"
    );
}

#[test]
fn replace_text_inverse_restores_text_and_marks_across_runs() {
    let (document, paragraph) = marked_fixture();

    // Replace across all three runs; the replacement inherits bold.
    let transaction = Transaction::new(TransactionOrigin::UserInput)
        .with_step(replace_text(paragraph, 3, 12, "XY"));
    assert_round_trips(&document, transaction);
}

#[test]
fn deletion_at_run_boundary_inverse_restores_marks() {
    let (document, paragraph) = marked_fixture();

    // Delete exactly the link-marked run; the restored text must not keep
    // the marks of the neighboring boundary.
    let transaction = Transaction::new(TransactionOrigin::UserInput)
        .with_step(replace_text(paragraph, 6, 12, ""));
    assert_round_trips(&document, transaction);
}

#[test]
fn pure_insertion_inverse_removes_inserted_text() {
    let (document, paragraph) = marked_fixture();

    // Insert at the bold/link boundary.
    let transaction = Transaction::new(TransactionOrigin::UserInput).with_step(replace_text(
        paragraph,
        6,
        6,
        "新文本🎉",
    ));
    assert_round_trips(&document, transaction);
}

#[test]
fn insert_node_inverse_removes_the_allocated_node() {
    let (document, _) = marked_fixture();

    let transaction =
        Transaction::new(TransactionOrigin::System).with_step(TransactionStep::InsertNode {
            parent: document.root(),
            index: 0,
            kind: NodeKind::Heading(HeadingLevel::new(2).unwrap()),
            attrs: NodeAttrs::empty(),
            content: inline(&[("标题", MarkSet::empty())]),
        });
    assert_round_trips(&document, transaction);
}

#[test]
fn remove_node_inverse_restores_subtree_identities() {
    let mut builder = NodeStoreBuilder::new();
    let child = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            inline(&[("child", bold())]),
        )
        .unwrap();
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
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([list]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();

    let removal = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveNode { node: list });
    let applied = removal.apply_with_changes(&document).unwrap();
    assert!(applied.document().node(list).is_none());

    // The inverse carries a RestoreSubtree and brings back the exact ids.
    let inverse = applied.inverse();
    let restored = inverse.apply(applied.document()).unwrap();
    assert!(restored.node(list).is_some());
    assert!(restored.node(list_item).is_some());
    assert!(restored.node(child).is_some());
    assert_eq!(restored.store(), document.store());
    assert_eq!(restored.root(), document.root());

    // Restoring twice must fail: the identities already exist again.
    let again = inverse.apply(&restored);
    assert!(matches!(again, Err(xiaomu_core::Error::InvalidTransaction)));
}

#[test]
fn attrs_and_mark_inverses_restore_exact_state() {
    let (document, paragraph) = marked_fixture();

    let mut values = BTreeMap::new();
    values.insert("lang".to_owned(), AttrValue::String("zh".to_owned()));
    let transaction = Transaction::new(TransactionOrigin::Extension("demo".to_owned()))
        .with_step(TransactionStep::SetNodeAttrs {
            node: paragraph,
            attrs: NodeAttrs::new(values).unwrap(),
        })
        // Replaces the existing link over "世界" with a conflicting target.
        .with_step(TransactionStep::AddMark {
            node: paragraph,
            range: range(6, 12),
            mark: Mark::Link(LinkMark::new("https://b.example", None)),
        })
        // Strips bold from "你好".
        .with_step(TransactionStep::RemoveMark {
            node: paragraph,
            range: range(0, 6),
            mark_kind: MarkKind::Bold,
        });
    assert_round_trips(&document, transaction);
}

#[test]
fn multi_step_inverse_round_trips_one_transaction() {
    let mut builder = NodeStoreBuilder::new();
    let first = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            inline(&[("你好世界", MarkSet::empty())]),
        )
        .unwrap();
    let doomed = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, doomed]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();

    let transaction = Transaction::new(TransactionOrigin::UserInput)
        .with_step(replace_text(first, 3, 6, "🎉"))
        .with_step(TransactionStep::AddMark {
            node: first,
            range: range(0, 3),
            mark: Mark::Italic,
        })
        .with_step(TransactionStep::InsertNode {
            parent: root,
            index: 0,
            kind: NodeKind::Paragraph,
            attrs: NodeAttrs::empty(),
            content: NodeContent::empty_inline(),
        })
        .with_step(TransactionStep::RemoveNode { node: doomed });
    assert_round_trips(&document, transaction);
}

#[test]
fn chained_undo_restores_the_initial_snapshot() {
    let (document, paragraph) = marked_fixture();

    let first =
        Transaction::new(TransactionOrigin::UserInput).with_step(replace_text(paragraph, 0, 3, ""));
    let applied_first = first.apply_with_changes(&document).unwrap();

    let second =
        Transaction::new(TransactionOrigin::System).with_step(TransactionStep::InsertNode {
            parent: document.root(),
            index: 1,
            kind: NodeKind::Paragraph,
            attrs: NodeAttrs::empty(),
            content: inline(&[("second", italic())]),
        });
    let applied_second = second.apply_with_changes(applied_first.document()).unwrap();

    let third = Transaction::new(TransactionOrigin::UserInput)
        .with_step(replace_text(paragraph, 0, 0, "序"));
    let applied_third = third.apply_with_changes(applied_second.document()).unwrap();

    let current = applied_third
        .inverse()
        .apply(applied_third.document())
        .unwrap();
    let current = applied_second.inverse().apply(&current).unwrap();
    let current = applied_first.inverse().apply(&current).unwrap();

    assert_eq!(current.store(), document.store());
    assert_eq!(current.root(), document.root());
    assert_eq!(
        current.revision().as_u64(),
        document.revision().as_u64() + 6
    );
}

/// Deterministic xorshift generator; keeps the suite dependency-free.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut state = self.0;
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        self.0 = state;
        state
    }

    fn below(&mut self, ceiling: usize) -> usize {
        (self.next_u64() % ceiling as u64) as usize
    }
}

const CHAR_POOL: [&str; 7] = ["a", "b", "中", "文", "🎉", "é", "ε"];

fn random_text(rng: &mut Rng) -> String {
    let len = 1 + rng.below(4);
    (0..len)
        .map(|_| CHAR_POOL[rng.below(CHAR_POOL.len())])
        .collect()
}

fn random_mark_set(rng: &mut Rng) -> MarkSet {
    let candidates = [
        Mark::Bold,
        Mark::Italic,
        Mark::Code,
        Mark::Strike,
        Mark::Link(LinkMark::new(
            format!("https://{}.example", rng.below(1000)),
            None,
        )),
    ];
    let picks = (0..=rng.below(candidates.len()))
        .map(|index| candidates[index].clone())
        .collect::<Vec<_>>();
    MarkSet::new(picks).unwrap()
}

fn random_inline(rng: &mut Rng) -> NodeContent {
    let runs = (0..1 + rng.below(3))
        .map(|_| TextRun::new(random_text(rng), random_mark_set(rng)).unwrap())
        .collect::<Vec<_>>();
    NodeContent::Inline(InlineContent::new(runs).unwrap())
}

fn random_document(rng: &mut Rng) -> XiaomuDocument {
    let mut builder = NodeStoreBuilder::new();
    let paragraphs = (0..2 + rng.below(3))
        .map(|_| {
            builder
                .insert(NodeKind::Paragraph, NodeAttrs::empty(), random_inline(rng))
                .unwrap()
        })
        .collect::<Vec<_>>();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(paragraphs),
        )
        .unwrap();
    XiaomuDocument::new(root, builder.finish()).unwrap()
}

fn boundaries(content: &InlineContent) -> Vec<usize> {
    let mut offsets = vec![0];
    let mut cursor = 0;
    for run in content.runs() {
        cursor += run.len_bytes();
        offsets.push(cursor);
    }
    offsets
}

fn pick_range(rng: &mut Rng, content: &InlineContent) -> TextRange {
    let bounds = boundaries(content);
    let start_index = rng.below(bounds.len());
    let end_index = start_index + rng.below(bounds.len() - start_index);
    range(bounds[start_index], bounds[end_index])
}

fn inline_targets(document: &XiaomuDocument) -> Vec<NodeId> {
    document
        .store()
        .iter()
        .filter(|node| node.content().as_inline().is_some())
        .map(|node| node.id())
        .collect()
}

fn container_targets(document: &XiaomuDocument) -> Vec<NodeId> {
    document
        .store()
        .iter()
        .filter(|node| node.content().as_children().is_some())
        .map(|node| node.id())
        .collect()
}

fn random_kind_and_content(rng: &mut Rng) -> (NodeKind, NodeContent) {
    match rng.below(3) {
        0 => (NodeKind::Paragraph, random_inline(rng)),
        1 => (
            NodeKind::Heading(HeadingLevel::new(1 + rng.below(6) as u8).unwrap()),
            random_inline(rng),
        ),
        _ => (NodeKind::Quote, NodeContent::children([])),
    }
}

fn random_step(rng: &mut Rng, document: &XiaomuDocument) -> TransactionStep {
    let inlines = inline_targets(document);
    if inlines.is_empty() {
        // Every inline node was removed; grow the document instead.
        return TransactionStep::InsertNode {
            parent: document.root(),
            index: 0,
            kind: NodeKind::Paragraph,
            attrs: NodeAttrs::empty(),
            content: random_inline(rng),
        };
    }
    match rng.below(8) {
        0..=2 => {
            // ReplaceText on a random inline node with a boundary-valid range.
            let node = inlines[rng.below(inlines.len())];
            let content = document.node(node).unwrap().content().as_inline().unwrap();
            let text_range = pick_range(rng, content);
            let replacement = if rng.below(4) == 0 {
                String::new()
            } else {
                random_text(rng)
            };
            TransactionStep::ReplaceText {
                node,
                range: text_range,
                replacement,
            }
        }
        3..=4 => {
            let node = inlines[rng.below(inlines.len())];
            let content = document.node(node).unwrap().content().as_inline().unwrap();
            let text_range = pick_range(rng, content);
            if rng.below(2) == 0 {
                TransactionStep::AddMark {
                    node,
                    range: text_range,
                    mark: Mark::Link(LinkMark::new(
                        format!("https://{}.example", rng.below(1000)),
                        None,
                    )),
                }
            } else {
                TransactionStep::RemoveMark {
                    node,
                    range: text_range,
                    mark_kind: [MarkKind::Bold, MarkKind::Italic, MarkKind::Link][rng.below(3)],
                }
            }
        }
        5 => {
            let containers = container_targets(document);
            let parent = containers[rng.below(containers.len())];
            let len = document
                .node(parent)
                .unwrap()
                .content()
                .as_children()
                .unwrap()
                .len();
            let (kind, content) = random_kind_and_content(rng);
            TransactionStep::InsertNode {
                parent,
                index: rng.below(len + 1),
                kind,
                attrs: NodeAttrs::empty(),
                content,
            }
        }
        6 => {
            let candidates: Vec<NodeId> = document
                .store()
                .iter()
                .map(|node| node.id())
                .filter(|id| *id != document.root())
                .collect();
            if candidates.is_empty() {
                TransactionStep::InsertNode {
                    parent: document.root(),
                    index: 0,
                    kind: NodeKind::Paragraph,
                    attrs: NodeAttrs::empty(),
                    content: NodeContent::empty_inline(),
                }
            } else {
                TransactionStep::RemoveNode {
                    node: candidates[rng.below(candidates.len())],
                }
            }
        }
        _ => {
            let ids: Vec<NodeId> = document.store().iter().map(|node| node.id()).collect();
            let node = ids[rng.below(ids.len())];
            let mut values = BTreeMap::new();
            if rng.below(2) == 0 {
                values.insert(
                    format!("key{}", rng.below(4)),
                    AttrValue::String(format!("v{}", rng.below(100))),
                );
            }
            TransactionStep::SetNodeAttrs {
                node,
                attrs: NodeAttrs::new(values).unwrap(),
            }
        }
    }
}

/// Every valid pre-state position that survives must land on a valid position
/// of the post-state snapshot.
fn check_mapping_invariants(before: &XiaomuDocument, changes: &ChangeMap, after: &XiaomuDocument) {
    for node in before.store().iter() {
        let id = node.id();

        if let Some(content) = node.content().as_inline() {
            for raw in boundaries(content) {
                let position = TextPoint::new(id, offset_at(raw), CursorAffinity::Before);
                for bias in [MapBias::Start, MapBias::End] {
                    if let MappedPosition::Mapped(mapped) = changes.map_text_point(position, bias) {
                        assert!(
                            mapped.validate(after).is_ok(),
                            "mapped point {mapped:?} invalid after {bias:?}"
                        );
                    }
                }
            }
        }

        if let Some(children) = node.content().as_children() {
            for index in 0..=children.len() {
                for bias in [MapBias::Start, MapBias::End] {
                    if let MappedPosition::Mapped(mapped) =
                        changes.map_node_gap(NodeGap::new(id, index), bias)
                    {
                        assert!(
                            mapped.validate(after).is_ok(),
                            "mapped gap {mapped:?} invalid after {bias:?}"
                        );
                    }
                }
            }
        }

        if let MappedPosition::Mapped(mapped) = changes.map_node_selection(NodeSelection::new(id)) {
            assert!(mapped.validate(after).is_ok());
        }
    }
}

#[test]
fn random_transaction_sequences_stay_valid_and_round_trip() {
    for seed in 1..=8u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let initial = random_document(&mut rng);
        let mut current = initial.clone();
        let mut inverses: Vec<Transaction> = Vec::new();

        for _ in 0..10 {
            // Generate a valid multi-step transaction by simulating each step
            // against the intermediate state it will be applied to.
            let mut transaction = Transaction::new(TransactionOrigin::UserInput);
            let mut simulation = current.clone();
            for _ in 0..1 + rng.below(3) {
                let step = random_step(&mut rng, &simulation);
                simulation = Transaction::new(TransactionOrigin::System)
                    .with_step(step.clone())
                    .apply(&simulation)
                    .unwrap();
                transaction.push_step(step);
            }

            let applied = transaction.apply_with_changes(&current).unwrap();
            assert!(applied.document().validate().is_ok());
            check_mapping_invariants(&current, applied.changes(), applied.document());

            // Single-transaction round trip restores the exact store.
            let undone = applied.inverse().apply(applied.document()).unwrap();
            assert_eq!(undone.store(), current.store());
            assert_eq!(undone.root(), current.root());

            inverses.push(applied.inverse().clone());
            current = applied.into_document();
        }

        // Chained undo in reverse order restores the initial store.
        for inverse in inverses.iter().rev() {
            current = inverse.apply(&current).unwrap();
            assert!(current.validate().is_ok());
        }
        assert_eq!(current.store(), initial.store());
        assert_eq!(current.root(), initial.root());
    }
}
