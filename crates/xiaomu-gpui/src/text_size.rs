//! Opt-in native font-size admission shared by session policy and block layout.
//!
//! Keep one capability for the editor's lifetime and give clones to both the
//! view and the host's final `SessionPolicy::validate_document` implementation.
//! Canonical strings are never rewritten. Uniform text uses stock GPUI shaping;
//! mixed-size text has conservative script, cluster/seam and work-budget gates.
//! Admission is independent of viewport width and is not browser-layout parity.

#[cfg(test)]
mod batch_tests;
mod caret;
mod error;
#[cfg(test)]
mod policy_tests;
mod resolve;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{Font, Hsla, Pixels, WindowTextSystem, black, px};
use xiaomu_core::document::{Node, NodeId, StringAttribute, XiaomuDocument};

use crate::block_view::{FontCatalog, project_atom_display_content};
use crate::font_size::{FontSizeContext, resolve_font_size};
use crate::inline_atom::InlineAtomRendererRegistry;
use crate::inline_atom_display::InlineAtomDisplayProjection;
use crate::mixed_size;

pub use caret::TextSizeCaretContext;
pub use error::{TextSizeError, TextSizeErrorKind};
pub(crate) use resolve::ResolvedText;

/// Maximum optional host caret height, in logical pixels.
/// This is a frontend resource limit, never a canonical font-size constraint.
pub const MAX_CARET_HEIGHT_PX: f32 = 1024.0;

/// Authoritative, view-independent text geometry for one inline-bearing block.
///
/// `context.parent_px()` is the block's inherited font size, in logical CSS
/// pixels; `line_height` is a unitless multiplier of each resolved run size.
/// Font weight/style and fallbacks belong to `font`. Base color is authoritative
/// too: GPUI shaping-run boundaries can depend on colors and decorations.
#[derive(Clone, Debug)]
pub struct TextSizeStyle {
    font: Font,
    context: FontSizeContext,
    line_height: f32,
    font_family: Option<String>,
    color: Hsla,
}

impl TextSizeStyle {
    /// Defines fixed geometry. Admission rejects invalid/non-finite line height,
    /// an out-of-budget inherited size, or any unresolvable canonical size.
    ///
    /// The provider owns heading, table, code and theme inheritance. Enabled
    /// views use these values instead of inferring geometry from window styles.
    #[must_use]
    pub fn new(font: Font, context: FontSizeContext, line_height: f32) -> Self {
        Self {
            font,
            context,
            line_height,
            font_family: None,
            color: black(),
        }
    }

    /// Returns the base native font, including weight, style and fallbacks.
    #[must_use]
    pub const fn base_font(&self) -> &Font {
        &self.font
    }

    /// Sets the exact base text color shared by admission and rendering.
    /// The default is black. Inline TextStyle/Link colors retain their normal
    /// overrides. GPUI may use color equality to coalesce shaping runs.
    #[must_use]
    pub fn with_color(mut self, color: Hsla) -> Self {
        self.color = color;
        self
    }

    /// Returns the authoritative base text color.
    #[must_use]
    pub const fn color(&self) -> Hsla {
        self.color
    }

    /// Resolves a CSS family list with the same native catalog used by inline
    /// TextStyle marks. Unsupported/unavailable families retain the base font.
    /// Resolution happens in the shared capability before admission or layout.
    #[must_use]
    pub fn with_font_family(mut self, family: impl Into<String>) -> Self {
        self.font_family = Some(family.into());
        self
    }

    /// Returns the explicit parent/root/medium logical-pixel resolution bases.
    #[must_use]
    pub const fn context(&self) -> &FontSizeContext {
        &self.context
    }

    /// Returns the unitless line-height multiplier.
    #[must_use]
    pub const fn line_height(&self) -> f32 {
        self.line_height
    }
}

/// Pure, deterministic geometry projection from a candidate canonical snapshot.
///
/// The document permits ancestor-dependent table/header defaults. Implementors
/// must not mutate the session, query mutable view state, or change their theme
/// after admission. For identical canonical input, this method must return the
/// same geometry in policy and view. Replace/revalidate the editor to change
/// fonts or theme. Native range-input proxies never invoke this provider.
pub trait TextSizeStyleProvider {
    /// Returns the complete fixed geometry for this inline-bearing node.
    fn style(&self, document: &XiaomuDocument, node: &Node) -> TextSizeStyle;

    /// Optionally prepares all inline-block styles in one pure document pass.
    ///
    /// Returning `Some` must cover every inline-bearing node exactly, with no
    /// other keys; missing/extra entries fail admission. Values must have the
    /// same meaning as `style`. This lets ancestor-aware hosts avoid repeated
    /// whole-tree parent searches. Policy and DocumentView use the batch map;
    /// standalone paragraph views retain the individual `style` fallback.
    fn styles_for_document(
        &self,
        _document: &XiaomuDocument,
    ) -> Option<BTreeMap<NodeId, TextSizeStyle>> {
        None
    }

    /// Optional centered caret height in logical pixels from effective typing
    /// size and explicit stored/adjacent size probes. `None` uses the visual
    /// line box. `Some` must be
    /// finite, positive and at most [`MAX_CARET_HEIGHT_PX`]. Like `style`, this
    /// must be deterministic and independent of mutable view state.
    fn caret_height(&self, _context: TextSizeCaretContext) -> Option<f32> {
        None
    }
}

/// Fixed, opt-in font-size capability shared by policy and frontend.
///
/// Construct with the window's text system and a pure style provider before
/// admitting the first document. Clones share both objects. The host must use
/// the same immutable atom renderer registry for validation and the view, and
/// renderer display text must be deterministic. Font-catalog mutation requires
/// revalidation before further edits. This object does not install a session
/// policy: the host must call [`Self::validate_document`] in its final candidate
/// check, including initial load and every edit/undo/redo publication.
#[derive(Clone)]
pub struct TextSizeCapability {
    system: Arc<WindowTextSystem>,
    provider: Rc<dyn TextSizeStyleProvider>,
}

impl TextSizeCapability {
    /// Captures the exact native text system and geometry provider used by views.
    #[must_use]
    pub fn new(system: Arc<WindowTextSystem>, provider: Rc<dyn TextSizeStyleProvider>) -> Self {
        Self { system, provider }
    }

    /// Preflights every inline block, including marked hard breaks and the exact
    /// registered atom display text, without changing canonical data or caches
    /// owned by the editor. Native shaping may populate GPUI's own font caches.
    ///
    /// Returns the first unsupported block with its display UTF-8 range. The
    /// width-independent mixed-layout reservation is 1 MiB of repeated shaped
    /// bytes and 4096 shape calls per block. Uniform text retains the stock
    /// native Unicode path and is not constrained by those mixed-work limits.
    /// Unsupported CSS sizes, complex mixed scripts and unsafe seams fail closed.
    pub fn validate_document(
        &self,
        document: &XiaomuDocument,
        renderers: &InlineAtomRendererRegistry,
    ) -> Result<(), TextSizeError> {
        let styles = self.prepare_styles(document)?;
        for node in document.store().iter() {
            let Some(inline) = node.content().as_inline() else {
                continue;
            };
            let projection = InlineAtomDisplayProjection::build(document, node.id(), renderers)
                .ok_or_else(|| {
                    TextSizeError::new(node.id(), 0..0, TextSizeErrorKind::InvalidProjection)
                })?;
            let (text, segments) = project_atom_display_content(inline, &projection);
            let style = &styles[&node.id()];
            let resolved = self.resolve_segments(node.id(), style, &segments)?;
            mixed_size::admission(&self.system, resolved.input(&text, style, px(1.0)))
                .map_err(|error| TextSizeError::from_layout(node.id(), error))?;
        }
        Ok(())
    }

    pub(crate) fn style(
        &self,
        document: &XiaomuDocument,
        node: &Node,
    ) -> Result<TextSizeStyle, TextSizeError> {
        self.resolve_style(node.id(), self.provider.style(document, node))
    }

    pub(crate) fn prepare_styles(
        &self,
        document: &XiaomuDocument,
    ) -> Result<BTreeMap<NodeId, TextSizeStyle>, TextSizeError> {
        let styles = match self.provider.styles_for_document(document) {
            Some(styles) => {
                for node in document.store().iter() {
                    if node.content().as_inline().is_some() && !styles.contains_key(&node.id()) {
                        return Err(TextSizeError::new(
                            node.id(),
                            0..0,
                            TextSizeErrorKind::InvalidStyleMap,
                        ));
                    }
                }
                if let Some(id) = styles.keys().find(|id| {
                    document
                        .node(**id)
                        .and_then(|node| node.content().as_inline())
                        .is_none()
                }) {
                    return Err(TextSizeError::new(
                        *id,
                        0..0,
                        TextSizeErrorKind::InvalidStyleMap,
                    ));
                }
                styles
            }
            None => document
                .store()
                .iter()
                .filter(|node| node.content().as_inline().is_some())
                .map(|node| (node.id(), self.provider.style(document, node)))
                .collect(),
        };
        styles
            .into_iter()
            .map(|(id, style)| self.resolve_style(id, style).map(|style| (id, style)))
            .collect()
    }

    fn resolve_style(
        &self,
        node: NodeId,
        mut style: TextSizeStyle,
    ) -> Result<TextSizeStyle, TextSizeError> {
        let inherited = resolve_font_size(&StringAttribute::Missing, style.context())
            .map_err(|error| TextSizeError::new(node, 0..0, TextSizeErrorKind::FontSize(error)))?;
        let height = inherited * style.line_height();
        if !style.line_height().is_finite() || !height.is_finite() || height <= 0.0 {
            return Err(TextSizeError::new(
                node,
                0..0,
                TextSizeErrorKind::InvalidStyle,
            ));
        }
        if let Some(family) = style.font_family.take() {
            style.font = FontCatalog::from_system(&self.system).apply(&family, &style.font);
        }
        self.caret_height(node, TextSizeCaretContext::new(inherited, None, None, None))?;
        Ok(style)
    }

    pub(crate) fn caret_height(
        &self,
        node: NodeId,
        context: TextSizeCaretContext,
    ) -> Result<Option<Pixels>, TextSizeError> {
        let height = self.provider.caret_height(context);
        if height.is_some_and(|height| {
            !height.is_finite() || height <= 0.0 || height > MAX_CARET_HEIGHT_PX
        }) {
            return Err(TextSizeError::new(
                node,
                0..0,
                TextSizeErrorKind::InvalidStyle,
            ));
        }
        Ok(height.map(px))
    }

    pub(crate) fn text_system(&self) -> &WindowTextSystem {
        &self.system
    }
}
