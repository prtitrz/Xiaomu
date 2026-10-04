//! Real Select All and clipboard actions retain trailing mixed-inline atoms.

use gpui::{AppContext as _, TestAppContext, WindowHandle};
use xiaomu_core::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark, MarkSet,
    NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::TextBuffer;
use xiaomu_runtime::clipboard::ClipboardSlice;
use xiaomu_runtime::session::{DocumentSelection, DocumentSession};

use super::DocumentView;
use crate::block_view::SharedSession;
use crate::editor::{EditorHooks, EditorInstance, bind_default_editor_keys};
use crate::input::platform_clipboard::{PlatformClipboard, PlatformClipboardContent};

fn document(
    text: &str,
    specs: &[(usize, AtomKind, Mark)],
) -> (XiaomuDocument, NodeId, Vec<NodeId>) {
    let mut builder = NodeStoreBuilder::new();
    let buffer = TextBuffer::from_string(text.to_owned());
    let mut atoms = Vec::new();
    let mut placements = Vec::new();
    for (raw, kind, mark) in specs {
        let (attrs, content) = if kind.is_hard_break() {
            (NodeAttrs::empty(), InlineAtomContent::hard_break())
        } else {
            (
                NodeAttrs::new([("reference".into(), AttrValue::String("原值🙂".into()))].into())
                    .unwrap(),
                InlineAtomContent::new("[ref🙂]").unwrap(),
            )
        };
        let atom = builder
            .insert(
                NodeKind::InlineAtom(kind.clone()),
                attrs,
                NodeContent::InlineAtom(content.with_marks(MarkSet::new([mark.clone()]).unwrap())),
            )
            .unwrap();
        atoms.push(atom);
        placements.push(InlineAtomPlacement::new(
            atom,
            buffer.offset_at(*raw).unwrap(),
        ));
    }
    let runs = if text.is_empty() {
        vec![]
    } else {
        vec![TextRun::new(text, MarkSet::new([Mark::Underline]).unwrap()).unwrap()]
    };
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::with_atoms(runs, placements).unwrap()),
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
        atoms,
    )
}

fn point(document: &XiaomuDocument, node: NodeId, raw: usize, ordinal: usize) -> InlinePoint {
    let inline = document.node(node).unwrap().content().as_inline().unwrap();
    InlinePoint::new(
        node,
        inline.offset_at(raw).unwrap(),
        ordinal,
        CursorAffinity::Before,
    )
}

fn open(
    cx: &mut TestAppContext,
    document: XiaomuDocument,
    node: NodeId,
) -> (WindowHandle<DocumentView>, SharedSession) {
    let selection = DocumentSelection::collapsed(point(&document, node, 0, 0));
    let editor = EditorInstance::new(document, selection, EditorHooks::default()).unwrap();
    let session = editor.session().clone();
    cx.update(bind_default_editor_keys);
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    handle
        .update(cx, |view: &mut DocumentView, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    (handle, session)
}

fn step(cx: &mut TestAppContext, handle: WindowHandle<DocumentView>, key: &str) {
    cx.simulate_keystrokes(handle.into(), key);
    cx.background_executor.run_until_parked();
}

fn clipboard(cx: &mut TestAppContext) -> ClipboardSlice {
    cx.update(|cx| {
        let Some(PlatformClipboardContent::Structured(slice)) =
            PlatformClipboard::new(cx).read_content()
        else {
            panic!("copy/cut must retain complete structured atom metadata");
        };
        slice
    })
}

fn assert_select_all_copy_cut_undo(
    cx: &mut TestAppContext,
    text: &str,
    specs: &[(usize, AtomKind, Mark)],
    trailing_atoms: usize,
    expected_plain_text: &str,
) {
    let (document, node, atoms) = document(text, specs);
    let expected_selection = DocumentSelection::new(
        point(&document, node, 0, 0),
        point(&document, node, text.len(), trailing_atoms),
    );
    let expected_slice = DocumentSession::new(document.clone(), expected_selection)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap();
    assert_eq!(expected_slice.plain_text(), expected_plain_text);
    let (handle, session) = open(cx, document.clone(), node);

    step(cx, handle, "ctrl-a");
    {
        let session = session.borrow();
        assert!(!session.selection().is_collapsed());
        assert_eq!(session.selection(), expected_selection);
        let (anchor, focus) = session.selection().as_same_node_inline().unwrap();
        assert_eq!(anchor.atom_index(), 0);
        assert_eq!(focus.atom_index(), trailing_atoms);
        assert_eq!(session.document().store(), document.store());
        assert_eq!(session.document().revision(), document.revision());
        assert_eq!(session.history_depths(), (0, 0));
    }
    step(cx, handle, "ctrl-c");
    assert_eq!(clipboard(cx), expected_slice);
    assert_eq!(session.borrow().selection(), expected_selection);
    assert_eq!(session.borrow().document().revision(), document.revision());
    assert_eq!(session.borrow().history_depths(), (0, 0));

    step(cx, handle, "ctrl-x");
    assert_eq!(clipboard(cx), expected_slice);
    {
        let session = session.borrow();
        let inline = session
            .document()
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap();
        assert!(inline.runs().is_empty());
        assert!(inline.atoms().is_empty());
        for atom in &atoms {
            assert!(session.document().node(*atom).is_none());
        }
        assert_eq!(session.document().node_count(), 2);
        assert_eq!(session.history_depths(), (1, 0));
        assert_eq!(
            session.selection(),
            DocumentSelection::collapsed(point(session.document(), node, 0, 0))
        );
    }

    step(cx, handle, "ctrl-z");
    let session = session.borrow();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), expected_selection);
    assert_eq!(session.history_depths(), (0, 1));
    for atom in atoms {
        assert_eq!(session.document().node(atom), document.node(atom));
        assert_eq!(session.document().parent_of(atom), Some(node));
    }
}

#[gpui::test]
fn select_all_only_consecutive_hard_breaks_copies_cuts_and_restores_every_atom(
    cx: &mut TestAppContext,
) {
    assert_select_all_copy_cut_undo(
        cx,
        "",
        &[
            (0, AtomKind::hard_break(), Mark::Bold),
            (0, AtomKind::hard_break(), Mark::Italic),
            (0, AtomKind::hard_break(), Mark::Code),
        ],
        3,
        "\n\n\n",
    );
}

#[gpui::test]
fn select_all_unicode_text_and_trailing_hard_breaks_keeps_literal_lf_distinct(
    cx: &mut TestAppContext,
) {
    let text = "中\n🙂";
    assert_select_all_copy_cut_undo(
        cx,
        text,
        &[
            (text.len(), AtomKind::hard_break(), Mark::Bold),
            (text.len(), AtomKind::hard_break(), Mark::Italic),
        ],
        2,
        "中\n🙂\n\n",
    );
}

#[gpui::test]
fn select_all_leading_break_text_and_trailing_extension_retains_extension_payload(
    cx: &mut TestAppContext,
) {
    let text = "中\n🙂";
    assert_select_all_copy_cut_undo(
        cx,
        text,
        &[
            (0, AtomKind::hard_break(), Mark::Bold),
            (
                text.len(),
                AtomKind::new("reference").unwrap(),
                Mark::Strike,
            ),
        ],
        1,
        "\n中\n🙂[ref🙂]",
    );
}
