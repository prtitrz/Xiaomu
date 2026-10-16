//! Rejected or cancelled explicit-All Cut preserves the original live source.

use super::*;
use crate::document_view::{EditorRejection, EditorRejectionReason, EditorRejectionStage};

pub(super) fn capture_rejections(
    m: &Mounted,
    cx: &mut TestAppContext,
) -> (Rc<RefCell<Vec<EditorRejection>>>, gpui::Subscription) {
    let entity = m.window.update(cx, |_, _, cx| cx.entity()).unwrap();
    let before = Snapshot::capture(m);
    let events = Rc::new(RefCell::new(Vec::new()));
    let seen = events.clone();
    let calls = m.calls.clone();
    let subscription = cx.update(|cx| {
        cx.subscribe(&entity, move |emitter, event: &EditorRejection, cx| {
            // GPUI delivers the event after the failed coordinator released
            // its session borrow. Inspect the original All, never a temporary
            // normalized text range, from the actual deferred callback.
            let session = emitter.read(cx).session().try_borrow_mut().unwrap();
            assert_eq!(session.document().store(), before.document.store());
            assert_eq!(session.document().revision(), before.document.revision());
            assert_eq!(session.selection(), before.selection);
            assert_eq!(session.history_depths(), before.history);
            assert_eq!(session.stored_marks(), before.marks.as_ref());
            assert_eq!(event.document_revision(), before.document.revision());
            assert_eq!(calls.borrow().selections, 0);
            assert!(calls.borrow().publications.is_empty());
            seen.borrow_mut().push(*event);
        })
    });
    (events, subscription)
}

pub(super) fn assert_cut_rejection(
    events: &Rc<RefCell<Vec<EditorRejection>>>,
    reason: EditorRejectionReason,
) {
    let events = events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].stage(), EditorRejectionStage::ClipboardCut);
    assert_eq!(events[0].reason(), reason);
}

#[gpui::test]
fn all_text_cut_expired_source_and_protected_candidate_never_write(cx: &mut TestAppContext) {
    for reject in [false, true] {
        let f = rich_fixture(None, 1);
        let m = mount_all(cx, &f, reject, false);
        let (events, _subscription) = capture_rejections(&m, cx);
        let before = Snapshot::capture(&m);
        let sentinel = previous(cx);
        if !reject {
            m.gate.set(false);
        }
        key(&m, cx, "ctrl-x");
        assert_cut_rejection(&events, EditorRejectionReason::Policy);
        before.unchanged(&m);
        assert_eq!(clipboard(cx), sentinel);
        assert_eq!(write_observation::calls(), 0);
        assert_eq!(m.calls.borrow().gates, 1);
        assert_eq!(
            m.calls.borrow().candidates,
            if reject { vec![0] } else { vec![] }
        );
        assert_eq!(m.calls.borrow().export_specs, usize::from(reject));
        assert_eq!(
            m.calls.borrow().sources,
            vec![before.selection; 1 + usize::from(reject)]
        );
        assert_eq!(m.calls.borrow().deletes, 0);
    }
}

#[gpui::test]
fn all_text_cut_nested_atomic_table_and_empty_container_barriers_never_write(
    cx: &mut TestAppContext,
) {
    for barrier in [
        NodeKind::Image,
        NodeKind::HorizontalRule,
        NodeKind::Table,
        NodeKind::Quote,
        NodeKind::BulletList,
    ] {
        let f = rich_fixture(Some(barrier), 1);
        let m = mount_all(cx, &f, false, false);
        let (events, _subscription) = capture_rejections(&m, cx);
        let before = Snapshot::capture(&m);
        let sentinel = previous(cx);
        key(&m, cx, "ctrl-x");
        assert_cut_rejection(&events, EditorRejectionReason::Policy);
        before.unchanged(&m);
        assert_eq!(clipboard(cx), sentinel);
        assert_eq!(write_observation::calls(), 0);
        assert_eq!(
            m.calls.borrow().sources,
            [before.selection, before.selection]
        );
        assert_eq!(m.calls.borrow().gates, 1);
        assert_eq!(m.calls.borrow().export_specs, 1);
        assert!(m.calls.borrow().candidates.is_empty());
        assert_eq!(m.calls.borrow().deletes, 0);
    }
}

#[gpui::test]
fn all_text_cut_prepared_item_cancellation_keeps_original_all_and_redo(cx: &mut TestAppContext) {
    let f = rich_fixture(None, 1);
    let m = mount_all(cx, &f, false, false);
    let (events, _subscription) = capture_rejections(&m, cx);
    let before = Snapshot::capture(&m);
    let sentinel = previous(cx);
    {
        let mut session = m.session.borrow_mut();
        let prepared = session.prepare_cut().unwrap().unwrap();
        let item = prepare_lossless_slice(prepared.clipboard_slice()).unwrap();
        let slice = decode_metadata(&item.text().unwrap(), item.metadata().unwrap()).unwrap();
        assert!(slice.is_closed());
        assert_eq!(&slice, prepared.clipboard_slice());
        drop(item);
        drop(prepared);
    }
    cx.background_executor.run_until_parked();
    assert!(events.borrow().is_empty(), "cancellation is silent");
    before.unchanged(&m);
    assert_eq!(before.history, (0, 1));
    assert_eq!(clipboard(cx), sentinel);
    assert_eq!(write_observation::calls(), 0);
    assert_eq!(m.calls.borrow().candidates, [0]);
    assert_eq!(
        m.calls.borrow().sources,
        [before.selection, before.selection]
    );
    assert_eq!(m.calls.borrow().deletes, 0);
}

#[gpui::test]
fn all_text_cut_budget_and_real_legacy_encoder_failure_keep_all(cx: &mut TestAppContext) {
    // The borrowed prepared budget is stricter than the wire encoder. Exercise
    // its refusal and a genuine legacy encoding failure, without a fake writer.
    for legacy in [false, true] {
        let f = rich_fixture(None, 160);
        let m = mount_all(cx, &f, false, legacy);
        let (events, _subscription) = capture_rejections(&m, cx);
        let before = Snapshot::capture(&m);
        let sentinel = previous(cx);
        if legacy {
            let slice = m
                .session
                .borrow()
                .clipboard_slice_for(ClipboardExportPurpose::Cut)
                .unwrap()
                .unwrap();
            assert!(slice.is_closed());
            assert!(prepare_lossless_slice(&slice).is_none());
        }
        key(&m, cx, "ctrl-x");
        assert_cut_rejection(
            &events,
            if legacy {
                EditorRejectionReason::ClipboardMetadata
            } else {
                EditorRejectionReason::Policy
            },
        );
        before.unchanged(&m);
        assert_eq!(clipboard(cx), sentinel);
        assert_eq!(write_observation::calls(), 0);
        assert!(m.calls.borrow().candidates.is_empty());
        assert_eq!(m.calls.borrow().deletes, 0);
        assert_eq!(
            m.calls.borrow().sources,
            vec![before.selection; if legacy { 0 } else { 2 }]
        );
    }
}

#[gpui::test]
fn all_text_cut_default_off_keeps_legacy_write_then_unsupported_delete(cx: &mut TestAppContext) {
    let f = rich_fixture(None, 1);
    let m = mount_all(cx, &f, false, true);
    let (events, _subscription) = capture_rejections(&m, cx);
    let before = Snapshot::capture(&m);
    assert!(m.session.borrow_mut().prepare_cut().unwrap().is_none());
    let expected = m
        .session
        .borrow()
        .clipboard_slice_for(ClipboardExportPurpose::Cut)
        .unwrap()
        .unwrap();
    let expected = prepare_lossless_slice(&expected).unwrap();
    previous(cx);
    key(&m, cx, "ctrl-x");
    assert_eq!(events.borrow().len(), 1);
    assert_eq!(events.borrow()[0].stage(), EditorRejectionStage::Intent);
    assert_eq!(
        events.borrow()[0].reason(),
        EditorRejectionReason::InvalidSelection
    );
    before.unchanged(&m);
    assert_eq!(clipboard(cx), expected);
    assert_eq!(write_observation::calls(), 1);
    assert_eq!(m.calls.borrow().gates, 0);
    assert!(m.calls.borrow().candidates.is_empty());
}
