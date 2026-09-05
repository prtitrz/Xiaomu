//! Visual-line caret navigation for [`DocumentView`].
//!
//! Focus endpoints stay Core [`InlinePoint`] values end to end. Blocks
//! without inline atoms keep the canonical byte layout path; blocks carrying
//! atoms translate focus through the display projection, so renderer bytes
//! never leak into Core coordinates and chip interiors are crossed as one
//! caret unit instead of becoming caret stops. `desired_x` remains transient
//! frontend state pairing the x column with its exact anchor.

use gpui::{App, Context, Entity, Pixels, Window};
use xiaomu_core::document::NodeId;
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_runtime::session::DocumentPosition;

use crate::block_view::ParagraphView;
use crate::inline_atom_display::InlineAtomDisplayProjection;

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
    /// Resolves the current focus as `(blocks, block index, InlinePoint)`.
    ///
    /// A same-boundary atom seam is a first-class caret position here; the
    /// per-step helpers decide which coordinate space the block layout speaks.
    fn visual_focus_location(&self) -> Option<(Vec<navigation::TextBlock>, usize, InlinePoint)> {
        let session = self.session.borrow();
        let blocks = navigation::text_blocks(session.document());
        let focus = match session.selection().focus() {
            DocumentPosition::Inline(point) => point,
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

    /// Returns the atom display projection for `node`, or `None` when the
    /// block carries no atoms and its layout bytes stay canonical.
    fn atom_projection_for(&self, node: NodeId, cx: &App) -> Option<InlineAtomDisplayProjection> {
        let child = self.child_for_node(node)?;
        let projection = child.read(cx).atom_display_projection()?;
        (!projection.atoms().is_empty()).then_some(projection)
    }

    /// Maps one display byte in `block`'s atom-aware layout back to a caret
    /// gap. Span interiors resolve by click side; exact boundaries map
    /// directly.
    fn point_for_display_byte(
        projection: &InlineAtomDisplayProjection,
        raw: usize,
        affinity: CursorAffinity,
    ) -> Option<InlinePoint> {
        projection
            .inline_point_for_display_boundary(raw, affinity)
            .or_else(|| projection.inline_point_for_display_hit(raw, affinity))
    }

    /// Maps one canonical byte in `block` back to a caret gap. Landing on the
    /// block's final byte selects the gap after any end-anchored atoms.
    fn point_for_canonical_byte(
        &self,
        blocks: &[navigation::TextBlock],
        block: usize,
        raw: usize,
        affinity: CursorAffinity,
        cx: &App,
    ) -> Option<InlinePoint> {
        let target = blocks.get(block)?;
        let offset = navigation::validated_offset(target, raw)?;
        let ordinal = if raw == target.text().len() {
            self.seam_ordinal_after(target.node, raw, cx)
        } else {
            0
        };
        Some(InlinePoint::new(target.node, offset, ordinal, affinity))
    }

    /// Number of atoms anchored at a canonical boundary, i.e. the ordinal of
    /// the gap right after them.
    fn seam_ordinal_after(&self, node: NodeId, canonical_raw: usize, cx: &App) -> usize {
        self.atom_projection_for(node, cx)
            .map(|projection| {
                projection
                    .atoms()
                    .iter()
                    .filter(|atom| atom.text_offset().as_usize() == canonical_raw)
                    .count()
            })
            .unwrap_or(0)
    }

    fn horizontal_target(
        &self,
        blocks: &[navigation::TextBlock],
        block: usize,
        focus: InlinePoint,
        forward: bool,
        cx: &App,
    ) -> Option<InlinePoint> {
        if let Some(projection) = self.atom_projection_for(focus.node_id(), cx) {
            return self.atom_horizontal_target(blocks, block, focus, forward, cx, &projection);
        }

        let raw = focus.text_offset().as_usize();
        let child = self.child_for_node(focus.node_id());
        let at_wrap = child
            .as_ref()
            .is_some_and(|view| view.read(cx).visual_is_soft_wrap_boundary(raw));

        // A soft-wrap boundary has two visual caret positions for one logical
        // byte index. Traverse those first, then advance to another scalar.
        if at_wrap {
            if forward && focus.affinity().is_before() {
                return self.point_for_canonical_byte(
                    blocks,
                    block,
                    raw,
                    CursorAffinity::After,
                    cx,
                );
            }
            if !forward && focus.affinity().is_after() {
                return self.point_for_canonical_byte(
                    blocks,
                    block,
                    raw,
                    CursorAffinity::Before,
                    cx,
                );
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
        self.point_for_canonical_byte(blocks, target_block, target_raw, target_affinity, cx)
    }

    fn atom_horizontal_target(
        &self,
        blocks: &[navigation::TextBlock],
        block: usize,
        focus: InlinePoint,
        forward: bool,
        cx: &App,
        projection: &InlineAtomDisplayProjection,
    ) -> Option<InlinePoint> {
        let raw = projection.display_offset_for_inline_point(focus)?;
        let child = self.child_for_node(focus.node_id());
        let at_wrap = child
            .as_ref()
            .is_some_and(|view| view.read(cx).visual_is_soft_wrap_boundary(raw));
        if at_wrap {
            if forward && focus.affinity().is_before() {
                return projection.inline_point_for_display_boundary(raw, CursorAffinity::After);
            }
            if !forward && focus.affinity().is_after() {
                return projection.inline_point_for_display_boundary(raw, CursorAffinity::Before);
            }
        }

        let display_len = projection.display_text().len();
        if (forward && raw >= display_len) || (!forward && raw == 0) {
            // Cross-block: the neighbor walk speaks canonical bytes.
            let canonical = if forward {
                projection.canonical_text().len()
            } else {
                0
            };
            let (target_block, target_raw) =
                navigation::step_horizontal(blocks, block, canonical, forward)?;
            return self.point_for_canonical_byte(
                blocks,
                target_block,
                target_raw,
                CursorAffinity::Before,
                cx,
            );
        }

        // Step one scalar in display space; renderer interiors are skipped as
        // a single caret unit so a chip never becomes a caret stop.
        let text = projection.display_text();
        let stepped = if forward {
            (raw + 1..=display_len)
                .find(|&index| text.is_char_boundary(index))
                .map(|next| match projection.atom_at_display_offset(next) {
                    Some(atom) => atom.display_range().end,
                    None => next,
                })?
        } else {
            (0..raw)
                .rev()
                .find(|&index| text.is_char_boundary(index))
                .map(|prev| match projection.atom_at_display_offset(prev) {
                    Some(atom) => atom.display_range().start,
                    None => prev,
                })?
        };
        let target_affinity = if !forward
            && child
                .as_ref()
                .is_some_and(|view| view.read(cx).visual_is_soft_wrap_boundary(stepped))
        {
            CursorAffinity::After
        } else {
            CursorAffinity::Before
        };
        Self::point_for_display_byte(projection, stepped, target_affinity)
    }

    fn vertical_target(
        &self,
        blocks: &[navigation::TextBlock],
        block: usize,
        focus: InlinePoint,
        down: bool,
        cx: &App,
    ) -> Option<(InlinePoint, Pixels)> {
        let current = self.child_for_node(focus.node_id())?;
        let projection = self.atom_projection_for(focus.node_id(), cx);
        let raw = match &projection {
            Some(projection) => projection.display_offset_for_inline_point(focus)?,
            None => focus.text_offset().as_usize(),
        };
        let desired_x = match self.desired_x {
            Some((anchor, x)) if anchor == focus => x,
            _ => current.read(cx).visual_caret_x(raw, focus.affinity())?,
        };

        if let Some((target_raw, affinity)) =
            current
                .read(cx)
                .visual_vertical_target(raw, focus.affinity(), desired_x, down)
        {
            let point = match &projection {
                Some(projection) => Self::point_for_display_byte(projection, target_raw, affinity)?,
                None => self.point_for_canonical_byte(blocks, block, target_raw, affinity, cx)?,
            };
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
        // The neighbor's layout bytes are display space when it carries atoms.
        if let Some(target_projection) = self.atom_projection_for(blocks[target_block].node, cx) {
            let point = Self::point_for_display_byte(&target_projection, target_raw, affinity)?;
            return Some((point, desired_x));
        }
        let point =
            self.point_for_canonical_byte(blocks, target_block, target_raw, affinity, cx)?;
        Some((point, desired_x))
    }

    fn line_edge_target(
        &self,
        blocks: &[navigation::TextBlock],
        block: usize,
        focus: InlinePoint,
        to_end: bool,
        cx: &App,
    ) -> Option<InlinePoint> {
        let projection = self.atom_projection_for(focus.node_id(), cx);
        let raw = match &projection {
            Some(projection) => projection.display_offset_for_inline_point(focus)?,
            None => focus.text_offset().as_usize(),
        };
        if let Some(child) = self.child_for_node(focus.node_id())
            && let Some((target_raw, affinity)) =
                child
                    .read(cx)
                    .visual_line_edge_target(raw, focus.affinity(), to_end)
        {
            let point = match &projection {
                Some(projection) => Self::point_for_display_byte(projection, target_raw, affinity)?,
                None => self.point_for_canonical_byte(blocks, block, target_raw, affinity, cx)?,
            };
            return Some(point);
        }

        let (target_block, target_raw) = navigation::line_edge(blocks, block, to_end)?;
        self.point_for_canonical_byte(blocks, target_block, target_raw, CursorAffinity::Before, cx)
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
                // `set_inline_selection` clears transient navigation state.
                // Restore the continuity anchor only after a successful move.
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
