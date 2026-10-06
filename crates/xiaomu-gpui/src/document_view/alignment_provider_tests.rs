//! Host projections use fresh canonical nodes in the mounted render pipeline.

use super::{DocumentView, cache_key::LayoutCacheKey};
use crate::{
    block_alignment::{BlockAlignment, BlockAlignmentProvider},
    editor::{EditorHooks, EditorInstance},
};
use gpui::{AppContext as _, EntityInputHandler, Pixels, TestAppContext, WindowHandle, px};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use xiaomu_core::{
    document::{
        AttrValue, HeadingLevel, InlineContent, MarkSet, Node, NodeAttrs, NodeContent, NodeId,
        NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
    },
    selection::{CursorAffinity, TextPoint},
    transaction::{Transaction, TransactionOrigin, TransactionStep},
};
use xiaomu_runtime::session::DocumentSelection;

const TEXT: &str = "short text";

fn attrs(value: i64) -> NodeAttrs {
    NodeAttrs::new(
        [
            ("host-position".into(), AttrValue::Integer(value)),
            ("uninterpreted".into(), AttrValue::Null),
        ]
        .into(),
    )
    .unwrap()
}

struct Fixture {
    document: XiaomuDocument,
    blocks: [NodeId; 3],
    rule: NodeId,
}

fn fixture() -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let blocks = [
        NodeKind::Paragraph,
        NodeKind::Heading(HeadingLevel::new(2).unwrap()),
        NodeKind::CodeBlock,
    ]
    .map(|kind| {
        builder
            .insert(
                kind,
                attrs(1),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new(TEXT, MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap()
    });
    let rule = builder
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(blocks.into_iter().chain([rule])),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        blocks,
        rule,
    }
}

fn editor(f: &Fixture) -> EditorInstance {
    EditorInstance::new(
        f.document.clone(),
        DocumentSelection::collapsed(TextPoint::at_start_of(f.blocks[0])),
        EditorHooks::default(),
    )
    .unwrap()
}

fn open(editor: &EditorInstance, cx: &mut TestAppContext) -> WindowHandle<DocumentView> {
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

fn notify(handle: WindowHandle<DocumentView>, cx: &mut TestAppContext) {
    handle.update(cx, |_, _, cx| cx.notify()).unwrap();
    cx.background_executor.run_until_parked();
}

fn check_alignment(
    handle: WindowHandle<DocumentView>,
    node: NodeId,
    alignment: BlockAlignment,
    cx: &mut TestAppContext,
) -> LayoutCacheKey {
    handle
        .update(cx, |view, _, cx| {
            let child = view
                .children
                .iter()
                .find(|(id, _)| *id == node)
                .unwrap()
                .1
                .read(cx);
            let start = child.visual_caret_x(0, CursorAffinity::Before).unwrap();
            let end = child
                .visual_caret_x(TEXT.len(), CursorAffinity::Before)
                .unwrap();
            let width = view.block_bounds(node).expect("painted block").size.width;
            assert!(end > start);
            assert!(width > end - start);
            let error: Pixels = match alignment {
                BlockAlignment::Left => start,
                BlockAlignment::Center => start + end - width,
                BlockAlignment::Right => end - width,
            };
            assert!(f32::from(error).abs() < 0.001, "{alignment:?}: {error:?}");
            child.cache_key.expect("painted reusable layout")
        })
        .unwrap()
}

struct AttributeAlignment;
impl BlockAlignmentProvider for AttributeAlignment {
    fn alignment(&self, node: &Node) -> BlockAlignment {
        if node.attrs().get("host-position") != Some(&AttrValue::Integer(1)) {
            return BlockAlignment::Left;
        }
        match node.kind() {
            NodeKind::Paragraph => BlockAlignment::Center,
            NodeKind::Heading(_) => BlockAlignment::Right,
            _ => BlockAlignment::Left,
        }
    }
}

// Observation is test instrumentation only; the production projection is pure.
struct ObservedAlignment(Rc<RefCell<Vec<Node>>>);
impl BlockAlignmentProvider for ObservedAlignment {
    fn alignment(&self, node: &Node) -> BlockAlignment {
        self.0.borrow_mut().push(node.clone());
        AttributeAlignment.alignment(node)
    }
}

struct ConfiguredAlignment(Rc<Cell<BlockAlignment>>);
impl BlockAlignmentProvider for ConfiguredAlignment {
    fn alignment(&self, _: &Node) -> BlockAlignment {
        self.0.get()
    }
}

fn fixed(alignment: BlockAlignment) -> Rc<dyn BlockAlignmentProvider> {
    Rc::new(ConfiguredAlignment(Rc::new(Cell::new(alignment))))
}

#[gpui::test]
fn provider_reads_fresh_attrs_and_kinds_from_real_canonical_blocks(cx: &mut TestAppContext) {
    let f = fixture();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let editor = editor(&f).with_block_alignment_provider(Rc::new(ObservedAlignment(seen.clone())));
    let handle = open(&editor, cx);
    for (node, alignment) in f.blocks.into_iter().zip([
        BlockAlignment::Center,
        BlockAlignment::Right,
        BlockAlignment::Left,
    ]) {
        check_alignment(handle, node, alignment, cx);
        assert!(seen.borrow().contains(f.document.node(node).unwrap()));
    }
    assert!(
        seen.borrow()
            .iter()
            .all(|node| f.blocks.contains(&node.id()))
    );
    assert_eq!(
        editor.session().borrow().document().store(),
        f.document.store()
    );
    assert_eq!(editor.session().borrow().history_depths(), (0, 0));

    seen.borrow_mut().clear();
    let transaction = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeAttrs {
            node: f.blocks[0],
            attrs: attrs(2),
        })
        .with_step(TransactionStep::SetNodeKind {
            node: f.blocks[1],
            kind: NodeKind::Paragraph,
        });
    let expected = transaction.apply(&f.document).unwrap();
    handle
        .update(cx, |view, window, cx| {
            view.apply_edit_transaction(&transaction, window, cx)
                .unwrap()
                .unwrap();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    check_alignment(handle, f.blocks[0], BlockAlignment::Left, cx);
    check_alignment(handle, f.blocks[1], BlockAlignment::Center, cx);
    assert!(!seen.borrow().is_empty());
    for node in seen.borrow().iter() {
        assert_eq!(expected.node(node.id()), Some(node));
    }
    assert_eq!(
        editor.session().borrow().document().store(),
        expected.store()
    );
    assert_eq!(editor.session().borrow().history_depths(), (1, 0));
}

#[gpui::test]
fn provider_notify_replacement_and_removal_relayout_without_canonical_mutation(
    cx: &mut TestAppContext,
) {
    let f = fixture();
    let editor = editor(&f);
    let handle = open(&editor, cx);
    let initial = check_alignment(handle, f.blocks[0], BlockAlignment::Left, cx);
    let epoch = handle.update(cx, |view, _, _| view.epoch.get()).unwrap();
    let selection = editor.session().borrow().selection();
    let configuration = Rc::new(Cell::new(BlockAlignment::Center));
    handle
        .update(cx, |view, _, cx| {
            view.set_block_alignment_provider(Some(Rc::new(ConfiguredAlignment(
                configuration.clone(),
            ))));
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let center = check_alignment(handle, f.blocks[0], BlockAlignment::Center, cx);
    assert_ne!(initial, center);
    configuration.set(BlockAlignment::Right);
    notify(handle, cx);
    let right = check_alignment(handle, f.blocks[0], BlockAlignment::Right, cx);
    assert_ne!(center, right);
    handle
        .update(cx, |view, _, cx| {
            view.set_block_alignment_provider(Some(fixed(BlockAlignment::Left)));
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let explicit_left = check_alignment(handle, f.blocks[0], BlockAlignment::Left, cx);
    assert_ne!(explicit_left, right);
    assert_ne!(
        explicit_left, initial,
        "explicit Left opts into aligned row geometry"
    );
    handle
        .update(cx, |view, _, cx| {
            view.set_block_alignment_provider(None);
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert_eq!(
        check_alignment(handle, f.blocks[0], BlockAlignment::Left, cx),
        initial
    );
    assert_eq!(
        handle.update(cx, |view, _, _| view.epoch.get()).unwrap(),
        epoch
    );
    let session = editor.session().borrow();
    assert_eq!(session.document().store(), f.document.store());
    assert_eq!(session.document().revision(), f.document.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), (0, 0));
}

#[gpui::test]
fn providers_and_mounted_setters_are_isolated_between_editor_instances(cx: &mut TestAppContext) {
    let f = fixture();
    let configuration = Rc::new(Cell::new(BlockAlignment::Center));
    let first = editor(&f)
        .with_block_alignment_provider(Rc::new(ConfiguredAlignment(configuration.clone())));
    let second = editor(&f).with_block_alignment_provider(fixed(BlockAlignment::Right));
    let first_handle = open(&first, cx);
    let second_handle = open(&second, cx);
    check_alignment(first_handle, f.blocks[0], BlockAlignment::Center, cx);
    let second_key = check_alignment(second_handle, f.blocks[0], BlockAlignment::Right, cx);
    configuration.set(BlockAlignment::Left);
    notify(first_handle, cx);
    notify(second_handle, cx);
    check_alignment(first_handle, f.blocks[0], BlockAlignment::Left, cx);
    assert_eq!(
        check_alignment(second_handle, f.blocks[0], BlockAlignment::Right, cx),
        second_key
    );
    first_handle
        .update(cx, |view, _, cx| {
            view.set_block_alignment_provider(None);
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    check_alignment(first_handle, f.blocks[0], BlockAlignment::Left, cx);
    assert_eq!(
        check_alignment(second_handle, f.blocks[0], BlockAlignment::Right, cx),
        second_key
    );
    for editor in [&first, &second] {
        let session = editor.session().borrow();
        assert_eq!(session.document().store(), f.document.store());
        assert_eq!(session.history_depths(), (0, 0));
    }
}

#[gpui::test]
fn composition_preserves_canonical_provider_input_and_excludes_native_range_proxy(
    cx: &mut TestAppContext,
) {
    let f = fixture();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let editor = editor(&f).with_block_alignment_provider(Rc::new(ObservedAlignment(seen.clone())));
    let handle = open(&editor, cx);
    let epoch = handle.update(cx, |view, _, _| view.epoch.get()).unwrap();
    handle
        .update(cx, |view, window, cx| {
            view.children[0].1.clone().update(cx, |child, cx| {
                child.replace_and_mark_text_in_range(None, "preedit", Some(3..3), window, cx);
            });
        })
        .unwrap();
    seen.borrow_mut().clear();
    notify(handle, cx);
    assert!(!seen.borrow().is_empty());
    assert!(
        seen.borrow()
            .iter()
            .all(|node| f.document.node(node.id()) == Some(node))
    );
    handle
        .update(cx, |view, window, cx| {
            view.children[0].1.clone().update(cx, |child, cx| {
                assert_eq!(child.marked_text_range(window, cx), Some(0..7));
                assert!(child.display_content().0.starts_with("preedit"));
                child.replace_and_mark_text_in_range(None, "", None, window, cx);
                child.unmark_text(window, cx);
            });
            assert!(view.select_node(f.rule, window, cx).unwrap().is_some());
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    seen.borrow_mut().clear();
    handle
        .update(cx, |view, window, cx| {
            let (anchor, input) = view.range_input.as_ref().expect("mounted native proxy");
            assert_eq!(*anchor, f.rule);
            input.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, "proxy", Some(2..2), window, cx);
            });
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert!(!seen.borrow().is_empty());
    assert!(
        seen.borrow()
            .iter()
            .all(|node| f.blocks.contains(&node.id()))
    );
    assert!(
        seen.borrow()
            .iter()
            .all(|node| f.document.node(node.id()) == Some(node))
    );
    handle
        .update(cx, |view, window, cx| {
            view.range_input
                .as_ref()
                .unwrap()
                .1
                .update(cx, |input, cx| {
                    assert_eq!(input.display_content().0, "proxy");
                    assert_eq!(input.marked_text_range(window, cx), Some(0..5));
                    assert_eq!(
                        input.visual_caret_x(0, CursorAffinity::Before),
                        Some(px(0.))
                    );
                    input.replace_and_mark_text_in_range(None, "", None, window, cx);
                    input.unmark_text(window, cx);
                });
            assert_eq!(view.epoch.get(), epoch);
        })
        .unwrap();
    let session = editor.session().borrow();
    assert_eq!(session.document().store(), f.document.store());
    assert_eq!(session.document().revision(), f.document.revision());
    assert_eq!(session.selection().as_node_selection(), Some(f.rule));
    assert_eq!(session.history_depths(), (0, 0));
}
