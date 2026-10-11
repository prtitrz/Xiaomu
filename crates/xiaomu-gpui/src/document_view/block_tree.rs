//! Recursive block presentation, including complete root-range highlighting.

use super::{DocumentView, markers};
use crate::image_block::{ImageBlockLayout, ImageBlockPresentation, render_image_block};
use gpui::{Context, MouseButton, MouseDownEvent, div, prelude::*, px};
use xiaomu_core::document::{NodeContent, NodeId, NodeKind};

impl DocumentView {
    /// Renders the document tree with kind-driven styling.
    pub(super) fn render_block_tree(
        &self,
        id: NodeId,
        in_quote: bool,
        list_depth: usize,
        index: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        self.render_block_tree_at_width(id, in_quote, list_depth, index, None, cx)
    }

    pub(super) fn render_block_tree_at_width(
        &self,
        id: NodeId,
        in_quote: bool,
        list_depth: usize,
        index: usize,
        width: Option<super::measured_table::BlockLayoutWidth>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let width = width.map(|width| {
            let session = self.session.borrow();
            let responsive_image = self.image_sizing
                == crate::image_block::ImageSizingPolicy::ResponsiveIntrinsic
                && session
                    .document()
                    .node(id)
                    .is_some_and(|node| matches!(node.kind(), NodeKind::Image));
            if session.selection().as_node_selection() == Some(id) && !responsive_image {
                width.inset(px(2.0))
            } else {
                width
            }
        });
        let node_data = {
            let session = self.session.borrow();
            session
                .document()
                .node(id)
                .map(|node| (node.kind().clone(), node.content().clone()))
        };
        let Some((kind, content)) = node_data else {
            return div().into_any_element();
        };

        let block = match content {
            NodeContent::Inline(_) => {
                let Some((_, view)) = self.children.iter().find(|(child, _)| *child == id) else {
                    return div().into_any_element();
                };
                let marker = {
                    let session = self.session.borrow();
                    markers::marker_for_block(
                        session.document(),
                        id,
                        self.list_marker_provider.as_deref(),
                    )
                };
                markers::style_block(
                    view.clone(),
                    &kind,
                    in_quote,
                    list_depth,
                    marker.as_ref(),
                    index,
                )
                .into_any_element()
            }
            NodeContent::Children(_) if matches!(kind, NodeKind::Table) => {
                if let Some(width) = width {
                    self.render_measured_table(id, index, width, cx)
                } else {
                    self.render_table(id, index, cx)
                }
            }
            NodeContent::Children(children) if matches!(kind, NodeKind::TaskItem) => self
                .render_task_item(
                    id,
                    children,
                    super::task_checkbox::TaskItemLayout {
                        in_quote,
                        list_depth,
                        index,
                        width,
                    },
                    cx,
                ),
            NodeContent::Children(children) => {
                let next_quote = in_quote || matches!(kind, NodeKind::Quote);
                let next_depth = list_depth
                    + usize::from(matches!(
                        kind,
                        NodeKind::BulletList | NodeKind::OrderedList | NodeKind::TaskList
                    ));
                let mut column = div().relative().flex().flex_col();
                if self
                    .session
                    .borrow()
                    .selection()
                    .is_all(self.session.borrow().document())
                {
                    column = column.bg(gpui::rgba(0x3377cc22)).min_h(px(28.0));
                }
                if matches!(kind, NodeKind::Quote) {
                    column = column.border_l_2().border_color(gpui::black()).pl_4();
                }
                let child_width = width.map(|width| {
                    if matches!(kind, NodeKind::Quote) {
                        width.inset(width.rem_size + px(2.0))
                    } else {
                        width
                    }
                });
                for (child_index, child) in children.into_iter().enumerate() {
                    column = column.child(self.render_block_tree_at_width(
                        child,
                        next_quote,
                        next_depth,
                        index + child_index,
                        child_width,
                        cx,
                    ));
                }
                // Paint the native composition overlay after the canonical
                // children so preedit stays visible above selected content.
                if let Some((anchor, input)) = &self.range_input
                    && *anchor == id
                    && matches!(kind, NodeKind::Document)
                {
                    let proxy = div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w_full()
                        .when(input.read(cx).is_composing(), |proxy| {
                            proxy.bg(gpui::white())
                        });
                    column = column.child(proxy.child(input.clone()));
                }
                column.into_any_element()
            }
            NodeContent::Atomic if matches!(kind, NodeKind::Image) => {
                let selected = {
                    let session = self.session.borrow();
                    session.selection().as_atomic_node() == Some(id)
                        || (self.image_sizing
                            == crate::image_block::ImageSizingPolicy::ResponsiveIntrinsic
                            && session.selection().as_node_selection() == Some(id))
                        || session.selection().is_all(session.document())
                };
                let (label, state_color) = self.image_placeholder_presentation(id);
                let source = self.image_render_source(id);
                let (width, height) = {
                    let session = self.session.borrow();
                    session
                        .document()
                        .node(id)
                        .and_then(|node| {
                            xiaomu_core::document::ImagePresentationAttrs::read(node.attrs())
                                .ok()
                                .map(|attrs| (attrs.width(), attrs.height()))
                        })
                        .unwrap_or_default()
                };
                let layout = ImageBlockLayout {
                    sizing: self.image_sizing,
                    cache: self.image_loads.clone(),
                    source_key: {
                        let session = self.session.borrow();
                        session.document().node(id).and_then(|node| {
                            xiaomu_core::document::ImagePresentationAttrs::read(node.attrs())
                                .ok()
                                .map(|attrs| attrs.source().value().to_owned())
                        })
                    },
                    width,
                    _height: height,
                };
                let presentation = ImageBlockPresentation {
                    selected,
                    label,
                    state_color,
                    source,
                };
                let image = render_image_block(id, index, &presentation, &layout, cx);
                if self.image_sizing == crate::image_block::ImageSizingPolicy::ResponsiveIntrinsic
                    && list_depth > 0
                {
                    // Ordinary list markers live on inline rows. Images need
                    // the same local content inset without a synthetic marker.
                    div()
                        .w_full()
                        .min_w_0()
                        .pl(px(markers::MARKER_COLUMN * list_depth as f32))
                        .child(image)
                        .into_any_element()
                } else {
                    image
                }
            }
            NodeContent::Atomic => {
                // Atomic blocks are whole-node selectable: the rule renders
                // thicker while its node selection is active, and a plain
                // click selects it instead of placing a text caret.
                let selected = {
                    let session = self.session.borrow();
                    session.selection().as_atomic_node() == Some(id)
                        || session.selection().is_all(session.document())
                };
                let (height, color) = if selected {
                    (px(5.0), gpui::rgba(0x2b6cb8ff))
                } else {
                    (px(3.0), gpui::rgba(0xccccccff))
                };
                div()
                    .id(("atomic-block", index))
                    .debug_selector(move || format!("atomic-block-{id:?}"))
                    .h(height)
                    .w_full()
                    .my_3()
                    .bg(color)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.select_atomic_block(id, window, cx);
                        }),
                    )
                    .into_any_element()
            }
            _ => div().into_any_element(),
        };
        self.render_node_selection(id, block, cx)
    }
}
