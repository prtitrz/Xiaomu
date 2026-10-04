//! Canonical line-break command construction.
//!
//! ADR 0004 makes LF the canonical inline line-break scalar. The public
//! constructor returns a distinct logical intent. Default dispatch still uses
//! isolated canonical LF, while product policies can distinguish clipboard LF.

use crate::session::EditIntent;

impl EditIntent {
    /// Builds an isolated canonical line-break command.
    ///
    /// In ordinary rich-text inline nodes the LF is a HardBreak; in a
    /// `CodeBlock` it is a code newline. The command owns one history entry
    /// and inherits Runtime StoredMarks exactly like other isolated text
    /// replacement. Soft-wrap never uses this command.
    #[must_use]
    pub fn insert_line_break() -> Self {
        Self::InsertLineBreak
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{
        DocumentSelection, DocumentSession, IntentDisposition, PolicyError, SessionContext,
        SessionPolicy,
    };
    use xiaomu_core::{
        document::{
            InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder,
            TextRun, XiaomuDocument,
        },
        selection::{CursorAffinity, TextPoint},
        text::TextOffset,
    };

    fn fixture() -> (XiaomuDocument, DocumentSelection) {
        let mut builder = NodeStoreBuilder::new();
        let node = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("ab", MarkSet::empty()).unwrap()]).unwrap(),
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
        (
            XiaomuDocument::new(root, builder.finish()).unwrap(),
            DocumentSelection::collapsed(TextPoint::new(
                node,
                TextOffset::ZERO,
                CursorAffinity::Before,
            )),
        )
    }

    #[test]
    fn default_logical_command_keeps_previous_lf_marks_selection_and_history() {
        let (doc, selection) = fixture();
        let mut logical = DocumentSession::new(doc.clone(), selection).unwrap();
        let mut old = DocumentSession::new(doc, selection).unwrap();
        for session in [&mut logical, &mut old] {
            session
                .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
                .unwrap();
        }
        logical
            .apply_intent(&EditIntent::insert_line_break())
            .unwrap();
        old.apply_intent(&EditIntent::PasteText { text: "\n".into() })
            .unwrap();
        assert_eq!(logical.document().store(), old.document().store());
        assert_eq!(logical.selection(), old.selection());
        assert_eq!(logical.stored_marks(), old.stored_marks());
        assert_eq!(logical.history_depths(), old.history_depths());
        logical.undo().unwrap();
        old.undo().unwrap();
        assert_eq!(logical.document().store(), old.document().store());
        logical.redo().unwrap();
        old.redo().unwrap();
        assert_eq!(logical.document().store(), old.document().store());
    }

    struct RefuseLogicalBreak;
    impl SessionPolicy for RefuseLogicalBreak {
        fn prepare_intent(
            &self,
            _: SessionContext<'_>,
            intent: &EditIntent,
        ) -> Result<IntentDisposition, PolicyError> {
            if matches!(intent, EditIntent::InsertLineBreak) {
                Err(PolicyError::new("logical break refused"))
            } else {
                Ok(IntentDisposition::Continue)
            }
        }
    }
    #[test]
    fn policy_distinguishes_keyboard_break_from_identical_clipboard_bytes() {
        let (doc, selection) = fixture();
        let mut session =
            DocumentSession::new_with_policy(doc, selection, Box::new(RefuseLogicalBreak)).unwrap();
        session
            .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
            .unwrap();
        let before = session.document().clone();
        let marks = session.stored_marks().cloned();
        assert!(
            session
                .apply_intent(&EditIntent::insert_line_break())
                .is_err()
        );
        assert_eq!(session.document().store(), before.store());
        assert_eq!(session.document().revision(), before.revision());
        assert_eq!(session.selection(), selection);
        assert_eq!(session.stored_marks(), marks.as_ref());
        assert_eq!(session.history_depths(), (0, 0));
        session
            .apply_intent(&EditIntent::PasteText { text: "\n".into() })
            .unwrap();
        assert_eq!(session.history_depths(), (1, 0));
    }

    #[test]
    fn constructor_keeps_logical_line_break_distinct_from_clipboard_text() {
        assert_eq!(EditIntent::insert_line_break(), EditIntent::InsertLineBreak);
    }
}
