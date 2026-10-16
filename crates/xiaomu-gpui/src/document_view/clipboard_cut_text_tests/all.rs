//! Explicit All through real Select All/Cut keys; no production key semantics change.

#[path = "all/paragraph_attrs.rs"]
mod paragraph_attrs;
#[path = "all/rejections.rs"]
mod rejections;

use super::*;
use crate::editor_commands::{
    CommandRoute, EditorCommand, EditorCommandContext, EditorCommandRouter,
};
use xiaomu_core::document::HeadingLevel;
use xiaomu_runtime::clipboard::{ClipboardNode, ClipboardSourceBoundary};

struct AllRouter;
impl EditorCommandRouter for AllRouter {
    fn route(
        &self,
        _: EditorCommandContext<'_>,
        _: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError> {
        Ok(CommandRoute::Default)
    }
    fn select_all(
        &self,
        context: EditorCommandContext<'_>,
    ) -> Result<Option<DocumentSelection>, PolicyError> {
        Ok(Some(DocumentSelection::all(context.document())))
    }
}

fn attrs(label: &str) -> NodeAttrs {
    NodeAttrs::new(
        [
            ("label".into(), AttrValue::String(label.into())),
            ("nullable".into(), AttrValue::Null),
        ]
        .into(),
    )
    .unwrap()
}

fn rich_fixture(barrier: Option<NodeKind>, depth: usize) -> Fixture {
    let mut b = NodeStoreBuilder::new();
    let mut payload = AttrValue::String("opaque Unicode 内容".into());
    for _ in 0..depth {
        payload = AttrValue::List(vec![payload]);
    }
    let atom = b
        .insert(
            NodeKind::InlineAtom(AtomKind::new("example.token.v1").unwrap()),
            NodeAttrs::new([("payload".into(), payload)].into()).unwrap(),
            NodeContent::InlineAtom(
                InlineAtomContent::new("token")
                    .unwrap()
                    .with_marks(MarkSet::new([Mark::Strike]).unwrap()),
            ),
        )
        .unwrap();
    let hard_break = b
        .insert(
            NodeKind::InlineAtom(AtomKind::hard_break()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(
                InlineAtomContent::hard_break()
                    .with_marks(MarkSet::new([Mark::Underline]).unwrap()),
            ),
        )
        .unwrap();
    let text =
        InlineContent::new([
            TextRun::new("中🙂e\u{301}", MarkSet::new([Mark::Bold]).unwrap()).unwrap(),
        ])
        .unwrap();
    let head = b
        .insert(
            NodeKind::Heading(HeadingLevel::new(2).unwrap()),
            attrs("heading"),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    text.runs().iter().cloned(),
                    [
                        InlineAtomPlacement::new(atom, TextOffset::ZERO),
                        InlineAtomPlacement::new(
                            hard_break,
                            text.offset_at(text.len_bytes()).unwrap(),
                        ),
                    ],
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let mut quoted = vec![paragraph(&mut b, "quoted🙂")];
    if let Some(kind) = barrier {
        let content = match kind {
            NodeKind::Table => {
                let text = paragraph(&mut b, "cell");
                let cell = b
                    .insert(
                        NodeKind::TableCell,
                        NodeAttrs::empty(),
                        NodeContent::children([text]),
                    )
                    .unwrap();
                let row = b
                    .insert(
                        NodeKind::TableRow,
                        NodeAttrs::empty(),
                        NodeContent::children([cell]),
                    )
                    .unwrap();
                NodeContent::children([row])
            }
            NodeKind::Quote | NodeKind::BulletList => NodeContent::children([]),
            _ => NodeContent::Atomic,
        };
        quoted.push(b.insert(kind, NodeAttrs::empty(), content).unwrap());
    }
    let quote = b
        .insert(
            NodeKind::Quote,
            attrs("quote"),
            NodeContent::children(quoted),
        )
        .unwrap();
    let tail = paragraph(&mut b, "qéw");
    let item = b
        .insert(
            NodeKind::ListItem,
            attrs("item"),
            NodeContent::children([tail]),
        )
        .unwrap();
    let list = b
        .insert(
            NodeKind::OrderedList,
            attrs("list"),
            NodeContent::children([item]),
        )
        .unwrap();
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([head, quote, list]),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, b.finish()).unwrap(),
        head,
        tail,
        atom,
    }
}

fn key(m: &Mounted, cx: &mut TestAppContext, key: &str) {
    cx.simulate_keystrokes(m.window.into(), key);
    cx.background_executor.run_until_parked();
}

fn mount_all(cx: &mut TestAppContext, f: &Fixture, reject: bool, legacy: bool) -> Mounted {
    let m = mount(
        cx,
        f,
        DocumentSelection::collapsed(point(f, f.head, 0)),
        reject,
        legacy,
    );
    m.window
        .update(cx, |view, _, _| {
            view.set_command_router(Some(Rc::new(AllRouter)));
        })
        .unwrap();
    let before = Snapshot::capture(&m);
    key(&m, cx, "ctrl-a");
    assert_eq!(
        m.session.borrow().selection(),
        DocumentSelection::all(&f.document)
    );
    assert_eq!(
        m.session.borrow().document().store(),
        before.document.store()
    );
    assert_eq!(
        m.session.borrow().document().revision(),
        before.document.revision()
    );
    assert_eq!(m.session.borrow().history_depths(), before.history);
    assert_eq!(m.calls.borrow().selections, 1);
    assert!(m.calls.borrow().publications.is_empty());
    *m.calls.borrow_mut() = Calls::default();
    m
}

fn assert_original_node(document: &XiaomuDocument, id: NodeId, detached: &ClipboardNode) {
    let original = document.node(id).unwrap();
    assert_eq!(detached.kind(), original.kind());
    assert_eq!(detached.attrs(), original.attrs());
    match original.content() {
        NodeContent::Inline(inline) => {
            let copy = detached.content().as_inline().unwrap();
            assert_eq!(copy.runs(), inline.runs());
            assert_eq!(copy.atoms().len(), inline.atoms().len());
            for (copy, placement) in copy.atoms().iter().zip(inline.atoms()) {
                let atom = document.node(placement.atom()).unwrap();
                assert_eq!(copy.anchor(), placement.text_offset());
                assert_eq!(&NodeKind::InlineAtom(copy.kind().clone()), atom.kind());
                assert_eq!(copy.attrs(), atom.attrs());
                assert_eq!(Some(copy.content()), atom.content().as_inline_atom());
            }
        }
        NodeContent::Children(children) => {
            let copies = detached.content().as_children().unwrap();
            assert_eq!(copies.len(), children.len());
            for (copy, id) in copies.iter().zip(children) {
                assert_original_node(document, *id, copy);
            }
        }
        _ => panic!("success fixture has only text and wrapper nodes"),
    }
}

#[gpui::test]
fn all_text_cut_keys_preserve_closed_rich_source_and_replace_every_shell(cx: &mut TestAppContext) {
    let f = rich_fixture(None, 1);
    let m = mount_all(cx, &f, false, false);
    let before = Snapshot::capture(&m);
    previous(cx);
    key(&m, cx, "ctrl-x");
    let item = clipboard(cx);
    assert_eq!(
        item.text().as_deref(),
        Some("\n中🙂e\u{301}\n\nquoted🙂\nqéw")
    );
    let slice = decode_metadata(&item.text().unwrap(), item.metadata().unwrap()).unwrap();
    assert_eq!(prepare_lossless_slice(&slice).unwrap(), item);
    assert!(slice.is_closed());
    assert_eq!(
        slice.source_boundary(),
        Some(ClipboardSourceBoundary::WholeRoots)
    );
    assert_eq!(slice.roots().len(), 3);
    for (copy, id) in slice.roots().iter().zip(
        f.document
            .node(f.document.root())
            .unwrap()
            .content()
            .as_children()
            .unwrap(),
    ) {
        assert_original_node(&f.document, *id, copy);
    }
    assert_eq!(write_observation::calls(), 1);
    assert_eq!(m.calls.borrow().candidates, [0]);
    assert_eq!(m.calls.borrow().publications, [1]);
    assert_eq!(
        m.calls.borrow().sources,
        [before.selection, before.selection]
    );
    assert_eq!(m.calls.borrow().gates, 1);
    assert_eq!(m.calls.borrow().export_specs, 1);
    assert_eq!(m.calls.borrow().deletes, 0);
    assert_eq!(
        m.calls.borrow().selections,
        0,
        "no normalization before publication"
    );
    let after = Snapshot::capture(&m);
    assert_eq!(after.history, (1, 0));
    assert_eq!(after.marks, None);
    assert_eq!(after.document.root(), f.document.root());
    assert_eq!(after.document.node_count(), 2);
    let roots = after
        .document
        .node(after.document.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap();
    assert_eq!(roots.len(), 1);
    let paragraph = after.document.node(roots[0]).unwrap();
    assert_eq!(paragraph.kind(), &NodeKind::Paragraph);
    assert_eq!(paragraph.attrs(), &NodeAttrs::empty());
    assert_eq!(paragraph.content(), &NodeContent::empty_inline());
    assert_eq!(
        after.selection,
        DocumentSelection::collapsed(InlinePoint::at_start_of(roots[0]))
    );
    for original in before.document.store().iter() {
        if original.id() != before.document.root() {
            assert!(
                after.document.node(original.id()).is_none(),
                "no original shell or atom survives"
            );
        }
    }
    key(&m, cx, "ctrl-z");
    assert_eq!(
        m.session.borrow().document().store(),
        before.document.store()
    );
    assert_eq!(m.session.borrow().selection(), before.selection);
    assert_eq!(m.session.borrow().history_depths(), (0, 1));
    key(&m, cx, "ctrl-shift-z");
    assert_eq!(
        m.session.borrow().document().store(),
        after.document.store()
    );
    assert_eq!(m.session.borrow().selection(), after.selection);
    assert_eq!(m.session.borrow().history_depths(), (1, 0));
    assert_eq!(clipboard(cx), item);
    assert_eq!(
        write_observation::calls(),
        1,
        "history never rewrites the clipboard"
    );
}

#[gpui::test]
fn all_text_cut_empty_paragraph_is_closed_structure_with_one_undo_unit(cx: &mut TestAppContext) {
    let mut b = NodeStoreBuilder::new();
    let head = b
        .insert(
            NodeKind::Paragraph,
            attrs("empty source"),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([head]),
        )
        .unwrap();
    let f = Fixture {
        document: XiaomuDocument::new(root, b.finish()).unwrap(),
        head,
        tail: head,
        atom: head,
    };
    let m = mount_all(cx, &f, false, false);
    let before = Snapshot::capture(&m);
    previous(cx);
    key(&m, cx, "ctrl-x");
    let item = clipboard(cx);
    assert_eq!(item.text().as_deref(), Some(""));
    let slice = decode_metadata("", item.metadata().unwrap()).unwrap();
    assert!(slice.is_closed());
    assert_eq!(
        slice.source_boundary(),
        Some(ClipboardSourceBoundary::WholeRoots)
    );
    assert_eq!(slice.roots().len(), 1);
    assert_original_node(&f.document, f.head, &slice.roots()[0]);
    assert_eq!(prepare_lossless_slice(&slice).unwrap(), item);
    let after = Snapshot::capture(&m);
    assert_eq!(after.history, (1, 0));
    assert_eq!(after.document.node_count(), 2);
    assert!(after.document.node(f.head).is_none());
    let (start, end) = after.selection.as_same_node_inline().unwrap();
    assert_eq!(start, end);
    assert_eq!(start, InlinePoint::at_start_of(start.node_id()));
    let replacement = after.document.node(start.node_id()).unwrap();
    assert_eq!(replacement.kind(), &NodeKind::Paragraph);
    assert_eq!(replacement.attrs(), &NodeAttrs::empty());
    assert_eq!(replacement.content(), &NodeContent::empty_inline());
    assert_eq!(m.calls.borrow().candidates, [0]);
    assert_eq!(m.calls.borrow().publications, [1]);
    key(&m, cx, "ctrl-z");
    assert_eq!(
        m.session.borrow().document().store(),
        before.document.store()
    );
    assert_eq!(m.session.borrow().selection(), before.selection);
    key(&m, cx, "ctrl-shift-z");
    assert_eq!(
        m.session.borrow().document().store(),
        after.document.store()
    );
    assert_eq!(m.session.borrow().selection(), after.selection);
    assert_eq!(clipboard(cx), item);
    assert_eq!(write_observation::calls(), 1);
}
