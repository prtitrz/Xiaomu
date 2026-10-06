//! Caret presentation probes literal neighbors, not insertion-mark inheritance.

use super::*;
use crate::block_view::SharedSession;
use crate::font_size::FontSizeContext;
use gpui::{AppContext as _, TestAppContext, font};
use std::cell::{Cell, RefCell};
use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineAtomPlacement, InlineContent, MarkSet, NodeAttrs,
    NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, TextStyleAttributes, TextStyleMark,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession, EditIntent};

fn size_mark(value: StringAttribute) -> Mark {
    Mark::TextStyle(TextStyleMark::from_attributes(
        TextStyleAttributes::default().with_font_size(value),
    ))
}

fn marks(size: &str) -> MarkSet {
    MarkSet::new([size_mark(size.into())]).unwrap()
}

fn fixture() -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let atoms = [
        (
            AtomKind::new("plain").unwrap(),
            InlineAtomContent::new("chip").unwrap(),
        ),
        (
            AtomKind::new("marked").unwrap(),
            InlineAtomContent::new("label")
                .unwrap()
                .with_marks(marks("32px")),
        ),
        (
            AtomKind::hard_break(),
            InlineAtomContent::hard_break().with_marks(marks("24px")),
        ),
    ]
    .into_iter()
    .map(|(kind, content)| {
        builder
            .insert(
                NodeKind::InlineAtom(kind),
                NodeAttrs::empty(),
                NodeContent::InlineAtom(content),
            )
            .unwrap()
    })
    .collect::<Vec<_>>();
    let inline = InlineContent::new([
        TextRun::new("A", marks("48px")).unwrap(),
        TextRun::new("B", marks("12px")).unwrap(),
    ])
    .unwrap();
    let offset = inline.offset_at(1).unwrap();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    inline.runs().iter().cloned(),
                    atoms
                        .into_iter()
                        .map(|id| InlineAtomPlacement::new(id, offset)),
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), node)
}

fn point(document: &XiaomuDocument, node: NodeId, byte: usize, ordinal: usize) -> InlinePoint {
    InlinePoint::new(
        node,
        document
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(byte)
            .unwrap(),
        ordinal,
        CursorAffinity::Before,
    )
}

#[test]
fn unmarked_extension_blocks_the_literal_neighbor_probe() {
    let (document, node) = fixture();
    let at = point(&document, node, 1, 1);
    assert_eq!(adjacent_marks(&document, at, true), Some(&MarkSet::empty()));
    assert_eq!(adjacent_marks(&document, at, false), Some(&marks("32px")));
    // Runtime's intentional transparent-atom inheritance must remain separate.
    assert_eq!(document.inherited_inline_marks(at).unwrap(), marks("48px"));
}

#[test]
fn every_atom_gap_and_text_edge_preserves_exact_neighbor_identity() {
    let (document, node) = fixture();
    for (ordinal, before, after) in [
        (0, marks("48px"), MarkSet::empty()),
        (1, MarkSet::empty(), marks("32px")),
        (2, marks("32px"), marks("24px")),
        (3, marks("24px"), marks("12px")),
    ] {
        let at = point(&document, node, 1, ordinal);
        assert_eq!(adjacent_marks(&document, at, true), Some(&before));
        assert_eq!(adjacent_marks(&document, at, false), Some(&after));
    }
    let start = point(&document, node, 0, 0);
    assert_eq!(adjacent_marks(&document, start, true), None);
    assert_eq!(
        adjacent_marks(&document, start, false),
        Some(&marks("48px"))
    );
    let end = point(&document, node, 2, 0);
    assert_eq!(adjacent_marks(&document, end, true), Some(&marks("12px")));
    assert_eq!(adjacent_marks(&document, end, false), None);
}

fn context(cx: &mut TestAppContext, session: &SharedSession, node: NodeId) -> TextSizeCaretContext {
    cx.update(|cx| {
        let entity = cx.new(|cx| {
            ParagraphView::new(
                session.clone(),
                Rc::new(Cell::new(0)),
                Rc::new(RefCell::new(Vec::new())),
                node,
                cx,
            )
        });
        let view = entity.read(cx);
        let style = TextSizeStyle::new(
            font(".SystemUIFont"),
            FontSizeContext::new(20.0, 20.0, 20.0).unwrap(),
            1.4,
        );
        view.caret_size_context(&style, view.effective_size(&style).unwrap())
    })
}

#[gpui::test]
fn context_separates_effective_inheritance_from_stored_and_adjacent_explicit_sizes(
    cx: &mut TestAppContext,
) {
    let (document, node) = fixture();
    let session = Rc::new(RefCell::new(
        DocumentSession::new(
            document.clone(),
            DocumentSelection::collapsed(point(&document, node, 0, 0)),
        )
        .unwrap(),
    ));
    for (byte, ordinal, effective, before, after) in [
        (0, 0, 48.0, None, Some(48.0)),
        (1, 0, 48.0, Some(48.0), None),
        (1, 1, 48.0, None, Some(32.0)),
        (1, 2, 32.0, Some(32.0), Some(24.0)),
        (1, 3, 24.0, Some(24.0), Some(12.0)),
        (2, 0, 12.0, Some(12.0), None),
    ] {
        session
            .borrow_mut()
            .set_document_selection(DocumentSelection::collapsed(point(
                &document, node, byte, ordinal,
            )))
            .unwrap();
        let actual = context(cx, &session, node);
        assert_eq!(actual.effective_size(), effective);
        assert_eq!(actual.stored_size(), None);
        assert_eq!(actual.before_size(), before);
        assert_eq!(actual.after_size(), after);
    }
    session
        .borrow_mut()
        .set_document_selection(DocumentSelection::collapsed(point(&document, node, 1, 0)))
        .unwrap();
    session
        .borrow_mut()
        .apply_intent(&EditIntent::SetMark {
            mark: size_mark("12px".into()),
        })
        .unwrap();
    let actual = context(cx, &session, node);
    assert_eq!(actual.effective_size(), 12.0);
    assert_eq!(actual.stored_size(), Some(12.0));
    assert_eq!(
        actual.before_size(),
        Some(48.0),
        "small stored size must not suppress the larger adjacent probe"
    );
    assert_eq!(actual.after_size(), None);
    for value in [
        StringAttribute::Missing,
        StringAttribute::Null,
        "".into(),
        "   ".into(),
    ] {
        session
            .borrow_mut()
            .apply_intent(&EditIntent::SetMark {
                mark: size_mark(value),
            })
            .unwrap();
        let actual = context(cx, &session, node);
        assert_eq!(actual.effective_size(), 20.0);
        assert_eq!(actual.stored_size(), None);
        assert_eq!(actual.before_size(), Some(48.0));
        assert_eq!(actual.after_size(), None);
    }
}
