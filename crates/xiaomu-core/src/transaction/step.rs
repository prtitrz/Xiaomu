//! Typed transaction steps.

use crate::document::{Mark, MarkKind, NodeAttrs, NodeContent, NodeId};
use crate::text::TextRange;

/// One typed canonical mutation inside a [`Transaction`](super::Transaction).
///
/// Steps are declarative: they describe *what* changes, and the applying
/// engine decides how to preserve invariants. Structural validity is checked
/// against the target snapshot during application, so a step that is valid
/// for one revision may be rejected for another.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransactionStep {
    /// Replaces `[range.start, range.end)` of one inline node's text with
    /// `replacement`. An empty replacement deletes the span.
    ReplaceText {
        /// Inline-bearing node whose concatenated text is edited.
        node: NodeId,
        /// Half-open byte range into the node's concatenated text.
        range: TextRange,
        /// UTF-8 replacement text; may be empty.
        replacement: String,
    },
    /// Inserts a newly allocated node as a child of `parent` at `index`.
    ///
    /// `index` counts existing children before the insertion point. The new
    /// node receives a fresh stable `NodeId` from the document allocator.
    InsertNode {
        /// Existing parent whose child list gains one entry.
        parent: NodeId,
        /// Number of children before the insertion point.
        index: usize,
        /// Kind of the created node.
        kind: crate::document::NodeKind,
        /// Attributes of the created node.
        attrs: NodeAttrs,
        /// Content of the created node; child references must already exist.
        content: NodeContent,
    },
    /// Removes `node` together with its whole subtree from the document.
    ///
    /// The root cannot be removed.
    RemoveNode {
        /// Node to remove.
        node: NodeId,
    },
    /// Replaces all attributes of `node` with `attrs`.
    SetNodeAttrs {
        /// Node whose attributes are replaced.
        node: NodeId,
        /// Complete replacement attribute set.
        attrs: NodeAttrs,
    },
    /// Applies `mark` to `[range.start, range.end)` of one inline node.
    ///
    /// A conflicting mark of the same kind inside the range is replaced.
    AddMark {
        /// Inline-bearing node being marked.
        node: NodeId,
        /// Half-open byte range into the node's concatenated text.
        range: TextRange,
        /// Mark to apply.
        mark: Mark,
    },
    /// Removes every mark of `kind` from `[range.start, range.end)` of one
    /// inline node.
    RemoveMark {
        /// Inline-bearing node being unmarked.
        node: NodeId,
        /// Half-open byte range into the node's concatenated text.
        range: TextRange,
        /// Semantic kind of the marks to remove.
        mark_kind: MarkKind,
    },
}
