//! Exercise the stock Linux callback order without adding an IME wait policy.
//! X11Client resets/unmarks the old handler before dispatching MouseDown. The
//! virtual platform does not implement XIM, so that callback is invoked here,
//! followed by a real GPUI pointer event on the submit control.
use super::DocumentView;
use crate::editor::{EditorHooks, EditorInstance};
use gpui::{
    AppContext as _, Context, Entity, EntityInputHandler, MouseButton, Render, TestAppContext,
    VisualTestContext, Window, div, prelude::*, px,
};
use std::{cell::RefCell, rc::Rc};
use xiaomu_core::{
    document::{InlineContent, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, XiaomuDocument},
    selection::TextPoint,
};
use xiaomu_runtime::session::DocumentSelection;

struct Form {
    input: Entity<DocumentView>,
    applied: Rc<RefCell<Option<String>>>,
}
impl Form {
    fn submit(&mut self, cx: &mut Context<Self>) {
        let input = self.input.read(cx);
        if input.has_active_composition(cx) {
            return;
        }
        let session = input.session().borrow();
        let doc = session.document();
        let id = doc
            .node(doc.root())
            .unwrap()
            .content()
            .as_children()
            .unwrap()[0];
        let text = doc
            .node(id)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .runs()
            .iter()
            .map(|r| r.text().as_str())
            .collect();
        *self.applied.borrow_mut() = Some(text);
    }
}
impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(div().h(px(90.)).child(self.input.clone()))
            .child(
                div()
                    .debug_selector(|| "form-apply".into())
                    .h(px(32.))
                    .child("Apply")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.submit(cx)),
                    ),
            )
    }
}

#[gpui::test]
fn old_url_handler_unmarks_before_real_pointer_submit_reads_canonical(cx: &mut TestAppContext) {
    let mut builder = NodeStoreBuilder::new();
    let node = builder
        .insert(
            NodeKind::Paragraph,
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
    let editor = EditorInstance::new(
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        DocumentSelection::collapsed(TextPoint::at_start_of(node)),
        EditorHooks::default(),
    )
    .unwrap();
    let applied = Rc::new(RefCell::new(None));
    let output = applied.clone();
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| Form {
                input: cx.new(|_| editor.build_view()),
                applied: output,
            })
        })
        .unwrap()
    });
    handle
        .update(cx, |form, window, cx| {
            window.activate_window();
            form.input
                .update(cx, |view, cx| view.focus_selection(window, cx));
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |form, window, cx| {
            form.input.update(cx, |view, cx| {
                let child = view.children[0].1.clone();
                child.update(cx, |child, cx| {
                    child.replace_and_mark_text_in_range(None, "你好", None, window, cx)
                });
                assert!(view.has_active_composition(cx));
            });
            // A direct programmatic call has not delivered the native mouse
            // callback and must not interpret the still-empty canonical value.
            form.submit(cx);
            assert!(applied.borrow().is_none());
            form.input.update(cx, |view, cx| {
                let child = view.children[0].1.clone();
                child.update(cx, |child, cx| child.unmark_text(window, cx));
                assert!(!view.has_active_composition(cx));
            });
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    let point = visual.debug_bounds("form-apply").unwrap().center();
    visual.simulate_click(point, gpui::Modifiers::default());
    assert_eq!(applied.borrow().as_deref(), Some("你好"));
}
