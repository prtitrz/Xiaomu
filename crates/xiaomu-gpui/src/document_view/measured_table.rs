//! Per-view opt-in connection of genuine child layout, table admission and input.
//! Automatic columns use the documented native equal-remainder policy; this is
//! not an intrinsic/min-content CSS table-width implementation.

use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, Context, Element, ElementId, Entity, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, MouseButton, ParentElement, Pixels, Styled, Window,
    div, prelude::InteractiveElement, px,
};
use xiaomu_core::document::NodeId;
use xiaomu_runtime::session::DocumentPosition;

use super::column_resize::{
    ResizeMeasurement, paint_resize_cursor, register_resize_pointer_handlers,
};
use super::{DocumentView, navigation};
use crate::block_view::BlockBoundsRegistry;
use crate::table_capability::{SharedTableCapability, TableCapabilityKey};
use crate::table_layout::{
    SpanningTableElement, TableCellElement, TableElementLayoutState, TableElementPrepaintState,
    TableLayoutPlan,
};
use crate::table_viewport::SameFrameTableViewport;

const CELL_HORIZONTAL_PADDING: f32 = 12.0;
const CELL_VERTICAL_PADDING: f32 = 9.0;

#[derive(Clone, Copy)]
pub(super) struct BlockLayoutWidth {
    pub available: Pixels,
    pub rem_size: Pixels,
}

impl BlockLayoutWidth {
    pub(super) fn inset(self, inset: Pixels) -> Self {
        Self {
            available: (self.available - inset).max(px(0.0)),
            ..self
        }
    }
}

impl DocumentView {
    pub(super) fn render_measured_viewport(&self, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity();
        SameFrameTableViewport::new("xiaomu-measured-viewport", move |viewport, window, cx| {
            view.update(cx, |this, cx| {
                this.registry.borrow_mut().clear();
                this.cell_registry.borrow_mut().clear();
                this.column_resize.clear_measurements();
                let width = BlockLayoutWidth {
                    available: (viewport.width - window.rem_size() * 2.0).max(px(0.0)),
                    rem_size: window.rem_size(),
                };
                let root = this.session.borrow().document().root();
                let tree = this.render_block_tree_at_width(root, false, 0, 0, Some(width), cx);
                FocusAfterMeasurement {
                    content: this.render_scroll_tree(tree, cx),
                    view: cx.entity(),
                }
            })
        })
        .into_any_element()
    }

    pub(super) fn render_measured_table(
        &self,
        table: NodeId,
        index: usize,
        width: BlockLayoutWidth,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let prepared = {
            let session = self.session.borrow();
            let document = session.document();
            self.table_capability
                .borrow()
                .key(document, table)
                .and_then(|key| {
                    let plan = TableLayoutPlan::from_document(document, table, Default::default())?;
                    let plan = self
                        .column_resize
                        .preview_plan(plan, f32::from(width.available))?;
                    let geometry =
                        plan.layout(f32::from(width.available), &vec![0.0; plan.cells().len()])?;
                    let selection = session.selection();
                    let focused =
                        navigation::selection_is_within(document, selection.focus(), table);
                    let focus_node = match selection.focus() {
                        DocumentPosition::Inline(point) => point.node_id(),
                        DocumentPosition::Atomic(node) => node,
                        DocumentPosition::Gap(gap) => gap.parent(),
                    };
                    let highlighted = selection
                        .active_cell_range()
                        .and_then(|range| range.unique_origins(document).ok())
                        .unwrap_or_else(|| {
                            navigation::table_cell_ancestor(document, focus_node)
                                .into_iter()
                                .collect()
                        });
                    let backgrounds = plan
                        .cells()
                        .iter()
                        .map(|cell| {
                            crate::table_capability::cell_background(
                                document
                                    .node(cell.placement.cell())
                                    .expect("checked cell")
                                    .attrs(),
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok((
                        key,
                        plan,
                        geometry,
                        backgrounds,
                        focused,
                        highlighted,
                        document.clone(),
                    ))
                })
        };
        let Ok((key, plan, geometry, backgrounds, focused, highlighted, document)) = prepared
        else {
            self.table_capability.borrow_mut().revoke(table);
            return measured_placeholder(table);
        };
        let mut children = Vec::with_capacity(plan.cells().len());
        for (cell_index, ((cell, bounds), background)) in plan
            .cells()
            .iter()
            .zip(&geometry.cells)
            .zip(backgrounds)
            .enumerate()
        {
            let id = cell.placement.cell();
            let content_width = BlockLayoutWidth {
                available: px(bounds.width),
                ..width
            }
            .inset(px(CELL_HORIZONTAL_PADDING * 2.0));
            let mut content = div()
                .debug_selector(move || format!("measured-table-cell-{id:?}"))
                .id(gpui::SharedString::from(format!(
                    "measured-table-cell-{id:?}"
                )))
                .relative()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .px(px(CELL_HORIZONTAL_PADDING))
                .py(px(CELL_VERTICAL_PADDING))
                .text_size(px(13.0))
                .line_height(px(13.0 * 1.55));
            if cell.is_header {
                content = content.font_weight(gpui::FontWeight::SEMIBOLD);
            }
            content = content.child(self.render_block_tree_at_width(
                id,
                false,
                0,
                index + cell_index,
                Some(content_width),
                cx,
            ));
            if let Some((anchor, input)) = &self.range_input
                && *anchor == id
            {
                let mut proxy = div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .px(px(CELL_HORIZONTAL_PADDING))
                    .py(px(CELL_VERTICAL_PADDING));
                if input.read(cx).is_composing() {
                    proxy = proxy.bg(gpui::white());
                }
                content = content.child(proxy.child(input.clone()));
            }
            content = content.child(
                div()
                    .id("cell-select")
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(px(7.0))
                    .h(px(7.0))
                    .bg(gpui::rgba(0x718096ff))
                    .cursor(gpui::CursorStyle::Crosshair)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.begin_cell_range(id, event.modifiers.shift, window, cx);
                        }),
                    ),
            );
            let mut child = TableCellElement::new(id, content);
            child.background = if highlighted.contains(&id) {
                Some(gpui::rgba(0xeef4fbff).into())
            } else {
                background.or_else(|| cell.is_header.then(|| gpui::rgba(0xf7f6f3ff).into()))
            };
            child.border = Some(
                if focused {
                    gpui::rgba(0x2b6cb8ff)
                } else {
                    gpui::rgba(0xccccccff)
                }
                .into(),
            );
            children.push(child);
        }
        let placements = plan.cells().iter().map(|cell| cell.placement).collect();
        let Ok(element) = SpanningTableElement::new(plan, width.available, children) else {
            self.table_capability.borrow_mut().revoke(table);
            return measured_placeholder(table);
        };
        div()
            .debug_selector(move || format!("measured-table-{table:?}"))
            .my_3()
            .w(px(geometry.width))
            .min_w(px(geometry.width))
            .flex_shrink_0()
            .child(AdmittedTable {
                table,
                key,
                capability: self.table_capability.clone(),
                registry: self.cell_registry.clone(),
                resize_measurements: self
                    .column_resize
                    .enabled()
                    .then(|| self.column_resize.measurements.clone()),
                document,
                placements,
                available: f32::from(width.available),
                element,
            })
            .into_any_element()
    }
}

fn measured_placeholder(table: NodeId) -> AnyElement {
    div()
        .debug_selector(move || format!("unsupported-measured-table-{table:?}"))
        .border_1()
        .p_2()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child("This table's layout is not available")
        .into_any_element()
}

struct AdmittedTable {
    table: NodeId,
    key: Rc<TableCapabilityKey>,
    capability: SharedTableCapability,
    registry: BlockBoundsRegistry,
    resize_measurements: Option<Rc<std::cell::RefCell<Vec<ResizeMeasurement>>>>,
    document: xiaomu_core::document::XiaomuDocument,
    placements: Vec<xiaomu_core::document::CellPlacement>,
    available: f32,
    element: SpanningTableElement,
}

impl IntoElement for AdmittedTable {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for AdmittedTable {
    type RequestLayoutState = TableElementLayoutState;
    type PrepaintState = TableElementPrepaintState;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let result = self.element.request_layout(id, inspector, window, cx);
        self.capability.borrow_mut().record(
            self.table,
            self.key.clone(),
            result.1.geometry.is_ok(),
        );
        result
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        // Parent cells precede nested cells, so reverse hit testing selects
        // the innermost containing cell, including its blank lower region.
        if let Ok(geometry) = &request.geometry {
            if let Some(measurements) = &self.resize_measurements {
                measurements.borrow_mut().push(ResizeMeasurement {
                    table: self.table,
                    revision: self.document.revision(),
                    document: self.document.clone(),
                    key: self.key.clone(),
                    origin: bounds.origin,
                    available: self.available,
                    clip: window.content_mask().bounds,
                    geometry: geometry.clone(),
                    placements: self.placements.clone(),
                });
            }
            self.registry.borrow_mut().extend(
                geometry
                    .cells
                    .iter()
                    .map(|cell| (cell.cell, cell.bounds_at(bounds.origin))),
            );
        }
        self.element
            .prepaint(id, inspector, bounds, request, window, cx)
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.element
            .paint(id, inspector, bounds, request, prepaint, window, cx);
    }
}

/// Measurements finish during child request_layout, before this wrapper's
/// prepaint. Route an already-owned focus only after every table has admitted
/// or revoked its key, then let children prepaint/paint real native handlers.
struct FocusAfterMeasurement {
    content: AnyElement,
    view: Entity<DocumentView>,
}

impl IntoElement for FocusAfterMeasurement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for FocusAfterMeasurement {
    type RequestLayoutState = ();
    type PrepaintState = gpui::Hitbox;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.content.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::Hitbox {
        let hitbox = window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal);
        self.view.update(cx, |view, cx| {
            let owned = view
                .focus_handle
                .as_ref()
                .is_some_and(|handle| handle.is_focused(window))
                || view.focused_child(window, cx).is_some();
            let hidden: Vec<_> = {
                let session = view.session.borrow();
                let capability = view.table_capability.borrow();
                view.children
                    .iter()
                    .map(|(node, child)| (*node, child.clone(), false))
                    .chain(
                        view.range_input
                            .iter()
                            .map(|(node, child)| (*node, child.clone(), true)),
                    )
                    .filter(|(node, _, range)| {
                        crate::table_capability::handler_is_hidden_by_table(
                            session.document(),
                            session.selection(),
                            *node,
                            *range,
                            &capability,
                        )
                    })
                    .map(|(_, child, _)| child)
                    .collect()
            };
            for child in hidden {
                child.update(cx, |child, cx| child.cancel_if_composing(cx));
            }
            if owned {
                view.route_focus(window, cx);
            }
        });
        self.content.prepaint(window, cx);
        self.view.update(cx, |view, cx| {
            view.finish_column_resize_measurement(window, cx)
        });
        hitbox
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        hitbox: &mut gpui::Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        register_resize_pointer_handlers(&self.view, hitbox.clone(), window, cx);
        self.content.paint(window, cx);
        paint_resize_cursor(&self.view, hitbox, window, cx);
    }
}
