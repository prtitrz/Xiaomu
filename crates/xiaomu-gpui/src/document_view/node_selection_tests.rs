//! Native selection proxies retain whole-block identity through real GPUI input.

use super::*;
use crate::block_view::{ParagraphView, SharedSession};
use crate::editor::{EditorHooks, EditorInstance, bind_default_editor_keys};
use crate::input::platform_clipboard::{PlatformClipboard, PlatformClipboardContent};
use gpui::{
    Bounds, Entity, EntityInputHandler, Pixels, TestAppContext, VisualTestContext, WindowHandle,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use xiaomu_core::document::{
    AttrValue, HeadingLevel, ImageAttrs, ImageSource, InlineContent, MarkSet, NodeAttrs,
    NodeContent, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::clipboard::ClipboardSlice;
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentPosition, DocumentSelection, EditIntent, EditPlan,
    IntentDisposition, PolicyError, SelectionUpdate, SessionContext, SessionPolicy,
};

#[path = "node_selection_atomic_pointer_tests.rs"]
mod atomic_pointer;
#[path = "node_selection_focus_tests.rs"]
mod focus;
#[path = "node_selection_ime_tests.rs"]
mod ime;
#[path = "node_selection_navigation_tests.rs"]
mod navigation;
#[path = "node_selection_range_focus_tests.rs"]
mod range_focus;
#[path = "node_selection_scroll_tests.rs"]
mod scroll;

struct Fixture {
    document: XiaomuDocument,
    intro: NodeId,
    quote: NodeId,
    inside: [NodeId; 2],
    rule: NodeId,
    targets: Vec<NodeId>,
}

fn text(b: &mut NodeStoreBuilder, kind: NodeKind, text: &str) -> NodeId {
    b.insert(
        kind,
        NodeAttrs::empty(),
        NodeContent::Inline(
            InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
        ),
    )
    .unwrap()
}

fn container(
    b: &mut NodeStoreBuilder,
    kind: NodeKind,
    children: impl IntoIterator<Item = NodeId>,
) -> NodeId {
    b.insert(kind, NodeAttrs::empty(), NodeContent::children(children))
        .unwrap()
}

fn fixture() -> Fixture {
    let mut b = NodeStoreBuilder::new();
    let intro = text(&mut b, NodeKind::Paragraph, "intro");
    let first = text(&mut b, NodeKind::Paragraph, "first nested");
    let second = text(&mut b, NodeKind::Paragraph, "second nested");
    let quote = container(&mut b, NodeKind::Quote, [first, second]);
    let heading = text(
        &mut b,
        NodeKind::Heading(HeadingLevel::new(2).unwrap()),
        "heading",
    );
    let code = text(&mut b, NodeKind::CodeBlock, "a\nb");
    let mut targets = vec![intro, quote, heading, code];
    for kind in [
        NodeKind::BulletList,
        NodeKind::OrderedList,
        NodeKind::TaskList,
    ] {
        let paragraph = text(&mut b, NodeKind::Paragraph, "list item");
        let task = kind == NodeKind::TaskList;
        let item = b
            .insert(
                if task {
                    NodeKind::TaskItem
                } else {
                    NodeKind::ListItem
                },
                if task {
                    NodeAttrs::new([("checked".into(), AttrValue::Bool(true))].into()).unwrap()
                } else {
                    NodeAttrs::empty()
                },
                NodeContent::children([paragraph]),
            )
            .unwrap();
        targets.push(container(&mut b, kind, [item]));
    }
    let image = b
        .insert(
            NodeKind::Image,
            ImageAttrs::new(
                ImageSource::AssetRef("fixture".into()),
                "image".into(),
                None,
                None,
                None,
            )
            .unwrap()
            .to_attrs()
            .unwrap(),
            NodeContent::Atomic,
        )
        .unwrap();
    let rule = b
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let cell_text = text(&mut b, NodeKind::Paragraph, "cell");
    let cell = container(&mut b, NodeKind::TableCell, [cell_text]);
    let row = container(&mut b, NodeKind::TableRow, [cell]);
    let table = container(&mut b, NodeKind::Table, [row]);
    targets.extend([image, rule, table]);
    let root = container(&mut b, NodeKind::Document, targets.iter().copied());
    Fixture {
        document: XiaomuDocument::new(root, b.finish()).unwrap(),
        intro,
        quote,
        inside: [first, second],
        rule,
        targets,
    }
}

type Seen = Rc<RefCell<Vec<(DocumentSelection, EditIntent)>>>;
type Counts = Rc<Cell<(usize, usize)>>;

struct Listener(Counts);
impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, document: &XiaomuDocument, selection: DocumentSelection) {
        selection.validate(document).unwrap();
        let (documents, selections) = self.0.get();
        self.0.set((documents + 1, selections));
    }
    fn selection_changed(&mut self, _: DocumentSelection) {
        let (documents, selections) = self.0.get();
        self.0.set((documents, selections + 1));
    }
}

/// Test-only opt-in replacement: one whole node becomes one plain paragraph.
struct ReplaceNode {
    seen: Seen,
    reject_prepare: bool,
    reject_final: bool,
}
impl SessionPolicy for ReplaceNode {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        self.seen
            .borrow_mut()
            .push((context.selection(), intent.clone()));
        if self.reject_prepare {
            return Err(PolicyError::new("fixture preflight rejection"));
        }
        let Some(node) = context.selection().as_node_selection() else {
            return Ok(IntentDisposition::Continue);
        };
        let value = match intent {
            EditIntent::InsertText { text } | EditIntent::PasteText { text } => text.as_str(),
            EditIntent::CommitComposition { range, text } => {
                assert_eq!(range.start().as_usize(), 0);
                assert_eq!(range.end().as_usize(), 0);
                text.as_str()
            }
            _ => return Ok(IntentDisposition::Continue),
        };
        let DocumentPosition::Gap(before) = context.selection().anchor() else {
            panic!("explicit node gap");
        };
        let transaction = Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::RemoveNode { node })
            .with_step(TransactionStep::InsertNode {
                parent: before.parent(),
                index: before.index(),
                kind: NodeKind::Paragraph,
                attrs: NodeAttrs::empty(),
                content: NodeContent::Inline(
                    InlineContent::new([TextRun::new(value, MarkSet::empty()).unwrap()]).unwrap(),
                ),
            });
        let after = transaction.apply(context.document()).unwrap();
        let inserted = after
            .node(before.parent())
            .unwrap()
            .content()
            .as_children()
            .unwrap()[before.index()];
        let offset = after
            .node(inserted)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(value.len())
            .unwrap();
        let selection = DocumentSelection::collapsed(InlinePoint::new(
            inserted,
            offset,
            0,
            CursorAffinity::Before,
        ));
        Ok(IntentDisposition::Apply(EditPlan::new(
            transaction,
            SelectionUpdate::Exact { selection },
            None,
        )))
    }
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if self.reject_final
            && super::super::navigation::text_blocks(document)
                .iter()
                .any(|block| block.text() == "blocked")
        {
            return Err(PolicyError::new("fixture final validation rejection"));
        }
        Ok(())
    }
}

fn open(
    cx: &mut TestAppContext,
    fixture: &Fixture,
    policy: Option<Box<dyn SessionPolicy>>,
) -> (WindowHandle<DocumentView>, SharedSession, Counts) {
    let counts = Rc::new(Cell::new((0, 0)));
    let hooks = EditorHooks {
        listener: Some(Box::new(Listener(counts.clone()))),
        ..EditorHooks::default()
    };
    let selection = DocumentSelection::collapsed(InlinePoint::at_start_of(fixture.intro));
    let editor = match policy {
        Some(policy) => {
            EditorInstance::new_with_policy(fixture.document.clone(), selection, hooks, policy)
        }
        None => EditorInstance::new(fixture.document.clone(), selection, hooks),
    }
    .unwrap();
    let session = editor.session().clone();
    let handle = mount(cx, editor);
    (handle, session, counts)
}

fn mount(cx: &mut TestAppContext, editor: EditorInstance) -> WindowHandle<DocumentView> {
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
    handle
}

fn select(cx: &mut TestAppContext, handle: WindowHandle<DocumentView>, node: NodeId) {
    handle
        .update(cx, |view, window, cx| {
            view.select_node(node, window, cx).unwrap();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
}

fn key(cx: &mut TestAppContext, handle: WindowHandle<DocumentView>, key: &str) {
    cx.simulate_keystrokes(handle.into(), key);
    cx.background_executor.run_until_parked();
}

fn proxy(cx: &mut TestAppContext, handle: WindowHandle<DocumentView>) -> Entity<ParagraphView> {
    handle
        .update(cx, |view, _, _| {
            view.range_input.as_ref().unwrap().1.clone()
        })
        .unwrap()
}

fn clipboard(cx: &mut TestAppContext) -> ClipboardSlice {
    cx.update(|cx| match PlatformClipboard::new(cx).read_content() {
        Some(PlatformClipboardContent::Structured(slice)) => slice,
        _ => panic!("closed structured clipboard required"),
    })
}

fn bounds(
    cx: &mut TestAppContext,
    handle: WindowHandle<DocumentView>,
    prefix: &str,
    node: NodeId,
) -> Bounds<Pixels> {
    VisualTestContext::from_window(handle.into(), cx)
        .debug_bounds(Box::leak(format!("{prefix}-{node:?}").into_boxed_str()))
        .expect("selected subtree must be painted")
}

fn assert_unchanged(
    session: &SharedSession,
    f: &Fixture,
    selection: DocumentSelection,
    counts: &Counts,
) {
    let session = session.borrow();
    assert_eq!(session.document().store(), f.document.store());
    assert_eq!(session.document().revision(), f.document.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(counts.get(), (0, 0));
}

#[gpui::test]
fn every_supported_node_renders_whole_bounds_and_a_local_empty_input_proxy(
    cx: &mut TestAppContext,
) {
    let f = fixture();
    let (handle, session, _) = open(cx, &f, None);
    for node in f.targets.iter().copied() {
        select(cx, handle, node);
        let selected = bounds(cx, handle, "node-selection", node);
        let input_bounds = bounds(cx, handle, "node-selection-input", node);
        assert!(selected.size.height >= px(28.0));
        assert!(selected.size.width > px(0.0));
        assert!(input_bounds.top() >= selected.top() && input_bounds.top() < selected.bottom());
        assert!(input_bounds.left() >= selected.left() && input_bounds.left() < selected.right());
        handle
            .update(cx, |view, window, cx| {
                assert!(view.range_input_is_focused(window, cx));
                let input = view.range_input.as_ref().unwrap().1.clone();
                input.update(cx, |input, cx| {
                    assert_eq!(input.node(), node);
                    assert_eq!(input.display_content().0, "");
                    assert_eq!(input.layout_content().0, "");
                    assert_eq!(
                        input.selected_text_range(false, window, cx).unwrap().range,
                        0..0
                    );
                    let native = input
                        .bounds_for_range(0..0, input_bounds, window, cx)
                        .unwrap();
                    assert_eq!(native.top(), input_bounds.top());
                    assert!(native.size.height > px(0.0));
                });
            })
            .unwrap();
        assert_eq!(session.borrow().selection().as_node_selection(), Some(node));
        key(cx, handle, "ctrl-c");
        let copied = clipboard(cx);
        assert!(copied.is_closed());
        assert_eq!(copied.roots().len(), 1);
        assert_eq!(
            copied.roots()[0].kind(),
            f.document.node(node).unwrap().kind()
        );
    }
    assert_eq!(session.borrow().document().store(), f.document.store());
    assert_eq!(session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn quote_highlight_contains_every_descendant_not_just_the_first_paragraph(cx: &mut TestAppContext) {
    let f = fixture();
    let (handle, _, _) = open(cx, &f, None);
    select(cx, handle, f.quote);
    let selected = bounds(cx, handle, "node-selection", f.quote);
    handle
        .update(cx, |view, _, _| {
            for node in f.inside {
                let child = view.block_bounds(node).unwrap();
                assert!(child.top() >= selected.top());
                assert!(child.bottom() <= selected.bottom());
                assert!(child.left() >= selected.left());
                assert!(child.right() <= selected.right());
            }
            let first = view.block_bounds(f.inside[0]).unwrap();
            assert!(selected.size.height > first.size.height);
            assert!(selected.top() > view.block_bounds(f.intro).unwrap().top());
        })
        .unwrap();
}

#[gpui::test]
fn default_node_edit_actions_fail_closed_while_cut_can_update_clipboard(cx: &mut TestAppContext) {
    let f = fixture();
    let (handle, session, counts) = open(cx, &f, None);
    select(cx, handle, f.quote);
    let selection = session.borrow().selection();
    counts.set((0, 0));
    for gesture in [
        "backspace",
        "delete",
        "enter",
        "shift-enter",
        "ctrl-c",
        "ctrl-x",
        "ctrl-v",
    ] {
        key(cx, handle, gesture);
        assert_unchanged(&session, &f, selection, &counts);
    }
    assert_eq!(clipboard(cx).roots()[0].kind(), &NodeKind::Quote);
    cx.simulate_input(handle.into(), "typed");
    cx.background_executor.run_until_parked();
    assert_unchanged(&session, &f, selection, &counts);
}
