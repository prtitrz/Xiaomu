//! Native tab targets layered on the same measured chip rectangles as paint.
use gpui::{App, Bounds, Entity, Pixels, Window};

use super::{ParagraphView, layout::BlockTextLayout};
use crate::{
    atom_capability::{ATOM_ACTION_CLICK, AtomAction},
    document_view::atom_keyboard::ChipFocus,
};

pub(super) fn record_targets(
    paragraph: &Entity<ParagraphView>,
    layout: &BlockTextLayout,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let view = paragraph.read(cx);
    let Some(shared) = view.atom_keyboard.clone() else {
        return;
    };
    if view.is_range_input() || view.input_is_hidden_by_table() {
        return;
    }
    let Some(capability) = shared.borrow().capability.clone() else {
        return;
    };
    let parent = view.node();
    let session = view.session.borrow();
    let candidates: Vec<_> = view
        .layout_atom_spans()
        .into_iter()
        .filter_map(|(span, range)| {
            let node = session.document().node(span.atom())?;
            let xiaomu_core::document::NodeKind::InlineAtom(kind) = node.kind() else {
                return None;
            };
            let action = AtomAction {
                node: span.atom(),
                kind: kind.clone(),
                action: ATOM_ACTION_CLICK.into(),
                attrs: node.attrs().clone(),
            };
            let placement = (parent, span.text_offset().as_usize(), span.atom_index());
            let rectangles: Vec<_> = layout
                .selection_rects(range)
                .into_iter()
                .filter_map(|mut rect| {
                    rect.origin += bounds.origin;
                    let visible = rect.intersect(&window.content_mask().bounds);
                    (visible.size.width > Pixels::ZERO && visible.size.height > Pixels::ZERO)
                        .then_some(visible)
                })
                .collect();
            (!rectangles.is_empty()).then_some((action, placement, rectangles))
        })
        .collect();
    drop(session);
    for (action, placement, rectangles) in candidates {
        if !capability.keyboard_focusable(&action) {
            continue;
        }
        let handle = {
            let mut state = shared.borrow_mut();
            let frame = state.frame;
            if state
                .entries
                .get(&action.node)
                .is_some_and(|entry| entry.action != action || entry.placement != placement)
            {
                state.entries.remove(&action.node);
            }
            let entry = state
                .entries
                .entry(action.node)
                .or_insert_with(|| ChipFocus {
                    handle: cx.focus_handle().tab_stop(true),
                    action,
                    placement,
                    frame,
                });
            entry.frame = frame;
            entry.handle.clone()
        };
        shared.borrow_mut().targets.push((handle, rectangles));
    }
}

impl ParagraphView {
    pub(super) fn chip_owns_input(&self, window: &Window) -> bool {
        self.atom_keyboard
            .as_ref()
            .is_some_and(|state| state.borrow().owns_focus(window))
    }
}
