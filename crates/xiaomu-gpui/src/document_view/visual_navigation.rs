//! Visual-line caret navigation for [`DocumentView`].
//!
//! Logical positions remain Core [`TextPoint`] values. This module consults
//! the last GPUI block layouts to translate vertical and visual-line gestures
//! while keeping `desired_x` as transient frontend state.

use gpui::{App, Context, Entity, Pixels, Window};
use xiaomu_core::document::NodeId;
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_runtime::session::DocumentPosition;

use crate::block_view::ParagraphView;

use super::{DocumentView, navigation};

/// One navigation step direction for the caret focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NavStep {
    /// One scalar left, respecting soft-wrap affinity before crossing bytes.
    Left,
    /// One scalar right, respecting soft-wrap affinity before crossing bytes.
    Right,
    /// One visual line up.
    Up,
    /// One visual line down.
    Down,
    /// Visual start of the current wrapped row.
    LineStart,
    /// Visual end of the current wrapped row.
    LineEnd,
}

impl DocumentView {
    /// Resolves the current focus as `(blocks, block index, TextPoint)`.
    fn visual_focus_location(&self) -> Option<(Vec<navigation::TextBlock>, usize, TextPoint)> {
        let session = self.session.borrow();
        let blocks = navigation::text_blocks(session.document());
        let focus = match session.selection().focus() {
            DocumentPosition::Text(point) => point,
            DocumentPosition::Gap(_) => return None,
        };
        let index = navigation::block_index(&blocks, focus.node_id())?;
        Some((blocks, index, focus))
    }

    fn child_for_node(&self, node: NodeId) -> Option<Entity<ParagraphView>> {
        self.children
            .iter()
            .find(|(id, _)| *id == node)
            .map(|(_, view)| view.clone())
    }

    fn point_for_target(
        blocks: &[navigation::TextBlock],
        block: usize,
        raw: usize,
        affinity: CursorAffinity,
    ) -> Option<TextPoint> {
        let target = blocks.get(block)?;
        let offset = navigation::validated_offset(target, raw)?;
        Some(TextPoint::new(target.node, offset, affinity))
    }

    fn horizontal_target(
        &self,
        blocks: &[navigation::TextBlock],
        block: usize,
        focus: TextPoint,
        forward: bool,
        cx: &App,
    ) -> Option<TextPoint> {
        let raw = focus.offset().as_usize();
        let child = self.child_for_node(focus.node_id());
        let at_wrap = child
            .as_ref()
            .is_some_and(|view| view.read(cx).visual_is_soft_wrap_boundary(raw));

        // A soft-wrap boundary has two visual caret positions for one logical
        // byte index. Traverse those first, then advance to another scalar.
        if at_wrap {
            if forward && focus.affinity().is_before() {
                return Self::point_for_target(blocks, block, raw, CursorAffinity::After);
            }
            if !forward && focus.affinity().is_after() {
                return Self::point_for_target(blocks, block, raw, CursorAffinity::Before);
            }
        }

        let (target_block, target_raw) = navigation::step_horizontal(blocks, block, raw, forward)?;
        let target_affinity = if !forward
            && self
                .child_for_node(blocks[target_block].node)
                .is_some_and(|view| view.read(cx).visual_is_soft_wrap_boundary(target_raw))
        {
            CursorAffinity::After
        } else {
            CursorAffinity::Before
        };
        Self::point_for_target(blocks, target_block, target_raw, target_affinity)
    }

    fn vertical_target(
        &self,
        blocks: &[navigation::TextBlock],
        block: usize,
        focus: TextPoint,
        down: bool,
        cx: &App,
    ) -> Option<(TextPoint, Pixels)> {
        let current = self.child_for_node(focus.node_id())?;
        let raw = focus.offset().as_usize();
        let desired_x = match self.desired_x {
            Some((anchor, x)) if anchor == focus => x,
            _ => current.read(cx).visual_caret_x(raw, focus.affinity())?,
        };

        if let Some((target_raw, affinity)) = current
            .read(cx)
            .visual_vertical_target(raw, focus.affinity(), desired_x, down)
        {
            let point = Self::point_for_target(blocks, block, target_raw, affinity)?;
            return Some((point, desired_x));
        }

        let target_block = if down {
            block.checked_add(1).filter(|index| *index < blocks.len())?
        } else {
            block.checked_sub(1)?
        };
        let target_child = self.child_for_node(blocks[target_block].node)?;
        let (target_raw, affinity) = target_child
            .read(cx)
            .visual_edge_row_target(desired_x, !down)?;
        let point = Self::point_for_target(blocks, target_block, target_raw, affinity)?;
        Some((point, desired_x))
    }

    fn line_edge_target(
        &self,
        blocks: &[navigation::TextBlock],
        block: usize,
        focus: TextPoint,
        to_end: bool,
        cx: &App,
    ) -> Option<TextPoint> {
        let raw = focus.offset().as_usize();
        if let Some(child) = self.child_for_node(focus.node_id())
            && let Some((target_raw, affinity)) = child
                .read(cx)
                .visual_line_edge_target(raw, focus.affinity(), to_end)
        {
            return Self::point_for_target(blocks, block, target_raw, affinity);
        }

        let (target_block, target_raw) = navigation::line_edge(blocks, block, to_end)?;
        Self::point_for_target(blocks, target_block, target_raw, CursorAffinity::Before)
    }

    /// Translates one keyboard gesture into a document selection update.
    pub(super) fn navigate(
        &mut self,
        step: NavStep,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.focused_child_composing(window, cx) {
            return;
        }
        let Some((blocks, block, focus)) = self.visual_focus_location() else {
            return;
        };

        match step {
            NavStep::Up | NavStep::Down => {
                let down = matches!(step, NavStep::Down);
                let Some((point, desired_x)) =
                    self.vertical_target(&blocks, block, focus, down, cx)
                else {
                    return;
                };
                self.move_focus_to(point, extend, window, cx);
                // `set_selection` clears transient navigation state. Restore
                // the continuity anchor only after a successful vertical move.
                self.desired_x = Some((point, desired_x));
            }
            NavStep::Left | NavStep::Right => {
                self.desired_x = None;
                let forward = matches!(step, NavStep::Right);
                let Some(point) = self.horizontal_target(&blocks, block, focus, forward, cx) else {
                    return;
                };
                self.move_focus_to(point, extend, window, cx);
            }
            NavStep::LineStart | NavStep::LineEnd => {
                self.desired_x = None;
                let to_end = matches!(step, NavStep::LineEnd);
                let Some(point) = self.line_edge_target(&blocks, block, focus, to_end, cx) else {
                    return;
                };
                self.move_focus_to(point, extend, window, cx);
            }
        }
    }
}
