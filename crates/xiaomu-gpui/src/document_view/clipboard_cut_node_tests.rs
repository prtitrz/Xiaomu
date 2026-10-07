//! Mounted whole-node Cut regressions. These are not OS ownership assertions.

include!("clipboard_cut_node_fixture.rs");

#[gpui::test]
fn node_and_atomic_cut_preflight_before_one_write_and_exact_publish(cx: &mut TestAppContext) {
    for atomic in [false, true] {
        for table_after in [false, true] {
            let f = node_fixture(table_after, false);
            let (m, notifications) = mount_node_cut(cx, &f, atomic, NodeFailure::None);
            let before = Snapshot::capture(&m);
            let expected = m.session.borrow().clipboard_slice().unwrap().unwrap();
            assert_eq!(
                expected.source_boundary(),
                Some(if atomic {
                    ClipboardSourceBoundary::Open
                } else {
                    ClipboardSourceBoundary::WholeRoots
                })
            );
            assert_eq!(expected.roots().len(), 1);
            assert_eq!(expected.roots()[0].kind(), &NodeKind::Image);
            assert_eq!(
                expected.roots()[0].attrs(),
                before.document.node(f.selected).unwrap().attrs()
            );
            m.seen.borrow_mut().exports.clear();
            install_previous(cx);
            write_observation::reset();
            let epoch = seed_view_state(&m, cx);
            let (rejections, _subscription) = capture_rejections(&m, cx);
            action(&m, ClipboardExportPurpose::Cut, cx);
            assert_eq!(write_observation::calls(), 1);
            assert_eq!(decoded(&clipboard(cx)), expected);
            assert_eq!(m.seen.borrow().cut_preparations, 1);
            assert_eq!(m.seen.borrow().cut_candidate_writes, [0]);
            assert_eq!(
                m.seen.borrow().cut_candidates,
                1,
                "publish cannot revalidate"
            );
            assert_eq!(m.seen.borrow().exports, [ClipboardExportPurpose::Cut]);
            assert_eq!(m.seen.borrow().deletes, 0);
            assert_eq!(notifications.borrow().document_writes, [1]);
            assert_eq!(notifications.borrow().selections, 0);
            assert_eq!(m.session.borrow().history_depths(), (1, 0));
            assert_eq!(m.session.borrow().stored_marks(), None);
            assert!(rejections.borrow().is_empty());
            let after = m.session.borrow().document().clone();
            let selection = DocumentSelection::collapsed(InlinePoint::at_start_of(f.caret));
            assert_eq!(m.session.borrow().selection(), selection);
            assert!(after.node(f.selected).is_none());
            assert_eq!(after.node(f.caret), before.document.node(f.caret));
            m.window
                .update(cx, |view, window, cx| {
                    assert_eq!(view.epoch.get(), epoch + 1);
                    assert_eq!(view.desired_x, None);
                    let child = &view
                        .children
                        .iter()
                        .find(|(node, _)| *node == f.caret)
                        .unwrap()
                        .1;
                    assert!(child.read(cx).focus_handle(cx).is_focused(window));
                })
                .unwrap();
            let copied = clipboard(cx);
            action(&m, ClipboardExportPurpose::Cut, cx);
            assert_eq!(
                write_observation::calls(),
                1,
                "collapsed repeat must not write"
            );
            assert_eq!(clipboard(cx), copied);
            assert_eq!(m.session.borrow().history_depths(), (1, 0));
            m.session.borrow_mut().undo().unwrap();
            assert_eq!(
                m.session.borrow().document().store(),
                before.document.store()
            );
            assert_eq!(m.session.borrow().selection(), before.selection);
            m.session.borrow_mut().redo().unwrap();
            assert_eq!(m.session.borrow().document().store(), after.store());
            assert_eq!(m.session.borrow().selection(), selection);
            assert_eq!(write_observation::calls(), 1);
        }
    }
}

#[gpui::test]
fn every_node_cut_semantic_failure_precedes_writer_without_legacy_fallback(
    cx: &mut TestAppContext,
) {
    for failure in [
        NodeFailure::Prepare,
        NodeFailure::Export,
        NodeFailure::MissingExport,
        NodeFailure::Empty,
        NodeFailure::Core,
        NodeFailure::StaleSelection,
        NodeFailure::GapAfter,
        NodeFailure::NodeAfter,
        NodeFailure::AtomicAfter,
        NodeFailure::RangeAfter,
        NodeFailure::Admission,
    ] {
        for atomic in [false, true] {
            let f = node_fixture(false, false);
            let (m, notifications) = mount_node_cut(cx, &f, atomic, failure);
            let before = Snapshot::capture(&m);
            let previous = install_previous(cx);
            let epoch = seed_view_state(&m, cx);
            let (rejections, _subscription) = capture_rejections(&m, cx);
            write_observation::reset();
            action(&m, ClipboardExportPurpose::Cut, cx);
            cx.background_executor.run_until_parked();
            assert_eq!(write_observation::calls(), 0, "{failure:?}");
            assert_eq!(clipboard(cx), previous);
            before.assert_unchanged(&m);
            assert_view_unchanged(&m, epoch, cx);
            assert_no_notifications(&notifications);
            assert_eq!(m.seen.borrow().cut_preparations, 1);
            assert_eq!(m.seen.borrow().deletes, 0);
            let events = rejections.borrow();
            assert_eq!(events.len(), 1, "{failure:?}");
            assert_eq!(events[0].stage(), EditorRejectionStage::ClipboardCut);
            assert_eq!(events[0].document_revision(), before.document.revision());
            let reason = match failure {
                NodeFailure::Prepare | NodeFailure::Export | NodeFailure::Admission => {
                    EditorRejectionReason::Policy
                }
                NodeFailure::Core => EditorRejectionReason::InvalidTransaction,
                NodeFailure::StaleSelection
                | NodeFailure::GapAfter
                | NodeFailure::NodeAfter
                | NodeFailure::AtomicAfter
                | NodeFailure::RangeAfter => EditorRejectionReason::InvalidSelection,
                _ => continue,
            };
            assert_eq!(events[0].reason(), reason);
        }
    }
}

#[gpui::test]
fn node_cut_budget_refusal_never_reaches_candidate_or_writer(cx: &mut TestAppContext) {
    for atomic in [false, true] {
        let f = node_fixture(false, true);
        let (m, notifications) = mount_node_cut(cx, &f, atomic, NodeFailure::None);
        let before = Snapshot::capture(&m);
        let previous = install_previous(cx);
        let epoch = seed_view_state(&m, cx);
        let (rejections, _subscription) = capture_rejections(&m, cx);
        write_observation::reset();
        action(&m, ClipboardExportPurpose::Cut, cx);
        cx.background_executor.run_until_parked();
        assert_eq!(write_observation::calls(), 0);
        assert_eq!(clipboard(cx), previous);
        before.assert_unchanged(&m);
        assert_view_unchanged(&m, epoch, cx);
        assert_no_notifications(&notifications);
        assert_eq!(m.seen.borrow().cut_candidates, 0);
        assert_eq!(m.seen.borrow().deletes, 0);
        assert_eq!(rejections.borrow().len(), 1);
        assert_eq!(
            rejections.borrow()[0].reason(),
            EditorRejectionReason::Policy
        );
    }
}

#[gpui::test]
fn dropping_prepared_node_guard_and_lossless_item_has_no_effect(cx: &mut TestAppContext) {
    for atomic in [false, true] {
        let f = node_fixture(false, false);
        let (m, notifications) = mount_node_cut(cx, &f, atomic, NodeFailure::None);
        let before = Snapshot::capture(&m);
        let previous = install_previous(cx);
        let epoch = seed_view_state(&m, cx);
        write_observation::reset();
        {
            let mut s = m.session.borrow_mut();
            let guard = s.prepare_cut().unwrap().unwrap();
            let item = prepare_lossless_slice(guard.clipboard_slice()).unwrap();
            assert_eq!(decoded(&item), *guard.clipboard_slice());
            drop(item);
            drop(guard);
        }
        assert_eq!(write_observation::calls(), 0);
        assert_eq!(clipboard(cx), previous);
        before.assert_unchanged(&m);
        assert_view_unchanged(&m, epoch, cx);
        assert_no_notifications(&notifications);
    }
}

#[gpui::test]
fn no_node_plan_retains_legacy_write_then_delete_refusal(cx: &mut TestAppContext) {
    for atomic in [false, true] {
        let f = node_fixture(false, false);
        let (m, notifications) = mount_node_cut(cx, &f, atomic, NodeFailure::NoPlan);
        let before = Snapshot::capture(&m);
        let expected = m.session.borrow().clipboard_slice().unwrap().unwrap();
        install_previous(cx);
        write_observation::reset();
        action(&m, ClipboardExportPurpose::Cut, cx);
        assert_eq!(
            write_observation::calls(),
            1,
            "None retains historical non-atomic legacy route"
        );
        assert_eq!(decoded(&clipboard(cx)), expected);
        assert_eq!(m.seen.borrow().deletes, 1);
        assert_eq!(m.seen.borrow().cut_candidates, 0);
        before.assert_unchanged(&m);
        assert_no_notifications(&notifications);
    }
}
