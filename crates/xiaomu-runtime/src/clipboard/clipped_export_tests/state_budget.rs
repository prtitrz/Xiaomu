//! Export never mutates session state or launders unadmitted source payloads.

use super::*;
use crate::session::{DocumentChangeListener, EditIntent};
use std::{cell::Cell, rc::Rc};
use xiaomu_core::document::{Mark, MarkSet};

struct Listener(Rc<Cell<usize>>);

impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        self.0.set(self.0.get() + 1);
    }

    fn selection_changed(&mut self, _: DocumentSelection) {
        self.0.set(self.0.get() + 1);
    }
}

fn checked_export(
    session: &mut DocumentSession,
    purpose: ClipboardExportPurpose,
) -> Result<Option<ClipboardSlice>, SessionError> {
    let before = session.document().clone();
    let selection = session.selection();
    let history = session.history_depths();
    let marks = session.stored_marks().cloned();
    let events = Rc::new(Cell::new(0));
    session.add_listener(Box::new(Listener(events.clone())));
    let result = session.clipboard_slice_for(purpose);
    assert_eq!(session.document().store(), before.store());
    assert_eq!(session.document().revision(), before.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), history);
    assert_eq!(session.stored_marks(), marks.as_ref());
    assert_eq!(
        events.get(),
        0,
        "export must not publish document or selection events"
    );
    result
}

#[test]
fn repeated_clipped_copy_preserves_document_selection_redo_and_listeners() {
    let f = fixture(3, 4, vec![CellSpec::new(1, 0, 1, 3).rich_header()]);
    let mut s = session(&f, Some(spec()));
    s.apply_intent(&EditIntent::InsertText {
        text: "pending".into(),
    })
    .unwrap();
    let redo_document = s.document().clone();
    s.undo().unwrap();
    s.set_cell_range_selection(f.cells[&(0, 1)], f.cells[&(2, 2)])
        .unwrap();
    assert_eq!(s.history_depths(), (0, 1));
    let first = checked_export(&mut s, ClipboardExportPurpose::Copy)
        .unwrap()
        .unwrap();
    let second = checked_export(&mut s, ClipboardExportPurpose::Copy)
        .unwrap()
        .unwrap();
    assert_eq!(first, second);
    roundtrip(&first);
    s.redo().unwrap();
    assert_eq!(s.document().store(), redo_document.store());
    assert_eq!(s.history_depths(), (1, 0));
}

#[test]
fn clipped_policy_copy_preserves_pending_marks_typing_group_and_undo_redo() {
    let f = fixture(2, 2, vec![]);
    let mut s = session(&f, Some(spec()));
    s.apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    s.apply_intent(&EditIntent::InsertText { text: "a".into() })
        .unwrap();
    assert_eq!(s.stored_marks(), Some(&MarkSet::new([Mark::Bold]).unwrap()));
    assert_eq!(
        checked_export(&mut s, ClipboardExportPurpose::Copy).unwrap(),
        None
    );
    s.apply_intent(&EditIntent::InsertText { text: "b".into() })
        .unwrap();
    assert_eq!(
        s.history_depths(),
        (1, 0),
        "copy must not split a typing group"
    );
    let redo_document = s.document().clone();
    s.undo().unwrap();
    assert_eq!(s.document().store(), f.document.store());
    s.apply_intent(&EditIntent::ToggleMark { mark: Mark::Italic })
        .unwrap();
    assert_eq!(
        s.stored_marks(),
        Some(&MarkSet::new([Mark::Italic]).unwrap())
    );
    assert_eq!(
        checked_export(&mut s, ClipboardExportPurpose::Copy).unwrap(),
        None
    );
    assert_eq!(s.history_depths(), (0, 1));
    s.redo().unwrap();
    assert_eq!(s.document().store(), redo_document.store());
}

fn top_crossings(count: usize, intro: &str) -> Fixture {
    fixture_with_intro(
        2,
        count + 2,
        (1..=count)
            .map(|column| CellSpec::new(0, column, 2, 1))
            .collect(),
        intro,
    )
}

fn select_top_crossings(
    f: &Fixture,
    count: usize,
    options: ClipboardExportSpec,
) -> DocumentSession {
    let mut s = session(f, Some(options));
    s.set_cell_range_selection(f.cells[&(1, 0)], f.cells[&(1, count + 1)])
        .unwrap();
    s
}

#[test]
fn oversized_and_deep_defaults_reject_only_when_a_paragraph_uses_them() {
    let mut nested = AttrValue::Null;
    for _ in 0..40 {
        nested = AttrValue::List(vec![nested]);
    }
    for raw in [
        attrs(&[("oversized", AttrValue::String("x".repeat(3 * 1024 * 1024)))]),
        attrs(&[("deep", nested)]),
    ] {
        let f = top_crossings(1, "intro");
        let options = spec_with(raw);
        let mut s = select_top_crossings(&f, 1, options.clone());
        assert!(matches!(
            checked_export(&mut s, ClipboardExportPurpose::Copy),
            Err(SessionError::Policy(_))
        ));
        // Whole-table and right/bottom-only clipping use no supplied default.
        let unused = copied(&f, (0, 0), (1, 2), Some(options.clone())).unwrap();
        roundtrip(&unused);
        let right = fixture(3, 4, vec![CellSpec::new(1, 1, 1, 3)]);
        roundtrip(&copied(&right, (0, 1), (2, 2), Some(options)).unwrap());
    }
}

#[test]
fn default_attribute_cost_is_multiplied_by_the_number_of_cleared_origins() {
    let options = spec_with(attrs(&[(
        "fill",
        AttrValue::String("x".repeat(1024 * 1024)),
    )]));
    let single = top_crossings(1, "intro");
    let mut s = select_top_crossings(&single, 1, options.clone());
    let one = checked_export(&mut s, ClipboardExportPurpose::Copy)
        .unwrap()
        .unwrap();
    assert_cleared(
        find_cell(&one, "0:1"),
        &attrs(&[("fill", AttrValue::String("x".repeat(1024 * 1024)))]),
    );
    roundtrip(&one);
    let repeated = top_crossings(3, "intro");
    let mut s = select_top_crossings(&repeated, 3, options);
    assert!(matches!(
        checked_export(&mut s, ClipboardExportPurpose::Copy),
        Err(SessionError::Policy(_))
    ));
}

#[test]
fn clipped_copy_checks_the_entire_source_before_discarding_unselected_content() {
    let f = top_crossings(1, &"x".repeat(3 * 1024 * 1024));
    let mut s = select_top_crossings(&f, 1, spec());
    assert!(matches!(
        checked_export(&mut s, ClipboardExportPurpose::Copy),
        Err(SessionError::Policy(_))
    ));
}

#[test]
fn cell_range_cut_rejects_before_source_or_supplied_default_budget() {
    for over_budget_source in [false, true] {
        let intro = if over_budget_source {
            "x".repeat(3 * 1024 * 1024)
        } else {
            "intro".into()
        };
        let f = top_crossings(1, &intro);
        let options = spec_with(attrs(&[(
            "fill",
            AttrValue::String("x".repeat(3 * 1024 * 1024)),
        )]));
        let mut s = select_top_crossings(&f, 1, options);
        assert_eq!(
            checked_export(&mut s, ClipboardExportPurpose::Cut),
            Err(SessionError::UnsupportedTableOperation)
        );
    }
}

#[test]
fn clipping_opt_in_does_not_bypass_host_source_attribute_admission() {
    struct Strict(&'static str);
    impl SessionPolicy for Strict {
        fn clipboard_export_spec(
            &self,
            _: SessionContext<'_>,
            _: ClipboardExportPurpose,
        ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
            Ok(Some(spec()))
        }

        fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
            if document
                .store()
                .iter()
                .any(|node| node.attrs().get(self.0).is_some())
            {
                return Err(PolicyError::new(
                    "source attributes are outside host schema",
                ));
            }
            Ok(())
        }
    }
    let f = fixture(
        3,
        4,
        vec![
            CellSpec::new(1, 0, 1, 3)
                .with_attrs(&[("cellOpaque", opaque())])
                .rich_header(),
        ],
    );
    for forbidden in ["cellOpaque", "headingOpaque"] {
        let selection = DocumentSelection::collapsed(InlinePoint::at_start_of(f.intro));
        assert!(matches!(
            DocumentSession::new_with_policy(
                f.document.clone(),
                selection,
                Box::new(Strict(forbidden))
            ),
            Err(SessionError::Policy(_))
        ));
    }
    // The generic policy does admit this source and preserves opaque cell attrs,
    // even though the left-crossing cell's rich children are replaced.
    let slice = copied(&f, (0, 1), (2, 2), Some(spec())).unwrap();
    assert_eq!(
        find_cell(&slice, "1:0").attrs().get("cellOpaque"),
        Some(&opaque())
    );
    assert_cleared(find_cell(&slice, "1:0"), &defaults());
    roundtrip(&slice);
}

#[test]
fn core_rejects_an_empty_cell_forest_before_it_can_enter_a_session() {
    let mut b = NodeStoreBuilder::new();
    let cell = b
        .insert(
            NodeKind::TableCell,
            NodeAttrs::empty(),
            NodeContent::children([]),
        )
        .unwrap();
    let row = b
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([cell]),
        )
        .unwrap();
    let table = b
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap();
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([table]),
        )
        .unwrap();
    assert!(matches!(
        XiaomuDocument::new(root, b.finish()),
        Err(xiaomu_core::Error::InvalidTableStructure)
    ));
}
