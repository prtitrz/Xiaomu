//! P4.5 Inline Atom Integration Gate (P4A closeout), GPUI end-to-end leg.
//!
//! Real windows, real keystrokes, and a renderer display text that differs
//! from the canonical bytes (CJK chip between CJK scalars): navigation steps
//! one caret unit per key, CJK typing in a mixed block keeps the chip
//! anchored, and atom edits stay isolated across editors. The session-level
//! seam matrix lives in `xiaomu-runtime/tests/p4a_integration_gate.rs`.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{AppContext as _, TestAppContext};
use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, TextPoint};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_gpui::atom_capability::{AtomAction, InlineAtomHostCapability};
use xiaomu_gpui::block_view::SharedSession;
use xiaomu_gpui::document_view::DocumentView;
use xiaomu_gpui::editor::{EditorHooks, EditorInstance, bind_default_editor_keys};
use xiaomu_runtime::session::{DocumentPosition, DocumentSelection};

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

fn insert_atom(
    document: XiaomuDocument,
    node: NodeId,
    raw: usize,
    ordinal: usize,
    kind: &str,
    fallback: &str,
) -> XiaomuDocument {
    let inline = document.node(node).unwrap().content().as_inline().unwrap();
    let at = InlinePoint::new(
        node,
        inline.offset_at(raw).unwrap(),
        ordinal,
        CursorAffinity::Before,
    );
    Transaction::new(TransactionOrigin::Extension("p4a-gate".into()))
        .with_step(TransactionStep::InsertInlineAtom {
            at,
            kind: AtomKind::new(kind).unwrap(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::new(fallback).unwrap(),
        })
        .apply(&document)
        .unwrap()
}

fn focus(session: &SharedSession) -> (NodeId, usize, usize) {
    match session.borrow().selection().focus() {
        DocumentPosition::Inline(point) => (
            point.node_id(),
            point.text_offset().as_usize(),
            point.atom_index(),
        ),
        DocumentPosition::Gap(_) | DocumentPosition::Atomic(_) => {
            panic!("mixed-inline caret must stay inline")
        }
    }
}

fn expect_focus(session: &SharedSession, node: NodeId, raw: usize, ordinal: usize, context: &str) {
    assert_eq!(focus(session), (node, raw, ordinal), "{context}");
}

fn place(
    window: &gpui::WindowHandle<DocumentView>,
    cx: &mut TestAppContext,
    session: &SharedSession,
    at: InlinePoint,
) {
    session.borrow_mut().set_inline_selection(at, at).unwrap();
    window
        .update(cx, |view: &mut DocumentView, window, cx| {
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
}

fn step(window: &gpui::WindowHandle<DocumentView>, cx: &mut TestAppContext, key: &str) {
    cx.simulate_keystrokes((*window).into(), key);
    cx.background_executor.run_until_parked();
}

fn text_of(session: &SharedSession, node: NodeId) -> String {
    session
        .borrow()
        .document()
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

fn atoms_of(session: &SharedSession, node: NodeId) -> Vec<usize> {
    session
        .borrow()
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .atoms()
        .iter()
        .map(|placement| placement.text_offset().as_usize())
        .collect()
}

fn open(editor: EditorInstance, cx: &mut TestAppContext) -> gpui::WindowHandle<DocumentView> {
    cx.update(bind_default_editor_keys);
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    window
        .update(cx, |view: &mut DocumentView, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    window
}

#[gpui::test]
fn cjk_mixed_block_walks_chips_and_types_with_real_keystrokes(cx: &mut TestAppContext) {
    let mut builder = NodeStoreBuilder::new();
    let mixed = paragraph(&mut builder, "你好B");
    let tail = paragraph(&mut builder, "尾");
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([mixed, tail]),
        )
        .unwrap();
    // The chip's display text is CJK and differs from the canonical bytes,
    // so layout really runs on the display projection.
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let document = insert_atom(document, mixed, 3, 0, "mention", "«@小沐»");

    let start = TextPoint::new(
        mixed,
        document
            .node(mixed)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(0)
            .unwrap(),
        CursorAffinity::Before,
    );
    let editor = EditorInstance::new(
        document,
        DocumentSelection::collapsed(start),
        EditorHooks::default(),
    )
    .unwrap();
    let session = editor.session().clone();
    let window = open(editor, cx);

    // 你 | chip | 好 | B: one caret unit per keystroke.
    step(&window, cx, "right");
    expect_focus(&session, mixed, 3, 0, "after 你, before the chip");
    step(&window, cx, "right");
    expect_focus(&session, mixed, 3, 1, "across the CJK chip");
    step(&window, cx, "right");
    expect_focus(&session, mixed, 6, 0, "after 好");
    step(&window, cx, "right");
    expect_focus(&session, mixed, 7, 0, "after B");
    for (raw, ordinal) in [(6, 0), (3, 1), (3, 0), (0, 0)] {
        step(&window, cx, "left");
        expect_focus(&session, mixed, raw, ordinal, "backward re-walk");
    }

    // CJK typing in the mixed block keeps the chip anchored at 3.
    place(
        &window,
        cx,
        &session,
        InlinePoint::new(
            mixed,
            text_offset(&session, mixed, 6),
            0,
            CursorAffinity::Before,
        ),
    );
    cx.simulate_input(window.into(), "界");
    cx.background_executor.run_until_parked();
    assert_eq!(text_of(&session, mixed), "你好界B");
    assert_eq!(atoms_of(&session, mixed), vec![3]);
}

fn text_offset(
    session: &SharedSession,
    node: NodeId,
    byte: usize,
) -> xiaomu_core::text::TextOffset {
    session
        .borrow()
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(byte)
        .unwrap()
}

/// Records host activations so tests can prove typing is not one.
#[derive(Default)]
struct GateRecorder {
    actions: RefCell<Vec<AtomAction>>,
}

impl InlineAtomHostCapability for GateRecorder {
    fn atom_action(&self, action: AtomAction) {
        self.actions.borrow_mut().push(action);
    }
}

#[gpui::test]
fn atom_edits_stay_isolated_across_two_editors(cx: &mut TestAppContext) {
    // Editor A owns a mixed block; editor B owns a plain document.
    let mut builder = NodeStoreBuilder::new();
    let mixed = paragraph(&mut builder, "尾");
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([mixed]),
        )
        .unwrap();
    let document_a = XiaomuDocument::new(root, builder.finish()).unwrap();
    let document_a = insert_atom(document_a, mixed, 0, 0, "mention", "@xiaomu");

    let mut builder = NodeStoreBuilder::new();
    let plain = paragraph(&mut builder, "plain");
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([plain]),
        )
        .unwrap();
    let document_b = XiaomuDocument::new(root, builder.finish()).unwrap();

    let recorder = Rc::new(GateRecorder::default());
    let editor_a = EditorInstance::new(
        document_a,
        DocumentSelection::collapsed(TextPoint::new(
            mixed,
            xiaomu_core::text::TextBuffer::from_string("尾".to_owned())
                .offset_at(3)
                .unwrap(),
            CursorAffinity::Before,
        )),
        EditorHooks {
            persistence: None,
            listener: None,
            atom_renderers: None,
            atom_capability: Some(recorder.clone()),
            asset_service: None,
        },
    )
    .unwrap();
    let session_a = editor_a.session().clone();
    let editor_b = EditorInstance::new(
        document_b,
        DocumentSelection::collapsed(TextPoint::new(
            plain,
            xiaomu_core::text::TextBuffer::from_string("plain".to_owned())
                .offset_at(5)
                .unwrap(),
            CursorAffinity::Before,
        )),
        EditorHooks::default(),
    )
    .unwrap();
    let session_b = editor_b.session().clone();

    let window_a = open(editor_a, cx);
    cx.simulate_input(window_a.into(), "你");
    cx.background_executor.run_until_parked();

    assert_eq!(text_of(&session_a, mixed), "尾你");
    assert_eq!(atoms_of(&session_a, mixed), vec![0]);
    assert!(
        recorder.actions.borrow().is_empty(),
        "typing never activates"
    );

    // Editor B is untouched: its selection and text stay exactly as built.
    assert_eq!(text_of(&session_b, plain), "plain");
    match session_b.borrow().selection().focus() {
        DocumentPosition::Inline(point) => {
            assert_eq!(point.node_id(), plain);
            assert_eq!(point.text_offset().as_usize(), 5);
        }
        DocumentPosition::Gap(_) | DocumentPosition::Atomic(_) => {
            panic!("B keeps its inline caret")
        }
    }
}
