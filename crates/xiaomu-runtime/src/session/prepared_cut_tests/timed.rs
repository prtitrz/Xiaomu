//! Timed grouping through the existing real scoped CellRange Cut fixture.
//! Public preparation/publication only; full transient state is observed.

use super::super::{HistoryOptions, HistoryTimestamp, SelectionOnlyGrouping};
use super::*;

struct TimedCutPolicy(Fault);
impl SessionPolicy for TimedCutPolicy {
    fn history_options(&self) -> HistoryOptions {
        HistoryOptions::new()
            .with_typing_group_delay_ms(500)
            .with_selection_only_grouping(SelectionOnlyGrouping::Preserve)
    }
    fn clipboard_export_spec(
        &self,
        context: SessionContext<'_>,
        purpose: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        CutPolicy(self.0).clipboard_export_spec(context, purpose)
    }
    fn prepare_cut(&self, context: SessionContext<'_>) -> Result<Option<EditPlan>, PolicyError> {
        CutPolicy(self.0).prepare_cut(context)
    }
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        CutPolicy(self.0).prepare_intent(context, intent)
    }
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        CutPolicy(self.0).validate_document(document)
    }
}
fn typed(s: &mut DocumentSession, text: &str, time: u64) {
    assert_eq!(
        s.apply_intent_at(
            &EditIntent::InsertText { text: text.into() },
            HistoryTimestamp::from_millis(time),
        ),
        Ok(SessionOutcome::DocumentChanged)
    );
}
fn seeded(f: &Fixture, fault: Fault) -> (DocumentSession, Events) {
    let mut s = session(f, Some(Box::new(TimedCutPolicy(fault))));
    let events = listen(&mut s);
    typed(&mut s, "X", 1_000);
    select_cells(&mut s, f, true);
    assert!(
        s.history.typing_group_open(),
        "public Preserve keeps anchor at selection"
    );
    (s, events)
}

#[test]
fn timed_cut_prepare_drop_and_platform_preparation_failure_leave_full_state_unchanged() {
    let f = fixture(false);
    let (mut s, events) = seeded(&f, Fault::None);
    let before = Snapshot::capture(&mut s, &events);
    {
        let prepared = s.prepare_cut().unwrap().unwrap();
        assert!(!prepared.clipboard_slice().plain_text().is_empty());
        drop(prepared);
    }
    before.assert_unchanged(&mut s, &events);
    // A local stand-in for a fallible frontend item conversion, not an OS test.
    let conversion: Result<(), &str> = {
        let prepared = s.prepare_cut().unwrap().unwrap();
        assert!(!prepared.clipboard_slice().plain_text().is_empty());
        Err("platform item conversion refused")
    };
    assert!(conversion.is_err());
    before.assert_unchanged(&mut s, &events);
    let after_x = xiaomu_core::text::TextBuffer::from_string("Xintro".into())
        .offset_at(1)
        .unwrap();
    s.set_document_selection(DocumentSelection::collapsed(InlinePoint::new(
        f.intro,
        after_x,
        0,
        xiaomu_core::selection::CursorAffinity::Before,
    )))
    .unwrap();
    typed(&mut s, "Y", 1_400);
    assert_eq!(
        s.history_depths(),
        (1, 0),
        "drop retained exact typing anchor"
    );
}

#[test]
fn timed_cut_all_prepublication_failures_and_budget_refusal_preserve_full_state() {
    for fault in [
        Fault::PreparePolicy,
        Fault::ExportPolicy,
        Fault::MissingExport,
        Fault::EmptyPlan,
        Fault::InvalidTransaction,
        Fault::InvalidAfterSelection,
        Fault::FinalCandidatePolicy,
    ] {
        let f = fixture(false);
        let (mut s, events) = seeded(&f, fault);
        let before = Snapshot::capture(&mut s, &events);
        assert!(s.prepare_cut().is_err(), "{fault:?}");
        before.assert_unchanged(&mut s, &events);
    }
    let f = fixture(true);
    let (mut s, events) = seeded(&f, Fault::None);
    let before = Snapshot::capture(&mut s, &events);
    assert!(s.prepare_cut().is_err());
    before.assert_unchanged(&mut s, &events);
}

#[test]
fn timed_cut_publish_is_one_isolated_entry_and_retains_high_water() {
    let f = fixture(false);
    let (mut s, events) = seeded(&f, Fault::None);
    let document = s.document().clone();
    let selection = s.selection();
    let event_count = events.borrow().len();
    assert_eq!(
        s.prepare_cut().unwrap().unwrap().publish(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(s.history_depths(), (2, 0));
    assert!(!s.history.typing_group_open());
    assert_eq!(events.borrow().len(), event_count + 1);
    let cut_document = s.document().clone();
    let cut_selection = s.selection();
    s.undo().unwrap();
    assert_eq!(s.document().store(), document.store());
    assert_eq!(s.selection(), selection);
    s.redo().unwrap();
    assert_eq!(s.document().store(), cut_document.store());
    assert_eq!(s.selection(), cut_selection);
    // Neither Cut nor traversal resets the successful typing high-water mark.
    typed(&mut s, "A", 900);
    typed(&mut s, "B", 950);
    assert_eq!(s.history_depths(), (4, 0));
    assert!(!s.history.typing_group_open());
    typed(&mut s, "C", 1_000);
    typed(&mut s, "D", 1_100);
    assert_eq!(s.history_depths(), (5, 0));
}
