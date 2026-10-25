use super::{DocumentView, child_pool};
use crate::editor::{EditorHooks, EditorInstance};
use gpui::{AppContext as _, Focusable as _, TestAppContext};
use std::{cell::Cell, rc::Rc};
use xiaomu_core::{
    document::{NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, XiaomuDocument},
    selection::InlinePoint,
};
use xiaomu_runtime::session::DocumentSelection;

fn fixture(count: usize) -> (XiaomuDocument, Vec<xiaomu_core::document::NodeId>) {
    let mut builder = NodeStoreBuilder::new();
    let nodes: Vec<_> = (0..count)
        .map(|_| {
            builder
                .insert(
                    NodeKind::Paragraph,
                    NodeAttrs::empty(),
                    NodeContent::empty_inline(),
                )
                .unwrap()
        })
        .collect();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(nodes.clone()),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), nodes)
}

#[test]
fn reorder_insert_delete_reuses_identity_and_drops_only_removed_children() {
    struct Child(Rc<Cell<usize>>);
    impl Drop for Child {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let (_, ids) = fixture(4);
    let removed = Rc::new(Cell::new(0));
    let retained = Rc::new(Cell::new(0));
    let old = vec![
        (ids[0], Child(retained.clone())),
        (ids[1], Child(removed.clone())),
        (ids[2], Child(retained.clone())),
    ];
    let mut created = vec![];
    let result = child_pool::reconcile(old, [ids[2], ids[3], ids[0]], |id| {
        created.push(id);
        Child(retained.clone())
    });
    assert_eq!(created, [ids[3]]);
    assert_eq!(
        result.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        [ids[2], ids[3], ids[0]]
    );
    assert_eq!(removed.get(), 1);
    assert_eq!(retained.get(), 0);
    drop(result);
    assert_eq!(retained.get(), 3);
}

#[gpui::test]
fn repeated_sync_retains_native_input_entities_and_focus_handles(cx: &mut TestAppContext) {
    let (doc, ids) = fixture(4);
    let editor = EditorInstance::new(
        doc,
        DocumentSelection::collapsed(InlinePoint::at_start_of(ids[0])),
        EditorHooks::default(),
    )
    .unwrap();
    let view = cx.new(|_| editor.build_view());
    view.update(cx, |view: &mut DocumentView, cx| {
        view.sync_children(cx);
        let before: Vec<_> = view
            .children
            .iter()
            .map(|(id, child)| (*id, child.entity_id(), child.read(cx).focus_handle(cx)))
            .collect();
        // Simulate an old pool ordered differently from the current document.
        view.children.reverse();
        view.sync_children(cx);
        for ((id, entity, focus), (actual_id, child)) in before.iter().zip(&view.children) {
            assert_eq!(id, actual_id);
            assert_eq!(*entity, child.entity_id());
            assert_eq!(*focus, child.read(cx).focus_handle(cx));
        }
        assert_eq!(view.children.len(), ids.len());
    });
}
