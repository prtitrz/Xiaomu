//! Frontend-neutral clipboard model and platform text seam.
//!
//! The runtime never touches a platform clipboard directly. Frontends bind
//! [`TextClipboard`] to their platform transport, while document selections
//! are first projected into a [`ClipboardSlice`]. The slice always carries a
//! plain-text fallback and also retains block kind/attributes plus normalized
//! inline runs so later structured transport/paste can preserve marks without
//! re-reading the source document.

use xiaomu_core::document::{
    InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind, TextRun, XiaomuDocument,
};

use crate::session::{DocumentPosition, DocumentSelection, SessionError};

/// Plain-text read/write seam between the editing layer and the platform.
///
/// Implementations must not interpret the text; they transport it as-is.
pub trait TextClipboard {
    /// Replaces the platform clipboard content with `text`.
    fn write_text(&mut self, text: String);

    /// Returns the current clipboard text when one is available.
    ///
    /// Non-text clipboard content (images, structured flavors) reads as
    /// `None`; implementations must not error on foreign content.
    fn read_text(&self) -> Option<String>;
}

/// One selected inline-bearing block stored in a [`ClipboardSlice`].
///
/// The first and last blocks may contain only the selected portion of their
/// source inline content. Intermediate blocks contain their full inline
/// content. Stable `NodeId`s are intentionally absent: clipboard fragments
/// are detached values and must receive fresh identities when pasted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardBlock {
    kind: NodeKind,
    attrs: NodeAttrs,
    inline: InlineContent,
}

impl ClipboardBlock {
    fn new(kind: NodeKind, attrs: NodeAttrs, inline: InlineContent) -> Self {
        Self {
            kind,
            attrs,
            inline,
        }
    }

    /// Returns the semantic block kind captured from the source document.
    #[must_use]
    pub const fn kind(&self) -> &NodeKind {
        &self.kind
    }

    /// Returns the source block attributes.
    #[must_use]
    pub const fn attrs(&self) -> &NodeAttrs {
        &self.attrs
    }

    /// Returns the selected normalized inline content, including marks.
    #[must_use]
    pub const fn inline(&self) -> &InlineContent {
        &self.inline
    }
}

/// Detached clipboard projection of one non-collapsed document selection.
///
/// `plain_text` is the interoperability fallback used by external apps. Each
/// selected inline block contributes one line, so block boundaries become
/// `\n`. `blocks` retains structured block-level values for Xiaomu-to-Xiaomu
/// transport; container reconstruction (lists/quotes) is layered on top of
/// this baseline in the rest of P3.3.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardSlice {
    plain_text: String,
    blocks: Vec<ClipboardBlock>,
}

impl ClipboardSlice {
    fn new(blocks: Vec<ClipboardBlock>) -> Self {
        let plain_text = blocks
            .iter()
            .map(|block| concatenated(block.inline()))
            .collect::<Vec<_>>()
            .join("\n");
        Self { plain_text, blocks }
    }

    /// Returns the plain-text fallback, with selected block boundaries as
    /// newline characters.
    #[must_use]
    pub fn plain_text(&self) -> &str {
        &self.plain_text
    }

    /// Returns the selected block slices in document order.
    #[must_use]
    pub fn blocks(&self) -> &[ClipboardBlock] {
        &self.blocks
    }
}

/// Projects a validated document selection into a detached clipboard slice.
///
/// The current P3.3 baseline accepts text endpoints. Gap-based structured
/// selections are reserved for the later node/atomic selection slices.
pub(crate) fn slice_selection(
    document: &XiaomuDocument,
    selection: DocumentSelection,
) -> Result<Option<ClipboardSlice>, SessionError> {
    selection.validate(document)?;

    let (head, tail) = selection.ordered(document)?;
    let (DocumentPosition::Text(head), DocumentPosition::Text(tail)) = (head, tail) else {
        return Err(SessionError::SelectionInvalid);
    };

    // Affinity can distinguish two visual caret positions at one soft-wrap
    // boundary, but it does not select canonical text by itself.
    if head.node_id() == tail.node_id() && head.offset() == tail.offset() {
        return Ok(None);
    }

    let mut source_blocks = Vec::new();
    collect_inline_blocks(document, document.root(), &mut source_blocks);

    let head_index = source_blocks
        .iter()
        .position(|block| block.node == head.node_id())
        .ok_or(SessionError::SelectionInvalid)?;
    let tail_index = source_blocks
        .iter()
        .position(|block| block.node == tail.node_id())
        .ok_or(SessionError::SelectionInvalid)?;
    if head_index > tail_index {
        return Err(SessionError::SelectionInvalid);
    }

    let mut blocks = Vec::with_capacity(tail_index - head_index + 1);
    for (index, source) in source_blocks[head_index..=tail_index].iter().enumerate() {
        let absolute_index = head_index + index;
        let start = if absolute_index == head_index {
            head.offset().as_usize()
        } else {
            0
        };
        let end = if absolute_index == tail_index {
            tail.offset().as_usize()
        } else {
            source.inline.len_bytes()
        };
        let inline = slice_inline(&source.inline, start, end)?;
        blocks.push(ClipboardBlock::new(
            source.kind.clone(),
            source.attrs.clone(),
            inline,
        ));
    }

    Ok(Some(ClipboardSlice::new(blocks)))
}

struct SourceBlock {
    node: NodeId,
    kind: NodeKind,
    attrs: NodeAttrs,
    inline: InlineContent,
}

fn collect_inline_blocks(document: &XiaomuDocument, id: NodeId, out: &mut Vec<SourceBlock>) {
    let Some(node) = document.node(id) else {
        return;
    };
    match node.content() {
        NodeContent::Inline(inline) => out.push(SourceBlock {
            node: id,
            kind: node.kind().clone(),
            attrs: node.attrs().clone(),
            inline: inline.clone(),
        }),
        NodeContent::Children(children) => {
            for child in children {
                collect_inline_blocks(document, *child, out);
            }
        }
        NodeContent::Atomic | _ => {}
    }
}

fn slice_inline(
    inline: &InlineContent,
    start: usize,
    end: usize,
) -> Result<InlineContent, SessionError> {
    inline.offset_at(start).map_err(SessionError::Core)?;
    inline.offset_at(end).map_err(SessionError::Core)?;
    if start > end {
        return Err(SessionError::SelectionInvalid);
    }

    let mut pieces = Vec::new();
    let mut cursor = 0usize;
    for run in inline.runs() {
        let run_start = cursor;
        let run_end = run_start + run.len_bytes();
        cursor = run_end;

        let overlap_start = start.max(run_start);
        let overlap_end = end.min(run_end);
        if overlap_start >= overlap_end {
            continue;
        }

        let text = &run.text().as_str()[overlap_start - run_start..overlap_end - run_start];
        pieces.push(TextRun::new(text, run.marks().clone()).map_err(SessionError::Core)?);
    }

    InlineContent::new(pieces).map_err(SessionError::Core)
}

fn concatenated(inline: &InlineContent) -> String {
    inline
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

/// Normalizes platform clipboard text for pasting into a single paragraph.
///
/// Line breaks (`\r\n`, `\r`, `\n`) cannot be represented in a paragraph's
/// inline text, so each break collapses to one space. This helper remains the
/// single-block fallback; P3.3 structured/multi-block paste bypasses it.
#[must_use]
pub fn normalize_paste_text(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(character) = chars.next() {
        match character {
            '\r' => {
                chars.next_if_eq(&'\n');
                normalized.push(' ');
            }
            '\n' => normalized.push(' '),
            other => normalized.push(other),
        }
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use xiaomu_core::document::{Mark, MarkSet, NodeStoreBuilder};
    use xiaomu_core::selection::{CursorAffinity, TextPoint};

    struct Fixture {
        document: XiaomuDocument,
        first: NodeId,
        second: NodeId,
        nested: NodeId,
    }

    fn fixture() -> Fixture {
        let mut builder = NodeStoreBuilder::new();
        let bold = MarkSet::new([Mark::Bold]).unwrap();
        let first_inline = InlineContent::new([
            TextRun::new("ab", bold).unwrap(),
            TextRun::new("中", MarkSet::empty()).unwrap(),
        ])
        .unwrap();
        let first = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(first_inline),
            )
            .unwrap();
        let second = builder
            .insert(
                NodeKind::Heading(xiaomu_core::document::HeadingLevel::new(2).unwrap()),
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("cd", MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap();
        let nested = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([
                        TextRun::new("尾", MarkSet::new([Mark::Italic]).unwrap()).unwrap()
                    ])
                    .unwrap(),
                ),
            )
            .unwrap();
        let item = builder
            .insert(
                NodeKind::ListItem,
                NodeAttrs::empty(),
                NodeContent::children([nested]),
            )
            .unwrap();
        let list = builder
            .insert(
                NodeKind::BulletList,
                NodeAttrs::empty(),
                NodeContent::children([item]),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([first, second, list]),
            )
            .unwrap();
        Fixture {
            document: XiaomuDocument::new(root, builder.finish()).unwrap(),
            first,
            second,
            nested,
        }
    }

    fn point(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextPoint {
        let inline = document.node(node).unwrap().content().as_inline().unwrap();
        TextPoint::new(node, inline.offset_at(raw).unwrap(), CursorAffinity::Before)
    }

    #[test]
    fn cross_block_slice_keeps_partial_runs_marks_and_plain_fallback() {
        let fixture = fixture();
        let selection = DocumentSelection::new(
            point(&fixture.document, fixture.first, 1),
            point(&fixture.document, fixture.second, 1),
        );
        let slice = slice_selection(&fixture.document, selection)
            .unwrap()
            .unwrap();

        assert_eq!(slice.plain_text(), "b中\nc");
        assert_eq!(slice.blocks().len(), 2);
        assert!(matches!(slice.blocks()[0].kind(), NodeKind::Paragraph));
        assert!(matches!(slice.blocks()[1].kind(), NodeKind::Heading(_)));

        let first = slice.blocks()[0].inline();
        assert_eq!(concatenated(first), "b中");
        assert_eq!(first.runs().len(), 2);
        assert!(
            first.runs()[0]
                .marks()
                .contains(xiaomu_core::document::MarkKind::Bold)
        );
        assert!(first.runs()[1].marks().is_empty());
    }

    #[test]
    fn reverse_selection_produces_document_order_slice() {
        let fixture = fixture();
        let selection = DocumentSelection::new(
            point(&fixture.document, fixture.nested, 3),
            point(&fixture.document, fixture.first, 1),
        );
        let slice = slice_selection(&fixture.document, selection)
            .unwrap()
            .unwrap();
        assert_eq!(slice.plain_text(), "b中\ncd\n尾");
        assert_eq!(slice.blocks().len(), 3);
        assert!(
            slice.blocks()[2].inline().runs()[0]
                .marks()
                .contains(xiaomu_core::document::MarkKind::Italic)
        );
    }

    #[test]
    fn block_boundary_only_selection_has_newline_plain_text() {
        let fixture = fixture();
        let first_len = fixture
            .document
            .node(fixture.first)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .len_bytes();
        let selection = DocumentSelection::new(
            point(&fixture.document, fixture.first, first_len),
            point(&fixture.document, fixture.second, 0),
        );
        let slice = slice_selection(&fixture.document, selection)
            .unwrap()
            .unwrap();
        assert_eq!(slice.plain_text(), "\n");
        assert_eq!(slice.blocks().len(), 2);
        assert!(slice.blocks().iter().all(|block| block.inline().is_empty()));
    }

    #[test]
    fn same_logical_text_point_with_different_affinity_is_not_content() {
        let fixture = fixture();
        let point = point(&fixture.document, fixture.first, 1);
        let after = TextPoint::new(point.node_id(), point.offset(), CursorAffinity::After);
        let selection = DocumentSelection::new(point, after);
        assert_eq!(slice_selection(&fixture.document, selection).unwrap(), None);
    }

    #[test]
    fn line_breaks_collapse_to_spaces() {
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
    }
}
