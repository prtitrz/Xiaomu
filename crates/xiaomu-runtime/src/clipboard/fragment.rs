//! Detached structured clipboard fragment values.
//!
//! Clipboard nodes deliberately carry no canonical `NodeId`. A fragment is a
//! value tree that preserves selected container semantics while allowing paste
//! to allocate fresh identities in the destination document. Inline atoms are
//! captured as detached payloads ([`ClipboardAtom`]: kind, attrs, and
//! `fallback_text`) anchored at fragment text boundaries, so an atom never
//! drags its source identity across the clipboard.

use std::collections::BTreeMap;

use xiaomu_core::Result;
use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::text::TextOffset;

/// One detached inline atom captured by the clipboard.
///
/// The payload is identity-free: paste allocates a fresh canonical `NodeId`
/// and re-anchors the atom at the corresponding destination boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardAtom {
    anchor: TextOffset,
    kind: AtomKind,
    attrs: NodeAttrs,
    content: InlineAtomContent,
}

impl ClipboardAtom {
    pub(crate) fn new(
        anchor: TextOffset,
        kind: AtomKind,
        attrs: NodeAttrs,
        content: InlineAtomContent,
    ) -> Self {
        Self {
            anchor,
            kind,
            attrs,
            content,
        }
    }

    /// Returns the validated UTF-8 text boundary anchoring the atom.
    #[must_use]
    pub const fn anchor(&self) -> TextOffset {
        self.anchor
    }

    /// Returns the stable semantic atom kind.
    #[must_use]
    pub const fn kind(&self) -> &AtomKind {
        &self.kind
    }

    /// Returns the extension payload attributes.
    #[must_use]
    pub const fn attrs(&self) -> &NodeAttrs {
        &self.attrs
    }

    /// Returns the host-neutral canonical atom content.
    #[must_use]
    pub const fn content(&self) -> &InlineAtomContent {
        &self.content
    }
}

/// Detached mixed inline content: text runs plus ordered atom payloads.
///
/// Atoms are ordered by anchor boundary; payloads sharing one anchor keep
/// their capture order, which is the canonical order a paste restores.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClipboardInline {
    runs: Vec<TextRun>,
    atoms: Vec<ClipboardAtom>,
}

impl ClipboardInline {
    /// Creates detached inline content, validating every atom anchor
    /// against the concatenated run text.
    pub fn new(
        runs: impl IntoIterator<Item = TextRun>,
        atoms: impl IntoIterator<Item = ClipboardAtom>,
    ) -> Result<Self> {
        let runs = runs.into_iter().collect::<Vec<_>>();
        // Anchors are text coordinates: validate them against a text-only
        // view of the runs.
        let text_view = InlineContent::new(runs.iter().cloned())?;
        let mut atoms = atoms.into_iter().collect::<Vec<_>>();
        for atom in &atoms {
            text_view.validate_offset(atom.anchor())?;
        }
        atoms.sort_by_key(|atom| atom.anchor().as_usize());
        Ok(Self { runs, atoms })
    }

    /// Creates atom-free detached inline content.
    pub fn text_only(runs: impl IntoIterator<Item = TextRun>) -> Result<Self> {
        Self::new(runs, [])
    }

    /// Returns normalized text runs.
    #[must_use]
    pub fn runs(&self) -> &[TextRun] {
        &self.runs
    }

    /// Returns detached atom payloads ordered by anchor boundary.
    #[must_use]
    pub fn atoms(&self) -> &[ClipboardAtom] {
        &self.atoms
    }

    /// Returns whether the content has neither text nor atoms.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty() && self.atoms.is_empty()
    }

    /// Returns the total UTF-8 byte length of the text runs.
    #[must_use]
    pub fn len_bytes(&self) -> usize {
        self.runs.iter().map(TextRun::len_bytes).sum()
    }

    /// Returns the concatenated run text without atom fallbacks.
    ///
    /// This is the text a structured paste re-materializes; atoms ride
    /// alongside as payloads.
    #[must_use]
    pub fn text(&self) -> String {
        self.runs.iter().map(|run| run.text().as_str()).collect()
    }

    /// Returns the plain-text fallback with `fallback_text` spliced in at
    /// every atom anchor.
    ///
    /// External applications see the same content a reader would: atoms
    /// appear as their host-neutral fallback text.
    #[must_use]
    pub fn plain_text(&self) -> String {
        let mut plain = String::new();
        let mut cursor = 0usize;
        for atom in &self.atoms {
            let anchor = atom.anchor().as_usize();
            append_run_text(&mut plain, &self.runs, cursor, anchor);
            cursor = anchor;
            plain.push_str(atom.content.fallback_text());
        }
        append_run_text(&mut plain, &self.runs, cursor, self.len_bytes());
        plain
    }

    /// Rebuilds canonical mixed-inline content, mapping every detached atom
    /// to a freshly allocated placement.
    ///
    /// `place` is called once per atom in canonical order and must return
    /// the identity allocated for it; the returned placements reference
    /// those identities.
    pub(crate) fn to_inline_content(
        &self,
        mut place: impl FnMut() -> NodeId,
    ) -> Result<InlineContent> {
        let placements = self
            .atoms
            .iter()
            .map(|_| xiaomu_core::document::InlineAtomPlacement::new(place(), TextOffset::ZERO));
        // Rebuild with placements in order; anchors come from the payloads.
        let mut atoms = placements.collect::<Vec<_>>();
        for (index, atom) in self.atoms.iter().enumerate() {
            atoms[index] =
                xiaomu_core::document::InlineAtomPlacement::new(atoms[index].atom(), atom.anchor());
        }
        InlineContent::with_atoms(self.runs.iter().cloned(), atoms)
    }
}

fn append_run_text(out: &mut String, runs: &[TextRun], from: usize, to: usize) {
    if to <= from {
        return;
    }
    let mut cursor = 0usize;
    for run in runs {
        let run_start = cursor;
        let run_end = run_start + run.len_bytes();
        cursor = run_end;
        if run_end <= from {
            continue;
        }
        if run_start >= to {
            break;
        }
        let start = from.max(run_start) - run_start;
        let end = to.min(run_end) - run_start;
        out.push_str(&run.text().as_str()[start..end]);
    }
}

/// Content of one detached clipboard node.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClipboardNodeContent {
    /// Inline-bearing selected content, including marks and detached atoms.
    Inline(ClipboardInline),
    /// Selected child fragment nodes in canonical document order.
    Children(Vec<ClipboardNode>),
}

impl ClipboardNodeContent {
    /// Returns inline content when this fragment node is inline-bearing.
    #[must_use]
    pub const fn as_inline(&self) -> Option<&ClipboardInline> {
        match self {
            Self::Inline(inline) => Some(inline),
            _ => None,
        }
    }

    /// Returns selected children when this fragment node is a container.
    #[must_use]
    pub fn as_children(&self) -> Option<&[ClipboardNode]> {
        match self {
            Self::Children(children) => Some(children),
            _ => None,
        }
    }
}

/// One detached node in a structured clipboard fragment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardNode {
    kind: NodeKind,
    attrs: NodeAttrs,
    content: ClipboardNodeContent,
}

impl ClipboardNode {
    pub(crate) fn new(kind: NodeKind, attrs: NodeAttrs, content: ClipboardNodeContent) -> Self {
        Self {
            kind,
            attrs,
            content,
        }
    }

    /// Returns the semantic node kind captured from the source document.
    #[must_use]
    pub const fn kind(&self) -> &NodeKind {
        &self.kind
    }

    /// Returns the source node attributes.
    #[must_use]
    pub const fn attrs(&self) -> &NodeAttrs {
        &self.attrs
    }

    /// Returns the selected detached content.
    #[must_use]
    pub const fn content(&self) -> &ClipboardNodeContent {
        &self.content
    }
}

/// One selected inline-bearing block in document order.
///
/// This flat projection coexists with [`ClipboardSlice::roots`]: editing
/// planners can use the leaf sequence for boundary merge rules while the
/// structured roots retain list/quote/container semantics for Xiaomu-native
/// transport and reconstruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardBlock {
    kind: NodeKind,
    attrs: NodeAttrs,
    inline: ClipboardInline,
}

impl ClipboardBlock {
    fn from_node(node: &ClipboardNode) -> Option<Self> {
        Some(Self {
            kind: node.kind.clone(),
            attrs: node.attrs.clone(),
            inline: node.content.as_inline()?.clone(),
        })
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

    /// Returns the selected normalized inline content, including marks and
    /// detached atoms.
    #[must_use]
    pub const fn inline(&self) -> &ClipboardInline {
        &self.inline
    }
}

/// Detached clipboard projection of one non-collapsed document selection.
///
/// `roots` preserves the minimal selected fragment tree. `blocks` is its
/// inline-leaf projection in document order. `plain_text` joins those leaves
/// with newline characters for interoperability with external applications;
/// inline atoms contribute their `fallback_text` at the anchored position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardSlice {
    plain_text: String,
    roots: Vec<ClipboardNode>,
    blocks: Vec<ClipboardBlock>,
}

impl ClipboardSlice {
    pub(crate) fn from_roots(roots: Vec<ClipboardNode>) -> Self {
        let mut blocks = Vec::new();
        flatten_blocks(&roots, &mut blocks);
        let plain_text = blocks
            .iter()
            .map(|block| block.inline().plain_text())
            .collect::<Vec<_>>()
            .join("\n");
        Self {
            plain_text,
            roots,
            blocks,
        }
    }

    /// Returns the plain-text fallback, with selected block boundaries as
    /// newline characters.
    #[must_use]
    pub fn plain_text(&self) -> &str {
        &self.plain_text
    }

    /// Returns the minimal structured fragment roots in document order.
    #[must_use]
    pub fn roots(&self) -> &[ClipboardNode] {
        &self.roots
    }

    /// Returns selected inline blocks in document order.
    #[must_use]
    pub fn blocks(&self) -> &[ClipboardBlock] {
        &self.blocks
    }
}

/// Projects selected inline leaves back through the canonical tree, pruning
/// every unselected branch. The document root itself is omitted from the
/// detached fragment.
pub(crate) fn project_roots(
    document: &XiaomuDocument,
    selected: &BTreeMap<NodeId, ClipboardInline>,
) -> Vec<ClipboardNode> {
    let Some(children) = document
        .node(document.root())
        .and_then(|root| root.content().as_children())
    else {
        return Vec::new();
    };
    children
        .iter()
        .filter_map(|child| project_node(document, *child, selected))
        .collect()
}

fn project_node(
    document: &XiaomuDocument,
    id: NodeId,
    selected: &BTreeMap<NodeId, ClipboardInline>,
) -> Option<ClipboardNode> {
    let node = document.node(id)?;
    match node.content() {
        NodeContent::Inline(_) => selected.get(&id).map(|inline| {
            ClipboardNode::new(
                node.kind().clone(),
                node.attrs().clone(),
                ClipboardNodeContent::Inline(inline.clone()),
            )
        }),
        NodeContent::Children(children) => {
            let projected: Vec<_> = children
                .iter()
                .filter_map(|child| project_node(document, *child, selected))
                .collect();
            (!projected.is_empty()).then(|| {
                ClipboardNode::new(
                    node.kind().clone(),
                    node.attrs().clone(),
                    ClipboardNodeContent::Children(projected),
                )
            })
        }
        NodeContent::Atomic | _ => None,
    }
}

/// Validates an untrusted detached fragment by rebuilding it through Core's
/// safe initial builder under a temporary Document root.
///
/// Detached atoms are inserted as fresh canonical inline-atom nodes, so the
/// rebuilt tree passes full document validation exactly like a paste would.
pub(crate) fn validate_roots(roots: &[ClipboardNode]) -> Result<()> {
    let mut builder = NodeStoreBuilder::new();
    let children = roots
        .iter()
        .map(|root| insert_fragment(&mut builder, root))
        .collect::<Result<Vec<_>>>()?;
    let root = builder.insert(
        NodeKind::Document,
        NodeAttrs::empty(),
        NodeContent::children(children),
    )?;
    XiaomuDocument::new(root, builder.finish()).map(|_| ())
}

fn insert_fragment(builder: &mut NodeStoreBuilder, node: &ClipboardNode) -> Result<NodeId> {
    let content = match node.content() {
        ClipboardNodeContent::Inline(inline) => {
            // Insert the detached atoms first so every placement can
            // reference a fresh canonical identity.
            let mut fresh_ids = Vec::with_capacity(inline.atoms().len());
            for atom in inline.atoms() {
                fresh_ids.push(builder.insert(
                    NodeKind::InlineAtom(atom.kind().clone()),
                    atom.attrs().clone(),
                    NodeContent::InlineAtom(atom.content().clone()),
                )?);
            }
            let mut next_id = fresh_ids.iter();
            NodeContent::Inline(inline.to_inline_content(|| {
                next_id
                    .next()
                    .copied()
                    .unwrap_or_else(|| unreachable!("one fresh identity per detached atom"))
            })?)
        }
        ClipboardNodeContent::Children(children) => NodeContent::children(
            children
                .iter()
                .map(|child| insert_fragment(builder, child))
                .collect::<Result<Vec<_>>>()?,
        ),
    };
    builder.insert(node.kind().clone(), node.attrs().clone(), content)
}

fn flatten_blocks(nodes: &[ClipboardNode], out: &mut Vec<ClipboardBlock>) {
    for node in nodes {
        if let Some(block) = ClipboardBlock::from_node(node) {
            out.push(block);
        } else if let Some(children) = node.content().as_children() {
            flatten_blocks(children, out);
        }
    }
}
