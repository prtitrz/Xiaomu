//! One bounded, opt-in, transient input-rule reversal. No schema semantics.

use std::{mem::size_of, rc::Rc};

use xiaomu_core::document::{
    AttrValue, DocumentRevision, InlineAtomContent, Mark, MarkSet, Node, NodeAttrs, NodeContent,
    NodeKind, StringAttribute, XiaomuDocument,
};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use super::{
    DocumentSelection, DocumentSession, EditPlan, SelectionUpdate, SessionError, SessionOutcome,
};

const MAX_STEPS: usize = 64;
const MAX_NODES: usize = 256;
const MAX_BYTES: usize = 64 * 1024;
const MAX_ATTR_DEPTH: usize = 32;

/// A host-precomputed, exact reversal of one input-rule transformation.
///
/// This holds a transaction and exact final selection, never a policy,
/// document snapshot, history stack, marker string or canonical sidecar.
/// Hosts must include the actual triggering input replacement in the reversal;
/// a plain transformation inverse is not always sufficient.
///
/// Construction admits at most 64 steps, 256 payload nodes and 64 KiB of
/// accounted owned payload (text, kinds, attributes, marks, vector slots and
/// metadata, with conservative collection overhead). Attribute nesting is
/// bounded to 32. Unknown Core payload variants are refused. This is a payload
/// budget, not a claim about allocator bookkeeping or total session memory.
/// Inputs are checked before compact cloning so caller-supplied spare capacity
/// is not retained. Runtime repeats admission and validates the reversal against
/// the actual final candidate before publishing the forward edit.
#[derive(Clone, Debug)]
pub struct InputRuleUndoSpec {
    transaction: Transaction,
    selection: DocumentSelection,
    stored_marks_after: Option<Option<MarkSet>>,
}

impl EditPlan {
    /// Offers one bounded, exact reversal after this plan commits.
    ///
    /// Runtime validates the reversal against the final candidate, including
    /// host validation, before publishing either edit or token. The host owns
    /// rule matching and must omit this option when later rule-chain edits
    /// invalidate restoration. No marker syntax or schema is inferred here.
    /// The reversal is an isolated forward edit, not grouped history parity.
    #[must_use]
    pub fn with_input_rule_undo(mut self, spec: InputRuleUndoSpec) -> Self {
        self.input_rule_undo = Some(Box::new(spec));
        self
    }

    pub(crate) fn take_input_rule_undo(&mut self) -> Option<InputRuleUndoSpec> {
        self.input_rule_undo.take().map(|spec| *spec)
    }
}

impl InputRuleUndoSpec {
    /// Admits an exact, bounded restoration description.
    ///
    /// Document-dependent validation occurs at commit. A budget refusal must
    /// be handled by the host before conversion, for example by leaving the
    /// triggering input literal; Runtime does not silently drop the option.
    pub fn new(
        transaction: Transaction,
        selection: DocumentSelection,
    ) -> Result<Self, SessionError> {
        account(&transaction, None)?;
        Ok(Self {
            transaction: transaction.clone(),
            selection,
            stored_marks_after: None,
        })
    }

    /// Specifies typing marks after successful restoration.
    ///
    /// As for [`EditPlan::with_stored_marks`], `None` restores inheritance and
    /// `Some(empty)` explicitly requests unmarked text. An explicit value
    /// requires a collapsed inline final selection, validated at commit.
    pub fn with_stored_marks(mut self, marks: Option<MarkSet>) -> Result<Self, SessionError> {
        account(&self.transaction, marks.as_ref())?;
        self.stored_marks_after = Some(marks.clone());
        Ok(self)
    }

    fn validate_budget(&self) -> Result<(), SessionError> {
        account(
            &self.transaction,
            self.stored_marks_after.as_ref().and_then(Option::as_ref),
        )
    }

    fn plan(&self) -> EditPlan {
        let plan = EditPlan::new(
            self.transaction.clone(),
            SelectionUpdate::Exact {
                selection: self.selection,
            },
            None,
        );
        match &self.stored_marks_after {
            Some(marks) => plan.with_stored_marks(marks.clone()),
            None => plan,
        }
    }
}

pub(super) struct InputRuleUndoToken {
    spec: InputRuleUndoSpec,
    revision: DocumentRevision,
    selection: DocumentSelection,
}

impl DocumentSession {
    /// Whether the last opted-in rule has an eligible exact restoration.
    ///
    /// Read-only operations, stored-mark-only edits and genuine no-ops preserve
    /// availability. Any successful document/history commit or explicit
    /// selection publication clears it, including same-coordinate setters.
    /// Empty-stack Undo/Redo produce no document transaction and preserve it;
    /// that boundary is a native API choice, not measured browser parity.
    #[must_use]
    pub fn input_rule_undo_available(&self) -> bool {
        self.input_rule_undo_available_at(self.selection)
    }

    pub(super) fn input_rule_undo_available_at(&self, target: DocumentSelection) -> bool {
        self.input_rule_undo.as_ref().is_some_and(|token| {
            token.revision == self.document.revision()
                && token.selection == self.selection
                && token.selection == target
        })
    }

    pub(super) fn prepare_input_rule_undo(
        &self,
        spec: InputRuleUndoSpec,
        candidate: &XiaomuDocument,
        selection: DocumentSelection,
    ) -> Result<Rc<InputRuleUndoToken>, SessionError> {
        spec.validate_budget()?;
        let reversed = spec.transaction.apply_with_changes(candidate)?;
        spec.selection.validate(reversed.document())?;
        if spec.stored_marks_after.is_some()
            && (!spec.selection.is_collapsed() || spec.selection.as_same_node_inline().is_none())
        {
            return Err(SessionError::SelectionInvalid);
        }
        self.validate_candidate(reversed.document())?;
        Ok(Rc::new(InputRuleUndoToken {
            spec,
            revision: candidate.revision(),
            selection,
        }))
    }

    pub(super) fn undo_input_rule(&mut self) -> Result<SessionOutcome, SessionError> {
        if !self.input_rule_undo_available() {
            return Err(SessionError::UnsupportedEdit);
        }
        // A single shallow shared reference preserves the prior token on a
        // rejected commit. The reversal has no recursively nested metadata.
        let token = self
            .input_rule_undo
            .as_ref()
            .expect("checked eligibility")
            .clone();
        let plan = token.spec.plan();
        self.history.break_group();
        self.clear_stored_marks();
        // This is a new isolated forward edit. It neither pops ordinary Undo
        // nor impersonates typing; grouped PM history parity is out of scope.
        self.commit(plan)
    }
}

#[derive(Default)]
struct Budget {
    nodes: usize,
    bytes: usize,
}

fn refused() -> SessionError {
    SessionError::InputRuleUndoBudgetExceeded
}

impl Budget {
    fn bytes(&mut self, bytes: usize) -> Result<(), SessionError> {
        self.bytes = self.bytes.checked_add(bytes).ok_or_else(refused)?;
        if self.bytes > MAX_BYTES {
            return Err(refused());
        }
        Ok(())
    }

    fn slots<T>(&mut self, count: usize) -> Result<(), SessionError> {
        // A compact clone drops caller spare capacity. Account extra vector
        // growth room too, rather than only text bytes or size_of(Transaction).
        self.bytes(
            count
                .checked_mul(size_of::<T>())
                .and_then(|n| n.checked_mul(2))
                .ok_or_else(refused)?,
        )
    }

    fn node_count(&mut self, count: usize) -> Result<(), SessionError> {
        self.nodes = self.nodes.checked_add(count).ok_or_else(refused)?;
        if self.nodes > MAX_NODES {
            return Err(refused());
        }
        Ok(())
    }

    fn map_entry<V>(&mut self, key: &str) -> Result<(), SessionError> {
        // Charge a whole B-tree node per entry, including unused slots and
        // child edges. This deliberately overestimates compact small maps.
        self.bytes(12 * (size_of::<String>() + size_of::<V>() + size_of::<usize>()))?;
        self.bytes(key.len())
    }

    fn attrs(&mut self, attrs: &NodeAttrs) -> Result<(), SessionError> {
        for (key, value) in attrs.iter() {
            self.map_entry::<AttrValue>(key)?;
            self.attr(value, 0)?;
        }
        Ok(())
    }

    fn attr(&mut self, value: &AttrValue, depth: usize) -> Result<(), SessionError> {
        if depth > MAX_ATTR_DEPTH {
            return Err(refused());
        }
        match value {
            AttrValue::Null | AttrValue::Bool(_) | AttrValue::Integer(_) => Ok(()),
            AttrValue::String(value) => self.bytes(value.len()),
            AttrValue::List(values) => {
                self.slots::<AttrValue>(values.len())?;
                for value in values {
                    self.attr(value, depth + 1)?;
                }
                Ok(())
            }
            AttrValue::Object(values) => {
                for (key, value) in values {
                    self.map_entry::<AttrValue>(key)?;
                    self.attr(value, depth + 1)?;
                }
                Ok(())
            }
            _ => Err(refused()),
        }
    }

    fn string_attr(&mut self, value: &StringAttribute) -> Result<(), SessionError> {
        self.bytes(value.as_str().map_or(0, str::len))
    }

    fn mark(&mut self, mark: &Mark) -> Result<(), SessionError> {
        match mark {
            Mark::Bold | Mark::Italic | Mark::Code | Mark::Underline | Mark::Strike => Ok(()),
            Mark::Link(link) => {
                let attrs = link.attributes();
                for value in [
                    attrs.href(),
                    attrs.target(),
                    attrs.rel(),
                    attrs.class(),
                    attrs.title(),
                ] {
                    self.string_attr(value)?;
                }
                Ok(())
            }
            Mark::TextStyle(style) => {
                let attrs = style.attributes();
                for value in [attrs.color(), attrs.font_family(), attrs.font_size()] {
                    self.string_attr(value)?;
                }
                Ok(())
            }
            _ => Err(refused()),
        }
    }

    fn marks(&mut self, marks: &MarkSet) -> Result<(), SessionError> {
        self.slots::<Mark>(marks.len())?;
        for mark in marks.as_slice() {
            self.mark(mark)?;
        }
        Ok(())
    }

    fn kind(&mut self, kind: &NodeKind) -> Result<(), SessionError> {
        match kind {
            NodeKind::Custom(key) => self.bytes(key.len()),
            NodeKind::InlineAtom(kind) => self.bytes(kind.as_str().len()),
            NodeKind::Document
            | NodeKind::Paragraph
            | NodeKind::Heading(_)
            | NodeKind::Quote
            | NodeKind::BulletList
            | NodeKind::OrderedList
            | NodeKind::ListItem
            | NodeKind::TaskList
            | NodeKind::TaskItem
            | NodeKind::CodeBlock
            | NodeKind::HorizontalRule
            | NodeKind::Image
            | NodeKind::Table
            | NodeKind::TableRow
            | NodeKind::TableCell
            | NodeKind::TableHeader => Ok(()),
            _ => Err(refused()),
        }
    }

    fn atom(&mut self, content: &InlineAtomContent) -> Result<(), SessionError> {
        self.bytes(content.fallback_text().len())?;
        self.marks(content.marks())
    }

    fn content(&mut self, content: &NodeContent) -> Result<(), SessionError> {
        match content {
            NodeContent::Atomic => Ok(()),
            NodeContent::Children(children) => {
                self.slots::<xiaomu_core::document::NodeId>(children.len())
            }
            NodeContent::InlineAtom(content) => self.atom(content),
            NodeContent::Inline(content) => {
                self.slots::<xiaomu_core::document::TextRun>(content.runs().len())?;
                self.slots::<xiaomu_core::document::InlineAtomPlacement>(content.atoms().len())?;
                for run in content.runs() {
                    self.bytes(run.len_bytes())?;
                    self.marks(run.marks())?;
                }
                Ok(())
            }
            _ => Err(refused()),
        }
    }

    fn node(&mut self, node: &Node) -> Result<(), SessionError> {
        self.node_count(1)?;
        self.kind(node.kind())?;
        self.attrs(node.attrs())?;
        self.content(node.content())
    }

    fn step(&mut self, step: &TransactionStep) -> Result<(), SessionError> {
        match step {
            TransactionStep::ReplaceText { replacement, .. }
            | TransactionStep::ReplaceInlineText { replacement, .. } => {
                self.bytes(replacement.len())
            }
            TransactionStep::InsertInlineAtom {
                kind,
                attrs,
                content,
                ..
            } => {
                self.node_count(1)?;
                self.bytes(kind.as_str().len())?;
                self.attrs(attrs)?;
                self.atom(content)
            }
            TransactionStep::SetInlineAtomMarks { marks, .. } => self.marks(marks),
            TransactionStep::RestoreInlineAtom { node, .. }
            | TransactionStep::RestoreJoinedNode { node, .. } => self.node(node),
            TransactionStep::InsertNode {
                kind,
                attrs,
                content,
                ..
            } => {
                self.node_count(1)?;
                self.kind(kind)?;
                self.attrs(attrs)?;
                self.content(content)
            }
            TransactionStep::SetNodeAttrs { attrs, .. } => self.attrs(attrs),
            TransactionStep::SetNodeKind { kind, .. } => self.kind(kind),
            TransactionStep::AddMark { mark, .. } => self.mark(mark),
            TransactionStep::RestoreSubtree { nodes, .. } => {
                self.slots::<Node>(nodes.len())?;
                for node in nodes {
                    self.node(node)?;
                }
                Ok(())
            }
            TransactionStep::RemoveInlineAtom { .. }
            | TransactionStep::InsertTable { .. }
            | TransactionStep::InsertTableRow { .. }
            | TransactionStep::InsertTableColumn { .. }
            | TransactionStep::RemoveNode { .. }
            | TransactionStep::RemoveMark { .. }
            | TransactionStep::SplitNode { .. }
            | TransactionStep::SplitInlineNode { .. }
            | TransactionStep::JoinNodes { .. } => Ok(()),
            _ => Err(refused()),
        }
    }
}

fn account(transaction: &Transaction, marks: Option<&MarkSet>) -> Result<(), SessionError> {
    if transaction.steps().len() > MAX_STEPS {
        return Err(refused());
    }
    let mut budget = Budget::default();
    budget.slots::<TransactionStep>(transaction.steps().len())?;
    match transaction.origin() {
        TransactionOrigin::Extension(name) => budget.bytes(name.len())?,
        TransactionOrigin::UserInput | TransactionOrigin::System => {}
        _ => return Err(refused()),
    }
    for (key, value) in transaction.metadata() {
        budget.map_entry::<String>(key)?;
        budget.bytes(value.len())?;
    }
    for step in transaction.steps() {
        budget.step(step)?;
    }
    if let Some(marks) = marks {
        budget.marks(marks)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "input_rule_undo_tests.rs"]
mod tests;
