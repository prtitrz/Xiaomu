//! Per-block layout cache key computation.
//!
//! Reshaping a line is expensive; a block only needs to re-shape when its
//! content, kind-driven style, or available width changed. The key bundles
//! those inputs behind a cheap equality check that the element consults
//! before shaping.

use xiaomu_core::document::NodeId;

/// Identity of one shaped-layout generation for a single block view.
///
/// Two equal keys guarantee the cached [`gpui::ShapedLine`] still depicts
/// the same content at the same width. The hash form is used as the element
/// id so GPUI's element state also follows identity changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LayoutCacheKey {
    node: NodeId,
    epoch: u64,
    width_whole_px: i32,
    style: u64,
    alignment: Option<crate::block_alignment::BlockAlignment>,
    aligned_width: Option<u32>,
    sized_width: Option<u32>,
}

impl LayoutCacheKey {
    /// Derives the key for one block render pass.
    ///
    /// `epoch` advances on edits. Composition is frontend-transient and its
    /// layout has no reusable key; intrinsic measurements without a definite
    /// width also cannot hit the cache. Widths are rounded to whole pixels so
    /// sub-pixel jitter does not invalidate the cache.
    #[must_use]
    pub(crate) fn new(node: NodeId, epoch: u64, width_px: f32) -> Self {
        Self {
            node,
            epoch,
            width_whole_px: width_px.round() as i32,
            style: 0,
            alignment: Default::default(),
            aligned_width: None,
            sized_width: None,
        }
    }
    /// Real-size reflow also uses exact fractional widths, with or without alignment.
    pub(crate) fn with_text_sizes(mut self, enabled: bool, width: gpui::Pixels) -> Self {
        self.sized_width = enabled.then(|| f32::from(width).to_bits());
        self
    }

    /// Includes effective shaping and paint inputs, not just document edits.
    pub(crate) const fn with_style(mut self, style: u64) -> Self {
        self.style = style;
        self
    }

    /// Opt-in row geometry depends on the exact, potentially fractional width.
    /// Keep absent-provider legacy left-layout cache quantization unchanged.
    pub(crate) fn with_alignment(
        mut self,
        alignment: Option<crate::block_alignment::BlockAlignment>,
        width: gpui::Pixels,
    ) -> Self {
        self.alignment = alignment;
        self.aligned_width = alignment.map(|_| f32::from(width).to_bits());
        self
    }
}

/// Hash the actual projection, resolved families and host style. In particular,
/// changing color/family without a document epoch cannot reuse painted runs.
pub(crate) fn style_fingerprint(
    text: &str,
    font: &gpui::Font,
    color: gpui::Hsla,
    font_size: gpui::Pixels,
    line_height: gpui::Pixels,
    runs: &[gpui::TextRun],
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut state = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut state);
    font.hash(&mut state);
    color.hash(&mut state);
    f32::from(font_size).to_bits().hash(&mut state);
    f32::from(line_height).to_bits().hash(&mut state);
    runs.len().hash(&mut state);
    for run in runs {
        run.len.hash(&mut state);
        run.font.hash(&mut state);
        run.color.hash(&mut state);
        run.background_color.hash(&mut state);
        run.underline.hash(&mut state);
        run.strikethrough.hash(&mut state);
    }
    state.finish()
}
#[cfg(test)]
mod tests {
    use super::*;
    use xiaomu_core::document::{
        InlineContent, MarkSet, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, TextRun,
        XiaomuDocument,
    };

    /// Document > [p("a"), p("b")]; returns both paragraph ids.
    fn two_paragraph_ids() -> (NodeId, NodeId) {
        let mut builder = NodeStoreBuilder::new();
        let mut insert = |text: &str| {
            builder
                .insert(
                    NodeKind::Paragraph,
                    NodeAttrs::empty(),
                    NodeContent::Inline(
                        InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()])
                            .unwrap(),
                    ),
                )
                .unwrap()
        };
        let first = insert("a");
        let second = insert("b");
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([first, second]),
            )
            .unwrap();
        let _ = XiaomuDocument::new(root, builder.finish()).unwrap();
        (first, second)
    }

    #[test]
    fn equal_inputs_produce_equal_keys_and_hashes() {
        let (first, _) = two_paragraph_ids();

        let key = LayoutCacheKey::new(first, 7, 320.0);
        assert_eq!(key, LayoutCacheKey::new(first, 7, 320.4));
        // Sub-pixel jitter below half a pixel rounds away.
        assert_eq!(LayoutCacheKey::new(first, 7, 320.4), key);
    }

    #[test]
    fn any_changed_input_invalidates_the_key() {
        let (first, second) = two_paragraph_ids();

        let base = LayoutCacheKey::new(first, 0, 300.0);
        assert_ne!(base, LayoutCacheKey::new(first, 1, 300.0), "epoch");
        assert_ne!(base, LayoutCacheKey::new(second, 0, 300.0), "node");
        assert_ne!(base, LayoutCacheKey::new(first, 0, 301.0), "width");
        assert_ne!(base.with_style(1), base.with_style(2), "render style");
    }

    #[test]
    fn optional_alignment_and_fractional_container_width_are_cache_inputs() {
        use crate::block_alignment::BlockAlignment;
        let (node, _) = two_paragraph_ids();
        let legacy = LayoutCacheKey::new(node, 0, 320.1);
        assert_eq!(legacy, LayoutCacheKey::new(node, 0, 320.4));
        let mut previous = legacy;
        for alignment in [
            BlockAlignment::Left,
            BlockAlignment::Center,
            BlockAlignment::Right,
        ] {
            let key = legacy.with_alignment(Some(alignment), gpui::px(320.1));
            assert_ne!(key, previous);
            assert_ne!(key, legacy.with_alignment(Some(alignment), gpui::px(320.4)));
            previous = key;
        }
    }
}
