//! GPUI 0.2.2 exposes no safe public multi-entry ClipboardItem constructor.
//! Decode tests cover the transport parts; this test-only, one-shot injection
//! exercises real Ctrl-V dispatch with that decoded content. It is not an OS
//! multi-MIME end-to-end test and makes no change to release clipboard reads.
use super::{PlatformClipboardContent, decode_fallback};
use crate::{
    block_view::SharedSession,
    document_view::DocumentView,
    editor::{EditorHooks, EditorInstance, bind_default_editor_keys},
    editor_commands::{
        CodePasteSource, CommandRoute, EditorCommand, EditorCommandContext, EditorCommandRouter,
    },
};
use gpui::{AppContext as _, TestAppContext, WindowHandle};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use xiaomu_core::{
    document::{
        ImageAttrs, ImageSource, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
        NodeStoreBuilder, XiaomuDocument,
    },
    selection::TextPoint,
};
use xiaomu_runtime::{
    assets::{AssetError, AssetFormat, AssetRef, AssetService, AssetSink},
    session::{DocumentChangeListener, DocumentSelection, EditIntent, PolicyError},
};

thread_local! {
    static CONTENT: RefCell<Option<PlatformClipboardContent>> = const { RefCell::new(None) };
}

pub(super) fn take_content() -> Option<PlatformClipboardContent> {
    CONTENT.with(|content| content.borrow_mut().take())
}

fn with_mixed_content(raw: Option<&str>, format: gpui::ImageFormat, run: impl FnOnce()) {
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            CONTENT.with(|content| *content.borrow_mut() = None);
        }
    }
    let content = decode_fallback(
        raw.map(str::to_owned),
        [gpui::ClipboardEntry::Image(gpui::Image::from_bytes(
            format,
            vec![1, 2, 3],
        ))],
    )
    .expect("supported mixed transport");
    CONTENT.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(content);
    });
    let _clear = Clear;
    run();
    CONTENT.with(|slot| {
        assert!(
            slot.borrow().is_none(),
            "real paste must read injected content"
        )
    });
}

#[derive(Clone, Copy)]
enum Decision {
    Raw,
    Default,
    Reject,
}
struct Router {
    decision: Decision,
    seen: RefCell<Vec<(String, CodePasteSource)>>,
    ordinary: Cell<usize>,
}
impl EditorCommandRouter for Router {
    fn route(
        &self,
        _: EditorCommandContext<'_>,
        _: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError> {
        self.ordinary.set(self.ordinary.get() + 1);
        Ok(CommandRoute::Default)
    }
    fn route_code_paste(
        &self,
        _: EditorCommandContext<'_>,
        raw: &str,
        source: CodePasteSource,
    ) -> Result<CommandRoute, PolicyError> {
        self.seen.borrow_mut().push((raw.into(), source));
        match self.decision {
            Decision::Raw => Ok(CommandRoute::Intent(EditIntent::PasteText {
                text: raw.into(),
            })),
            Decision::Default => Ok(CommandRoute::Default),
            Decision::Reject => Err(PolicyError::new("mixed clipboard rejected")),
        }
    }
}

struct Host(Cell<usize>);
impl AssetService for Host {
    fn resolve(&self, _: AssetRef, sink: Rc<dyn AssetSink>) {
        sink.resolved(Err(AssetError::NotFound));
    }
    fn import_image(&self, format: AssetFormat, bytes: &[u8]) -> Result<ImageAttrs, AssetError> {
        assert!(matches!(format, AssetFormat::Png | AssetFormat::Jpeg));
        assert_eq!(bytes, [1, 2, 3]);
        self.0.set(self.0.get() + 1);
        ImageAttrs::new(
            ImageSource::AssetRef("mixed-image".into()),
            "mixed".into(),
            None,
            None,
            None,
        )
        .map_err(|_| AssetError::InvalidImage)
    }
}

type Counts = Rc<Cell<(usize, usize)>>;
struct Listener(Counts);
impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        let (d, s) = self.0.get();
        self.0.set((d + 1, s));
    }
    fn selection_changed(&mut self, _: DocumentSelection) {
        let (d, s) = self.0.get();
        self.0.set((d, s + 1));
    }
}

struct Mounted {
    window: WindowHandle<DocumentView>,
    session: SharedSession,
    node: NodeId,
    router: Rc<Router>,
    host: Rc<Host>,
    counts: Counts,
}
fn mount(kind: NodeKind, decision: Option<Decision>, cx: &mut TestAppContext) -> Mounted {
    let mut builder = NodeStoreBuilder::new();
    let node = builder
        .insert(
            kind,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::empty()),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let router = Rc::new(Router {
        decision: decision.unwrap_or(Decision::Default),
        seen: RefCell::new(Vec::new()),
        ordinary: Cell::new(0),
    });
    let host = Rc::new(Host(Cell::new(0)));
    let counts = Rc::new(Cell::new((0, 0)));
    let editor = EditorInstance::new(
        document,
        DocumentSelection::collapsed(TextPoint::at_start_of(node)),
        EditorHooks {
            asset_service: Some(host.clone()),
            listener: Some(Box::new(Listener(counts.clone()))),
            ..Default::default()
        },
    )
    .unwrap();
    let editor = if decision.is_some() {
        editor.with_command_router(router.clone())
    } else {
        editor
    };
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
    Mounted {
        window,
        session,
        node,
        router,
        host,
        counts,
    }
}

fn text(m: &Mounted) -> String {
    m.session
        .borrow()
        .document()
        .node(m.node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}
fn paste(m: &Mounted, raw: Option<&str>, format: gpui::ImageFormat, cx: &mut TestAppContext) {
    with_mixed_content(raw, format, || {
        cx.simulate_keystrokes(m.window.into(), "ctrl-v")
    });
}

struct Snapshot {
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
    history: (usize, usize),
    counts: (usize, usize),
}
impl Snapshot {
    fn capture(m: &Mounted) -> Self {
        let s = m.session.borrow();
        Self {
            document: s.document().clone(),
            selection: s.selection(),
            marks: s.stored_marks().cloned(),
            history: s.history_depths(),
            counts: m.counts.get(),
        }
    }
    fn assert_unchanged(&self, m: &Mounted) {
        let s = m.session.borrow();
        assert_eq!(s.document().store(), self.document.store());
        assert_eq!(s.document().revision(), self.document.revision());
        assert_eq!(s.selection(), self.selection);
        assert_eq!(s.stored_marks(), self.marks.as_ref());
        assert_eq!(s.history_depths(), self.history);
        assert_eq!(m.counts.get(), self.counts);
    }
}

#[gpui::test]
fn code_can_opt_into_mixed_raw_text_before_png_or_jpeg_with_one_undo(cx: &mut TestAppContext) {
    for format in [gpui::ImageFormat::Png, gpui::ImageFormat::Jpeg] {
        for raw in ["甲\r\n乙\r丙\n丁\t", " \t\r"] {
            let m = mount(NodeKind::CodeBlock, Some(Decision::Raw), cx);
            let before = Snapshot::capture(&m);
            paste(&m, Some(raw), format, cx);
            assert_eq!(text(&m), raw);
            assert_eq!(
                *m.router.seen.borrow(),
                [(raw.into(), CodePasteSource::PlatformText)]
            );
            assert_eq!(m.router.ordinary.get(), 0);
            assert_eq!(m.host.0.get(), 0);
            assert_eq!(m.counts.get().0, 1);
            assert_eq!(m.session.borrow().history_depths(), (1, 0));
            cx.simulate_keystrokes(m.window.into(), "ctrl-z");
            assert_eq!(
                m.session.borrow().document().store(),
                before.document.store()
            );
            assert_eq!(m.session.borrow().selection(), before.selection);
            cx.simulate_keystrokes(m.window.into(), "ctrl-shift-z");
            assert_eq!(text(&m), raw);
            assert_eq!(m.router.seen.borrow().len(), 1);
        }
    }
}

#[gpui::test]
fn mixed_default_no_router_and_hook_error_keep_code_rejection_atomic(cx: &mut TestAppContext) {
    for decision in [None, Some(Decision::Default), Some(Decision::Reject)] {
        let m = mount(NodeKind::CodeBlock, decision, cx);
        cx.simulate_keystrokes(m.window.into(), "ctrl-b");
        cx.simulate_input(m.window.into(), "z");
        cx.simulate_keystrokes(m.window.into(), "ctrl-z");
        let before = Snapshot::capture(&m);
        paste(&m, Some("raw\r\ntext"), gpui::ImageFormat::Png, cx);
        before.assert_unchanged(&m);
        assert_eq!(
            m.router.seen.borrow().len(),
            usize::from(decision.is_some())
        );
        assert_eq!(m.router.ordinary.get(), 0);
        assert_eq!(m.host.0.get(), 0);
    }
}

#[gpui::test]
fn mixed_clipboard_keeps_paragraph_image_first_and_empty_code_text_unrouted(
    cx: &mut TestAppContext,
) {
    for format in [gpui::ImageFormat::Png, gpui::ImageFormat::Jpeg] {
        for raw in [None, Some(""), Some("raw\r\ntext")] {
            let paragraph = mount(NodeKind::Paragraph, Some(Decision::Raw), cx);
            paste(&paragraph, raw, format, cx);
            assert!(paragraph.router.seen.borrow().is_empty());
            assert_eq!(paragraph.router.ordinary.get(), 0);
            assert_eq!(paragraph.host.0.get(), 1);
            assert_eq!(paragraph.session.borrow().history_depths(), (1, 0));
            assert_eq!(text(&paragraph), "");
            if raw.is_none_or(str::is_empty) {
                let code = mount(NodeKind::CodeBlock, Some(Decision::Raw), cx);
                let before = Snapshot::capture(&code);
                paste(&code, raw, format, cx);
                before.assert_unchanged(&code);
                assert!(code.router.seen.borrow().is_empty());
                assert_eq!(code.host.0.get(), 0);
            }
        }
    }
}
