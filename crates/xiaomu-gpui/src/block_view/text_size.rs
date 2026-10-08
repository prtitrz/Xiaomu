//! Opt-in fixed size inputs and transient typing/preedit geometry.

#[cfg(test)]
#[path = "text_size_caret_tests.rs"]
mod caret_tests;

use super::{DisplaySegment, ParagraphView};
use crate::font_size::resolve_font_size;
use crate::mixed_size;
use crate::text_size::{
    ResolvedText, TextSizeCapability, TextSizeCaretContext, TextSizeError, TextSizeErrorKind,
    TextSizeStyle,
};
use gpui::{Pixels, px};
use std::rc::Rc;
use xiaomu_core::document::{Mark, StringAttribute};

pub(super) struct SizedContent {
    pub(super) capability: Rc<TextSizeCapability>,
    pub(super) style: TextSizeStyle,
    pub(super) resolved: ResolvedText,
    pub(super) empty_size: Option<Pixels>,
}

impl SizedContent {
    pub(super) fn input<'a>(&'a self, text: &'a str, width: Pixels) -> mixed_size::Input<'a> {
        let mut input = self.resolved.input(text, &self.style, width);
        if text.is_empty()
            && let Some(size) = self.empty_size
        {
            input.empty_size = Some(size);
        }
        input
    }

    pub(super) fn fingerprint(&self, previous: u64) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut state = std::collections::hash_map::DefaultHasher::new();
        previous.hash(&mut state);
        self.empty_size
            .map(|size| f32::from(size).to_bits())
            .hash(&mut state);
        for span in &self.resolved.sizes {
            span.range.hash(&mut state);
            f32::from(span.size).to_bits().hash(&mut state);
        }
        state.finish()
    }
}

impl ParagraphView {
    pub(super) fn effective_block_alignment(
        &self,
    ) -> Option<crate::block_alignment::BlockAlignment> {
        self.block_alignment.or_else(|| {
            (self.text_size_capability.is_some() && !self.is_range_input())
                .then_some(crate::block_alignment::BlockAlignment::Left)
        })
    }

    pub(crate) fn attach_text_size_capability(
        &mut self,
        capability: Option<Rc<TextSizeCapability>>,
    ) {
        let same = match (&self.text_size_capability, &capability) {
            (None, None) => true,
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            _ => false,
        };
        if !same {
            self.text_size_capability = capability;
            self.text_size_style = None;
            self.last_layout = None;
            self.last_bounds = None;
            self.last_caret = None;
            self.cache_key = None;
        }
    }

    pub(crate) fn set_text_size_style(
        &mut self,
        revision: xiaomu_core::document::DocumentRevision,
        style: Result<TextSizeStyle, TextSizeError>,
    ) {
        self.text_size_style = Some((revision, style));
    }

    fn resolved_size_style(
        &self,
        capability: &TextSizeCapability,
    ) -> Result<Option<TextSizeStyle>, TextSizeError> {
        let session = self.session.borrow();
        if let Some((revision, style)) = &self.text_size_style
            && *revision == session.document().revision()
        {
            return style.clone().map(Some);
        }
        session
            .document()
            .node(self.node)
            .map(|node| capability.style(session.document(), node))
            .transpose()
    }

    pub(super) fn sized_content(
        &self,
        segments: &[DisplaySegment],
    ) -> Result<Option<SizedContent>, TextSizeError> {
        let Some(capability) = self
            .text_size_capability
            .as_ref()
            .filter(|_| !self.is_range_input())
        else {
            return Ok(None);
        };
        let Some(style) = self.resolved_size_style(capability)? else {
            return Ok(None);
        };
        let resolved = capability.resolve_segments(self.node, &style, segments)?;
        let empty_size = if segments.iter().all(|segment| segment.text.is_empty()) {
            let size = self.effective_size(&style)?;
            capability.caret_height(self.node, self.caret_size_context(&style, size))?;
            Some(size)
        } else {
            None
        };
        Ok(Some(SizedContent {
            capability: capability.clone(),
            style,
            resolved,
            empty_size,
        }))
    }

    fn effective_size(&self, style: &TextSizeStyle) -> Result<Pixels, TextSizeError> {
        let marks = if self.composition.is_some() {
            self.preedit_marks()
        } else {
            let session = self.session.borrow();
            session
                .selection()
                .as_same_node_inline()
                .filter(|(_, focus)| focus.node_id() == self.node)
                .and_then(|(_, focus)| session.effective_input_marks_at(focus).ok())
                .unwrap_or_else(xiaomu_core::document::MarkSet::empty)
        };
        let attribute = marks
            .as_slice()
            .iter()
            .find_map(|mark| match mark {
                Mark::TextStyle(style) => Some(style.attributes().font_size()),
                _ => None,
            })
            .unwrap_or(&StringAttribute::Missing);
        resolve_font_size(attribute, style.context())
            .map(px)
            .map_err(|error| {
                TextSizeError::new(self.node, 0..0, TextSizeErrorKind::FontSize(error))
            })
    }

    fn caret_size_context(&self, style: &TextSizeStyle, effective: Pixels) -> TextSizeCaretContext {
        use xiaomu_core::selection::{CursorAffinity, InlinePoint};
        let session = self.session.borrow();
        let document = session.document();
        let focus = if let Some(state) = &self.composition {
            self.inline()
                .and_then(|inline| inline.offset_at(state.base_range().start).ok())
                .map(|offset| {
                    InlinePoint::new(
                        self.node,
                        offset,
                        state.start_atom_index(),
                        CursorAffinity::Before,
                    )
                })
        } else {
            session
                .selection()
                .as_same_node_inline()
                .map(|(_, focus)| focus)
                .filter(|focus| focus.node_id() == self.node)
        };
        let explicit = |marks: &xiaomu_core::document::MarkSet| {
            let attribute = marks.as_slice().iter().find_map(|mark| match mark {
                Mark::TextStyle(mark) => Some(mark.attributes().font_size()),
                _ => None,
            })?;
            match attribute {
                StringAttribute::Value(value) if !value.trim().is_empty() => {
                    resolve_font_size(attribute, style.context()).ok()
                }
                _ => None,
            }
        };
        let before = focus.and_then(|focus| adjacent_marks(document, focus, true));
        let after = focus.and_then(|focus| adjacent_marks(document, focus, false));
        let stored = focus.and(session.stored_marks()).and_then(explicit);
        TextSizeCaretContext::new(
            f32::from(effective),
            stored,
            before.and_then(explicit),
            after.and_then(explicit),
        )
    }

    /// Resolve a reading endpoint without temporarily installing it as focus.
    pub(super) fn reading_caret_height(
        &self,
        at: xiaomu_core::selection::InlinePoint,
    ) -> Result<Option<Pixels>, TextSizeError> {
        let Some(capability) = self
            .text_size_capability
            .as_ref()
            .filter(|_| !self.is_range_input())
        else {
            return Ok(None);
        };
        let Some(style) = self.resolved_size_style(capability)? else {
            return Ok(None);
        };
        let session = self.session.borrow();
        let document = session.document();
        let is_caret = session.selection().is_collapsed()
            && session.selection().focus() == xiaomu_runtime::session::DocumentPosition::Inline(at);
        let inherited = document
            .inherited_inline_marks(at)
            .map_err(|_| TextSizeError::new(self.node, 0..0, TextSizeErrorKind::InvalidStyle))?;
        let marks = if is_caret {
            session.stored_marks().unwrap_or(&inherited)
        } else {
            &inherited
        };
        let attribute = |marks: &xiaomu_core::document::MarkSet| {
            marks
                .as_slice()
                .iter()
                .find_map(|mark| match mark {
                    Mark::TextStyle(style) => Some(style.attributes().font_size().clone()),
                    _ => None,
                })
                .unwrap_or(StringAttribute::Missing)
        };
        let effective = resolve_font_size(&attribute(marks), style.context()).map_err(|error| {
            TextSizeError::new(self.node, 0..0, TextSizeErrorKind::FontSize(error))
        })?;
        let explicit = |marks: &xiaomu_core::document::MarkSet| {
            let value = attribute(marks);
            match &value {
                StringAttribute::Value(value) if !value.trim().is_empty() => {
                    resolve_font_size(&attribute(marks), style.context()).ok()
                }
                _ => None,
            }
        };
        capability.caret_height(
            self.node,
            TextSizeCaretContext::new(
                effective,
                is_caret
                    .then(|| session.stored_marks().and_then(explicit))
                    .flatten(),
                adjacent_marks(document, at, true).and_then(explicit),
                adjacent_marks(document, at, false).and_then(explicit),
            ),
        )
    }

    pub(super) fn presented_caret_height(&self) -> Result<Option<Pixels>, TextSizeError> {
        let Some(capability) = self
            .text_size_capability
            .as_ref()
            .filter(|_| !self.is_range_input())
        else {
            return Ok(None);
        };
        let Some(style) = self.resolved_size_style(capability)? else {
            return Ok(None);
        };
        let size = self.effective_size(&style)?;
        capability.caret_height(self.node, self.caret_size_context(&style, size))
    }

    /// A rejected overlay must never be drawn at substituted/default sizes or
    /// later fall through as a normal replacement of the selected content.
    pub(super) fn admit_preedit_size(&self) -> bool {
        let (text, segments) = self.layout_content();
        match self.sized_content(&segments) {
            Ok(Some(content)) => mixed_size::admission(
                content.capability.text_system(),
                content.input(&text, px(1.0)),
            )
            .is_ok(),
            Ok(None) => true,
            Err(_) => false,
        }
    }
}

/// Literal neighbors for presentation. Unlike insertion-mark inheritance, an
/// unmarked extension atom is not transparent to these host-facing probes.
fn adjacent_marks(
    document: &xiaomu_core::document::XiaomuDocument,
    at: xiaomu_core::selection::InlinePoint,
    before: bool,
) -> Option<&xiaomu_core::document::MarkSet> {
    let inline = document.node(at.node_id())?.content().as_inline()?;
    let atoms = inline
        .atoms()
        .iter()
        .filter(|atom| atom.text_offset() == at.text_offset());
    let atom = if before {
        at.atom_index().checked_sub(1)
    } else {
        (at.atom_index() < inline.atom_count_at(at.text_offset())).then_some(at.atom_index())
    };
    if let Some(index) = atom {
        let id = atoms.into_iter().nth(index)?.atom();
        return Some(document.node(id)?.content().as_inline_atom()?.marks());
    }
    let byte = at.text_offset().as_usize();
    let mut start = 0;
    inline.runs().iter().find_map(|run| {
        let end = start + run.len_bytes();
        let contains = if before {
            start < byte && byte <= end
        } else {
            start <= byte && byte < end
        };
        start = end;
        contains.then_some(run.marks())
    })
}
