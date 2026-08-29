//! Detached structured clipboard fragment values.
//!
//! Clipboard nodes deliberately carry no canonical `NodeId`. A fragment is a
//! value tree that preserves selected container semantics while allowing paste
//! to allocate fresh identities in the destination document.

use std::collections::BTreeMap;

use xiaomu_core::document::{
    InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, XiaomuDocument,
};

/// Content of one detached clipboard node.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClipboardNodeContent {
    /// Inline-bearing selected content, including marks.
    Inline(InlineContent),
    /// Selected child fragment nodes in canonical document order.
    Children(Vec<ClipboardNode>),
}

impl ClipboardNodeContent {
    /// Returns inline content when this fragment node is inline-bearing.
    #[must_use]
    pub const fn as_inline(&self) -> Option<&InlineContent> {
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
    inline: InlineContent,
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

    /// Returns the selected normalized inline content, including marks.
    #[must_use]
    pub const fn inline(&self) -> &InlineContent {
        &self.inline
    }
}

/// Detached clipboard projection of one non-collapsed document selection.
///
/// `roots` preserves the minimal selected fragment tree. `blocks` is its
/// inline-leaf projection in document order. `plain_text` joins those leaves
/// with newline characters for interoperability with external applications.
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
            .map(|block| concatenated(block.inline()))
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
    selected: &BTreeMap<NodeId, InlineContent>,
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
    selected: &BTreeMap<NodeId, InlineContent>,
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
pub(crate) fn validate_roots(roots: &[ClipboardNode]) -> xiaomu_core::Result<()> {
    let mut builder = NodeStoreBuilder::new();
    let children = roots
        .iter()
        .map(|root| insert_fragment(&mut builder, root))
        .collect::<xiaomu_core::Result<Vec<_>>>()?;
    let root = builder.insert(
        NodeKind::Document,
        NodeAttrs::empty(),
        NodeContent::children(children),
    )?;
    XiaomuDocument::new(root, builder.finish()).map(|_| ())
}

fn insert_fragment(
    builder: &mut NodeStoreBuilder,
    node: &ClipboardNode,
) -> xiaomu_core::Result<NodeId> {
    let content = match node.content() {
        ClipboardNodeContent::Inline(inline) => NodeContent::Inline(inline.clone()),
        ClipboardNodeContent::Children(children) => NodeContent::children(
            children
                .iter()
                .map(|child| insert_fragment(builder, child))
                .collect::<xiaomu_core::Result<Vec<_>>>()?,
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

fn concatenated(inline: &InlineContent) -> String {
    inline
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}
