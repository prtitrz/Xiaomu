//! Native task controls. The checkbox is separate from editable text and
//! targets a stable item identity, never the current selection or list index.
//!
//! GPUI 0.2.2 has no public semantic-role / checked accessibility builder.
//! This is an actual pointer control, but does not claim a platform checkbox
//! accessibility node. No unchecked/checked glyph is inserted into the text.

use gpui::{
    BorderStyle, Context, MouseButton, PathBuilder, SharedString, Window, canvas, div, outline,
    point, prelude::*, px,
};
use xiaomu_core::document::{AttrValue, NodeAttrs, NodeId, NodeKind};
use xiaomu_runtime::session::{EditIntent, SessionError, SessionOutcome};

use super::{DocumentView, markers::MARKER_COLUMN};

fn is_checked(attrs: &NodeAttrs) -> bool {
    matches!(attrs.get("checked"), Some(AttrValue::Bool(true)))
}

fn control_id(item: NodeId) -> SharedString {
    // NodeId deliberately exposes no raw numeric representation. Its Debug
    // form is only a local GPUI identity, never a persisted or wire value.
    format!("task-checkbox-{item:?}").into()
}

pub(super) struct TaskItemLayout {
    pub in_quote: bool,
    pub list_depth: usize,
    pub index: usize,
    pub width: Option<super::measured_table::BlockLayoutWidth>,
}

impl DocumentView {
    pub(super) fn render_task_item(
        &self,
        item: NodeId,
        children: Vec<NodeId>,
        layout: TaskItemLayout,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let TaskItemLayout {
            in_quote,
            list_depth,
            index,
            width,
        } = layout;
        let checked = self
            .session
            .borrow()
            .document()
            .node(item)
            .is_some_and(|node| is_checked(node.attrs()));
        let checkbox = div()
            .id(control_id(item))
            .debug_selector(|| format!("task-checkbox-{item:?}"))
            .w(px(MARKER_COLUMN))
            .h(px(28.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .cursor(gpui::CursorStyle::PointingHand)
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                // The stock platform has already unmarked the previous input
                // handler before pointer dispatch. Do not initiate an unmark,
                // trap focus, or add a waiting protocol here. Suppress the
                // editor's caret placement and default mouse focus transfer.
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.toggle_task_checked(item, window, cx);
            }))
            .child(
                canvas(
                    |_, window, _| window.text_style().color,
                    move |bounds, color, window, _| {
                        window.paint_quad(outline(bounds, color, BorderStyle::Solid));
                        if checked {
                            let mut check = PathBuilder::stroke(px(1.8));
                            check.move_to(bounds.origin + point(px(3.0), px(8.0)));
                            check.line_to(bounds.origin + point(px(6.5), px(11.5)));
                            check.line_to(bounds.origin + point(px(13.0), px(4.0)));
                            if let Ok(path) = check.build() {
                                window.paint_path(path, color);
                            }
                        }
                    },
                )
                .size(px(16.0)),
            );
        // Consume the list indentation once at the item boundary. All its
        // descendants, including images, code, quotes and tail blocks, stay
        // in this content column. Nested ordinary/task lists add their own
        // indent inside it rather than repeating an absolute ancestor depth.
        let mut content = div().flex().flex_col().flex_1().min_w_0();
        let child_width = width.map(|width| {
            width.inset(px(
                MARKER_COLUMN * list_depth.saturating_sub(1) as f32 + MARKER_COLUMN
            ))
        });
        for (child_index, child) in children.into_iter().enumerate() {
            content = content.child(
                div()
                    .debug_selector(|| format!("task-content-{child:?}"))
                    .w_full()
                    .min_w_0()
                    .child(self.render_block_tree_at_width(
                        child,
                        in_quote,
                        0,
                        index + child_index,
                        child_width,
                        cx,
                    )),
            );
        }
        div()
            .flex()
            .flex_row()
            .items_start()
            .min_w_0()
            .ml(px(MARKER_COLUMN * list_depth.saturating_sub(1) as f32))
            .child(checkbox)
            .child(content)
            .into_any_element()
    }

    fn toggle_task_checked(&mut self, item: NodeId, window: &mut Window, cx: &mut Context<Self>) {
        // Programmatic commands retain the ordinary composition safeguard;
        // real platform pointer delivery unmarks before reaching this path.
        if self.focused_child_composing(window, cx) {
            return;
        }
        let outcome = {
            let mut session = self.session.borrow_mut();
            // The visual can be older than the live snapshot (including two
            // clicks in one frame). Only the identity is captured by render;
            // decide the new value from this item's current checked state.
            let checked: Result<bool, SessionError> = match session.document().node(item) {
                Some(node) if matches!(node.kind(), NodeKind::TaskItem) => {
                    Ok(!is_checked(node.attrs()))
                }
                Some(_) => Err(xiaomu_core::Error::InvalidNodeContent.into()),
                None => Err(xiaomu_core::Error::UnknownNode.into()),
            };
            checked.and_then(|checked| {
                session.apply_intent(&EditIntent::SetTaskChecked { item, checked })
            })
        };
        match outcome {
            Ok(outcome) => {
                self.desired_x = None;
                if outcome != SessionOutcome::NoChange {
                    self.epoch.set(self.epoch.get() + 1);
                }
                self.sync_children(cx);
                // Even an accepted NoChange can come from a click in an
                // inactive pane. Restore this editor's existing selection
                // focus, without moving the caret or requesting scrolling.
                self.route_focus(window, cx);
                cx.notify();
            }
            Err(error) => eprintln!("xiaomu: task checkbox rejected: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn successive_command_callbacks_without_render_read_live_checked(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::document_view::task_checkbox_tests::{
            checked_value, fixture, open, other_item_range,
        };
        use crate::editor::{EditorHooks, EditorInstance};

        let (document, items, blocks) = fixture();
        let selection = other_item_range(&document, blocks[1]);
        let editor = EditorInstance::new(document, selection, EditorHooks::default()).unwrap();
        let session = editor.session().clone();
        let handle = open(editor, cx);
        // A narrow callback-level test, not a claim of native double-click
        // delivery in one frame: GPUI's public simulator redraws each event.
        // Both callbacks run before this update returns or any view redraws.
        handle
            .update(cx, |view, window, cx| {
                for expected in [true, false] {
                    view.toggle_task_checked(items[0], window, cx);
                    assert_eq!(
                        checked_value(&session, items[0]),
                        Some(AttrValue::Bool(expected))
                    );
                }
            })
            .unwrap();
        assert_eq!(session.borrow().selection(), selection);
        assert_eq!(session.borrow().history_depths(), (2, 0));
    }

    #[test]
    fn checking_is_a_read_only_projection_of_exact_attrs() {
        for (checked, expected) in [
            (None, false),
            (Some(AttrValue::Null), false),
            (Some(AttrValue::Bool(false)), false),
            (Some(AttrValue::Bool(true)), true),
        ] {
            let attrs = NodeAttrs::new(
                checked
                    .map(|value| ("checked".into(), value))
                    .into_iter()
                    .collect(),
            )
            .unwrap();
            let before = attrs.clone();
            assert_eq!(is_checked(&attrs), expected);
            assert_eq!(attrs, before);
        }
    }
}
