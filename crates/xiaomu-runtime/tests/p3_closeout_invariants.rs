//! P3.7 Unicode cross-block and randomized history/mapping closeout invariants.

use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_runtime::session::{
    DocumentSelection, DocumentSession, EditIntent, SessionOutcome,
};

fn inline(text: &str) -> InlineContent {
    InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
}

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline(text)),
        )
        .unwrap()
}

fn point(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextPoint {
    let content = document.node(node).unwrap().content().as_inline().unwrap();
    TextPoint::new(
        node,
        content.offset_at(raw).unwrap(),
        CursorAffinity::Before,
    )
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

fn two_paragraphs(first: &str, second: &str) -> (XiaomuDocument, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let first = paragraph(&mut builder, first);
    let second = paragraph(&mut builder, second);
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, second]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        first,
        second,
    )
}

#[test]
fn unicode_cross_block_matrix_preserves_boundaries_clipboard_and_history() {
    let cases = [
        ("ascii", "alpha"),
        ("cjk", "中文"),
        ("mixed", "A中B"),
        ("emoji", "👍🏽🚀"),
        ("combining", "e\u{301}"),
        ("cjk_emoji", "中👍文"),
        ("bidi", "abc אבג مرحبا"),
    ];

    for (label, sample) in cases {
        let first_text = format!("L{sample}");
        let second_text = format!("{sample}R");
        let (document, first, second) = two_paragraphs(&first_text, &second_text);
        let selection = DocumentSelection::new(
            point(&document, first, "L".len()),
            point(&document, second, sample.len()),
        );
        selection.validate(&document).unwrap();

        let before_store = document.store().clone();
        let mut session = DocumentSession::new(document, selection).unwrap();
        let slice = session
            .clipboard_slice()
            .unwrap()
            .expect("cross-block selection must project clipboard content");
        assert_eq!(
            slice.plain_text(),
            format!("{sample}\n{sample}"),
            "clipboard mismatch for {label}"
        );

        assert_eq!(
            session.apply_intent(&EditIntent::Delete).unwrap(),
            SessionOutcome::DocumentChanged,
            "delete must mutate for {label}"
        );
        session.document().validate().unwrap();
        session.selection().validate(session.document()).unwrap();
        assert_eq!(text(session.document(), first), "LR", "seam mismatch for {label}");
        assert!(
            session.document().node(second).is_none(),
            "covered tail must be removed for {label}"
        );
        let after_store = session.document().store().clone();

        assert_eq!(session.undo().unwrap(), SessionOutcome::DocumentChanged);
        assert_eq!(session.document().store(), &before_store, "undo mismatch for {label}");
        session.selection().validate(session.document()).unwrap();

        assert_eq!(session.redo().unwrap(), SessionOutcome::DocumentChanged);
        assert_eq!(session.document().store(), &after_store, "redo mismatch for {label}");
        session.selection().validate(session.document()).unwrap();
    }
}

struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, ceiling: usize) -> usize {
        if ceiling == 0 {
            0
        } else {
            (self.next_u64() as usize) % ceiling
        }
    }
}

fn inline_nodes(document: &XiaomuDocument) -> Vec<NodeId> {
    fn walk(document: &XiaomuDocument, node: NodeId, output: &mut Vec<NodeId>) {
        let Some(node_value) = document.node(node) else {
            return;
        };
        match node_value.content() {
            NodeContent::Inline(_) => output.push(node),
            NodeContent::Children(children) => {
                for child in children {
                    walk(document, *child, output);
                }
            }
            _ => {}
        }
    }

    let mut output = Vec::new();
    walk(document, document.root(), &mut output);
    output
}

fn scalar_boundaries(document: &XiaomuDocument, node: NodeId) -> Vec<usize> {
    let content = document.node(node).unwrap().content().as_inline().unwrap();
    let value: String = content
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect();
    let mut boundaries = vec![0];
    boundaries.extend(
        value
            .char_indices()
            .map(|(index, character)| index + character.len_utf8()),
    );
    boundaries
}

fn set_collapsed(session: &mut DocumentSession, node: NodeId, raw: usize) {
    let caret = point(session.document(), node, raw);
    session
        .apply_intent(&EditIntent::SetSelection {
            anchor: caret,
            focus: caret,
        })
        .unwrap();
}

fn randomized_fixture() -> XiaomuDocument {
    let mut builder = NodeStoreBuilder::new();
    let first = paragraph(&mut builder, "a中👍e\u{301}");
    let second = paragraph(&mut builder, "אבג mixed 文");
    let third = paragraph(&mut builder, "مرحبا 🚀 tail");
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, second, third]),
        )
        .unwrap();
    XiaomuDocument::new(root, builder.finish()).unwrap()
}

#[test]
fn randomized_unicode_history_and_mapping_sequences_round_trip_exact_store() {
    for seed in [0x5033_7001_u64, 7, 42, 0xC0FFEE] {
        run_randomized_sequence(seed);
    }
}

fn run_randomized_sequence(seed: u64) {
    let initial = randomized_fixture();
    initial.validate().unwrap();
    let first = inline_nodes(&initial)[0];
    let caret = point(&initial, first, 0);
    let mut session = DocumentSession::new(initial.clone(), DocumentSelection::collapsed(caret)).unwrap();
    let mut rng = Rng(seed | 1);
    let inserts = ["x", "中", "👍", "e\u{301}", "אב", "م"];

    for _ in 0..96 {
        let nodes = inline_nodes(session.document());
        assert!(!nodes.is_empty(), "document must retain an inline block for seed {seed}");

        if nodes.len() > 1 && rng.below(7) == 0 {
            let left_index = rng.below(nodes.len() - 1);
            let left = nodes[left_index];
            let right = nodes[left_index + 1];
            let left_boundaries = scalar_boundaries(session.document(), left);
            let right_boundaries = scalar_boundaries(session.document(), right);
            let anchor = point(
                session.document(),
                left,
                left_boundaries[rng.below(left_boundaries.len())],
            );
            let focus = point(
                session.document(),
                right,
                right_boundaries[rng.below(right_boundaries.len())],
            );
            session
                .apply_intent(&EditIntent::SetSelection { anchor, focus })
                .unwrap();
            let before_store = session.document().store().clone();
            let before_history = session.history_depths();
            match session.apply_intent(&EditIntent::Delete) {
                Ok(SessionOutcome::DocumentChanged) => {}
                Ok(SessionOutcome::NoChange | SessionOutcome::SelectionChanged) => {}
                Err(_) => {
                    assert_eq!(session.document().store(), &before_store);
                    assert_eq!(session.history_depths(), before_history);
                }
            }
        } else {
            let node = nodes[rng.below(nodes.len())];
            let boundaries = scalar_boundaries(session.document(), node);
            set_collapsed(
                &mut session,
                node,
                boundaries[rng.below(boundaries.len())],
            );

            let intent = match rng.below(6) {
                0 => EditIntent::InsertText {
                    text: inserts[rng.below(inserts.len())].to_owned(),
                },
                1 => EditIntent::Backspace,
                2 => EditIntent::Delete,
                3 => EditIntent::SplitBlock,
                4 => EditIntent::JoinWithPrevious,
                _ => EditIntent::PasteText {
                    text: inserts[rng.below(inserts.len())].to_owned(),
                },
            };
            let before_store = session.document().store().clone();
            let before_history = session.history_depths();
            if session.apply_intent(&intent).is_err() {
                assert_eq!(session.document().store(), &before_store);
                assert_eq!(session.history_depths(), before_history);
            }
        }

        session.document().validate().unwrap();
        session.selection().validate(session.document()).unwrap();
    }

    let final_store = session.document().store().clone();
    while session.history_depths().0 > 0 {
        assert_eq!(session.undo().unwrap(), SessionOutcome::DocumentChanged);
        session.document().validate().unwrap();
        session.selection().validate(session.document()).unwrap();
    }
    assert_eq!(
        session.document().store(),
        initial.store(),
        "undo chain must restore the exact initial store for seed {seed}"
    );

    while session.history_depths().1 > 0 {
        assert_eq!(session.redo().unwrap(), SessionOutcome::DocumentChanged);
        session.document().validate().unwrap();
        session.selection().validate(session.document()).unwrap();
    }
    assert_eq!(
        session.document().store(),
        &final_store,
        "redo chain must restore the exact final store for seed {seed}"
    );
}
