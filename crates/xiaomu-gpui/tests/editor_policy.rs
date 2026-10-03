//! Construction-time policy wiring without a native window or event loop.

use gpui::{AppContext as _, TestAppContext};
use xiaomu_core::document::HeadingLevel;
use xiaomu_core::document::{
    InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder,
    XiaomuDocument,
};
use xiaomu_core::selection::TextPoint;
use xiaomu_gpui::document_view::DocumentView;
use xiaomu_gpui::editor::{EditorHooks, EditorInstance};
use xiaomu_runtime::session::{
    DocumentSelection, EditIntent, IntentDisposition, PolicyError, SessionContext, SessionError,
    SessionPolicy,
};

struct Marks(Mark);

impl SessionPolicy for Marks {
    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        Ok(match intent {
            EditIntent::ToggleMark { .. } => {
                IntentDisposition::StoredMarks(Some(MarkSet::new([self.0.clone()]).unwrap()))
            }
            _ => IntentDisposition::Continue,
        })
    }
}

fn fixture() -> (XiaomuDocument, DocumentSelection) {
    let mut builder = NodeStoreBuilder::new();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::empty()),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    (
        document,
        DocumentSelection::collapsed(TextPoint::at_start_of(node)),
    )
}

#[test]
fn separate_editor_policies_and_legacy_constructor_stay_isolated() {
    let (document, selection) = fixture();
    let a = EditorInstance::new_with_policy(
        document.clone(),
        selection,
        EditorHooks::default(),
        Box::new(Marks(Mark::Italic)),
    )
    .unwrap();
    let b = EditorInstance::new_with_policy(
        document.clone(),
        selection,
        EditorHooks::default(),
        Box::new(Marks(Mark::Strike)),
    )
    .unwrap();
    let plain = EditorInstance::new(document, selection, EditorHooks::default()).unwrap();
    for editor in [&a, &b, &plain] {
        editor
            .session()
            .borrow_mut()
            .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
            .unwrap();
    }
    assert_eq!(
        a.session().borrow().stored_marks(),
        Some(&MarkSet::new([Mark::Italic]).unwrap())
    );
    assert_eq!(
        b.session().borrow().stored_marks(),
        Some(&MarkSet::new([Mark::Strike]).unwrap())
    );
    assert_eq!(
        plain.session().borrow().stored_marks(),
        Some(&MarkSet::new([Mark::Bold]).unwrap())
    );
    a.session()
        .borrow_mut()
        .apply_intent(&EditIntent::InsertText { text: "A".into() })
        .unwrap();
    assert_eq!(a.session().borrow().history_depths(), (1, 0));
    assert_eq!(b.session().borrow().history_depths(), (0, 0));
    assert_eq!(plain.session().borrow().history_depths(), (0, 0));
    assert_eq!(b.session().borrow().selection(), selection);
    assert_eq!(plain.session().borrow().selection(), selection);
}

#[test]
fn editor_rejects_policy_invalid_initial_document() {
    struct Reject;
    impl SessionPolicy for Reject {
        fn validate_document(&self, _: &XiaomuDocument) -> Result<(), PolicyError> {
            Err(PolicyError::new("unsupported initial snapshot"))
        }
    }
    let (document, selection) = fixture();
    assert!(matches!(
        EditorInstance::new_with_policy(
            document,
            selection,
            EditorHooks::default(),
            Box::new(Reject)
        ),
        Err(SessionError::Policy(_))
    ));
}

#[gpui::test]
fn external_host_commands_use_the_mounted_editors_policy(cx: &mut TestAppContext) {
    let (document, selection) = fixture();
    let node = selection.as_single_node().unwrap().focus().node_id();
    let editor = EditorInstance::new_with_policy(
        document,
        selection,
        EditorHooks::default(),
        Box::new(Marks(Mark::Italic)),
    )
    .unwrap();
    let session = editor.session().clone();
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    handle
        .update(cx, |view: &mut DocumentView, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
            view.apply_edit_intent(
                EditIntent::TurnInto {
                    kind: NodeKind::Heading(HeadingLevel::new(2).unwrap()),
                },
                window,
                cx,
            );
            view.apply_edit_intent(EditIntent::ToggleMark { mark: Mark::Bold }, window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert_eq!(
        session.borrow().document().node(node).unwrap().kind(),
        &NodeKind::Heading(HeadingLevel::new(2).unwrap())
    );
    assert_eq!(
        session.borrow().stored_marks(),
        Some(&MarkSet::new([Mark::Italic]).unwrap())
    );
    cx.simulate_input(handle.into(), "host");
    let borrowed = session.borrow();
    let runs = borrowed
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs();
    assert_eq!(runs[0].text().as_str(), "host");
    assert_eq!(runs[0].marks(), &MarkSet::new([Mark::Italic]).unwrap());
    assert_eq!(borrowed.history_depths(), (2, 0));
}
