//! Marker callbacks project real attrs through the existing render chain.

use super::{DocumentView, markers::marker_for_block};
use crate::{
    editor::{EditorHooks, EditorInstance},
    list_marker::{ListMarkerContext, ListMarkerLabel, ListMarkerLabelProvider},
};
use gpui::{AppContext as _, TestAppContext};
use std::{cell::RefCell, rc::Rc};
use xiaomu_core::{
    document::{
        AttrValue, InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
        XiaomuDocument,
    },
    selection::TextPoint,
    transaction::{Transaction, TransactionOrigin, TransactionStep},
};
use xiaomu_runtime::session::DocumentSelection;

fn attrs(start: i64, kind: &str) -> NodeAttrs {
    NodeAttrs::new(
        [
            ("start".into(), AttrValue::Integer(start)),
            ("type".into(), AttrValue::String(kind.into())),
        ]
        .into(),
    )
    .unwrap()
}

fn fixture() -> (XiaomuDocument, NodeId, [NodeId; 2], [NodeId; 2]) {
    let mut builder = NodeStoreBuilder::new();
    let blocks = std::array::from_fn(|_| {
        builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(InlineContent::empty()),
            )
            .unwrap()
    });
    let items = blocks.map(|block| {
        builder
            .insert(
                NodeKind::ListItem,
                NodeAttrs::empty(),
                NodeContent::children([block]),
            )
            .unwrap()
    });
    let list = builder
        .insert(
            NodeKind::OrderedList,
            attrs(7, "A"),
            NodeContent::children(items),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([list]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        list,
        items,
        blocks,
    )
}

struct DefaultLabels;
impl ListMarkerLabelProvider for DefaultLabels {
    fn label(&self, _: ListMarkerContext<'_>) -> ListMarkerLabel {
        ListMarkerLabel::Default
    }
}

struct AttributeLabels;
impl ListMarkerLabelProvider for AttributeLabels {
    fn label(&self, context: ListMarkerContext<'_>) -> ListMarkerLabel {
        let document = context.document();
        let list = document.node(context.list()).unwrap();
        assert_eq!(list.kind(), &NodeKind::OrderedList);
        assert_eq!(
            document.node(context.item()).unwrap().kind(),
            &NodeKind::ListItem
        );
        assert_eq!(document.parent_of(context.item()), Some(context.list()));
        assert_eq!(
            list.content().as_children().unwrap()[context.index()],
            context.item()
        );
        assert_eq!(context.depth(), 1);
        let Some(AttrValue::Integer(start)) = list.attrs().get("start") else {
            return ListMarkerLabel::Default;
        };
        let Some(AttrValue::String(kind)) = list.attrs().get("type") else {
            return ListMarkerLabel::Default;
        };
        let ordinal = start + i64::try_from(context.index()).unwrap();
        let label = if kind == "A" {
            format!("{}.", char::from(b'A' + u8::try_from(ordinal - 1).unwrap()))
        } else {
            format!("{ordinal}.")
        };
        ListMarkerLabel::Label(label)
    }
}

#[test]
fn default_labels_ignore_attrs_and_override_uses_real_list_context() {
    let (document, _, _, blocks) = fixture();
    let original = document.clone();
    for (index, block) in blocks.into_iter().enumerate() {
        let expected = format!("{}.", index + 1);
        assert_eq!(
            marker_for_block(&document, block, None).unwrap().glyph,
            expected
        );
        assert_eq!(
            marker_for_block(&document, block, Some(&DefaultLabels))
                .unwrap()
                .glyph,
            expected
        );
        assert_eq!(
            marker_for_block(&document, block, Some(&AttributeLabels))
                .unwrap()
                .glyph,
            if index == 0 { "G." } else { "H." }
        );
    }
    assert_eq!(document.store(), original.store());
}

#[test]
fn changed_attrs_are_read_fresh_without_rewriting_canonical_values() {
    let (document, list, _, blocks) = fixture();
    let changed = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeAttrs {
            node: list,
            attrs: attrs(12, "1"),
        })
        .apply(&document)
        .unwrap();
    assert_eq!(
        marker_for_block(&changed, blocks[0], Some(&AttributeLabels))
            .unwrap()
            .glyph,
        "12."
    );
    assert_eq!(
        marker_for_block(&changed, blocks[1], Some(&AttributeLabels))
            .unwrap()
            .glyph,
        "13."
    );
    assert_eq!(
        marker_for_block(&document, blocks[0], Some(&AttributeLabels))
            .unwrap()
            .glyph,
        "G."
    );
    assert_eq!(changed.node(list).unwrap().attrs(), &attrs(12, "1"));
}

// Observation is test instrumentation only; production callbacks must be pure.
struct ObservedLabels(Rc<RefCell<Vec<String>>>);
impl ListMarkerLabelProvider for ObservedLabels {
    fn label(&self, context: ListMarkerContext<'_>) -> ListMarkerLabel {
        let label = AttributeLabels.label(context);
        if let ListMarkerLabel::Label(text) = &label {
            self.0.borrow_mut().push(text.clone());
        }
        label
    }
}

#[gpui::test]
fn instance_builder_and_view_setter_feed_the_actual_render_chain(cx: &mut TestAppContext) {
    let (document, list, _, blocks) = fixture();
    let selection = DocumentSelection::collapsed(TextPoint::at_start_of(blocks[0]));
    let observed = Rc::new(RefCell::new(Vec::new()));
    let custom = EditorInstance::new(document.clone(), selection, EditorHooks::default())
        .unwrap()
        .with_list_marker_provider(Rc::new(ObservedLabels(observed.clone())));
    let default = EditorInstance::new(document, selection, EditorHooks::default()).unwrap();
    let mut plain_view = default.build_view();
    assert!(plain_view.list_marker_provider.is_none());
    plain_view.set_list_marker_provider(Some(Rc::new(DefaultLabels)));
    assert!(plain_view.list_marker_provider.is_some());
    plain_view.set_list_marker_provider(None);
    assert!(plain_view.list_marker_provider.is_none());
    let session = custom.session().clone();
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| custom.build_view()))
            .unwrap()
    });
    handle
        .update(cx, |view: &mut DocumentView, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
            // Build exactly the same tree the normal Render impl uses.
            let root = session.borrow().document().root();
            let _ = view.render_block_tree(root, false, 0, 0, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert!(observed.borrow().iter().any(|label| label == "G."));
    assert!(observed.borrow().iter().any(|label| label == "H."));
    observed.borrow_mut().clear();
    session
        .borrow_mut()
        .apply(&Transaction::new(TransactionOrigin::UserInput).with_step(
            TransactionStep::SetNodeAttrs {
                node: list,
                attrs: attrs(12, "1"),
            },
        ))
        .unwrap();
    handle
        .update(cx, |view, _, cx| {
            let root = session.borrow().document().root();
            let _ = view.render_block_tree(root, false, 0, 0, cx);
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert!(observed.borrow().iter().any(|label| label == "12."));
    assert!(observed.borrow().iter().any(|label| label == "13."));
    assert!(
        observed
            .borrow()
            .iter()
            .all(|label| label != "G." && label != "H.")
    );
    assert_eq!(
        default
            .session()
            .borrow()
            .document()
            .node(list)
            .unwrap()
            .attrs(),
        &attrs(7, "A")
    );
    handle
        .update(cx, |view, _, cx| {
            view.set_list_marker_provider(None);
            let count = observed.borrow().len();
            let root = session.borrow().document().root();
            let _ = view.render_block_tree(root, false, 0, 0, cx);
            assert_eq!(observed.borrow().len(), count);
        })
        .unwrap();
}
