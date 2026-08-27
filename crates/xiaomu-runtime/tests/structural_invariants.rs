//! Session-level random structural invariants (P2.7 closeout 3.2).

use xiaomu_core::document::{
    HeadingLevel, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint, TextSelection};
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentSelection, DocumentSession, EditIntent, SessionOutcome,
};

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

#[derive(Default)]
struct Counters {
    documents: u64,
    selections: u64,
}

struct CountingListener(std::rc::Rc<std::cell::RefCell<Counters>>);

impl DocumentChangeListener for CountingListener {
    fn document_changed(&mut self, _document: &XiaomuDocument, _selection: DocumentSelection) {
        self.0.borrow_mut().documents += 1;
    }

    fn selection_changed(&mut self, _selection: DocumentSelection) {
        self.0.borrow_mut().selections += 1;
    }
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

fn item(builder: &mut NodeStoreBuilder, block: NodeId) -> NodeId {
    builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([block]),
        )
        .unwrap()
}

fn legal_fixture() -> XiaomuDocument {
    let mut builder = NodeStoreBuilder::new();
    let heading = builder
        .insert(
            NodeKind::Heading(HeadingLevel::new(2).unwrap()),
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("Title", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let intro = paragraph(&mut builder, "hello world");
    let quoted = paragraph(&mut builder, "quoted");
    let quote = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([quoted]),
        )
        .unwrap();
    let a = paragraph(&mut builder, "alpha");
    let b = paragraph(&mut builder, "beta");
    let nested = paragraph(&mut builder, "nested");
    let nested_item = item(&mut builder, nested);
    let nested_list = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([nested_item]),
        )
        .unwrap();
    let item_b = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([b, nested_list]),
        )
        .unwrap();
    let item_a = item(&mut builder, a);
    let ul = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([item_a, item_b]),
        )
        .unwrap();
    let one = paragraph(&mut builder, "one");
    let two = paragraph(&mut builder, "two");
    let item_one = item(&mut builder, one);
    let item_two = item(&mut builder, two);
    let ol = builder
        .insert(
            NodeKind::OrderedList,
            NodeAttrs::empty(),
            NodeContent::children([item_one, item_two]),
        )
        .unwrap();
    let tail = paragraph(&mut builder, "tail");
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([heading, intro, quote, ul, ol, tail]),
        )
        .unwrap();
    XiaomuDocument::new(root, builder.finish()).unwrap()
}

fn inline_nodes(document: &XiaomuDocument) -> Vec<NodeId> {
    let mut nodes = Vec::new();
    fn walk(document: &XiaomuDocument, id: NodeId, nodes: &mut Vec<NodeId>) {
        let Some(node) = document.node(id) else {
            return;
        };
        match node.content() {
            NodeContent::Inline(_) => nodes.push(id),
            NodeContent::Children(children) => {
                for child in children {
                    walk(document, *child, nodes);
                }
            }
            _ => {}
        }
    }
    walk(document, document.root(), &mut nodes);
    nodes
}

fn boundaries(document: &XiaomuDocument, node: NodeId) -> Vec<usize> {
    let inline = document.node(node).unwrap().content().as_inline().unwrap();
    let text: String = inline
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect();
    let mut offsets = vec![0];
    for (index, character) in text.char_indices() {
        offsets.push(index + character.len_utf8());
    }
    offsets.sort_unstable();
    offsets.dedup();
    offsets
}

fn place_on(session: &mut DocumentSession, node: NodeId, raw: usize) {
    let inline = session
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    let offset = inline.offset_at(raw).unwrap();
    let point = TextPoint::new(node, offset, CursorAffinity::Before);
    let _ = session.apply_intent(&EditIntent::SetSelection {
        anchor: point,
        focus: point,
    });
}

fn random_intent(rng: &mut Rng) -> EditIntent {
    match rng.below(8) {
        0 => EditIntent::SplitBlock,
        1 => EditIntent::JoinWithPrevious,
        2 => EditIntent::TurnInto {
            kind: NodeKind::BulletList,
        },
        3 => EditIntent::TurnInto {
            kind: NodeKind::OrderedList,
        },
        4 => EditIntent::TurnInto {
            kind: NodeKind::Paragraph,
        },
        5 => EditIntent::TurnInto {
            kind: NodeKind::Heading(HeadingLevel::new(2).unwrap()),
        },
        6 => EditIntent::IndentListItem,
        _ => EditIntent::OutdentListItem,
    }
}

#[test]
fn random_structural_sequences_preserve_invariants() {
    for seed in [0xC0FFEE_u64, 1, 42, 99, 7] {
        run_sequence(seed);
    }
}

fn run_sequence(seed: u64) {
    let initial = legal_fixture();
    initial.validate().unwrap();
    let first = inline_nodes(&initial)[0];
    let selection = TextSelection::collapsed(TextPoint::new(
        first,
        initial
            .node(first)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(0)
            .unwrap(),
        CursorAffinity::Before,
    ));
    let counters = std::rc::Rc::new(std::cell::RefCell::new(Counters::default()));
    let mut session =
        DocumentSession::new(initial.clone(), DocumentSelection::text(selection)).unwrap();
    session.add_listener(Box::new(CountingListener(counters.clone())));

    let mut rng = Rng(seed | 1);
    let mut changed = 0usize;

    for _ in 0..48 {
        let nodes = inline_nodes(session.document());
        if nodes.is_empty() {
            break;
        }
        let node = nodes[rng.below(nodes.len())];
        let offsets = boundaries(session.document(), node);
        let raw = offsets[rng.below(offsets.len())];
        place_on(&mut session, node, raw);

        let intent = random_intent(&mut rng);
        let revision = session.document().revision();
        let history = session.history_depths();
        let docs = counters.borrow().documents;
        let sels = counters.borrow().selections;

        match session.apply_intent(&intent) {
            Ok(SessionOutcome::NoChange) => {
                assert_eq!(session.document().revision(), revision);
                assert_eq!(session.history_depths(), history);
                assert_eq!(counters.borrow().documents, docs);
                assert_eq!(counters.borrow().selections, sels);
            }
            Ok(SessionOutcome::DocumentChanged) => {
                session.document().validate().unwrap();
                session.selection().validate(session.document()).unwrap();
                changed += 1;
            }
            Ok(SessionOutcome::SelectionChanged) => {
                session.selection().validate(session.document()).unwrap();
            }
            Err(_) => {
                assert_eq!(session.document().revision(), revision);
                assert_eq!(session.history_depths(), history);
            }
        }
    }

    let after_chain = session.document().clone();
    let after_selection = session.selection();
    while session.history_depths().0 > 0 {
        session.undo().unwrap();
        session.document().validate().unwrap();
        session.selection().validate(session.document()).unwrap();
    }
    assert_eq!(
        session.document().store(),
        initial.store(),
        "undo chain must restore identities for seed {seed}"
    );

    for _ in 0..changed {
        session.redo().unwrap();
        session.document().validate().unwrap();
        session.selection().validate(session.document()).unwrap();
    }
    assert_eq!(session.document().store(), after_chain.store());
    // Redo restores each command's recorded after-selection, which remains
    // valid; later caret placements are not history entries.
    session.selection().validate(session.document()).unwrap();
    let _ = after_selection;
}
