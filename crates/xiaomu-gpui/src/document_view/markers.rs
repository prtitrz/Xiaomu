//! List marker projection for the multi-block view.
//!
//! Markers are a frontend-only visual: they never enter canonical text,
//! `TextRun`s, or selection offsets. Hit-testing and selection paint stay on
//! the paragraph view; this module only decides *what glyph* to draw beside
//! the first inline block of a list item.

use gpui::{div, prelude::*, px};

use xiaomu_core::document::{NodeId, NodeKind, XiaomuDocument};

use crate::block_view::ParagraphView;
use crate::list_marker::{ListMarkerContext, ListMarkerLabel, ListMarkerLabelProvider};

/// Where the focused block sits relative to list structure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ListContext {
    /// The item has a previous sibling item (so it can indent).
    pub has_previous_item: bool,
    /// The item's list is itself inside another list item (so it can
    /// outdent).
    pub nested: bool,
}

/// A projected list marker for one inline block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ListMarker {
    /// Bullet glyph or ordered ordinal such as `"1."`.
    pub glyph: String,
    /// Nesting depth of the enclosing list (1 = top-level list).
    pub depth: usize,
    /// Host labels use the same text renderer but may need a wider column.
    custom_label: bool,
}

/// Classifies `node`'s position when it is a block directly inside a list
/// item; `None` when the node is not in a list at all.
#[must_use]
pub(crate) fn list_context(document: &XiaomuDocument, node: NodeId) -> Option<ListContext> {
    let parent = document.parent_of(node)?;
    if document.node(parent)?.kind() != &NodeKind::ListItem {
        return None;
    }
    let list = document.parent_of(parent)?;
    if !matches!(
        document.node(list)?.kind(),
        NodeKind::BulletList | NodeKind::OrderedList
    ) {
        return None;
    }
    let list_parent = document.parent_of(list)?;
    let nested = document.node(list_parent)?.kind() == &NodeKind::ListItem;
    let siblings = document.node(list)?.content().as_children()?;
    let index = siblings.iter().position(|child| child == &parent)?;
    Some(ListContext {
        has_previous_item: index > 0,
        nested,
    })
}

/// Returns the visual marker for `node` when it is the first inline block of
/// a list item. Nested lists and later paragraphs in the same item get no
/// extra marker of their own.
#[must_use]
pub(crate) fn marker_for_block(
    document: &XiaomuDocument,
    node: NodeId,
    provider: Option<&dyn ListMarkerLabelProvider>,
) -> Option<ListMarker> {
    let parent = document.parent_of(node)?;
    let parent_node = document.node(parent)?;
    if parent_node.kind() != &NodeKind::ListItem {
        return None;
    }
    let item_children = parent_node.content().as_children()?;
    let first_inline = item_children.iter().copied().find(|id| {
        document
            .node(*id)
            .is_some_and(|child| child.content().as_inline().is_some())
    })?;
    if first_inline != node {
        return None;
    }

    let list_id = document.parent_of(parent)?;
    let list_node = document.node(list_id)?;
    let siblings = list_node.content().as_children()?;
    let index = siblings.iter().position(|child| *child == parent)?;
    let depth = list_nesting_depth(document, list_id);
    let glyph = match list_node.kind() {
        NodeKind::BulletList => bullet_glyph(depth).to_owned(),
        NodeKind::OrderedList => format!("{}.", index + 1),
        _ => return None,
    };
    let label = provider.map(|provider| {
        provider.label(ListMarkerContext {
            document,
            list: list_id,
            item: parent,
            index,
            depth,
        })
    });
    let (glyph, custom_label) = match label {
        Some(ListMarkerLabel::Label(label)) => (label, true),
        _ => (glyph, false),
    };
    Some(ListMarker {
        glyph,
        depth,
        custom_label,
    })
}

fn list_nesting_depth(document: &XiaomuDocument, list_id: NodeId) -> usize {
    let mut depth = 1;
    let mut current = list_id;
    while let Some(parent) = document.parent_of(current) {
        if document
            .node(parent)
            .is_some_and(|node| matches!(node.kind(), NodeKind::BulletList | NodeKind::OrderedList))
        {
            depth += 1;
        }
        current = parent;
    }
    depth
}

fn bullet_glyph(depth: usize) -> &'static str {
    match (depth.saturating_sub(1)) % 3 {
        0 => "\u{2022}",
        1 => "\u{25e6}",
        _ => "\u{25aa}",
    }
}

/// Width reserved for a marker column, matching one list indent step.
pub(crate) const MARKER_COLUMN: f32 = 24.0;

/// Wraps one block view with kind-driven visual styling and an optional
/// list marker drawn beside the text, never inside it.
pub(crate) fn style_block(
    view: gpui::Entity<ParagraphView>,
    kind: &NodeKind,
    in_quote: bool,
    list_depth: usize,
    marker: Option<&ListMarker>,
    index: usize,
) -> gpui::Stateful<gpui::Div> {
    let nest = MARKER_COLUMN * list_depth.saturating_sub(1) as f32;
    let text_indent = if list_depth > 0 && marker.is_none() {
        nest + MARKER_COLUMN
    } else {
        nest
    };

    let mut row = div().id(index).w_full().flex().flex_row().items_start();
    if text_indent > 0.0 {
        row = row.ml(px(text_indent));
    }
    if let NodeKind::Heading(level) = kind {
        let scale = match level.as_u8() {
            1 => 1.6,
            2 => 1.35,
            _ => 1.15,
        };
        row = row
            .text_size(px(20.0 * scale))
            .font_weight(gpui::FontWeight::BOLD);
    }
    if in_quote {
        row = row.text_color(gpui::rgba(0x444444ff));
    }
    if let Some(marker) = marker {
        let label = div().flex_shrink_0().child(marker.glyph.clone());
        let label = if marker.custom_label {
            label.min_w(px(MARKER_COLUMN)).whitespace_nowrap()
        } else {
            label.w(px(MARKER_COLUMN))
        };
        row = row.child(label);
    }
    row.child(view)
}

#[cfg(test)]
mod tests {
    use super::*;
    use xiaomu_core::document::{
        InlineContent, MarkSet, NodeAttrs, NodeContent, NodeStoreBuilder, TextRun,
    };

    use crate::document_view::navigation::text_blocks;

    fn paragraph(text: &str, builder: &mut NodeStoreBuilder) -> NodeId {
        builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap()
    }

    fn item(block: NodeId, builder: &mut NodeStoreBuilder) -> NodeId {
        builder
            .insert(
                NodeKind::ListItem,
                NodeAttrs::empty(),
                NodeContent::children([block]),
            )
            .unwrap()
    }

    fn sample_lists() -> (XiaomuDocument, NodeId, NodeId, NodeId, NodeId, NodeId) {
        // Document > ul > [li > p(first), li > [p(second), ul > li > p(nested)]]
        //            > ol > [li > p(one), li > p(two)]
        let mut builder = NodeStoreBuilder::new();
        let first = paragraph("first", &mut builder);
        let second = paragraph("second", &mut builder);
        let nested = paragraph("nested", &mut builder);
        let nested_item = item(nested, &mut builder);
        let nested_list = builder
            .insert(
                NodeKind::BulletList,
                NodeAttrs::empty(),
                NodeContent::children([nested_item]),
            )
            .unwrap();
        let item_a = item(first, &mut builder);
        let item_b = builder
            .insert(
                NodeKind::ListItem,
                NodeAttrs::empty(),
                NodeContent::children([second, nested_list]),
            )
            .unwrap();
        let ul = builder
            .insert(
                NodeKind::BulletList,
                NodeAttrs::empty(),
                NodeContent::children([item_a, item_b]),
            )
            .unwrap();
        let one = paragraph("one", &mut builder);
        let two = paragraph("two", &mut builder);
        let item_one = item(one, &mut builder);
        let item_two = item(two, &mut builder);
        let ol = builder
            .insert(
                NodeKind::OrderedList,
                NodeAttrs::empty(),
                NodeContent::children([item_one, item_two]),
            )
            .unwrap();
        let extra = paragraph("plain", &mut builder);
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([ul, ol, extra]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        (document, first, second, nested, one, two)
    }

    #[test]
    fn bullet_and_ordered_markers_are_visual_only() {
        let (document, first, second, nested, one, two) = sample_lists();

        assert_eq!(
            marker_for_block(&document, first, None).map(|m| m.glyph),
            Some("\u{2022}".to_owned())
        );
        assert_eq!(
            marker_for_block(&document, second, None).map(|m| m.glyph),
            Some("\u{2022}".to_owned())
        );
        assert_eq!(
            marker_for_block(&document, nested, None).map(|m| (m.glyph, m.depth)),
            Some(("\u{25e6}".to_owned(), 2))
        );
        assert_eq!(
            marker_for_block(&document, one, None).map(|m| m.glyph),
            Some("1.".to_owned())
        );
        assert_eq!(
            marker_for_block(&document, two, None).map(|m| m.glyph),
            Some("2.".to_owned())
        );

        // Canonical concatenated text is unchanged: no glyph in the runs.
        let blocks = text_blocks(&document);
        let texts: Vec<String> = blocks.iter().map(|block| block.text()).collect();
        assert_eq!(texts, ["first", "second", "nested", "one", "two", "plain"]);
        for block in &blocks {
            assert!(!block.text().contains('\u{2022}'));
            assert!(!block.text().contains('\u{25e6}'));
            assert!(!block.text().ends_with('.'));
            let offset = block.inline.offset_at(block.text().len());
            assert!(offset.is_ok(), "canonical offsets ignore markers");
        }
    }

    #[test]
    fn later_paragraph_in_an_item_has_no_marker() {
        let mut builder = NodeStoreBuilder::new();
        let a = paragraph("a", &mut builder);
        let b = paragraph("b", &mut builder);
        let li = builder
            .insert(
                NodeKind::ListItem,
                NodeAttrs::empty(),
                NodeContent::children([a, b]),
            )
            .unwrap();
        let ul = builder
            .insert(
                NodeKind::BulletList,
                NodeAttrs::empty(),
                NodeContent::children([li]),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([ul]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();

        assert!(marker_for_block(&document, a, None).is_some());
        assert_eq!(marker_for_block(&document, b, None), None);
        assert_eq!(text_blocks(&document)[1].text(), "b");
    }

    #[test]
    fn plain_paragraphs_have_no_marker() {
        let mut builder = NodeStoreBuilder::new();
        let p = paragraph("plain", &mut builder);
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([p]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        assert_eq!(marker_for_block(&document, p, None), None);
    }

    #[test]
    fn list_context_classifies_items_and_outsiders() {
        let (document, first, second, nested, _, _) = sample_lists();
        let extra = text_blocks(&document).last().unwrap().node;
        assert_eq!(list_context(&document, extra), None);
        assert_eq!(
            list_context(&document, first),
            Some(ListContext {
                has_previous_item: false,
                nested: false
            })
        );
        assert_eq!(
            list_context(&document, second),
            Some(ListContext {
                has_previous_item: true,
                nested: false
            })
        );
        assert_eq!(
            list_context(&document, nested),
            Some(ListContext {
                has_previous_item: false,
                nested: true
            })
        );
    }

    #[gpui::test]
    fn code_wrapper_keeps_the_list_marker_outside_and_builder_is_instance_local(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::code_presentation::CodeBlockPresentation;
        use crate::editor::{EditorHooks, EditorInstance};
        use gpui::AppContext as _;
        use xiaomu_core::selection::InlinePoint;
        use xiaomu_runtime::session::DocumentSelection;

        let mut builder = NodeStoreBuilder::new();
        let code = builder
            .insert(
                NodeKind::CodeBlock,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("a\tb\r\nc", MarkSet::empty()).unwrap()])
                        .unwrap(),
                ),
            )
            .unwrap();
        let list_item = item(code, &mut builder);
        let list = builder
            .insert(
                NodeKind::BulletList,
                NodeAttrs::empty(),
                NodeContent::children([list_item]),
            )
            .unwrap();
        let plain = paragraph("plain", &mut builder);
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([list, plain]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        let selection = DocumentSelection::collapsed(InlinePoint::at_start_of(code));
        let default =
            EditorInstance::new(document.clone(), selection, EditorHooks::default()).unwrap();
        let styled = EditorInstance::new(document.clone(), selection, EditorHooks::default())
            .unwrap()
            .with_code_block_presentation(CodeBlockPresentation::default());
        for (instance, enabled) in [(&default, false), (&styled, true)] {
            let handle = cx.update(|cx| {
                cx.open_window(Default::default(), |_, cx| {
                    cx.new(|_| instance.build_view())
                })
                .unwrap()
            });
            cx.background_executor.run_until_parked();
            handle
                .update(cx, |view, _, cx| {
                    let code_bounds = view
                        .children
                        .iter()
                        .find(|(id, _)| *id == code)
                        .unwrap()
                        .1
                        .read(cx)
                        .last_bounds
                        .unwrap();
                    let plain_bounds = view
                        .children
                        .iter()
                        .find(|(id, _)| *id == plain)
                        .unwrap()
                        .1
                        .read(cx)
                        .last_bounds
                        .unwrap();
                    // Document inset + independent 24px marker column, then
                    // only the code child gets its 16px padding + 1px border.
                    assert_eq!(plain_bounds.left(), px(16.0));
                    assert_eq!(
                        code_bounds.left() - plain_bounds.left(),
                        px(MARKER_COLUMN + if enabled { 17.0 } else { 0.0 })
                    );
                    assert_eq!(
                        code_bounds.top(),
                        px(16.0 + if enabled { 15.0 } else { 0.0 })
                    );
                    assert_eq!(marker_for_block(&document, code, None).unwrap().glyph, "•");
                    view.set_code_block_presentation(None);
                    cx.notify();
                })
                .unwrap();
            cx.background_executor.run_until_parked();
            handle
                .update(cx, |view, _, cx| {
                    let bounds = view
                        .children
                        .iter()
                        .find(|(id, _)| *id == code)
                        .unwrap()
                        .1
                        .read(cx)
                        .last_bounds
                        .unwrap();
                    assert_eq!(bounds.left(), px(16.0 + MARKER_COLUMN));
                    assert_eq!(bounds.top(), px(16.0));
                })
                .unwrap();
            assert_eq!(
                instance.session().borrow().document().store(),
                document.store()
            );
            assert_eq!(instance.session().borrow().selection(), selection);
            assert_eq!(instance.session().borrow().history_depths(), (0, 0));
        }
    }
}
