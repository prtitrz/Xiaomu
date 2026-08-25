//! Typed transaction steps.

use crate::document::{Mark, MarkKind, Node, NodeAttrs, NodeContent, NodeId};
use crate::text::{TextOffset, TextRange};

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
    /// Replaces the semantic kind of `node`, keeping its identity, attributes,
    /// and content.
    ///
    /// The new kind must accept the node's existing content shape, and the
    /// parent must still allow the node as a child. The document root cannot
    /// change kind. Positions do not move.
    SetNodeKind {
        /// Node whose kind is replaced.
        node: NodeId,
        /// Replacement semantic kind.
        kind: crate::document::NodeKind,
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
    /// Re-inserts a previously removed subtree under `parent` at `index`,
    /// keeping every original node identity and payload.
    ///
    /// This step exists so that undo can restore a removed subtree exactly;
    /// its intended producer is [`AppliedTransaction::inverse`](super::AppliedTransaction::inverse).
    /// It is **not** a general-purpose copy or move primitive:
    ///
    /// - every identity in `nodes` must currently be absent from the store,
    ///   so the step can never duplicate or overwrite live nodes;
    /// - node identities cannot be minted by callers, so `nodes` can only
    ///   carry payloads obtained from snapshots of the same document
    ///   lineage — in practice, payloads that this document previously
    ///   removed;
    /// - `root` must be one of `nodes`, and the re-attached subtree must
    ///   pass full-tree validation like any other step.
    ///
    /// Violations fail application atomically with `InvalidTransaction` or a
    /// validation error. Its mapping data is a `NodeInserted` entry carrying
    /// the subtree root.
    RestoreSubtree {
        /// Existing parent whose child list gains the subtree root.
        parent: NodeId,
        /// Number of children before the re-inserted root.
        index: usize,
        /// Identity of the subtree root within `nodes`.
        root: NodeId,
        /// The removed nodes with their original payloads, in deterministic
        /// identity order.
        nodes: Vec<Node>,
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
    /// Splits one inline-bearing node at `at` in its concatenated text.
    ///
    /// The original node keeps its identity and the text before `at`; a
    /// freshly allocated sibling with the same kind and attributes receives
    /// the text from `at` onward and enters the parent's child list directly
    /// after it. Splitting inside a run gives both halves that run's marks;
    /// splitting exactly at a run boundary leaves each whole run on one
    /// side. Either resulting half may be empty.
    SplitNode {
        /// Inline-bearing node being split; must not be the document root.
        node: NodeId,
        /// UTF-8 byte offset of the split point; a validated boundary of
        /// the node's concatenated text (zero through total length
        /// inclusive).
        at: TextOffset,
    },
    /// Merges two adjacent inline-bearing siblings into one.
    ///
    /// `second` must be the child immediately following `first`. The merged
    /// node keeps `first`'s identity, kind, and attributes; its inline
    /// content is the normalized concatenation of both contents. `second`
    /// leaves the document together with its whole subtree, so undo can
    /// restore it with its exact identity via
    /// [`TransactionStep::RestoreSubtree`].
    JoinNodes {
        /// Surviving sibling whose child position stays put.
        first: NodeId,
        /// Sibling immediately after `first` that is absorbed into it.
        second: NodeId,
    },
}
