//! No-publication, rejection, and allocator regressions.

use super::*;

#[test]
fn default_prepare_cut_none_preserves_real_typing_marks_group_and_rule_token() {
    let f = fixture(false);
    for with_export in [false, true] {
        let policy = || with_export.then(|| Box::new(ExportOnly) as Box<dyn SessionPolicy>);
        let mut s = session(&f, policy());
        let mut control = session(&f, policy());
        let events = listen(&mut s);
        for target in [&mut s, &mut control] {
            target
                .apply_intent(&EditIntent::ToggleMark { mark: Mark::Italic })
                .unwrap();
            insert(target, "a");
        }
        assert!(s.stored_marks().is_some());
        assert!(s.history.typing_group_open());
        let before = Snapshot::capture(&mut s, &events);
        let mut writer = Writer::seeded();
        assert_eq!(publish_through_writer(&mut s, &mut writer), Ok(None));
        assert_eq!(writer, Writer::seeded());
        before.assert_unchanged(&mut s, &events);
        insert(&mut s, "b");
        insert(&mut control, "b");
        assert_eq!(
            s.history_depths(),
            (1, 0),
            "None must not break adjacent typing"
        );
        install_rule_token(&mut s);
        install_rule_token(&mut control);
        let with_token = Snapshot::capture(&mut s, &events);
        assert!(s.prepare_cut().unwrap().is_none());
        with_token.assert_unchanged(&mut s, &events);
        assert_future_allocation_matches(&mut s, &mut control);
    }
}

#[test]
fn export_opt_in_without_cut_plan_is_none_and_public_cell_cut_stays_closed() {
    let f = fixture(false);
    let mut s = session(&f, Some(Box::new(ExportOnly)));
    let events = listen(&mut s);
    seed_redo(&mut s, &f);
    let before = Snapshot::capture(&mut s, &events);
    assert!(s.prepare_cut().unwrap().is_none());
    assert_eq!(
        s.clipboard_slice_for(ClipboardExportPurpose::Cut),
        Err(SessionError::UnsupportedTableOperation),
    );
    assert!(
        s.clipboard_slice_for(ClipboardExportPurpose::Copy)
            .unwrap()
            .is_some()
    );
    before.assert_unchanged(&mut s, &events);
    // Legacy unit-cell projection remains its existing API; this regression
    // intentionally does not broaden the public Cut refusal to legacy users.
    let mut legacy = session(&f, None);
    select_cells(&mut legacy, &f, false);
    assert!(legacy.prepare_cut().unwrap().is_none());
    assert!(
        legacy
            .clipboard_slice_for(ClipboardExportPurpose::Cut)
            .unwrap()
            .is_some()
    );
}

#[test]
fn dropping_prepared_cut_preserves_every_state_field_and_next_allocated_identity() {
    let f = fixture(false);
    for sentinel_state in [false, true] {
        let mut s = session(&f, Some(Box::new(CutPolicy(Fault::None))));
        let mut control = session(&f, Some(Box::new(CutPolicy(Fault::None))));
        let events = listen(&mut s);
        for target in [&mut s, &mut control] {
            seed_redo(target, &f);
            if sentinel_state {
                seed_transient_sentinels(target, &f);
            }
        }
        let before = Snapshot::capture(&mut s, &events);
        assert_eq!(
            s.clipboard_slice_for(ClipboardExportPurpose::Cut),
            Err(SessionError::UnsupportedTableOperation),
            "a dedicated Cut policy must not open the public projection-only path",
        );
        let copied = {
            let mut copy_session = session(&f, Some(Box::new(ExportOnly)));
            select_cells(&mut copy_session, &f, true);
            copy_session.clipboard_slice().unwrap().unwrap()
        };
        {
            let prepared = s.prepare_cut().unwrap().unwrap();
            assert_eq!(prepared.clipboard_slice(), &copied);
            assert_eq!(
                *events.borrow(),
                before.events,
                "preparation must not notify"
            );
            drop(prepared);
        }
        before.assert_unchanged(&mut s, &events);
        assert_future_allocation_matches(&mut s, &mut control);
    }
}

#[test]
fn all_prepublication_rejections_leave_writer_state_history_and_allocator_unchanged() {
    for fault in [
        Fault::PreparePolicy,
        Fault::ExportPolicy,
        Fault::MissingExport,
        Fault::EmptyPlan,
        Fault::InvalidTransaction,
        Fault::InvalidAfterSelection,
        Fault::FinalCandidatePolicy,
    ] {
        for sentinels in [false, true] {
            let f = fixture(false);
            let mut s = session(&f, Some(Box::new(CutPolicy(fault))));
            let mut control = session(&f, Some(Box::new(CutPolicy(fault))));
            let events = listen(&mut s);
            for target in [&mut s, &mut control] {
                seed_redo(target, &f);
                if sentinels {
                    seed_transient_sentinels(target, &f);
                }
            }
            let before = Snapshot::capture(&mut s, &events);
            let mut writer = Writer::seeded();
            let error = publish_through_writer(&mut s, &mut writer).unwrap_err();
            match fault {
                Fault::InvalidTransaction => assert!(matches!(error, SessionError::Core(_))),
                Fault::InvalidAfterSelection => assert_eq!(error, SessionError::SelectionInvalid),
                Fault::PreparePolicy | Fault::ExportPolicy | Fault::FinalCandidatePolicy => {
                    assert!(
                        matches!(error, SessionError::Policy(_)),
                        "{fault:?}: {error:?}"
                    );
                }
                // The rejection type is intentionally not a new API contract.
                Fault::MissingExport | Fault::EmptyPlan => {}
                _ => unreachable!(),
            }
            assert_eq!(writer, Writer::seeded(), "{fault:?}");
            before.assert_unchanged(&mut s, &events);
            assert_future_allocation_matches(&mut s, &mut control);
        }
    }
}

#[test]
fn projection_budget_rejection_is_before_writer_and_preserves_complete_state() {
    let f = fixture(true);
    let mut s = session(&f, Some(Box::new(CutPolicy(Fault::None))));
    let mut control = session(&f, Some(Box::new(CutPolicy(Fault::None))));
    let events = listen(&mut s);
    for target in [&mut s, &mut control] {
        seed_redo(target, &f);
        seed_transient_sentinels(target, &f);
    }
    let before = Snapshot::capture(&mut s, &events);
    let mut writer = Writer::seeded();
    assert!(matches!(
        publish_through_writer(&mut s, &mut writer),
        Err(SessionError::Policy(_)),
    ));
    assert_eq!(writer, Writer::seeded());
    before.assert_unchanged(&mut s, &events);
    assert_future_allocation_matches(&mut s, &mut control);
}

#[test]
fn a_host_plan_without_a_cell_range_does_not_admit_prepared_cut() {
    let f = fixture(false);
    let mut s = session(&f, Some(Box::new(CutPolicy(Fault::NonCellPlan))));
    let events = listen(&mut s);
    let before = Snapshot::capture(&mut s, &events);
    let mut writer = Writer::seeded();
    assert!(publish_through_writer(&mut s, &mut writer).is_err());
    assert_eq!(writer, Writer::seeded());
    before.assert_unchanged(&mut s, &events);
}
