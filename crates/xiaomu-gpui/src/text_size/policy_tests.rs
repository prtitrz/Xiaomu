//! A host adapter composes the native capability with Runtime's final gate.

use super::*;
use gpui::{TestAppContext, font};
use std::cell::Cell;
use xiaomu_core::document::{
    InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, TextRun,
    TextStyleAttributes, TextStyleMark,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_core::text::TextRange;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentSelection, DocumentSession, EditIntent, PolicyError,
    SessionError, SessionPolicy,
};

struct Style;
impl TextSizeStyleProvider for Style {
    fn style(&self, _: &XiaomuDocument, _: &Node) -> TextSizeStyle {
        TextSizeStyle::new(
            font(".SystemUIFont"),
            FontSizeContext::new(18.0, 16.0, 16.0).unwrap(),
            1.5,
        )
    }
}

struct Policy(TextSizeCapability);
impl SessionPolicy for Policy {
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        self.0
            .validate_document(document, &InlineAtomRendererRegistry::new())
            .map_err(|error| PolicyError::new(error.to_string()))
    }
}

fn policy(system: &Arc<WindowTextSystem>) -> Box<dyn SessionPolicy> {
    Box::new(Policy(TextSizeCapability::new(
        system.clone(),
        Rc::new(Style),
    )))
}

fn mark(size: &str) -> Mark {
    Mark::TextStyle(TextStyleMark::from_attributes(
        TextStyleAttributes::default().with_font_size(size.into()),
    ))
}

fn fixture(parts: &[(&str, MarkSet)]) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new(
                    parts
                        .iter()
                        .map(|(text, marks)| TextRun::new(*text, marks.clone()).unwrap()),
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

fn point(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextPoint {
    TextPoint::new(
        node,
        document
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(raw)
            .unwrap(),
        CursorAffinity::Before,
    )
}

struct Listener(Rc<Cell<usize>>);
impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        self.0.set(self.0.get() + 1);
    }
    fn selection_changed(&mut self, _: DocumentSelection) {
        self.0.set(self.0.get() + 1);
    }
}

struct Snapshot {
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
    history: (usize, usize),
    notifications: usize,
}
impl Snapshot {
    fn capture(session: &DocumentSession, notifications: &Cell<usize>) -> Self {
        Self {
            document: session.document().clone(),
            selection: session.selection(),
            marks: session.stored_marks().cloned(),
            history: session.history_depths(),
            notifications: notifications.get(),
        }
    }
    fn assert_unchanged(&self, session: &DocumentSession, notifications: &Cell<usize>) {
        assert_eq!(session.document().store(), self.document.store());
        assert_eq!(session.document().revision(), self.document.revision());
        assert_eq!(session.document().root(), self.document.root());
        assert_eq!(session.selection(), self.selection);
        assert_eq!(session.stored_marks(), self.marks.as_ref());
        assert_eq!(session.history_depths(), self.history);
        assert_eq!(notifications.get(), self.notifications);
    }
}

fn listen(session: &mut DocumentSession) -> Rc<Cell<usize>> {
    let counter = Rc::new(Cell::new(0));
    session.add_listener(Box::new(Listener(counter.clone())));
    counter
}

#[gpui::test]
fn unsupported_initial_snapshot_is_rejected_only_for_opted_in_policy(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let (document, node) = fixture(&[
            ("ع", MarkSet::new([mark("24px")]).unwrap()),
            ("a", MarkSet::empty()),
        ]);
        let selection = DocumentSelection::collapsed(point(&document, node, 0));
        assert!(matches!(
            DocumentSession::new_with_policy(
                document.clone(),
                selection,
                policy(window.text_system())
            ),
            Err(SessionError::Policy(_))
        ));
        assert!(DocumentSession::new(document, selection).is_ok());
    });
}

#[gpui::test]
fn unsafe_insert_restores_marks_history_group_selection_and_listener(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let (document, node) = fixture(&[
            ("A", MarkSet::new([mark("24px")]).unwrap()),
            ("B", MarkSet::empty()),
        ]);
        let original = document.clone();
        let selection = DocumentSelection::collapsed(point(&document, node, 2));
        let mut session =
            DocumentSession::new_with_policy(document, selection, policy(window.text_system()))
                .unwrap();
        let counter = listen(&mut session);
        session
            .apply_intent(&EditIntent::SetMark { mark: Mark::Bold })
            .unwrap();
        session
            .apply_intent(&EditIntent::InsertText { text: "x".into() })
            .unwrap();
        let snapshot = Snapshot::capture(&session, &counter);
        assert!(matches!(
            session.apply_intent(&EditIntent::InsertText { text: "ع".into() }),
            Err(SessionError::Policy(_))
        ));
        snapshot.assert_unchanged(&session, &counter);
        session
            .apply_intent(&EditIntent::InsertText { text: "y".into() })
            .unwrap();
        assert_eq!(
            session.history_depths(),
            snapshot.history,
            "failed input must not end the typing group"
        );
        session.undo().unwrap();
        assert_eq!(session.document().store(), original.store());
        session.redo().unwrap();
        assert_eq!(session.history_depths(), (1, 0));
    });
}

#[gpui::test]
fn inserted_combining_mark_cannot_create_an_unsafe_size_seam(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let (document, node) = fixture(&[
            ("e", MarkSet::new([mark("24px")]).unwrap()),
            ("x", MarkSet::empty()),
        ]);
        let selection = DocumentSelection::collapsed(point(&document, node, 1));
        let mut session =
            DocumentSession::new_with_policy(document, selection, policy(window.text_system()))
                .unwrap();
        session
            .apply_intent(&EditIntent::SetMark { mark: mark("18px") })
            .unwrap();
        let counter = listen(&mut session);
        let snapshot = Snapshot::capture(&session, &counter);
        assert!(matches!(
            session.apply_intent(&EditIntent::InsertText {
                text: "\u{301}".into()
            }),
            Err(SessionError::Policy(_))
        ));
        snapshot.assert_unchanged(&session, &counter);
    });
}

#[gpui::test]
fn raw_and_typed_mark_transactions_share_the_final_atomic_admission_gate(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        for (text, boundary) in [("عربي", "ع".len()), ("e\u{301}z", 1)] {
            let (document, node) = fixture(&[(text, MarkSet::empty())]);
            let start = point(&document, node, 0);
            let end = point(&document, node, boundary);
            let selection = DocumentSelection::new(start, end);
            let mut session =
                DocumentSession::new_with_policy(document, selection, policy(window.text_system()))
                    .unwrap();
            let counter = listen(&mut session);
            let snapshot = Snapshot::capture(&session, &counter);
            let transaction = Transaction::new(TransactionOrigin::UserInput).with_step(
                TransactionStep::AddMark {
                    node,
                    range: TextRange::new(start.offset(), end.offset()).unwrap(),
                    mark: mark("24px"),
                },
            );
            assert!(matches!(
                session.apply(&transaction),
                Err(SessionError::Policy(_))
            ));
            snapshot.assert_unchanged(&session, &counter);
            assert!(matches!(
                session.apply_intent(&EditIntent::SetMark { mark: mark("24px") }),
                Err(SessionError::Policy(_))
            ));
            snapshot.assert_unchanged(&session, &counter);
        }
    });
}
