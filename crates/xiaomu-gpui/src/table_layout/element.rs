//! Public GPUI lifecycle prototype; no registry internals or paint transforms.

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, LayoutId, ParentElement, Pixels, Style, Styled, Window, div,
    fill, outline, px, size,
};
use xiaomu_core::document::NodeId;

use super::{TableGeometry, TableLayoutError, TableLayoutPlan};

/// A real child subtree and optional full-cell decoration.
pub struct TableCellElement {
    /// Cell identity; order must match the plan's unique origins.
    pub cell: NodeId,
    /// Ordinary GPUI content, retaining its own caret, hit and input handlers.
    pub content: AnyElement,
    /// Optional full-cell fill; a Header can use a different host-owned fill.
    pub background: Option<Hsla>,
    /// Optional one-pixel full-cell outline.
    pub border: Option<Hsla>,
}

impl TableCellElement {
    /// Creates an undecorated child. The host owns cell padding and styling.
    #[must_use]
    pub fn new(cell: NodeId, content: impl IntoElement) -> Self {
        Self {
            cell,
            content: content.into_any_element(),
            background: None,
            border: None,
        }
    }
}

/// An opt-in custom element that requires a same-frame known content width.
///
/// Construct a new element for each render. Width resolution and measurement
/// happen before the parent requests its layout. Changing available width
/// requires reconstructing this element, not post-paint translation or cached
/// heights. This is not yet a drop-in `w_full()` replacement for the renderer.
/// A second direct `request_layout` call is explicitly refused with
/// [`TableLayoutError::RepeatedLayoutRequest`]; GPUI's ordinary `AnyElement`
/// wrapper already enforces a single request. Any layout error renders a
/// visible, noninteractive placeholder and does not prepaint real children.
pub struct SpanningTableElement {
    plan: TableLayoutPlan,
    available_width: f32,
    children: Option<Vec<TableCellElement>>,
}

impl SpanningTableElement {
    /// Validates explicit sizing and the identity/order of child subtrees.
    pub fn new(
        plan: TableLayoutPlan,
        available_width: Pixels,
        children: Vec<TableCellElement>,
    ) -> Result<Self, TableLayoutError> {
        let available_width = f32::from(available_width);
        plan.column_widths(available_width)?;
        if children.len() != plan.cells().len()
            || children
                .iter()
                .zip(plan.cells())
                .any(|(child, cell)| child.cell != cell.placement.cell())
        {
            return Err(TableLayoutError::CellMismatch);
        }
        Ok(Self {
            plan,
            available_width,
            children: Some(children),
        })
    }
}

/// Layout-phase result, exposed so prototype hosts/tests can inspect failures.
pub struct TableElementLayoutState {
    /// One geometry source for cell placement, backgrounds and full-cell hits.
    pub geometry: Result<TableGeometry, TableLayoutError>,
    children: Vec<TableCellElement>,
    fallback: Option<AnyElement>,
}

/// Absolute full-cell bounds, generated in prepaint before any child paints.
#[derive(Default)]
pub struct TableElementPrepaintState {
    /// Unique cells paired with their full selectable rectangles.
    pub cells: Vec<(NodeId, Bounds<Pixels>)>,
    /// Visible placeholder bounds on failure; failed children have no bounds
    /// or input registration. A host must retain its edit-admission guards.
    pub unavailable_bounds: Option<Bounds<Pixels>>,
}

impl IntoElement for SpanningTableElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for SpanningTableElement {
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
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut children = self.children.take();
        // Zero-height geometry supplies exactly the same shared column edges
        // used below by the final measured geometry.
        let geometry = children
            .as_mut()
            .ok_or(TableLayoutError::RepeatedLayoutRequest)
            .and_then(|children| {
                let provisional = self
                    .plan
                    .layout(self.available_width, &vec![0.0; children.len()])?;
                let mut heights = Vec::with_capacity(children.len());
                for (child, cell) in children.iter_mut().zip(&provisional.cells) {
                    let content =
                        std::mem::replace(&mut child.content, gpui::Empty.into_any_element());
                    // An explicit wrapper forces available width into a real
                    // layout constraint, even for otherwise intrinsic child roots.
                    child.content = div()
                        .flex()
                        .flex_col()
                        .w(px(cell.width))
                        .min_w(px(cell.width))
                        .max_w(px(cell.width))
                        .child(content)
                        .into_any_element();
                    let measured = child.content.layout_as_root(
                        size(
                            AvailableSpace::Definite(px(cell.width)),
                            AvailableSpace::MaxContent,
                        ),
                        window,
                        cx,
                    );
                    heights.push(f32::from(measured.height));
                }
                self.plan.layout(self.available_width, &heights)
            });
        let mut fallback = None;
        let measured = match &geometry {
            Ok(geometry) => size(px(geometry.width), px(geometry.height)),
            Err(_) => {
                // Keep refusal visible. Never make an unavailable table a
                // zero-sized success or reuse partially measured input trees.
                let width = px(self.available_width.max(40.0));
                let mut placeholder = div()
                    .w(width)
                    .min_h(px(28.0))
                    .border_1()
                    .p_2()
                    .child("Table layout is unavailable")
                    .into_any_element();
                let measured = placeholder.layout_as_root(
                    size(AvailableSpace::Definite(width), AvailableSpace::MaxContent),
                    window,
                    cx,
                );
                fallback = Some(placeholder);
                measured
            }
        };
        let mut style = Style::default();
        style.size = size(measured.width.into(), measured.height.into());
        style.min_size = style.size;
        style.max_size = style.size;
        style.flex_shrink = 0.0;
        (
            window.request_layout(style, None, cx),
            TableElementLayoutState {
                geometry,
                children: children.unwrap_or_default(),
                fallback,
            },
        )
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        if let Some(fallback) = &mut request.fallback {
            fallback.prepaint_at(bounds.origin, window, cx);
            return TableElementPrepaintState {
                cells: Vec::new(),
                unavailable_bounds: Some(bounds),
            };
        }
        let Ok(geometry) = &request.geometry else {
            return TableElementPrepaintState::default();
        };
        let mut cells = Vec::with_capacity(geometry.cells.len());
        for (child, cell) in request.children.iter_mut().zip(&geometry.cells) {
            let cell_bounds = cell.bounds_at(bounds.origin);
            // Child prepaint, including its input handler/IME geometry, sees
            // the final absolute position. Paint uses those committed bounds.
            child.content.prepaint_at(cell_bounds.origin, window, cx);
            cells.push((cell.cell, cell_bounds));
        }
        TableElementPrepaintState {
            cells,
            unavailable_bounds: None,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        request: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(fallback) = &mut request.fallback {
            fallback.paint(window, cx);
            return;
        }
        for (child, (_, bounds)) in request.children.iter_mut().zip(&prepaint.cells) {
            if let Some(background) = child.background {
                window.paint_quad(fill(*bounds, background));
            }
            child.content.paint(window, cx);
            if let Some(border) = child.border {
                window.paint_quad(outline(*bounds, border, gpui::BorderStyle::Solid));
            }
        }
    }
}
