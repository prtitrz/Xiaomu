//! Mixed-inline keyboard navigation: seams are first-class caret positions.
//!
//! Blocks carrying inline atoms shape their layout on renderer display bytes.
//! These tests drive real keystrokes through the bound navigation actions and
//! assert that chip interiors are crossed as one caret unit, end-anchored
//! seam ordinals survive Home/End, and vertical moves cross block boundaries
//! in both directions.

use gpui::{AppContext as _, TestAppContext};
use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, TextPoint};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
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
    Transaction::new(TransactionOrigin::Extension(
        "mixed-inline-navigation-test".into(),
    ))
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
        DocumentPosition::Gap(_) => panic!("mixed-inline navigation must keep an inline focus"),
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

#[gpui::test]
fn mixed_inline_navigation_crosses_chips_and_keeps_seam_ordinals(cx: &mut TestAppContext) {
    let mut builder = NodeStoreBuilder::new();
    let mixed = paragraph(&mut builder, "AB");
    let tail = paragraph(&mut builder, "tail");
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([mixed, tail]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let document = insert_atom(document, mixed, 1, 0, "mention", "@alice");
    let document = insert_atom(document, mixed, 1, 1, "reference", "#42");
    let document = insert_atom(document, mixed, 2, 0, "cursor", "\u{25b8}");

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
        document.clone(),
        DocumentSelection::collapsed(start),
        EditorHooks::default(),
    )
    .unwrap();
    let session = editor.session().clone();

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

    let seam = |raw: usize, ordinal: usize| {
        InlinePoint::new(
            mixed,
            document
                .node(mixed)
                .unwrap()
                .content()
                .as_inline()
                .unwrap()
                .offset_at(raw)
                .unwrap(),
            ordinal,
            CursorAffinity::Before,
        )
    };

    // Right crosses each chip as one caret unit, ordinals advancing.
    place(&window, cx, &session, seam(1, 0));
    step(&window, cx, "right");
    expect_focus(&session, mixed, 1, 1, "right from before both chips");
    step(&window, cx, "right");
    expect_focus(&session, mixed, 1, 2, "right past the second chip");
    step(&window, cx, "right");
    expect_focus(&session, mixed, 2, 1, "right past the end-anchored chip");

    // Left skips whole chips too, walking the seams back down.
    place(&window, cx, &session, seam(2, 1));
    step(&window, cx, "left");
    expect_focus(&session, mixed, 2, 0, "left re-enters before the end chip");
    step(&window, cx, "left");
    expect_focus(&session, mixed, 1, 2, "left lands between B and the chips");
    step(&window, cx, "left");
    expect_focus(&session, mixed, 1, 1, "left re-enters the chip seam");
    step(&window, cx, "left");
    expect_focus(&session, mixed, 1, 0, "left reaches before both chips");
    step(&window, cx, "left");
    expect_focus(&session, mixed, 0, 0, "left skips both chips as units");

    // Right from the block end crosses into the next block.
    place(&window, cx, &session, seam(2, 1));
    step(&window, cx, "right");
    expect_focus(&session, tail, 0, 0, "right from the end enters the tail");

    // Home/End keep end-anchored seam ordinals exact.
    place(&window, cx, &session, seam(0, 0));
    step(&window, cx, "end");
    expect_focus(
        &session,
        mixed,
        2,
        1,
        "end lands after the end-anchored chip",
    );
    step(&window, cx, "home");
    expect_focus(&session, mixed, 0, 0, "home returns to the block start");

    // Vertical moves cross the block boundary in both directions.
    place(&window, cx, &session, seam(0, 0));
    step(&window, cx, "down");
    expect_focus(&session, tail, 0, 0, "down moves to the next block start");
    step(&window, cx, "up");
    expect_focus(&session, mixed, 0, 0, "up returns to the mixed block start");
}
