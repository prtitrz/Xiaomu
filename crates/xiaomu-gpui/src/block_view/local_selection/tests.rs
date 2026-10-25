use super::*;
use gpui::{AppContext as _, TestAppContext};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use xiaomu_core::{
    document::*,
    selection::{CursorAffinity, InlinePoint},
    text::TextOffset,
};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession};

#[gpui::test]
fn local_caret_backward_text_and_atom_seams_do_not_collect_document_order(cx: &mut TestAppContext) {
    let mut builder = NodeStoreBuilder::new();
    let atom = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::new("mention").unwrap()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::new("@A").unwrap()),
        )
        .unwrap();
    let a = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new("中文", MarkSet::empty()).unwrap()],
                    [InlineAtomPlacement::new(atom, TextOffset::ZERO)],
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let b = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([a, b]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let start = InlinePoint::at_start_of(a);
    let after_atom = InlinePoint::new(a, TextOffset::ZERO, 1, CursorAffinity::Before);
    let end = InlinePoint::new(
        a,
        document
            .node(a)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(6)
            .unwrap(),
        0,
        CursorAffinity::Before,
    );
    for selection in [
        DocumentSelection::collapsed(start),
        DocumentSelection::collapsed(after_atom),
        DocumentSelection::collapsed(InlinePoint::at_start_of(b)),
        DocumentSelection::new(after_atom, start),
        DocumentSelection::new(end, after_atom),
    ] {
        let session = Rc::new(RefCell::new(
            DocumentSession::new(document.clone(), selection).unwrap(),
        ));
        for node in [a, b] {
            let view = cx.new(|cx| {
                ParagraphView::new(
                    session.clone(),
                    Rc::new(Cell::new(0)),
                    Rc::new(RefCell::new(Default::default())),
                    node,
                    cx,
                )
            });
            view.update(cx, |view, _| {
                let expected = view.projected_display_selection(&[a, b]);
                assert_eq!(
                    view.projected_selection_with_order(|| panic!(
                        "local selection scanned the document"
                    )),
                    expected
                );
            });
        }
    }
}

#[gpui::test]
fn cross_block_and_all_selection_still_use_document_order(cx: &mut TestAppContext) {
    let mut builder = NodeStoreBuilder::new();
    let ids: Vec<_> = (0..2)
        .map(|_| {
            builder
                .insert(
                    NodeKind::Paragraph,
                    NodeAttrs::empty(),
                    NodeContent::Inline(
                        InlineContent::new([TextRun::new("中文", MarkSet::empty()).unwrap()])
                            .unwrap(),
                    ),
                )
                .unwrap()
        })
        .collect();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(ids.clone()),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    for selection in [
        DocumentSelection::new(
            InlinePoint::at_start_of(ids[1]),
            InlinePoint::at_start_of(ids[0]),
        ),
        DocumentSelection::all(&document),
    ] {
        let session = Rc::new(RefCell::new(
            DocumentSession::new(document.clone(), selection).unwrap(),
        ));
        let view = cx.new(|cx| {
            ParagraphView::new(
                session,
                Rc::new(Cell::new(0)),
                Rc::new(RefCell::new(Default::default())),
                ids[0],
                cx,
            )
        });
        view.update(cx, |view, _| {
            let called = Cell::new(false);
            assert_eq!(
                view.projected_selection_with_order(|| {
                    called.set(true);
                    ids.clone()
                }),
                view.projected_display_selection(&ids)
            );
            assert!(called.get());
        });
    }
}
