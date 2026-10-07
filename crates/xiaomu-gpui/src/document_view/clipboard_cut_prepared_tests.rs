//! Mounted virtual-platform tests. They do not establish OS clipboard ownership.

use crate::document_view::{EditorRejection, EditorRejectionReason, EditorRejectionStage};
use crate::input::platform_clipboard::{prepare_lossless_slice, write_observation};
use gpui::{EntityInputHandler, Focusable as _};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{DocumentChangeListener, DocumentSession, EditPlan, SelectionUpdate};

include!("clipboard_cut_prepared_fixture.rs");

#[gpui::test]
fn prepared_cell_cut_writes_once_then_publishes_once_and_reuses_redo_ids(cx: &mut TestAppContext) {
    for reverse in [false, true] {
        let f = fixture(false, 1);
        let (m, notifications) = mount_cut(cx, &f, Failure::None);
        if reverse {
            m.session
                .borrow_mut()
                .set_cell_range_selection(*f.cells.last().unwrap(), f.cells[0])
                .unwrap();
            notifications.borrow_mut().selections = 0;
        }
        let before = Snapshot::capture(&m);
        let expected = m.session.borrow().clipboard_slice().unwrap().unwrap();
        // Merely asking for public Cut projection still does not admit deletion.
        assert!(
            m.session
                .borrow()
                .clipboard_slice_for(ClipboardExportPurpose::Cut)
                .is_err()
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
            "publish must not revalidate"
        );
        assert_eq!(m.seen.borrow().exports, [ClipboardExportPurpose::Cut]);
        assert_eq!(m.seen.borrow().deletes, 0);
        assert_eq!(notifications.borrow().document_writes, [1]);
        assert_eq!(notifications.borrow().selections, 0);
        assert_eq!(m.session.borrow().history_depths(), (1, 0));
        assert!(rejections.borrow().is_empty());
        let after_document = m.session.borrow().document().clone();
        let after_selection = m.session.borrow().selection();
        assert!(after_selection.is_collapsed());
        let head = before.selection.active_cell_range().unwrap().focus();
        let fresh = after_document
            .node(head)
            .unwrap()
            .content()
            .as_children()
            .unwrap()[0];
        assert_eq!(
            after_selection,
            DocumentSelection::collapsed(InlinePoint::at_start_of(fresh))
        );
        assert!(before.document.node(fresh).is_none());
        assert_eq!(m.session.borrow().stored_marks(), None);
        for cell in &f.cells {
            let original = before.document.node(*cell).unwrap();
            let actual = after_document.node(*cell).unwrap();
            assert_eq!(actual.kind(), original.kind());
            assert_eq!(actual.attrs(), original.attrs());
            let children = actual.content().as_children().unwrap();
            assert_eq!(children.len(), 1);
            let paragraph = after_document.node(children[0]).unwrap();
            assert_eq!(paragraph.kind(), &NodeKind::Paragraph);
            assert_eq!(paragraph.attrs(), &NodeAttrs::empty());
            assert!(paragraph.content().as_inline().unwrap().runs().is_empty());
        }
        assert_eq!(after_document.node(f.intro), before.document.node(f.intro));
        assert_eq!(after_document.node(f.table), before.document.node(f.table));
        m.window
            .update(cx, |view, window, cx| {
                assert_eq!(view.epoch.get(), epoch + 1);
                assert_eq!(view.desired_x, None);
                let child = view
                    .children
                    .iter()
                    .find(|(node, _)| *node == fresh)
                    .unwrap()
                    .1
                    .clone();
                assert!(child.read(cx).focus_handle(cx).is_focused(window));
            })
            .unwrap();
        let copied = clipboard(cx);
        action(&m, ClipboardExportPurpose::Cut, cx);
        assert_eq!(write_observation::calls(), 1, "collapsed repeat is a no-op");
        assert_eq!(clipboard(cx), copied);
        assert_eq!(m.session.borrow().history_depths(), (1, 0));
        assert_eq!(notifications.borrow().document_writes, [1]);
        m.session.borrow_mut().undo().unwrap();
        assert_eq!(
            m.session.borrow().document().store(),
            before.document.store()
        );
        assert_eq!(m.session.borrow().selection(), before.selection);
        m.session.borrow_mut().redo().unwrap();
        assert_eq!(
            m.session.borrow().document().store(),
            after_document.store()
        );
        assert_eq!(m.session.borrow().selection(), after_selection);
        m.window
            .update(cx, |view, window, cx| {
                view.apply_edit_intent(
                    EditIntent::InsertText {
                        text: "next".into(),
                    },
                    window,
                    cx,
                );
            })
            .unwrap();
        assert_eq!(
            m.session
                .borrow()
                .document()
                .node(fresh)
                .unwrap()
                .content()
                .as_inline()
                .unwrap()
                .runs()[0]
                .text()
                .as_str(),
            "next"
        );
        assert_eq!(
            write_observation::calls(),
            1,
            "Undo/Redo/typing never rewrite clipboard"
        );
    }
}

#[gpui::test]
fn complete_candidate_rejections_precede_writer_and_leave_view_unchanged(cx: &mut TestAppContext) {
    for (failure, reason) in [
        (Failure::FinalPolicy, EditorRejectionReason::Policy),
        (
            Failure::FinalSelection,
            EditorRejectionReason::InvalidSelection,
        ),
        (Failure::Core, EditorRejectionReason::InvalidTransaction),
    ] {
        let f = fixture(false, 1);
        let (m, notifications) = mount_cut(cx, &f, failure);
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
        assert_eq!(m.seen.borrow().cut_preparations, 1);
        assert_eq!(m.seen.borrow().deletes, 0);
        let events = rejections.borrow();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].stage(), EditorRejectionStage::ClipboardCut);
        assert_eq!(events[0].reason(), reason);
        assert_eq!(events[0].document_revision(), before.document.revision());
    }
}

#[gpui::test]
fn prepared_source_budget_rejection_never_reaches_candidate_or_writer(cx: &mut TestAppContext) {
    let f = fixture(false, 160);
    let (m, notifications) = mount_cut(cx, &f, Failure::None);
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
        rejections.borrow()[0].stage(),
        EditorRejectionStage::ClipboardCut
    );
}

#[gpui::test]
fn real_legacy_metadata_budget_rejection_never_writes_or_deletes(cx: &mut TestAppContext) {
    // Projected sources have a conservative pre-clone budget. Do not invent
    // an override to force an invalid slice through that production admission.
    let f = fixture(false, 160);
    let m = mount(cx, &f, false, false);
    let before = Snapshot::capture(&m);
    let previous = install_previous(cx);
    let epoch = seed_view_state(&m, cx);
    let (rejections, _subscription) = capture_rejections(&m, cx);
    write_observation::reset();
    let slice = m
        .session
        .borrow()
        .clipboard_slice_for(ClipboardExportPurpose::Cut)
        .unwrap()
        .unwrap();
    assert!(prepare_lossless_slice(&slice).is_none());
    assert_eq!(write_observation::calls(), 0);
    assert_eq!(clipboard(cx), previous);
    action(&m, ClipboardExportPurpose::Cut, cx);
    cx.background_executor.run_until_parked();
    assert_eq!(write_observation::calls(), 0);
    assert_eq!(clipboard(cx), previous);
    before.assert_unchanged(&m);
    assert_view_unchanged(&m, epoch, cx);
    assert_eq!(m.seen.borrow().deletes, 0);
    assert_eq!(rejections.borrow().len(), 1);
    assert_eq!(
        rejections.borrow()[0].stage(),
        EditorRejectionStage::ClipboardCut
    );
    assert_eq!(
        rejections.borrow()[0].reason(),
        EditorRejectionReason::ClipboardMetadata
    );
}

#[gpui::test]
fn dropping_prepared_guard_and_owned_item_does_not_publish(cx: &mut TestAppContext) {
    let f = fixture(false, 1);
    let (m, notifications) = mount_cut(cx, &f, Failure::None);
    let before = Snapshot::capture(&m);
    let previous = install_previous(cx);
    let epoch = seed_view_state(&m, cx);
    write_observation::reset();
    {
        let mut session = m.session.borrow_mut();
        let guard = session.prepare_cut().unwrap().unwrap();
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
    // A matched untouched session confirms that preparation consumed no IDs.
    let mut control = DocumentSession::new(before.document.clone(), before.selection).unwrap();
    let parent = before.document.root();
    let next =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::InsertNode {
            parent,
            index: before
                .document
                .node(parent)
                .unwrap()
                .content()
                .as_children()
                .unwrap()
                .len(),
            kind: NodeKind::Paragraph,
            attrs: NodeAttrs::empty(),
            content: NodeContent::Inline(InlineContent::empty()),
        });
    m.session.borrow_mut().apply(&next).unwrap();
    control.apply(&next).unwrap();
    assert_eq!(
        m.session.borrow().document().store(),
        control.document().store()
    );
}

#[gpui::test]
fn hidden_endpoints_block_dedicated_policy_before_clipboard(cx: &mut TestAppContext) {
    let f = fixture(true, 1);
    let (m, notifications) = mount_cut(cx, &f, Failure::None);
    let before = Snapshot::capture(&m);
    let previous = install_previous(cx);
    let epoch = seed_view_state(&m, cx);
    let (rejections, _subscription) = capture_rejections(&m, cx);
    write_observation::reset();
    m.window
        .update(cx, |view, window, cx| {
            assert!(view.selection_has_hidden_table_endpoint());
            view.cut(&ClipboardCut, window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert_eq!(write_observation::calls(), 0);
    assert_eq!(clipboard(cx), previous);
    before.assert_unchanged(&m);
    assert_view_unchanged(&m, epoch, cx);
    assert_no_notifications(&notifications);
    assert_eq!(m.seen.borrow().cut_preparations, 0);
    assert!(m.seen.borrow().exports.is_empty());
    assert_eq!(m.seen.borrow().deletes, 0);
    assert!(rejections.borrow().is_empty());
}

#[gpui::test]
fn native_composition_blocks_dedicated_and_legacy_cut_before_any_write(cx: &mut TestAppContext) {
    for dedicated in [false, true] {
        let f = fixture(false, 1);
        let m = if dedicated {
            mount_cut(cx, &f, Failure::None).0
        } else {
            mount(cx, &f, false, false)
        };
        {
            let mut session = m.session.borrow_mut();
            let inline = session
                .document()
                .node(f.intro)
                .unwrap()
                .content()
                .as_inline()
                .unwrap();
            let end = xiaomu_core::selection::InlinePoint::new(
                f.intro,
                inline.offset_at(1).unwrap(),
                0,
                xiaomu_core::selection::CursorAffinity::Before,
            );
            session
                .set_inline_selection(InlinePoint::at_start_of(f.intro), end)
                .unwrap();
        }
        m.window
            .update(cx, |view, window, cx| view.focus_selection(window, cx))
            .unwrap();
        let before = Snapshot::capture(&m);
        let previous = install_previous(cx);
        let epoch = seed_view_state(&m, cx);
        let (rejections, _subscription) = capture_rejections(&m, cx);
        write_observation::reset();
        m.window
            .update(cx, |view, window, cx| {
                let child = view
                    .children
                    .iter()
                    .find(|(node, _)| *node == f.intro)
                    .unwrap()
                    .1
                    .clone();
                child.update(cx, |child, cx| {
                    child.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx)
                });
                assert!(view.focused_child_composing(window, cx));
                view.cut(&ClipboardCut, window, cx);
                assert!(child.read(cx).is_composing());
                assert!(child.read(cx).focus_handle(cx).is_focused(window));
                assert_eq!(view.epoch.get(), epoch);
            })
            .unwrap();
        assert_eq!(write_observation::calls(), 0);
        assert_eq!(clipboard(cx), previous);
        before.assert_unchanged(&m);
        assert_view_unchanged(&m, epoch, cx);
        assert_eq!(m.seen.borrow().cut_preparations, 0);
        assert!(m.seen.borrow().exports.is_empty());
        assert_eq!(m.seen.borrow().deletes, 0);
        assert!(rejections.borrow().is_empty());
    }
}

#[path = "clipboard_cut_node_tests.rs"]
mod node_tests;
