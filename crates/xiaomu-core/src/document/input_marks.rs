//! Exact mixed-inline mark inheritance shared by queries and transactions.

use crate::selection::InlinePoint;
use crate::{Error, Result};

use super::{InlineContent, MarkSet, NodeKind, NodeStore, XiaomuDocument};

impl XiaomuDocument {
    /// Returns insertion marks at one validated mixed-inline caret gap.
    ///
    /// The left inline child wins; without a left child, the right child
    /// supplies marks. The atom ordinal distinguishes those children even
    /// when they share a byte offset. Built-in hard breaks contribute their
    /// own marks, including an empty set. Marked extension atoms contribute
    /// their marks; unmarked extensions remain transparent for compatibility
    /// with the original text-only inheritance contract.
    ///
    /// This query preserves every mark and attribute, including links. It
    /// does not apply host exclusion rules or transient stored marks.
    pub fn inherited_inline_marks(&self, at: InlinePoint) -> Result<MarkSet> {
        at.validate(self)?;
        let inline = self
            .node(at.node_id())
            .unwrap()
            .content()
            .as_inline()
            .unwrap();
        inherited_marks_with_store(inline, at, self.store())
    }

    /// Returns replacement marks from the inline child after `start`.
    ///
    /// Both endpoints must be valid, ordered gaps of the same inline node.
    /// Unlike caret inheritance, this range query uses the right child even
    /// when a left child exists. A collapsed range still uses this rule;
    /// `None` means no contributing child follows its start. Built-in breaks
    /// with empty marks return `Some(empty)`, not `None`. Unmarked extension
    /// atoms are transparent; all other marks and attributes are preserved.
    ///
    /// Hosts may use this for selection replacement. Runtime retains its
    /// legacy left-run behavior for text-only selections and uses this rule
    /// for mixed-inline ranges. No stored-mark or product policy is applied.
    pub fn inherited_inline_range_marks(
        &self,
        start: InlinePoint,
        end: InlinePoint,
    ) -> Result<Option<MarkSet>> {
        start.validate(self)?;
        end.validate(self)?;
        if start.node_id() != end.node_id()
            || (start.text_offset(), start.atom_index()) > (end.text_offset(), end.atom_index())
        {
            return Err(Error::InvalidSelection);
        }
        let inline = self
            .node(start.node_id())
            .unwrap()
            .content()
            .as_inline()
            .unwrap();
        right_marks(inline, start, self.store()).map(|marks| marks.cloned())
    }
}

/// Applies the same query to a transaction's validated intermediate store.
pub(crate) fn inherited_marks_with_store(
    inline: &InlineContent,
    at: InlinePoint,
    store: &NodeStore,
) -> Result<MarkSet> {
    inline.validate_offset(at.text_offset())?;
    if at.atom_index() > inline.atom_count_at(at.text_offset()) {
        return Err(Error::InvalidSelection);
    }
    let mut left_atom_marks = None;
    for placement in inline
        .atoms()
        .iter()
        .filter(|placement| placement.text_offset() == at.text_offset())
        .take(at.atom_index())
    {
        if let Some(marks) = atom_marks(store, placement.atom())? {
            left_atom_marks = Some(marks);
        }
    }
    if let Some(marks) = left_atom_marks {
        return Ok(marks.clone());
    }
    if at.text_offset().as_usize() > 0 {
        let mut end = 0;
        for run in inline.runs() {
            end += run.len_bytes();
            if at.text_offset().as_usize() <= end {
                return Ok(run.marks().clone());
            }
        }
    }
    Ok(right_marks(inline, at, store)?
        .cloned()
        .unwrap_or_else(MarkSet::empty))
}

fn right_marks<'a>(
    inline: &'a InlineContent,
    at: InlinePoint,
    store: &'a NodeStore,
) -> Result<Option<&'a MarkSet>> {
    for placement in inline
        .atoms()
        .iter()
        .filter(|placement| placement.text_offset() == at.text_offset())
        .skip(at.atom_index())
    {
        if let Some(marks) = atom_marks(store, placement.atom())? {
            return Ok(Some(marks));
        }
    }
    let mut end = 0;
    for run in inline.runs() {
        end += run.len_bytes();
        if at.text_offset().as_usize() < end {
            return Ok(Some(run.marks()));
        }
    }
    Ok(None)
}

fn atom_marks(store: &NodeStore, id: super::NodeId) -> Result<Option<&MarkSet>> {
    let node = store.get(id).ok_or(Error::UnknownNode)?;
    let NodeKind::InlineAtom(kind) = node.kind() else {
        return Err(Error::InvalidSelection);
    };
    let content = node
        .content()
        .as_inline_atom()
        .ok_or(Error::InvalidSelection)?;
    Ok((kind.is_hard_break() || !content.marks().is_empty()).then_some(content.marks()))
}
