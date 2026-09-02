//! Frontend accessibility projection over the canonical document tree.
//!
//! The pinned crates.io GPUI 0.2.2 artifact does not export its later
//! semantic-role / AccessKit builder API. Xiaomu therefore owns a
//! frontend-neutral projection here and keeps the eventual platform tree as
//! an adapter concern. No GPUI or platform type defines canonical semantics.

use xiaomu_core::document::{NodeContent, NodeId, NodeKind, XiaomuDocument};
use xiaomu_runtime::session::DocumentSelection;

/// Frontend-neutral semantic role used by the accessibility projection.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AccessibilityRole {
    /// Root document container.
    Document,
    /// Ordinary editable paragraph.
    Paragraph,
    /// Editable heading with a semantic outline level.
    Heading {
        /// Canonical heading level in the inclusive range `1..=6`.
        level: u8,
    },
    /// Block quote container.
    BlockQuote,
    /// Ordered or unordered list container.
    List,
    /// One list item container.
    ListItem,
    /// Editable code block.
    CodeBlock,
    /// Horizontal separator.
    Separator,
    /// Image block.
    Image,
    /// Extension-defined or otherwise unclassified block.
    Generic,
    /// Atomic inline extension node, read through its `fallback_text`.
    InlineAtom,
}

/// One canonical node projected for assistive frontend consumers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessibilityNode {
    node_id: NodeId,
    kind: NodeKind,
    role: AccessibilityRole,
    text: Option<String>,
    editable: bool,
    children: Vec<AccessibilityNode>,
}

impl AccessibilityNode {
    /// Returns the stable canonical node identity represented by this entry.
    #[must_use]
    pub const fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Returns the canonical semantic node kind.
    #[must_use]
    pub const fn kind(&self) -> &NodeKind {
        &self.kind
    }

    /// Returns the frontend-neutral accessibility role.
    #[must_use]
    pub const fn role(&self) -> &AccessibilityRole {
        &self.role
    }

    /// Returns canonical editable text for inline-bearing nodes.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// Returns whether this node is an editable inline-bearing block in the
    /// current frontend contract.
    #[must_use]
    pub const fn editable(&self) -> bool {
        self.editable
    }

    /// Returns projected canonical children in document order.
    #[must_use]
    pub fn children(&self) -> &[AccessibilityNode] {
        &self.children
    }
}

/// Current accessibility-readable state of one Xiaomu editor instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessibilityProjection {
    root: AccessibilityNode,
    selection: DocumentSelection,
    focus_owner: Option<NodeId>,
}

impl AccessibilityProjection {
    /// Returns the projected canonical document root.
    #[must_use]
    pub const fn root(&self) -> &AccessibilityNode {
        &self.root
    }

    /// Returns the current canonical document selection.
    #[must_use]
    pub const fn selection(&self) -> DocumentSelection {
        self.selection
    }

    /// Returns the inline node currently holding real frontend keyboard focus.
    ///
    /// This is intentionally independent from selection focus: an inactive
    /// editor may retain a caret while owning no platform focus.
    #[must_use]
    pub const fn focus_owner(&self) -> Option<NodeId> {
        self.focus_owner
    }
}

/// Projects one validated canonical snapshot plus current editing state.
///
/// Returns `None` only when `document` no longer contains its root, which a
/// valid [`XiaomuDocument`] cannot normally reach through the public API.
#[must_use]
pub fn project_accessibility(
    document: &XiaomuDocument,
    selection: DocumentSelection,
    focus_owner: Option<NodeId>,
) -> Option<AccessibilityProjection> {
    Some(AccessibilityProjection {
        root: project_node(document, document.root())?,
        selection,
        focus_owner,
    })
}

fn project_node(document: &XiaomuDocument, id: NodeId) -> Option<AccessibilityNode> {
    let node = document.node(id)?;
    let role = role_for_kind(node.kind());
    let (text, editable, children) = match node.content() {
        NodeContent::Inline(inline) => {
            let text = inline
                .runs()
                .iter()
                .map(|run| run.text().as_str())
                .collect::<String>();
            // Inline atoms are tree edges of the inline content: each one
            // projects as a non-editable child read through its
            // `fallback_text`, so assistive consumers always see atomic
            // content deterministically.
            let children = inline
                .atoms()
                .iter()
                .filter_map(|placement| project_node(document, placement.atom()))
                .collect();
            (Some(text), true, children)
        }
        NodeContent::Children(children) => {
            let projected = children
                .iter()
                .filter_map(|child| project_node(document, *child))
                .collect();
            (None, false, projected)
        }
        NodeContent::InlineAtom(content) => {
            (Some(content.fallback_text().to_owned()), false, Vec::new())
        }
        NodeContent::Atomic => (None, false, Vec::new()),
        _ => (None, false, Vec::new()),
    };
    Some(AccessibilityNode {
        node_id: id,
        kind: node.kind().clone(),
        role,
        text,
        editable,
        children,
    })
}

fn role_for_kind(kind: &NodeKind) -> AccessibilityRole {
    match kind {
        NodeKind::Document => AccessibilityRole::Document,
        NodeKind::Paragraph => AccessibilityRole::Paragraph,
        NodeKind::Heading(level) => AccessibilityRole::Heading {
            level: level.as_u8(),
        },
        NodeKind::Quote => AccessibilityRole::BlockQuote,
        NodeKind::BulletList | NodeKind::OrderedList => AccessibilityRole::List,
        NodeKind::ListItem => AccessibilityRole::ListItem,
        NodeKind::CodeBlock => AccessibilityRole::CodeBlock,
        NodeKind::HorizontalRule => AccessibilityRole::Separator,
        NodeKind::Image => AccessibilityRole::Image,
        NodeKind::Custom(_) => AccessibilityRole::Generic,
        NodeKind::InlineAtom(_) => AccessibilityRole::InlineAtom,
        _ => AccessibilityRole::Generic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xiaomu_core::document::{
        HeadingLevel, InlineContent, MarkSet, NodeAttrs, NodeStoreBuilder, TextRun,
    };
    use xiaomu_core::selection::{CursorAffinity, TextPoint};

    fn leaf(builder: &mut NodeStoreBuilder, kind: NodeKind, text: &str) -> NodeId {
        builder
            .insert(
                kind,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap()
    }

    #[test]
    fn projection_preserves_semantic_tree_text_selection_and_focus() {
        let mut builder = NodeStoreBuilder::new();
        let heading = leaf(
            &mut builder,
            NodeKind::Heading(HeadingLevel::new(2).unwrap()),
            "Title",
        );
        let item_text = leaf(&mut builder, NodeKind::Paragraph, "item");
        let item = builder
            .insert(
                NodeKind::ListItem,
                NodeAttrs::empty(),
                NodeContent::children([item_text]),
            )
            .unwrap();
        let list = builder
            .insert(
                NodeKind::BulletList,
                NodeAttrs::empty(),
                NodeContent::children([item]),
            )
            .unwrap();
        let code = leaf(&mut builder, NodeKind::CodeBlock, "a\nb");
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([heading, list, code]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        let code_inline = document.node(code).unwrap().content().as_inline().unwrap();
        let point = TextPoint::new(
            code,
            code_inline.offset_at(2).unwrap(),
            CursorAffinity::Before,
        );
        let selection = DocumentSelection::collapsed(point);

        let projection = project_accessibility(&document, selection, Some(code)).unwrap();
        assert_eq!(projection.selection(), selection);
        assert_eq!(projection.focus_owner(), Some(code));
        assert_eq!(projection.root().role(), &AccessibilityRole::Document);
        assert_eq!(projection.root().children().len(), 3);
        assert_eq!(
            projection.root().children()[0].role(),
            &AccessibilityRole::Heading { level: 2 }
        );
        assert_eq!(projection.root().children()[0].text(), Some("Title"));
        assert_eq!(
            projection.root().children()[1].role(),
            &AccessibilityRole::List
        );
        assert_eq!(
            projection.root().children()[1].children()[0].role(),
            &AccessibilityRole::ListItem
        );
        assert_eq!(
            projection.root().children()[2].role(),
            &AccessibilityRole::CodeBlock
        );
        assert_eq!(projection.root().children()[2].text(), Some("a\nb"));
        assert!(projection.root().children()[2].editable());
    }

    #[test]
    fn focus_owner_is_not_inferred_from_a_retained_selection() {
        let mut builder = NodeStoreBuilder::new();
        let paragraph = leaf(&mut builder, NodeKind::Paragraph, "text");
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([paragraph]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        let inline = document
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap();
        let selection = DocumentSelection::collapsed(TextPoint::new(
            paragraph,
            inline.offset_at(0).unwrap(),
            CursorAffinity::Before,
        ));

        let inactive = project_accessibility(&document, selection, None).unwrap();
        assert_eq!(inactive.selection(), selection);
        assert_eq!(inactive.focus_owner(), None);
    }
    #[test]
    fn inline_atoms_project_as_fallback_text_children() {
        use xiaomu_core::document::{AtomKind, InlineAtomContent, InlineAtomPlacement};
        use xiaomu_core::selection::InlinePoint;
        use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

        let mut builder = NodeStoreBuilder::new();
        let paragraph = leaf(&mut builder, NodeKind::Paragraph, "ab");
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([paragraph]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        let document = Transaction::new(TransactionOrigin::Extension("atom-test".into()))
            .with_step(TransactionStep::InsertInlineAtom {
                at: InlinePoint::new(
                    paragraph,
                    document
                        .node(paragraph)
                        .unwrap()
                        .content()
                        .as_inline()
                        .unwrap()
                        .offset_at(1)
                        .unwrap(),
                    0,
                    CursorAffinity::Before,
                ),
                kind: AtomKind::new("mention").unwrap(),
                attrs: NodeAttrs::empty(),
                content: InlineAtomContent::new("@Ann").unwrap(),
            })
            .apply(&document)
            .unwrap();

        let inline = document
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .clone();
        let caret = DocumentSelection::collapsed(InlinePoint::from(TextPoint::new(
            paragraph,
            inline.offset_at(0).unwrap(),
            CursorAffinity::Before,
        )));

        let projection = project_accessibility(&document, caret, None).unwrap();
        let paragraph_node = &projection.root().children()[0];
        assert_eq!(paragraph_node.role(), &AccessibilityRole::Paragraph);
        // The paragraph keeps its own text; each atom becomes a child read
        // through its fallback text.
        assert_eq!(paragraph_node.text(), Some("ab"));
        assert_eq!(paragraph_node.children().len(), 1);
        let atom = &paragraph_node.children()[0];
        assert_eq!(atom.role(), &AccessibilityRole::InlineAtom);
        assert_eq!(atom.text(), Some("@Ann"));
        assert!(!atom.editable());
        let _ = InlineAtomPlacement::new;
    }
}
