//! Mixed-inline canonical-to-display coordinate projection.
//!
//! Canonical [`TextOffset`](xiaomu_core::text::TextOffset) values count only
//! UTF-8 text bytes. Inline atoms count as caret units but consume no canonical
//! bytes, while their renderer output does consume bytes in GPUI's shaped
//! display string. This module keeps those coordinate spaces explicit so
//! layout, selection, hit-testing, and platform input never assume that a
//! display byte index is a canonical text offset.

use std::ops::Range;

use xiaomu_core::document::{NodeId, NodeKind, XiaomuDocument};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::{TextBuffer, TextOffset};

use crate::inline_atom::{InlineAtomRendererRegistry, InlineAtomView};

/// One rendered atom span inside a mixed-inline display string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineAtomDisplaySpan {
    atom: NodeId,
    text_offset: TextOffset,
    atom_index: usize,
    display_range: Range<usize>,
}

impl InlineAtomDisplaySpan {
    /// Returns the canonical atom identity.
    #[must_use]
    pub const fn atom(&self) -> NodeId {
        self.atom
    }

    /// Returns the canonical text boundary anchoring this atom.
    #[must_use]
    pub const fn text_offset(&self) -> TextOffset {
        self.text_offset
    }

    /// Returns this atom's ordinal among atoms sharing the same text boundary.
    #[must_use]
    pub const fn atom_index(&self) -> usize {
        self.atom_index
    }

    /// Returns the UTF-8 byte range occupied by the renderer output.
    #[must_use]
    pub const fn display_range(&self) -> &Range<usize> {
        &self.display_range
    }
}

/// GPUI-local projection of one canonical mixed-inline node.
///
/// `canonical_text` contains text runs only. `display_text` additionally
/// splices renderer output at each atom placement. The projection records the
/// exact relationship between canonical caret gaps and display byte boundaries
/// without changing Core's coordinate contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineAtomDisplayProjection {
    node: NodeId,
    canonical_text: TextBuffer,
    display_text: String,
    atoms: Vec<InlineAtomDisplaySpan>,
}

impl InlineAtomDisplayProjection {
    /// Builds the display projection for one canonical inline-bearing node.
    ///
    /// Returns `None` only when `node` is not a valid inline-bearing node or a
    /// referenced atom does not have the canonical inline-atom shape. A custom
    /// renderer returning an empty string fails soft to the atom's non-empty
    /// canonical `fallback_text`, preventing zero-width atomic caret units.
    #[must_use]
    pub fn build(
        document: &XiaomuDocument,
        node: NodeId,
        renderers: &InlineAtomRendererRegistry,
    ) -> Option<Self> {
        let inline = document.node(node)?.content().as_inline()?;
        let canonical_text = TextBuffer::from_string(
            inline
                .runs()
                .iter()
                .map(|run| run.text().as_str())
                .collect(),
        );

        let mut display_text = String::new();
        let mut atoms = Vec::with_capacity(inline.atoms().len());
        let mut canonical_cursor = 0usize;
        let mut previous_offset = None;
        let mut atom_index = 0usize;

        for placement in inline.atoms() {
            let raw = placement.text_offset().as_usize();
            if raw < canonical_cursor
                || canonical_text
                    .validate_offset(placement.text_offset())
                    .is_err()
            {
                return None;
            }
            display_text.push_str(&canonical_text.as_str()[canonical_cursor..raw]);
            canonical_cursor = raw;

            if previous_offset == Some(raw) {
                atom_index += 1;
            } else {
                previous_offset = Some(raw);
                atom_index = 0;
            }

            let atom = document.node(placement.atom())?;
            let NodeKind::InlineAtom(kind) = atom.kind() else {
                return None;
            };
            let content = atom.content().as_inline_atom()?;
            let view = InlineAtomView::new(
                placement.atom(),
                kind.clone(),
                content.fallback_text(),
                atom.attrs().clone(),
            );
            let renderer = renderers.renderer_for(kind);
            let mut rendered = renderer.display_text(&view);
            if rendered.is_empty() {
                rendered = content.fallback_text().to_owned();
            }

            let start = display_text.len();
            display_text.push_str(&rendered);
            let end = display_text.len();
            atoms.push(InlineAtomDisplaySpan {
                atom: placement.atom(),
                text_offset: placement.text_offset(),
                atom_index,
                display_range: start..end,
            });
        }

        display_text.push_str(&canonical_text.as_str()[canonical_cursor..]);
        Some(Self {
            node,
            canonical_text,
            display_text,
            atoms,
        })
    }

    /// Returns the inline node represented by this projection.
    #[must_use]
    pub const fn node(&self) -> NodeId {
        self.node
    }

    /// Returns canonical UTF-8 text without atom renderer output.
    #[must_use]
    pub fn canonical_text(&self) -> &str {
        self.canonical_text.as_str()
    }

    /// Returns the shaped display text including atom renderer output.
    #[must_use]
    pub fn display_text(&self) -> &str {
        &self.display_text
    }

    /// Returns rendered atom spans in canonical order.
    #[must_use]
    pub fn atoms(&self) -> &[InlineAtomDisplaySpan] {
        &self.atoms
    }

    /// Maps one canonical mixed-inline caret gap to an exact display boundary.
    ///
    /// This mapping is total for valid `InlinePoint`s of this node. The
    /// returned byte index is always a UTF-8 boundary in [`Self::display_text`].
    #[must_use]
    pub fn display_offset_for_inline_point(&self, point: InlinePoint) -> Option<usize> {
        if point.node_id() != self.node
            || self
                .canonical_text
                .validate_offset(point.text_offset())
                .is_err()
        {
            return None;
        }
        let raw = point.text_offset().as_usize();
        let atom_count = self
            .atoms
            .iter()
            .filter(|atom| atom.text_offset.as_usize() == raw)
            .count();
        if point.atom_index() > atom_count {
            return None;
        }

        let inserted_before: usize = self
            .atoms
            .iter()
            .filter(|atom| {
                let atom_raw = atom.text_offset.as_usize();
                atom_raw < raw || (atom_raw == raw && atom.atom_index < point.atom_index())
            })
            .map(|atom| atom.display_range.len())
            .sum();
        Some(raw + inserted_before)
    }

    /// Maps an exact display byte boundary back to a canonical caret gap.
    ///
    /// Display positions strictly inside an atom renderer span are not
    /// canonical caret boundaries and therefore return `None`. Hit-testing
    /// should use [`Self::inline_point_for_display_hit`], which also resolves
    /// span interiors to the atom's before/after gap by click side.
    #[must_use]
    pub fn inline_point_for_display_boundary(
        &self,
        display_offset: usize,
        affinity: CursorAffinity,
    ) -> Option<InlinePoint> {
        if display_offset > self.display_text.len()
            || !self.display_text.is_char_boundary(display_offset)
        {
            return None;
        }

        for atom in &self.atoms {
            if display_offset > atom.display_range.start && display_offset < atom.display_range.end
            {
                return None;
            }
            if display_offset == atom.display_range.start {
                return Some(InlinePoint::new(
                    self.node,
                    atom.text_offset,
                    atom.atom_index,
                    affinity,
                ));
            }
            if display_offset == atom.display_range.end {
                return Some(InlinePoint::new(
                    self.node,
                    atom.text_offset,
                    atom.atom_index + 1,
                    affinity,
                ));
            }
        }

        let inserted_before: usize = self
            .atoms
            .iter()
            .filter(|atom| atom.display_range.end <= display_offset)
            .map(|atom| atom.display_range.len())
            .sum();
        let raw = display_offset.checked_sub(inserted_before)?;
        let offset = self.canonical_text.offset_at(raw).ok()?;
        Some(InlinePoint::new(self.node, offset, 0, affinity))
    }

    /// Returns the atom whose rendered bytes contain `display_offset`.
    ///
    /// The end boundary is excluded, matching Rust range semantics. Pointer
    /// hit-testing combines this with [`Self::inline_point_for_display_hit`].
    #[must_use]
    pub fn atom_at_display_offset(&self, display_offset: usize) -> Option<&InlineAtomDisplaySpan> {
        self.atoms
            .iter()
            .find(|atom| atom.display_range.contains(&display_offset))
    }

    /// Resolves a pointer hit at `display_offset` into a canonical caret gap.
    ///
    /// Display bytes strictly inside an atom span resolve to the span's before
    /// or after gap by click side: bytes left of the span midpoint keep
    /// [`InlineAtomDisplaySpan::atom_index`], bytes right of it select the
    /// following gap (`atom_index + 1`). Every other display boundary maps
    /// through [`Self::inline_point_for_display_boundary`].
    #[must_use]
    pub fn inline_point_for_display_hit(
        &self,
        display_offset: usize,
        affinity: CursorAffinity,
    ) -> Option<InlinePoint> {
        if let Some(atom) = self.atom_at_display_offset(display_offset) {
            let range = atom.display_range();
            let before = display_offset < range.start + range.len().div_ceil(2);
            let ordinal = if before {
                atom.atom_index()
            } else {
                atom.atom_index() + 1
            };
            return Some(InlinePoint::new(
                self.node,
                atom.text_offset(),
                ordinal,
                CursorAffinity::Before,
            ));
        }
        self.inline_point_for_display_boundary(display_offset, affinity)
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::inline_atom::InlineAtomRenderer;
    use xiaomu_core::document::{
        AtomKind, InlineAtomContent, InlineContent, NodeAttrs, NodeContent, NodeStoreBuilder,
        TextRun,
    };
    use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

    struct WrappedRenderer;

    impl InlineAtomRenderer for WrappedRenderer {
        fn display_text(&self, atom: &InlineAtomView) -> String {
            format!("«{}»", atom.fallback_text())
        }
    }

    struct EmptyRenderer;

    impl InlineAtomRenderer for EmptyRenderer {
        fn display_text(&self, _: &InlineAtomView) -> String {
            String::new()
        }
    }

    fn fixture() -> (XiaomuDocument, NodeId) {
        let mut builder = NodeStoreBuilder::new();
        let paragraph = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("A中B", Default::default()).unwrap()])
                        .unwrap(),
                ),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([paragraph]),
            )
            .unwrap();
        (
            XiaomuDocument::new(root, builder.finish()).unwrap(),
            paragraph,
        )
    }

    fn insert_atom(
        document: XiaomuDocument,
        paragraph: NodeId,
        raw: usize,
        ordinal: usize,
        kind: &str,
        fallback: &str,
    ) -> XiaomuDocument {
        let offset = document
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(raw)
            .unwrap();
        Transaction::new(TransactionOrigin::Extension(
            "display-projection-test".into(),
        ))
        .with_step(TransactionStep::InsertInlineAtom {
            at: InlinePoint::new(paragraph, offset, ordinal, CursorAffinity::Before),
            kind: AtomKind::new(kind).unwrap(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::new(fallback).unwrap(),
        })
        .apply(&document)
        .unwrap()
    }

    fn projection() -> InlineAtomDisplayProjection {
        let (document, paragraph) = fixture();
        let document = insert_atom(document, paragraph, 1, 0, "mention", "@A");
        let document = insert_atom(document, paragraph, 1, 1, "reference", "R");
        let document = insert_atom(document, paragraph, 5, 0, "unknown", "!");
        let mut renderers = InlineAtomRendererRegistry::new();
        renderers.register(&AtomKind::new("mention").unwrap(), Rc::new(WrappedRenderer));
        renderers.register(&AtomKind::new("reference").unwrap(), Rc::new(EmptyRenderer));
        InlineAtomDisplayProjection::build(&document, paragraph, &renderers).unwrap()
    }

    fn offset(raw: usize) -> TextOffset {
        TextBuffer::from_string("A中B".to_owned())
            .offset_at(raw)
            .unwrap()
    }

    #[test]
    fn projection_splices_renderer_text_without_changing_canonical_text() {
        let projection = projection();
        assert_eq!(projection.canonical_text(), "A中B");
        assert_eq!(projection.display_text(), "A«@A»R中B!");
        assert_eq!(projection.atoms().len(), 3);

        assert_eq!(projection.atoms()[0].text_offset().as_usize(), 1);
        assert_eq!(projection.atoms()[0].atom_index(), 0);
        assert_eq!(projection.atoms()[0].display_range(), &(1..7));
        assert_eq!(projection.atoms()[1].atom_index(), 1);
        assert_eq!(projection.atoms()[1].display_range(), &(7..8));
        assert_eq!(projection.atoms()[2].text_offset().as_usize(), 5);
        assert_eq!(projection.atoms()[2].display_range(), &(12..13));
    }

    #[test]
    fn canonical_gaps_map_to_display_boundaries_across_adjacent_atoms_and_unicode() {
        let projection = projection();
        let node = projection.node();
        let before = CursorAffinity::Before;
        let cases = [
            (InlinePoint::new(node, TextOffset::ZERO, 0, before), 0),
            (InlinePoint::new(node, offset(1), 0, before), 1),
            (InlinePoint::new(node, offset(1), 1, before), 7),
            (InlinePoint::new(node, offset(1), 2, before), 8),
            (InlinePoint::new(node, offset(4), 0, before), 11),
            (InlinePoint::new(node, offset(5), 0, before), 12),
            (InlinePoint::new(node, offset(5), 1, before), 13),
        ];

        for (point, display) in cases {
            assert_eq!(
                projection.display_offset_for_inline_point(point),
                Some(display)
            );
            assert_eq!(
                projection.inline_point_for_display_boundary(display, before),
                Some(point)
            );
        }
    }

    #[test]
    fn renderer_interior_is_not_a_canonical_caret_boundary() {
        let projection = projection();
        assert!(
            projection
                .inline_point_for_display_boundary(3, CursorAffinity::Before)
                .is_none()
        );
        let atom = projection.atom_at_display_offset(3).unwrap();
        assert_eq!(atom.atom_index(), 0);
        assert_eq!(atom.text_offset().as_usize(), 1);
    }

    #[test]
    fn empty_custom_renderer_falls_back_to_nonempty_canonical_text() {
        let projection = projection();
        let reference = &projection.atoms()[1];
        assert_eq!(reference.display_range().len(), 1);
        assert_eq!(
            &projection.display_text()[reference.display_range().clone()],
            "R"
        );
    }

    #[test]
    fn display_hit_resolves_atom_interior_by_click_side() {
        let projection = projection();
        let node = projection.node();
        let before = CursorAffinity::Before;

        // «@A» spans display 1..7; the midpoint rule splits 1..4 / 4..7.
        for display in [1, 2, 3] {
            assert_eq!(
                projection.inline_point_for_display_hit(display, before),
                Some(InlinePoint::new(node, offset(1), 0, before)),
                "display byte {display} must hit the before gap"
            );
        }
        for display in [4, 5, 6] {
            assert_eq!(
                projection.inline_point_for_display_hit(display, before),
                Some(InlinePoint::new(node, offset(1), 1, before)),
                "display byte {display} must hit the after gap"
            );
        }
    }

    #[test]
    fn display_hit_maps_text_and_span_edges_through_boundaries() {
        let projection = projection();
        let node = projection.node();
        let before = CursorAffinity::Before;

        // Text bytes stay canonical text gaps with ordinal zero.
        assert_eq!(
            projection.inline_point_for_display_hit(11, before),
            Some(InlinePoint::new(node, offset(4), 0, before))
        );
        // Span end boundaries land on the following gap.
        assert_eq!(
            projection.inline_point_for_display_hit(8, before),
            Some(InlinePoint::new(node, offset(1), 2, before))
        );
        // Span start boundaries land on the preceding gap.
        assert_eq!(
            projection.inline_point_for_display_hit(7, before),
            Some(InlinePoint::new(node, offset(1), 1, before))
        );
        // End of display text resolves through the last span-end boundary.
        assert_eq!(
            projection.inline_point_for_display_hit(13, before),
            Some(InlinePoint::new(node, offset(5), 1, before))
        );
        // Non-boundary bytes inside multi-byte text are no caret gap.
        assert_eq!(projection.inline_point_for_display_hit(9, before), None);
    }
}
