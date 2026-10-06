//! Full EditorInstance/DocumentView integration on GPUI's virtual NoopTextSystem.
//! Batch counters are test instrumentation; production providers remain pure.

use super::{DocumentView, EditorRejection, EditorRejectionReason, EditorRejectionStage};
use crate::editor::{EditorHooks, EditorInstance};
use crate::font_size::FontSizeContext;
use crate::inline_atom::InlineAtomRendererRegistry;
use crate::text_size::{TextSizeCapability, TextSizeStyle, TextSizeStyleProvider};
use gpui::{AppContext as _, EntityInputHandler, TestAppContext, WindowHandle, font};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use xiaomu_core::document::{
    AttrValue, DocumentRevision, InlineContent, Mark, MarkSet, Node, NodeAttrs, NodeContent,
    NodeId, NodeKind, NodeStoreBuilder, StringAttribute, TextRun, TextStyleAttributes,
    TextStyleMark, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{DocumentSelection, EditIntent, PolicyError, SessionPolicy};

#[derive(Default)]
struct BatchedStyle {
    calls: Cell<usize>,
    visited: Cell<usize>,
    revisions: RefCell<Vec<DocumentRevision>>,
}

impl TextSizeStyleProvider for BatchedStyle {
    fn style(&self, _: &XiaomuDocument, _: &Node) -> TextSizeStyle {
        panic!("a full editor must use its prepared batch, never per-child tree scans")
    }

    fn styles_for_document(
        &self,
        document: &XiaomuDocument,
    ) -> Option<BTreeMap<NodeId, TextSizeStyle>> {
        self.calls.set(self.calls.get() + 1);
        self.revisions.borrow_mut().push(document.revision());
        let mut styles = BTreeMap::new();
        let mut pending = vec![(document.root(), 20.0, String::from(".SystemUIFont"))];
        let mut visited = 0;
        while let Some((id, inherited, family)) = pending.pop() {
            visited += 1;
            let node = document.node(id).unwrap();
            let inherited = match node.attrs().get("host-size") {
                Some(AttrValue::Integer(value)) => *value as f32,
                _ => inherited,
            };
            let family = match node.attrs().get("host-font") {
                Some(AttrValue::String(value)) => value.clone(),
                _ => family,
            };
            if node.content().as_inline().is_some() {
                styles.insert(
                    id,
                    TextSizeStyle::new(
                        font(family.clone()),
                        FontSizeContext::new(inherited, 20.0, 20.0).unwrap(),
                        1.4,
                    ),
                );
            }
            if let Some(children) = node.content().as_children() {
                pending.extend(children.iter().map(|id| (*id, inherited, family.clone())));
            }
        }
        assert_eq!(visited, document.store().iter().count());
        self.visited.set(visited);
        Some(styles)
    }
}

struct SizePolicy(Rc<TextSizeCapability>);
impl SessionPolicy for SizePolicy {
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        self.0
            .validate_document(document, &InlineAtomRendererRegistry::new())
            .map_err(|_| PolicyError::new("unsupported test size"))
    }
}

fn size_mark(value: &str) -> Mark {
    Mark::TextStyle(TextStyleMark::from_attributes(
        TextStyleAttributes::default().with_font_size(StringAttribute::Value(value.into())),
    ))
}

fn inherited_style(size: i64, family: &str) -> NodeAttrs {
    NodeAttrs::new(
        [
            ("host-size".into(), AttrValue::Integer(size)),
            ("host-font".into(), AttrValue::String(family.into())),
        ]
        .into(),
    )
    .unwrap()
}

struct Fixture {
    document: XiaomuDocument,
    body: NodeId,
    cell_text: NodeId,
    ancestor: NodeId,
}

fn fixture(mixed: bool) -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let body_content = if mixed {
        InlineContent::new([
            TextRun::new("e", MarkSet::new([size_mark("12px")]).unwrap()).unwrap(),
            TextRun::new("z", MarkSet::new([size_mark("48px")]).unwrap()).unwrap(),
        ])
        .unwrap()
    } else {
        InlineContent::new([TextRun::new("BODY", MarkSet::empty()).unwrap()]).unwrap()
    };
    let body = builder
        .insert(
            NodeKind::Paragraph,
            inherited_style(20, ".SystemUIFont"),
            NodeContent::Inline(body_content),
        )
        .unwrap();
    let cell_text = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("CELL", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let cell = builder
        .insert(
            NodeKind::TableCell,
            NodeAttrs::new(
                [(
                    "colwidth".into(),
                    AttrValue::List(vec![AttrValue::Integer(300)]),
                )]
                .into(),
            )
            .unwrap(),
            NodeContent::children([cell_text]),
        )
        .unwrap();
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([cell]),
        )
        .unwrap();
    // Measured tables deliberately reject unknown table attrs. Host style
    // defaults live on their document ancestor, outside that geometry schema.
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            inherited_style(24, ".SystemUIFont"),
            NodeContent::children([body, table]),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        body,
        cell_text,
        ancestor: root,
    }
}

struct Mounted {
    handle: WindowHandle<DocumentView>,
    editor: EditorInstance,
    capability: Rc<TextSizeCapability>,
    provider: Rc<BatchedStyle>,
}

fn open(fixture: &Fixture, cx: &mut TestAppContext) -> Mounted {
    let provider = Rc::new(BatchedStyle::default());
    let (handle, editor, capability) = cx.update(|cx| {
        let mut configured = None;
        let handle = cx
            .open_window(Default::default(), |window, cx| {
                let capability = Rc::new(TextSizeCapability::new(
                    window.text_system().clone(),
                    provider.clone(),
                ));
                let before = provider.calls.get();
                capability
                    .validate_document(&fixture.document, &InlineAtomRendererRegistry::new())
                    .unwrap();
                assert_eq!(
                    provider.calls.get(),
                    before + 1,
                    "one batch for all admission blocks"
                );
                let editor = EditorInstance::new_with_policy(
                    fixture.document.clone(),
                    DocumentSelection::collapsed(InlinePoint::at_start_of(fixture.body)),
                    EditorHooks::default(),
                    Box::new(SizePolicy(capability.clone())),
                )
                .unwrap()
                .with_text_size_capability(capability.clone());
                let mut view = editor.build_view();
                view.set_measured_table_layout(true);
                configured = Some((editor, capability));
                cx.new(|_| view)
            })
            .unwrap();
        let (editor, capability) = configured.unwrap();
        (handle, editor, capability)
    });
    handle
        .update(cx, |view, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    Mounted {
        handle,
        editor,
        capability,
        provider,
    }
}

fn width(
    mounted: &Mounted,
    node: NodeId,
    cx: &mut TestAppContext,
) -> (super::cache_key::LayoutCacheKey, f32) {
    mounted
        .handle
        .update(cx, |view, _, cx| {
            let child = view
                .children
                .iter()
                .find(|(id, _)| *id == node)
                .unwrap()
                .1
                .read(cx);
            let start = child.visual_caret_x(0, CursorAffinity::Before).unwrap();
            let end = child.visual_caret_x(4, CursorAffinity::Before).unwrap();
            (child.cache_key.unwrap(), f32::from(end - start))
        })
        .unwrap()
}

fn change_inherited_style(
    mounted: &Mounted,
    node: NodeId,
    size: i64,
    family: &str,
    cx: &mut TestAppContext,
) {
    let transaction =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::SetNodeAttrs {
            node,
            attrs: inherited_style(size, family),
        });
    // Exercise host-owned canonical changes followed by ordinary render sync.
    // No view epoch bump is available to conceal a stale style fingerprint.
    mounted.provider.revisions.borrow_mut().clear();
    mounted
        .editor
        .session()
        .borrow_mut()
        .apply(&transaction)
        .unwrap();
    let revision = mounted.editor.session().borrow().document().revision();
    mounted.handle.update(cx, |_, _, cx| cx.notify()).unwrap();
    cx.background_executor.run_until_parked();
    assert!(!mounted.provider.revisions.borrow().is_empty());
    assert!(
        mounted
            .provider
            .revisions
            .borrow()
            .iter()
            .all(|seen| *seen == revision)
    );
}

#[gpui::test]
fn full_editor_batches_body_and_table_styles_once_per_sync_and_refreshes_ancestor_inputs(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(false);
    let mounted = open(&fixture, cx);
    mounted
        .handle
        .update(cx, |view, _, cx| {
            assert_eq!(view.children.len(), 2);
            let before = mounted.provider.calls.get();
            view.sync_children(cx);
            assert_eq!(
                mounted.provider.calls.get(),
                before + 1,
                "one batch per sync, not per child"
            );
            assert_eq!(view.epoch.get(), 0);
        })
        .unwrap();
    let original_body = width(&mounted, fixture.body, cx);
    let original_cell = width(&mounted, fixture.cell_text, cx);
    assert!(original_cell.1 > original_body.1);
    let before = mounted.provider.calls.get();
    mounted
        .capability
        .validate_document(
            mounted.editor.session().borrow().document(),
            &InlineAtomRendererRegistry::new(),
        )
        .unwrap();
    assert_eq!(mounted.provider.calls.get(), before + 1);
    change_inherited_style(&mounted, fixture.ancestor, 48, ".SystemUIFont", cx);
    let larger_cell = width(&mounted, fixture.cell_text, cx);
    assert_ne!(larger_cell.0, original_cell.0);
    assert!((larger_cell.1 - original_cell.1 * 2.0).abs() < 0.02);
    assert_eq!(width(&mounted, fixture.body, cx), original_body);
    change_inherited_style(&mounted, fixture.ancestor, 48, "monospace", cx);
    let changed_font = width(&mounted, fixture.cell_text, cx);
    assert_ne!(
        changed_font.0, larger_cell.0,
        "font-only ancestor edit refreshes actual shaping inputs"
    );
    assert_eq!(width(&mounted, fixture.body, cx), original_body);
    mounted
        .handle
        .update(cx, |view, window, cx| {
            assert_eq!(view.epoch.get(), 0);
            mounted
                .editor
                .session()
                .borrow_mut()
                .set_document_selection(DocumentSelection::collapsed(InlinePoint::at_start_of(
                    fixture.cell_text,
                )))
                .unwrap();
            view.focus_selection(window, cx);
            let child = view
                .children
                .iter()
                .find(|(id, _)| *id == fixture.cell_text)
                .unwrap()
                .1
                .clone();
            let before = mounted.provider.calls.get();
            child.update(cx, |child, cx| {
                let caret = child
                    .bounds_for_range(0..0, child.last_bounds.unwrap(), window, cx)
                    .unwrap();
                assert!((f32::from(caret.size.height) - 48.0 * 1.4).abs() < 0.02);
            });
            assert_eq!(
                mounted.provider.calls.get(),
                before,
                "native queries reuse the prepared snapshot"
            );
        })
        .unwrap();
    assert_eq!(mounted.editor.session().borrow().history_depths(), (2, 0));
}

#[gpui::test]
fn unsupported_preedit_feedback_is_forwarded_from_child_to_full_editor_once(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(true);
    let mounted = open(&fixture, cx);
    let session = mounted.editor.session();
    let offset = session
        .borrow()
        .document()
        .node(fixture.body)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(1)
        .unwrap();
    session
        .borrow_mut()
        .set_document_selection(DocumentSelection::collapsed(InlinePoint::new(
            fixture.body,
            offset,
            0,
            CursorAffinity::Before,
        )))
        .unwrap();
    session
        .borrow_mut()
        .apply_intent(&EditIntent::SetMark {
            mark: size_mark("48px"),
        })
        .unwrap();
    let entity = mounted
        .handle
        .update(cx, |view, window, cx| {
            view.focus_selection(window, cx);
            cx.entity()
        })
        .unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let seen = events.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&entity, move |_, event: &EditorRejection, _| {
            seen.borrow_mut().push(*event)
        })
    });
    let before = session.borrow().document().clone();
    let selection = session.borrow().selection();
    let marks = session.borrow().stored_marks().cloned();
    mounted
        .handle
        .update(cx, |view, window, cx| {
            view.focused_child(window, cx)
                .unwrap()
                .update(cx, |child, cx| {
                    child.replace_and_mark_text_in_range(None, "אב", None, window, cx);
                    assert_eq!(child.marked_text_range(window, cx), None);
                    assert_eq!(child.display_content().0, "ez");
                    child.replace_text_in_range(None, "אב", window, cx);
                });
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let events = events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].stage(), EditorRejectionStage::TextSizePreedit);
    assert_eq!(
        events[0].reason(),
        EditorRejectionReason::UnsupportedTextSize
    );
    assert_eq!(events[0].document_revision(), before.revision());
    assert_eq!(session.borrow().document().store(), before.store());
    assert_eq!(session.borrow().document().revision(), before.revision());
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().stored_marks(), marks.as_ref());
    assert_eq!(session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn invalid_stored_size_cannot_cache_unavailable_geometry_and_valid_size_recovers(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(false);
    let mounted = open(&fixture, cx);
    let original = width(&mounted, fixture.body, cx);
    let before = mounted.editor.session().borrow().document().clone();
    let selection = mounted.editor.session().borrow().selection();
    let epoch = mounted
        .handle
        .update(cx, |view, _, _| view.epoch.get())
        .unwrap();
    for (css, available) in [("513px", false), ("24px", true)] {
        let outcome = mounted
            .editor
            .session()
            .borrow_mut()
            .apply_intent(&EditIntent::SetMark {
                mark: size_mark(css),
            })
            .unwrap();
        assert_eq!(outcome, xiaomu_runtime::session::SessionOutcome::NoChange);
        mounted.handle.update(cx, |_, _, cx| cx.notify()).unwrap();
        cx.background_executor.run_until_parked();
        mounted
            .handle
            .update(cx, |view, window, cx| {
                assert_eq!(
                    view.epoch.get(),
                    epoch,
                    "stored marks have no canonical epoch"
                );
                let child = view
                    .children
                    .iter()
                    .find(|(id, _)| *id == fixture.body)
                    .unwrap()
                    .1
                    .clone();
                child.update(cx, |child, cx| {
                    assert!(
                        child.last_layout.is_some(),
                        "the failed frame was actually painted"
                    );
                    assert_eq!(
                        child.visual_caret_x(0, CursorAffinity::Before).is_some(),
                        available
                    );
                    assert_eq!(child.cache_key.is_some(), available);
                    let bounds = child.last_bounds.unwrap();
                    assert_eq!(
                        child.bounds_for_range(0..0, bounds, window, cx).is_some(),
                        available
                    );
                    assert_eq!(
                        child.hit_test_caret_position(bounds.origin).is_some(),
                        available
                    );
                });
            })
            .unwrap();
        assert_eq!(
            mounted.editor.session().borrow().document().store(),
            before.store()
        );
        assert_eq!(
            mounted.editor.session().borrow().document().revision(),
            before.revision()
        );
        assert_eq!(mounted.editor.session().borrow().selection(), selection);
        assert_eq!(mounted.editor.session().borrow().history_depths(), (0, 0));
    }
    // Only pending marks changed: recovery must produce the original text
    // layout/key rather than reuse the failed frame with this same identity.
    assert_eq!(width(&mounted, fixture.body, cx), original);
}

#[gpui::test]
fn size_capability_without_alignment_provider_hits_exact_second_logical_line_top(
    cx: &mut TestAppContext,
) {
    let mut fixture = fixture(false);
    let end = fixture
        .document
        .node(fixture.body)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(4)
        .unwrap();
    fixture.document = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::ReplaceInlineText {
            at: InlinePoint::at_start_of(fixture.body),
            end,
            replacement: "one\nTWO".into(),
        })
        .apply(&fixture.document)
        .unwrap();
    let mounted = open(&fixture, cx);
    mounted
        .handle
        .update(cx, |view, window, cx| {
            assert!(view.block_alignment_provider.is_none());
            let child = view
                .children
                .iter()
                .find(|(id, _)| *id == fixture.body)
                .unwrap()
                .1
                .clone();
            child.update(cx, |child, cx| {
                let bounds = child.last_bounds.unwrap();
                let first = child.bounds_for_range(0..0, bounds, window, cx).unwrap();
                let second = child.bounds_for_range(4..4, bounds, window, cx).unwrap();
                assert_eq!(first.size.height, gpui::px(28.0));
                assert_eq!(
                    second.origin,
                    first.origin + gpui::point(gpui::px(0.0), gpui::px(28.0))
                );
                // No epsilon: the exact row top must select the second logical
                // line, rather than stock legacy <= bottom selecting row one.
                assert_eq!(
                    child.character_index_for_point(second.origin, window, cx),
                    Some(4)
                );
                assert_eq!(
                    child.hit_test_caret_position(second.origin),
                    Some((4, CursorAffinity::Before))
                );
                let selected = child.bounds_for_range(4..7, bounds, window, cx).unwrap();
                assert_eq!(selected.top(), second.top());
                assert_eq!(selected.size.height, second.size.height);
            });
        })
        .unwrap();
    assert_eq!(
        mounted.editor.session().borrow().document().store(),
        fixture.document.store()
    );
    assert_eq!(mounted.editor.session().borrow().history_depths(), (0, 0));
}
