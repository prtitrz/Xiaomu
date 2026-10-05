//! Exact publication, history, normalization, and fresh-selection regressions.

use super::*;

#[test]
fn publish_notifies_once_is_isolated_and_undo_redo_restore_exact_ids_and_selection() {
    let f = fixture(false);
    for backward in [false, true] {
        for with_token in [false, true] {
            let mut s = session(&f, Some(Box::new(CutPolicy(Fault::None))));
            seed_redo(&mut s, &f);
            select_cells(&mut s, &f, backward);
            if with_token {
                install_rule_token(&mut s);
            }
            let before = s.document().clone();
            let before_selection = s.selection();
            let depths = s.history_depths();
            let events = listen(&mut s);
            let mut writer = Writer::seeded();
            assert_eq!(
                publish_through_writer(&mut s, &mut writer),
                Ok(Some(SessionOutcome::DocumentChanged)),
            );
            assert_eq!(writer.calls, 1);
            assert_eq!(writer.text, "alpha\nnested\nbeta");
            assert_eq!(s.history_depths(), (depths.0 + 1, 0));
            assert!(!s.history.typing_group_open());
            assert_eq!(s.stored_marks(), None);
            assert!(s.input_rule_undo.is_none());
            assert!(!s.input_rule_undo_available());
            assert!(s.selection().active_cell_range().is_none());
            let head_cell = f.cells[usize::from(!backward)];
            let new_head = s
                .document()
                .node(head_cell)
                .unwrap()
                .content()
                .as_children()
                .unwrap()[0];
            let (_, focus) = s.selection().as_same_node_inline().unwrap();
            assert!(s.selection().is_collapsed());
            assert_eq!(focus.node_id(), new_head);
            assert_eq!(focus.text_offset().as_usize(), 0);
            assert_eq!(focus.atom_index(), 0);
            assert!(
                before.node(new_head).is_none(),
                "head paragraph must really be newly allocated"
            );
            for id in [f.table, f.row, f.cells[2]] {
                assert_eq!(s.document().node(id), before.node(id));
            }
            for id in [f.cells[0], f.cells[1]] {
                let old = before.node(id).unwrap();
                let new = s.document().node(id).unwrap();
                assert_eq!(new.kind(), old.kind());
                assert_eq!(new.attrs(), old.attrs());
                assert_eq!(new.content().as_children().unwrap().len(), 1);
            }
            let committed = s.document().clone();
            let after_selection = s.selection();
            assert_eq!(
                *events.borrow(),
                [Event::Document(
                    committed.revision().as_u64(),
                    after_selection
                )]
            );
            let collapsed = Snapshot::capture(&mut s, &events);
            assert_eq!(publish_through_writer(&mut s, &mut writer), Ok(None));
            assert_eq!(writer.calls, 1, "a collapsed repeat has no clipboard write");
            collapsed.assert_unchanged(&mut s, &events);
            let image = history_image(&mut s);
            assert_eq!(image.undo[0].group, HistoryGroup::Isolated);
            assert_eq!(image.undo[0].before, before_selection);
            assert_eq!(image.undo[0].after, after_selection);
            assert_eq!(s.undo(), Ok(SessionOutcome::DocumentChanged));
            assert_eq!(s.document().store(), before.store());
            assert_eq!(s.document().root(), before.root());
            assert_eq!(s.selection(), before_selection);
            assert_eq!(s.history_depths(), (depths.0, 1));
            let undo_revision = s.document().revision().as_u64();
            assert_eq!(s.redo(), Ok(SessionOutcome::DocumentChanged));
            assert_eq!(s.document().store(), committed.store());
            assert_eq!(s.document().root(), committed.root());
            assert_eq!(s.selection(), after_selection);
            assert_eq!(s.history_depths(), (depths.0 + 1, 0));
            assert_eq!(
                *events.borrow(),
                [
                    Event::Document(committed.revision().as_u64(), after_selection),
                    Event::Document(undo_revision, before_selection),
                    Event::Document(s.document().revision().as_u64(), after_selection),
                ]
            );
            // Typing after the Cut gets its own group and cannot absorb Cut.
            insert(&mut s, "z");
            let typed = s
                .document()
                .node(new_head)
                .unwrap()
                .content()
                .as_inline()
                .unwrap();
            assert_eq!(
                typed
                    .runs()
                    .iter()
                    .map(|run| run.text().as_str())
                    .collect::<String>(),
                "z"
            );
            assert_eq!(s.history_depths(), (depths.0 + 2, 0));
            s.undo().unwrap();
            assert_eq!(s.document().store(), committed.store());
            s.undo().unwrap();
            assert_eq!(s.document().store(), before.store());
            assert_eq!(s.selection(), before_selection);
        }
    }
}

#[test]
fn dropping_old_guard_then_reselecting_requires_fresh_projection_and_head() {
    let f = fixture(false);
    let mut s = session(&f, Some(Box::new(CutPolicy(Fault::None))));
    select_cells(&mut s, &f, false);
    let old = s.prepare_cut().unwrap().unwrap();
    assert_eq!(old.clipboard_slice().plain_text(), "alpha\nnested\nbeta");
    drop(old);
    s.set_cell_range_selection(f.cells[2], f.cells[2]).unwrap();
    let before = s.document().clone();
    let before_selection = s.selection();
    let events = listen(&mut s);
    let fresh = s.prepare_cut().unwrap().unwrap();
    assert_eq!(fresh.clipboard_slice().plain_text(), "untouched");
    assert_eq!(fresh.publish(), SessionOutcome::DocumentChanged);
    assert_eq!(events.borrow().len(), 1);
    let (_, focus) = s.selection().as_same_node_inline().unwrap();
    assert_eq!(s.document().parent_of(focus.node_id()), Some(f.cells[2]));
    for cell in [f.cells[0], f.cells[1]] {
        assert_eq!(s.document().node(cell), before.node(cell));
    }
    s.undo().unwrap();
    assert_eq!(s.document().store(), before.store());
    assert_eq!(s.selection(), before_selection);
}

#[test]
fn publish_forces_isolated_history_no_marks_and_no_input_rule_from_host_plan() {
    let f = fixture(false);
    let mut s = session(&f, Some(Box::new(CutPolicy(Fault::AncillaryPlan))));
    select_cells(&mut s, &f, false);
    install_rule_token(&mut s);
    let before = s.document().clone();
    let before_selection = s.selection();
    let depth = s.history_depths().0;
    // The incoming rule token is real; the marks sentinel deliberately checks
    // that the publisher clears prior marks as well as the plan's marks.
    s.stored_marks = Some(MarkSet::new([Mark::Bold]).unwrap());
    let events = listen(&mut s);
    let mut writer = Writer::seeded();
    assert_eq!(
        publish_through_writer(&mut s, &mut writer),
        Ok(Some(SessionOutcome::DocumentChanged)),
    );
    assert_eq!(writer.calls, 1);
    assert_eq!(events.borrow().len(), 1);
    assert_eq!(s.stored_marks(), None);
    assert!(s.input_rule_undo.is_none());
    assert!(!s.input_rule_undo_available());
    assert!(!s.history.typing_group_open());
    assert_eq!(history_image(&mut s).undo[0].group, HistoryGroup::Isolated);
    assert_eq!(s.history_depths(), (depth + 1, 0));
    s.undo().unwrap();
    assert_eq!(s.document().store(), before.store());
    assert_eq!(s.selection(), before_selection);
}
