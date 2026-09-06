//! Mouse hit-testing and drag selection for [`DocumentView`].
//!
//! Split out of `actions.rs` so P3 clipboard / history wiring can grow
//! without stacking onto the same file as pointer dispatch.

use gpui::{App, Context, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Window};

use super::{DocumentView, navigation};
use crate::atom_capability::{ATOM_ACTION_CLICK, AtomAction};
use xiaomu_core::document::{AtomKind, NodeAttrs, NodeId, NodeKind};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, TextPoint};

/// The atom chip a pointer click landed inside, with the canonical snapshot
/// the host capability receives.
pub(crate) struct ChipHit {
    pub node: NodeId,
    pub kind: AtomKind,
    pub attrs: NodeAttrs,
}

/// One resolved pointer hit: the caret gap plus the chip, if any, that
/// absorbed the click.
pub(crate) struct MouseHit {
    pub point: InlinePoint,
    pub chip: Option<ChipHit>,
}

impl DocumentView {
    // ---- mouse ----

    pub(crate) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_dragging = true;
        if let Some(hit) = self.hit_test(event.position, cx) {
            #[cfg(debug_assertions)]
            {
                // Click-placement diagnostic: shows which block the click landed
                // on, so mis-hits are visible during real-machine testing.
                let point = hit.point;
                let clicked = {
                    let session = self.session.borrow();
                    session.document().node(point.node_id()).map(|node| {
                        let text = node
                            .content()
                            .as_inline()
                            .map(|inline| {
                                let text: String = inline
                                    .runs()
                                    .iter()
                                    .map(|run| run.text().as_str())
                                    .collect();
                                let preview: String = text.chars().take(8).collect();
                                format!(" \u{201c}{preview}\u{201d}")
                            })
                            .unwrap_or_default();
                        format!(
                            "{:?}{text} at byte {} ordinal {} ({:?})",
                            node.kind(),
                            point.text_offset().as_usize(),
                            point.atom_index(),
                            point.affinity()
                        )
                    })
                };
                if let Some(description) = clicked {
                    eprintln!("xiaomu: click placed caret in {description}");
                }
            }
            if event.modifiers.shift {
                self.move_focus_to(hit.point, true, window, cx);
            } else {
                self.place(hit.point, window, cx);
                // Activation is a plain click on a chip; shift-extend only
                // grows the selection.
                if let Some(chip) = hit.chip {
                    self.emit_atom_action(chip, ATOM_ACTION_CLICK);
                }
            }
        }
    }

    /// Forwards one atom activation to the host capability when installed.
    fn emit_atom_action(&self, chip: ChipHit, action: &'static str) {
        if let Some(capability) = self.atom_capability.clone() {
            capability.atom_action(AtomAction {
                node: chip.node,
                kind: chip.kind,
                action: action.into(),
                attrs: chip.attrs,
            });
        }
    }

    pub(crate) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_dragging {
            return;
        }
        if let Some(hit) = self.hit_test(event.position, cx) {
            self.move_focus_to(hit.point, true, window, cx);
        }
    }

    pub(crate) fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_dragging = false;
    }

    /// Maps a window-space point to a validated caret point via the paint
    /// registry: nearest block by vertical position, then two-dimensional
    /// hit-testing inside that block's wrapped text layout.
    ///
    /// Blocks carrying inline atoms shape their layout on renderer display
    /// bytes, so the raw hit is back-projected through the atom display
    /// projection: chip interiors resolve to the atom's before/after gap by
    /// click side, everything else maps through exact display boundaries.
    /// Plain blocks keep the canonical byte path. A click that landed strictly
    /// inside a renderer span also reports the chip for host activation.
    fn hit_test(&self, position: Point<Pixels>, cx: &App) -> Option<MouseHit> {
        let registry = self.registry.borrow();
        let mut nearest: Option<(NodeId, Pixels)> = None;
        for (node, bounds) in registry.iter() {
            let distance = if position.y < bounds.top() {
                bounds.top() - position.y
            } else if position.y > bounds.bottom() {
                position.y - bounds.bottom()
            } else {
                Pixels::ZERO
            };
            if nearest.is_none_or(|(_, best)| distance < best) {
                nearest = Some((*node, distance));
            }
        }
        let (node, _) = nearest?;

        let child = self
            .children
            .iter()
            .find(|(id, _)| *id == node)
            .map(|(_, view)| view.clone())?;
        let (raw, affinity) = child.read(cx).hit_test_caret_position(position)?;

        // During IME composition the layout falls back to the canonical
        // editable projection, so the raw hit is a canonical byte again.
        if !child.read(cx).is_composing()
            && let Some(projection) = child.read(cx).atom_display_projection()
            && !projection.atoms().is_empty()
        {
            let point = projection.inline_point_for_display_hit(raw, affinity)?;
            // Only strict span interiors count as chip activation; boundary
            // hits click beside the chip.
            let chip = projection
                .atom_at_display_offset(raw)
                .map(|span| span.atom())
                .and_then(|atom| self.chip_for(atom));
            return Some(MouseHit { point, chip });
        }

        let session = self.session.borrow();
        let blocks = navigation::text_blocks(session.document());
        drop(session);
        let block = blocks.iter().find(|block| block.node == node)?;
        let clamped = raw.min(block.text().len());
        let offset = navigation::validated_offset(block, clamped)
            .or_else(|| navigation::validated_offset(block, block.text().len()))?;
        let affinity = if clamped == raw {
            affinity
        } else {
            CursorAffinity::Before
        };
        Some(MouseHit {
            point: InlinePoint::from(TextPoint::new(node, offset, affinity)),
            chip: None,
        })
    }

    /// Builds the host-facing canonical snapshot for one atom node.
    fn chip_for(&self, atom: NodeId) -> Option<ChipHit> {
        let session = self.session.borrow();
        let node = session.document().node(atom)?;
        let NodeKind::InlineAtom(kind) = node.kind() else {
            return None;
        };
        Some(ChipHit {
            node: atom,
            kind: kind.clone(),
            attrs: node.attrs().clone(),
        })
    }
}
