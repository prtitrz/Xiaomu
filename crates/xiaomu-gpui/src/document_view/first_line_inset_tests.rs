//! Full mounted editor/provider tests. NoopTextSystem gives deterministic engine
//! evidence only; it does not establish native-font or browser-caret parity.
use super::{DocumentView, ReadingRange, cache_key::LayoutCacheKey};
use crate::{
    block_alignment::{BlockAlignment, BlockAlignmentProvider},
    editor::{EditorHooks, EditorInstance},
    first_line_inset::FirstLineInsetProvider,
    font_size::FontSizeContext,
    inline_atom::InlineAtomRendererRegistry,
    text_size::{TextSizeCapability, TextSizeStyle, TextSizeStyleProvider},
};
use gpui::{
    AppContext as _, Context, Entity, EntityInputHandler, IntoElement, Pixels, Render, Styled,
    TestAppContext, Window, WindowHandle, div, font, prelude::ParentElement, px,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use xiaomu_core::{
    document::{
        InlineContent, MarkSet, Node, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
        TextRun, XiaomuDocument,
    },
    selection::{CursorAffinity, InlinePoint},
};
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentSelection, PolicyError, SessionPolicy,
};

struct Style {
    base: f32,
    family: &'static str,
}
impl TextSizeStyleProvider for Style {
    fn style(&self, _: &XiaomuDocument, _: &Node) -> TextSizeStyle {
        TextSizeStyle::new(
            font(self.family),
            FontSizeContext::new(self.base, self.base, self.base).unwrap(),
            1.4,
        )
    }
}
struct Inset(f32);
impl FirstLineInsetProvider for Inset {
    fn first_line_inset(&self, _: &XiaomuDocument, _: &Node, _: f32) -> f32 {
        self.0
    }
}
struct Align(BlockAlignment);
impl BlockAlignmentProvider for Align {
    fn alignment(&self, _: &Node) -> BlockAlignment {
        self.0
    }
}
struct Policy(Rc<RefCell<Rc<TextSizeCapability>>>);
impl SessionPolicy for Policy {
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        self.0
            .borrow()
            .validate_document(document, &InlineAtomRendererRegistry::default())
            .map_err(|_| PolicyError::new("unsupported inset candidate"))
    }
}
struct Changes(Rc<Cell<usize>>);
impl DocumentChangeListener for Changes {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        self.0.set(self.0.get() + 1);
    }
    fn selection_changed(&mut self, _: DocumentSelection) {
        self.0.set(self.0.get() + 1);
    }
}
struct Host {
    view: Entity<DocumentView>,
    width: f32,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(self.width)).h(px(600.)).child(self.view.clone())
    }
}
struct Mounted {
    handle: WindowHandle<Host>,
    editor: EditorInstance,
    policy: Rc<RefCell<Rc<TextSizeCapability>>>,
    changes: Rc<Cell<usize>>,
    node: NodeId,
}
fn fixture(text: &str) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let inline = if text.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
    };
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), node)
}
fn capability(
    window: &Window,
    inset: Option<f32>,
    base: f32,
    family: &'static str,
) -> Rc<TextSizeCapability> {
    let capability = TextSizeCapability::new(
        window.text_system().clone(),
        Rc::new(Style { base, family }),
    );
    Rc::new(if let Some(inset) = inset {
        capability.with_first_line_inset_provider(Rc::new(Inset(inset)))
    } else {
        capability
    })
}
fn open(
    cx: &mut TestAppContext,
    document: XiaomuDocument,
    node: NodeId,
    inset: Option<f32>,
) -> Mounted {
    let changes = Rc::new(Cell::new(0));
    let mut configured = None;
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let capability = capability(window, inset, 18., ".SystemUIFont");
            capability
                .validate_document(&document, &InlineAtomRendererRegistry::default())
                .unwrap();
            let policy = Rc::new(RefCell::new(capability.clone()));
            let editor = EditorInstance::new_with_policy(
                document,
                DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
                EditorHooks {
                    listener: Some(Box::new(Changes(changes.clone()))),
                    ..Default::default()
                },
                Box::new(Policy(policy.clone())),
            )
            .unwrap()
            .with_text_size_capability(capability);
            let view = cx.new(|_| editor.build_view());
            configured = Some((editor, policy));
            cx.new(|_| Host {
                view,
                width: 260.25,
            })
        })
        .unwrap()
    });
    handle
        .update(cx, |host, window, cx| {
            window.activate_window();
            host.view
                .update(cx, |view, cx| view.focus_selection(window, cx));
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let (editor, policy) = configured.unwrap();
    Mounted {
        handle,
        editor,
        policy,
        changes,
        node,
    }
}
fn near(value: Pixels, expected: f32) {
    assert!(
        (f32::from(value) - expected).abs() < 0.02,
        "{value:?} != {expected}"
    );
}
fn state(mounted: &Mounted, expected: f32, cx: &mut TestAppContext) -> LayoutCacheKey {
    mounted
        .handle
        .update(cx, |host, _, cx| {
            let view = host.view.read(cx);
            let child = view
                .children
                .iter()
                .find(|(id, _)| *id == mounted.node)
                .unwrap()
                .1
                .read(cx);
            near(
                child.visual_caret_x(0, CursorAffinity::Before).unwrap(),
                expected,
            );
            child.cache_key.expect("painted reusable inset layout")
        })
        .unwrap()
}
fn reconfigure(
    mounted: &Mounted,
    cx: &mut TestAppContext,
    inset: Option<f32>,
    base: f32,
    family: &'static str,
    width: f32,
) {
    mounted
        .handle
        .update(cx, |host, window, cx| {
            let capability = capability(window, inset, base, family);
            capability
                .validate_document(
                    mounted.editor.session().borrow().document(),
                    &InlineAtomRendererRegistry::default(),
                )
                .unwrap();
            *mounted.policy.borrow_mut() = capability.clone();
            host.width = width;
            host.view.update(cx, |view, cx| {
                view.attach_text_size_capability(Some(capability));
                cx.notify();
            });
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
}
fn assert_clean(mounted: &Mounted, before: &XiaomuDocument, selection: DocumentSelection) {
    let session = mounted.editor.session().borrow();
    assert_eq!(session.document().store(), before.store());
    assert_eq!(session.document().revision(), before.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(
        mounted.changes.get(),
        0,
        "provider/layout changes must not dirty the host"
    );
}

#[gpui::test]
fn absent_zero_replacement_removal_and_fractional_cache_inputs_are_presentation_only(
    cx: &mut TestAppContext,
) {
    let (document, node) = fixture("中文🙂 e\u{301} Alpha beta gamma delta");
    let before = document.clone();
    let mounted = open(cx, document, node, None);
    let selection = mounted.editor.session().borrow().selection();
    let initial = state(&mounted, 0., cx);
    reconfigure(&mounted, cx, Some(0.), 18., ".SystemUIFont", 260.25);
    assert_eq!(
        state(&mounted, 0., cx),
        initial,
        "zero must preserve the absent provider path"
    );
    let mut previous = initial;
    for (inset, base, family, width) in [
        (36., 18., ".SystemUIFont", 260.25),
        (36.25, 18., ".SystemUIFont", 260.25),
        (36.25, 18., ".SystemUIFont", 261.4),
        (36.25, 18.25, ".SystemUIFont", 261.4),
        (36.25, 18.25, "monospace", 261.4),
    ] {
        reconfigure(&mounted, cx, Some(inset), base, family, width);
        let current = state(&mounted, inset, cx);
        assert_ne!(
            current, previous,
            "actual inset/font/box-width changes must invalidate cache"
        );
        previous = current;
        assert_clean(&mounted, &before, selection);
    }
    reconfigure(&mounted, cx, None, 18., ".SystemUIFont", 260.25);
    assert_eq!(state(&mounted, 0., cx), initial);
    assert_clean(&mounted, &before, selection);
}

#[gpui::test]
fn mounted_provider_replacement_is_isolated_between_two_editors(cx: &mut TestAppContext) {
    let (document, node) = fixture("shared snapshot two independent editors");
    let first = open(cx, document.clone(), node, Some(36.));
    let second = open(cx, document.clone(), node, Some(18.));
    let first_selection = first.editor.session().borrow().selection();
    let second_selection = second.editor.session().borrow().selection();
    state(&first, 36., cx);
    let second_key = state(&second, 18., cx);
    for inset in [Some(12.5), Some(0.), None] {
        reconfigure(&first, cx, inset, 18., ".SystemUIFont", 180.5);
        state(&first, inset.unwrap_or(0.), cx);
        second
            .handle
            .update(cx, |host, _, cx| host.view.update(cx, |_, cx| cx.notify()))
            .unwrap();
        cx.background_executor.run_until_parked();
        assert_eq!(state(&second, 18., cx), second_key);
    }
    assert_clean(&first, &document, first_selection);
    assert_clean(&second, &document, second_selection);
}

#[gpui::test]
fn invalid_insets_fail_admission_before_a_live_capability_is_replaced(cx: &mut TestAppContext) {
    let (document, node) = fixture("canonical stays safe");
    let mounted = open(cx, document.clone(), node, Some(36.));
    let selection = mounted.editor.session().borrow().selection();
    let key = state(&mounted, 36., cx);
    for invalid in [-1., f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 65_537.] {
        mounted
            .handle
            .update(cx, |_, window, _| {
                let candidate = capability(window, Some(invalid), 18., ".SystemUIFont");
                assert!(
                    candidate
                        .validate_document(&document, &InlineAtomRendererRegistry::default())
                        .is_err(),
                    "invalid inset {invalid}"
                );
            })
            .unwrap();
        assert_eq!(state(&mounted, 36., cx), key);
        assert_clean(&mounted, &document, selection);
    }
}

#[gpui::test]
fn reading_and_native_range_geometry_share_the_mounted_first_row_origin(cx: &mut TestAppContext) {
    for alignment in [
        BlockAlignment::Left,
        BlockAlignment::Center,
        BlockAlignment::Right,
    ] {
        let (document, node) = fixture("A🙂中 end");
        let mounted = open(cx, document.clone(), node, Some(36.));
        mounted
            .handle
            .update(cx, |host, _, cx| {
                host.view.update(cx, |view, cx| {
                    view.set_block_alignment_provider(Some(Rc::new(Align(alignment))));
                    cx.notify();
                })
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        mounted
            .handle
            .update(cx, |host, window, cx| {
                host.view.update(cx, |view, cx| {
                    let snapshot = view.reading_snapshot();
                    let point = InlinePoint::at_start_of(node);
                    let reading = view
                        .reading_target_bounds(&snapshot, ReadingRange::new(point, point), cx)
                        .unwrap();
                    let child = view.children[0].1.clone();
                    child.update(cx, |child, cx| {
                        let bounds = child.last_bounds.unwrap();
                        let native = child.bounds_for_range(0..0, bounds, window, cx).unwrap();
                        let caret = child.reading_caret_bounds(&snapshot, point).unwrap();
                        assert_eq!(native.origin, caret.origin);
                        assert_eq!(reading.origin.x, native.origin.x);
                        assert_eq!(reading.origin.y, native.origin.y);
                        assert_eq!(
                            child.character_index_for_point(
                                native.origin + gpui::point(px(0.), px(1.)),
                                window,
                                cx
                            ),
                            Some(0)
                        );
                    });
                })
            })
            .unwrap();
        assert_clean(
            &mounted,
            &document,
            DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
        );
    }
}

#[gpui::test]
fn absent_and_zero_insets_preserve_uniform_unicode_admission_and_geometry(cx: &mut TestAppContext) {
    // Positive insets deliberately have narrower admission. Zero must not
    // accidentally impose that gate on existing bidi or combining text.
    for text in ["אב عربي", "中🙂e\u{301} 👨‍👩‍👧‍👦", "\u{301}tail", ""] {
        let (document, node) = fixture(text);
        let mounted = open(cx, document.clone(), node, None);
        let selection = mounted.editor.session().borrow().selection();
        let baseline = mounted
            .handle
            .update(cx, |host, _, cx| {
                let view = host.view.read(cx);
                let child = view.children[0].1.read(cx);
                let layout = child.last_layout.as_ref().unwrap();
                text.char_indices()
                    .map(|(byte, _)| byte)
                    .chain([text.len()])
                    .map(|byte| layout.position_for_caret(byte, CursorAffinity::Before))
                    .collect::<Vec<_>>()
            })
            .unwrap();
        reconfigure(&mounted, cx, Some(0.), 18., ".SystemUIFont", 260.25);
        mounted
            .handle
            .update(cx, |host, _, cx| {
                let view = host.view.read(cx);
                let child = view.children[0].1.read(cx);
                assert_eq!(child.canonical_text(), text);
                let layout = child.last_layout.as_ref().unwrap();
                let actual = text
                    .char_indices()
                    .map(|(byte, _)| byte)
                    .chain([text.len()])
                    .map(|byte| layout.position_for_caret(byte, CursorAffinity::Before))
                    .collect::<Vec<_>>();
                assert_eq!(actual, baseline);
            })
            .unwrap();
        assert_clean(&mounted, &document, selection);
    }
}

#[path = "first_line_inset_context_tests.rs"]
mod additional;
