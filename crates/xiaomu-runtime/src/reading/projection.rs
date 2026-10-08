//! Bounded, owned reading projections over immutable canonical snapshots.

use xiaomu_core::{
    document::{DocumentRevision, InlineContent, Node, NodeId, NodeKind, XiaomuDocument},
    selection::{CursorAffinity, InlinePoint},
    text::TextBuffer,
};

use super::error::checked_add;
use super::{ReadingBudget, ReadingError, ReadingSpan, ReadingSpanKind, ReadingTextBlock};

/// Explicit atom emission policy. Atom fallback text is not implicitly used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AtomText {
    /// Omit textual output, retaining a typed zero-width source event.
    #[default]
    Omit,
    /// Emit exactly one Unicode scalar without inventing canonical text bytes.
    Character(char),
}

impl AtomText {
    fn len_bytes(self) -> usize {
        match self {
            Self::Omit => 0,
            Self::Character(ch) => ch.len_utf8(),
        }
    }
}

/// Independent typed-atom policies; block separators belong to the consumer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadingProjectionOptions {
    /// Policy for the typed built-in HardBreak only.
    pub hard_break: AtomText,
    /// Policy for extension inline atoms, including ones named "hardBreak".
    pub other_atoms: AtomText,
}

/// Preflight bounds for all newly owned projection payloads.
///
/// These are logical bounds, not exact allocator/RSS accounting. Text bytes and
/// kind keys are counted before cloning, spans before allocating, and traversal
/// scratch and block indexes are bounded by `max_nodes`. Exceeding any limit
/// rejects the whole build. Only canonical text is copied; document trees and
/// marks are not retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadingProjectionLimits {
    /// All canonical nodes, including containers and atoms (default 100,000).
    pub max_nodes: usize,
    /// Total projected/canonical UTF-8 and cloned kind-key bytes (default 32 MiB).
    pub max_bytes: usize,
    /// Source spans, counting each atom and nonempty text segment (200,000).
    pub max_spans: usize,
}

impl Default for ReadingProjectionLimits {
    fn default() -> Self {
        Self {
            max_nodes: 100_000,
            max_bytes: 32 * 1024 * 1024,
            max_spans: 200_000,
        }
    }
}

/// Revision-bound ordered textblocks with exact source mapping.
///
/// This value owns its projection and does not borrow or mutate the session.
/// The revision is meaningful only within its originating document sequence;
/// consumers must bind it to that owning session/view. A heading remains a
/// textblock with its typed level, whitespace and original identity intact.
/// Atomic blocks and structural containers emit no textblocks or separators.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadingProjection {
    root: NodeId,
    revision: DocumentRevision,
    options: ReadingProjectionOptions,
    blocks: Vec<ReadingTextBlock>,
    by_node: Vec<(NodeId, usize)>,
    pub(super) positions: super::prefix::PositionIndex,
}

impl ReadingProjection {
    /// Builds after a complete, allocation-free payload preflight.
    /// Does not consult selection, history, UI, codecs or persistence.
    pub fn build(
        document: &XiaomuDocument,
        options: ReadingProjectionOptions,
        limits: ReadingProjectionLimits,
    ) -> Result<Self, ReadingError> {
        preflight(document, options, limits)?;
        let mut blocks = Vec::new();
        let mut by_node = Vec::new();
        let mut stack = Vec::new();
        reserve(&mut by_node, document.node_count())?;
        reserve(&mut blocks, document.node_count())?;
        reserve(&mut stack, document.node_count())?;
        stack.push(document.root());
        while let Some(id) = stack.pop() {
            let node = document.node(id).expect("validated canonical tree");
            if let Some(inline) = node.content().as_inline() {
                let block = build_block(document, node, inline, blocks.len(), options)?;
                by_node.push((id, blocks.len()));
                blocks.push(block);
            } else if let Some(children) = node.content().as_children() {
                stack.extend(children.iter().rev().copied());
            }
        }
        by_node.sort_unstable_by_key(|entry| entry.0);
        let positions = super::prefix::PositionIndex::build(document)?;
        Ok(Self {
            root: document.root(),
            revision: document.revision(),
            options,
            blocks,
            by_node,
            positions,
        })
    }

    /// Canonical document root, whose structural events frame the traversal.
    #[must_use]
    pub const fn root(&self) -> NodeId {
        self.root
    }

    /// Ordered structural events, including atomic blocks and containers.
    /// Inline atom types and coordinates remain in each textblock's spans.
    #[must_use]
    pub fn events(&self) -> &[super::ReadingEvent] {
        &self.positions.events
    }

    /// Source snapshot revision, not a session identity or persistence version.
    #[must_use]
    pub const fn revision(&self) -> DocumentRevision {
        self.revision
    }

    /// Atom policies used for this projection.
    #[must_use]
    pub const fn options(&self) -> ReadingProjectionOptions {
        self.options
    }

    /// Ordered inline-bearing nodes, including empty textblocks.
    #[must_use]
    pub fn blocks(&self) -> &[ReadingTextBlock] {
        &self.blocks
    }

    /// Finds one source textblock by canonical identity in logarithmic time.
    #[must_use]
    pub fn block(&self, node_id: NodeId) -> Option<&ReadingTextBlock> {
        let index = self
            .by_node
            .binary_search_by_key(&node_id, |entry| entry.0)
            .ok()?;
        self.blocks.get(self.by_node[index].1)
    }
}

fn reserve<T>(buffer: &mut Vec<T>, size: usize) -> Result<(), ReadingError> {
    buffer
        .try_reserve_exact(size)
        .map_err(|_| ReadingError::AllocationFailed)
}

fn atom_text(node: &Node, options: ReadingProjectionOptions) -> AtomText {
    let NodeKind::InlineAtom(kind) = node.kind() else {
        unreachable!("validated atom")
    };
    if kind.is_hard_break() {
        options.hard_break
    } else {
        options.other_atoms
    }
}

fn preflight(
    document: &XiaomuDocument,
    options: ReadingProjectionOptions,
    limits: ReadingProjectionLimits,
) -> Result<(), ReadingError> {
    if document.node_count() > limits.max_nodes {
        return Err(ReadingError::BudgetExceeded(ReadingBudget::Nodes));
    }
    let mut bytes = 0;
    let mut spans = 0;
    for node in document.store().iter() {
        if let Some(inline) = node.content().as_inline() {
            checked_add(
                &mut bytes,
                inline
                    .len_bytes()
                    .checked_mul(2)
                    .ok_or(ReadingError::BudgetExceeded(ReadingBudget::ProjectionBytes))?,
                limits.max_bytes,
                ReadingBudget::ProjectionBytes,
            )?;
            if let NodeKind::Custom(key) = node.kind() {
                checked_add(
                    &mut bytes,
                    key.len(),
                    limits.max_bytes,
                    ReadingBudget::ProjectionBytes,
                )?;
            }
            // Exact source segment count, independent of formatting runs.
            checked_add(
                &mut spans,
                span_capacity(inline)?,
                limits.max_spans,
                ReadingBudget::Spans,
            )?;
        } else if let NodeKind::InlineAtom(kind) = node.kind() {
            checked_add(
                &mut bytes,
                atom_text(node, options).len_bytes(),
                limits.max_bytes,
                ReadingBudget::ProjectionBytes,
            )?;
            // Typed built-ins allocate no key; extension keys are cloned once.
            if !kind.is_hard_break() {
                checked_add(
                    &mut bytes,
                    kind.as_str().len(),
                    limits.max_bytes,
                    ReadingBudget::ProjectionBytes,
                )?;
            }
        }
    }
    Ok(())
}

fn span_capacity(inline: &InlineContent) -> Result<usize, ReadingError> {
    let mut spans = inline.atoms().len();
    let mut previous = 0;
    for atom in inline.atoms() {
        let offset = atom.text_offset().as_usize();
        if offset > previous {
            spans = spans
                .checked_add(1)
                .ok_or(ReadingError::BudgetExceeded(ReadingBudget::Spans))?;
        }
        previous = offset;
    }
    if inline.len_bytes() > previous {
        spans = spans
            .checked_add(1)
            .ok_or(ReadingError::BudgetExceeded(ReadingBudget::Spans))?;
    }
    Ok(spans)
}

fn build_block(
    document: &XiaomuDocument,
    node: &Node,
    inline: &InlineContent,
    order: usize,
    options: ReadingProjectionOptions,
) -> Result<ReadingTextBlock, ReadingError> {
    let mut canonical_text = String::new();
    canonical_text
        .try_reserve_exact(inline.len_bytes())
        .map_err(|_| ReadingError::AllocationFailed)?;
    for run in inline.runs() {
        canonical_text.push_str(run.text().as_str());
    }
    let mut block = ReadingTextBlock {
        node_id: node.id(),
        order,
        kind: node.kind().clone(),
        text: String::new(),
        canonical: TextBuffer::from_string(canonical_text),
        spans: Vec::new(),
    };
    let atom_bytes: usize = inline
        .atoms()
        .iter()
        .map(|placement| {
            atom_text(
                document.node(placement.atom()).expect("validated atom"),
                options,
            )
            .len_bytes()
        })
        .sum();
    block
        .text
        .try_reserve_exact(inline.len_bytes() + atom_bytes)
        .map_err(|_| ReadingError::AllocationFailed)?;
    reserve(&mut block.spans, span_capacity(inline)?)?;
    let mut placements = inline.atoms().iter().peekable();
    let mut canonical = 0;
    let mut ordinal = 0;
    for run in inline.runs() {
        let text = run.text().as_str();
        let mut local = 0;
        while local < text.len() {
            while placements
                .peek()
                .is_some_and(|placement| placement.text_offset().as_usize() == canonical)
            {
                let placement = placements.next().expect("peeked atom");
                append_atom(
                    &mut block,
                    document,
                    placement.atom(),
                    canonical,
                    ordinal,
                    options,
                );
                ordinal += 1;
            }
            let end = placements.peek().map_or(text.len(), |placement| {
                (placement.text_offset().as_usize() - canonical + local).min(text.len())
            });
            append_text(&mut block, &text[local..end], canonical, ordinal);
            canonical += end - local;
            local = end;
            ordinal = 0;
        }
    }
    for placement in placements {
        append_atom(
            &mut block,
            document,
            placement.atom(),
            canonical,
            ordinal,
            options,
        );
        ordinal += 1;
    }
    Ok(block)
}

fn point(
    block: &ReadingTextBlock,
    offset: usize,
    ordinal: usize,
    affinity: CursorAffinity,
) -> InlinePoint {
    InlinePoint::new(
        block.node_id,
        block
            .canonical
            .offset_at(offset)
            .expect("validated canonical boundary"),
        ordinal,
        affinity,
    )
}

fn append_text(block: &mut ReadingTextBlock, text: &str, canonical: usize, ordinal: usize) {
    if text.is_empty() {
        return;
    }
    let start = block.text.len();
    block.text.push_str(text);
    let end = point(block, canonical + text.len(), 0, CursorAffinity::After);
    if let Some(previous) = block.spans.last_mut()
        && matches!(previous.kind, ReadingSpanKind::Text)
    {
        previous.projected.end = block.text.len();
        previous.end = end;
    } else {
        block.spans.push(ReadingSpan {
            projected: start..block.text.len(),
            start: point(block, canonical, ordinal, CursorAffinity::Before),
            end,
            kind: ReadingSpanKind::Text,
        });
    }
}

fn append_atom(
    block: &mut ReadingTextBlock,
    document: &XiaomuDocument,
    atom: NodeId,
    canonical: usize,
    ordinal: usize,
    options: ReadingProjectionOptions,
) {
    let node = document.node(atom).expect("validated atom");
    let NodeKind::InlineAtom(kind) = node.kind() else {
        unreachable!("validated atom kind")
    };
    let start = block.text.len();
    if let AtomText::Character(ch) = atom_text(node, options) {
        block.text.push(ch);
    }
    block.spans.push(ReadingSpan {
        projected: start..block.text.len(),
        start: point(block, canonical, ordinal, CursorAffinity::Before),
        end: point(block, canonical, ordinal + 1, CursorAffinity::After),
        kind: ReadingSpanKind::InlineAtom {
            node_id: atom,
            kind: kind.clone(),
        },
    });
}
