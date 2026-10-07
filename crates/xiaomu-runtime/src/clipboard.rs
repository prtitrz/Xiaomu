//! Frontend-neutral clipboard model and platform transport seam.
//!
//! Runtime owns detached clipboard values and their versioned metadata codec.
//! The platform-visible clipboard body remains ordinary text; Xiaomu-native
//! structure is carried as optional metadata and never leaks canonical
//! `NodeId`s or frontend-specific types into Core.

mod cell_export;
#[cfg(test)]
mod clipped_export_tests;
mod export;
mod export_budget;
#[cfg(test)]
mod export_tests;
mod fragment;
mod projection;
mod table_template;
mod text_projection;
mod wire;

pub use export::{
    ClipboardCellRangeRoot, ClipboardExportPurpose, ClipboardExportSpec, ClipboardSourceBoundary,
    ClipboardTextProjection,
};

pub use fragment::{
    ClipboardAtom, ClipboardBlock, ClipboardInline, ClipboardNode, ClipboardNodeContent,
    ClipboardSlice,
};
pub use wire::{
    ClipboardMetadataDecode, ClipboardMetadataError, decode_metadata, decode_metadata_checked,
    encode_metadata,
};

pub(crate) use fragment::{require_unit_tables, validate_roots};
pub(crate) use projection::slice_selection;

pub(crate) fn export_selection(
    document: &xiaomu_core::document::XiaomuDocument,
    selection: crate::session::DocumentSelection,
    purpose: ClipboardExportPurpose,
    spec: ClipboardExportSpec,
) -> Result<Option<ClipboardSlice>, crate::session::SessionError> {
    // Cut safety precedes all borrowed scans, cloning and platform side effects.
    if purpose == ClipboardExportPurpose::Cut && selection.active_cell_range().is_some() {
        return Err(crate::session::SessionError::UnsupportedTableOperation);
    }
    export_with_spec(document, selection, spec)
}

/// Only the session's dedicated prepared-Cut coordinator may use this after
/// explicit Cut policy admission. Projection-only callers cannot bypass the
/// public CellRange Cut guard or substitute the Copy purpose.
pub(crate) fn export_prepared_cut(
    document: &xiaomu_core::document::XiaomuDocument,
    selection: crate::session::DocumentSelection,
    spec: ClipboardExportSpec,
) -> Result<Option<ClipboardSlice>, crate::session::SessionError> {
    if selection.active_cell_range().is_none()
        && selection.as_node_selection().is_none()
        && selection.as_atomic_node().is_none()
    {
        return Err(crate::session::SessionError::UnsupportedTableOperation);
    }
    export_with_spec(document, selection, spec)
}

fn export_with_spec(
    document: &xiaomu_core::document::XiaomuDocument,
    selection: crate::session::DocumentSelection,
    spec: ClipboardExportSpec,
) -> Result<Option<ClipboardSlice>, crate::session::SessionError> {
    use crate::session::{PolicyError, SessionError};
    use export::CellRangeExport;
    let clipped_budget = if selection.active_cell_range().is_some()
        && matches!(spec.cell_ranges(), CellRangeExport::Clipped(_))
    {
        Some(
            export_budget::clipped_document(document).map_err(|()| {
                PolicyError::new("clipboard export exceeds bounded projection budget")
            })?,
        )
    } else {
        export_budget::document(document)
            .map_err(|()| PolicyError::new("clipboard export exceeds bounded projection budget"))?;
        None
    };
    selection.validate(document)?;
    if let Some(range) = selection.active_cell_range() {
        let (roots, boundary) = match spec.cell_ranges() {
            CellRangeExport::Unit => {
                // Text projection alone retains historical unit-only geometry.
                range.cells(document)?;
                cell_export::capture(document, range)?
            }
            CellRangeExport::Closed => cell_export::capture(document, range)?,
            CellRangeExport::Clipped(attrs) => cell_export::capture_clipped(
                document,
                range,
                attrs,
                clipped_budget.ok_or(SessionError::SelectionInvalid)?,
            )?,
        };
        return ClipboardSlice::from_export_roots(roots, boundary, spec.text_projection())
            .map(Some)
            .map_err(|()| {
                PolicyError::new("clipboard export cannot preserve source projection").into()
            });
    }
    let Some(mut slice) = slice_selection(document, selection)? else {
        return Ok(None);
    };
    let boundary = if slice.is_closed() {
        ClipboardSourceBoundary::WholeRoots
    } else {
        ClipboardSourceBoundary::Open
    };
    slice
        .set_export(boundary, spec.text_projection())
        .map_err(|()| PolicyError::new("clipboard export cannot preserve source projection"))?;
    Ok(Some(slice))
}

/// Plain-text read/write seam between the editing layer and the platform.
///
/// Structured GPUI transport is implemented by the frontend adapter; Runtime
/// keeps this minimal text seam for generic hosts and plain-text fallback.
pub trait TextClipboard {
    /// Replaces the platform clipboard content with `text`.
    fn write_text(&mut self, text: String);

    /// Returns the current clipboard text when one is available.
    ///
    /// Non-text clipboard content reads as `None`; implementations must not
    /// error on foreign content.
    fn read_text(&self) -> Option<String>;
}

/// Normalizes platform line endings to Xiaomu's canonical LF representation.
///
/// `CRLF` and lone `CR` both become one `\n`; existing LF is preserved. This
/// is the multiline-safe normalization used by CodeBlock paste and other
/// adapters that intentionally preserve canonical line breaks.
#[must_use]
pub fn normalize_multiline_paste_text(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(character) = chars.next() {
        match character {
            '\r' => {
                chars.next_if_eq(&'\n');
                normalized.push('\n');
            }
            other => normalized.push(other),
        }
    }

    normalized
}

/// Normalizes platform clipboard text for the current ordinary rich-text
/// plain-text fallback.
///
/// Canonical HardBreak is representable as LF (ADR 0004), but unstructured
/// platform paste into an ordinary paragraph does not infer document break
/// semantics yet. Line endings are first normalized to LF, then every LF is
/// collapsed to one space. Xiaomu-native structured paste bypasses this
/// fallback and reconstructs canonical structure directly.
#[must_use]
pub fn normalize_paste_text(text: &str) -> String {
    normalize_multiline_paste_text(text).replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_normalization_preserves_breaks_as_lf() {
        assert_eq!(normalize_multiline_paste_text("a\r\nb"), "a\nb");
        assert_eq!(normalize_multiline_paste_text("a\rb"), "a\nb");
        assert_eq!(normalize_multiline_paste_text("a\nb"), "a\nb");
        assert_eq!(normalize_multiline_paste_text("a\r\r\nb"), "a\n\nb");
        assert_eq!(normalize_multiline_paste_text("\n"), "\n");
        assert_eq!(normalize_multiline_paste_text(""), "");
    }

    #[test]
    fn ordinary_plain_text_line_breaks_collapse_to_spaces() {
        assert_eq!(normalize_paste_text("a\r\nb"), "a b");
        assert_eq!(normalize_paste_text("a\rb"), "a b");
        assert_eq!(normalize_paste_text("a\nb"), "a b");
        assert_eq!(normalize_paste_text("a\r\r\nb"), "a  b");
        assert_eq!(normalize_paste_text("\n"), " ");
        assert_eq!(normalize_paste_text(""), "");
    }

    #[test]
    fn non_breaking_content_is_preserved() {
        assert_eq!(normalize_paste_text("你好 world 👍"), "你好 world 👍");
        assert_eq!(
            normalize_paste_text("combining é\u{301}"),
            "combining é\u{301}"
        );
        assert_eq!(
            normalize_multiline_paste_text("combining é\u{301}"),
            "combining é\u{301}"
        );
    }
}
