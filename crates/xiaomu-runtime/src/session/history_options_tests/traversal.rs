//! Publicly reachable traversal: no direct canonical or history mutations.

use super::*;

#[test]
fn recorded_defaults_and_capture_diverge_on_repeated_cross_block_text_all_and_cell_moves() {
    struct DefaultPolicy;
    impl SessionPolicy for DefaultPolicy {}

    let f = fixture("abcdef");
    for mode in 0..3 {
        let original = caret(f.first, 2);
        let mut s = match mode {
            0 => DocumentSession::new(f.document.clone(), original).unwrap(),
            1 => DocumentSession::new_with_policy(
                f.document.clone(),
                original,
                Box::new(DefaultPolicy),
            )
            .unwrap(),
            _ => DocumentSession::new_with_policy(
                f.document.clone(),
                original,
                Box::new(OptionsPolicy(capture_options())),
            )
            .unwrap(),
        };
        let capture = mode == 2;
        let events = listen(&mut s);
        insert(&mut s, "X");
        let edited = s.document().clone();
        let recorded_after = caret(f.first, 3);
        assert_eq!(s.selection(), recorded_after);
        let cross = DocumentSelection::new(
            InlinePoint::new(f.second, offset(3), 0, CursorAffinity::Before),
            point(f.first, 1),
        );
        select(&mut s, cross);
        traverse(
            &mut s,
            &events,
            Direction::Undo,
            &f.document,
            original,
            (0, 1),
        );

        let all = DocumentSelection::all(s.document());
        select(&mut s, all);
        traverse(
            &mut s,
            &events,
            Direction::Redo,
            &edited,
            if capture { cross } else { recorded_after },
            (1, 0),
        );
        let reverse_text = DocumentSelection::new(point(f.first, 6), point(f.first, 1));
        select(&mut s, reverse_text);
        traverse(
            &mut s,
            &events,
            Direction::Undo,
            &f.document,
            if capture { all } else { original },
            (0, 1),
        );

        let cells = cell_range(&f);
        select(&mut s, cells);
        traverse(
            &mut s,
            &events,
            Direction::Redo,
            &edited,
            if capture {
                reverse_text
            } else {
                recorded_after
            },
            (1, 0),
        );
        select(&mut s, caret(f.second, 1));
        traverse(
            &mut s,
            &events,
            Direction::Undo,
            &f.document,
            if capture { cells } else { original },
            (0, 1),
        );
        assert_eq!(
            s.history_options().selection_mode(),
            if capture {
                HistorySelectionMode::CaptureOnTraversal
            } else {
                HistorySelectionMode::Recorded
            }
        );
    }
}

#[test]
fn captured_selections_belong_to_each_entry_in_coalesced_and_two_level_history() {
    let f = fixture("abcdef");
    let mut s = session(&f, capture_options());
    let events = listen(&mut s);
    insert(&mut s, "a");
    insert(&mut s, "b");
    assert_eq!(s.history_depths(), (1, 0), "adjacent typing is one entry");
    let first_edit = s.document().clone();
    select(&mut s, caret(f.second, 0));
    insert(&mut s, "Z");
    let second_edit = s.document().clone();
    assert_eq!(s.history_depths(), (2, 0));

    let pre_undo_second = caret(f.first, 4);
    select(&mut s, pre_undo_second);
    traverse(
        &mut s,
        &events,
        Direction::Undo,
        &first_edit,
        caret(f.second, 0),
        (1, 1),
    );
    let pre_undo_first = caret(f.second, 2);
    select(&mut s, pre_undo_first);
    traverse(
        &mut s,
        &events,
        Direction::Undo,
        &f.document,
        caret(f.first, 0),
        (0, 2),
    );

    let pre_redo_first = DocumentSelection::all(s.document());
    select(&mut s, pre_redo_first);
    traverse(
        &mut s,
        &events,
        Direction::Redo,
        &first_edit,
        pre_undo_first,
        (1, 1),
    );
    let pre_redo_second = cell_range(&f);
    select(&mut s, pre_redo_second);
    traverse(
        &mut s,
        &events,
        Direction::Redo,
        &second_edit,
        pre_undo_second,
        (2, 0),
    );

    let new_pre_undo_second = caret(f.first, 7);
    select(&mut s, new_pre_undo_second);
    traverse(
        &mut s,
        &events,
        Direction::Undo,
        &first_edit,
        pre_redo_second,
        (1, 1),
    );
    let new_pre_undo_first = DocumentSelection::new(point(f.first, 6), point(f.first, 1));
    select(&mut s, new_pre_undo_first);
    traverse(
        &mut s,
        &events,
        Direction::Undo,
        &f.document,
        pre_redo_first,
        (0, 2),
    );
    traverse(
        &mut s,
        &events,
        Direction::Redo,
        &first_edit,
        new_pre_undo_first,
        (1, 1),
    );
    traverse(
        &mut s,
        &events,
        Direction::Redo,
        &second_edit,
        new_pre_undo_second,
        (2, 0),
    );
}

#[test]
fn redo_captures_source_only_caret_without_validating_or_mapping_it_into_short_target() {
    let f = fixture("abcdefghij");
    let mut s = session(&f, capture_options());
    let events = listen(&mut s);
    let deleted_range = DocumentSelection::new(point(f.first, 1), point(f.first, 9));
    select(&mut s, deleted_range);
    assert_eq!(
        s.apply_intent(&EditIntent::Delete),
        Ok(SessionOutcome::DocumentChanged)
    );
    assert_eq!(plain(&s, f.first), "aj");
    let short = s.document().clone();
    let old_caret = caret(f.first, 1);
    assert_eq!(s.selection(), old_caret);
    assert_eq!(
        s.apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold }),
        Ok(SessionOutcome::NoChange)
    );
    assert!(s.stored_marks().is_some());
    traverse(
        &mut s,
        &events,
        Direction::Undo,
        &f.document,
        deleted_range,
        (0, 1),
    );
    let source_only = caret(f.first, 9);
    assert!(source_only.validate(&short).is_err());
    select(&mut s, source_only);
    assert_eq!(
        s.apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold }),
        Ok(SessionOutcome::NoChange)
    );
    assert!(s.stored_marks().is_some());
    traverse(&mut s, &events, Direction::Redo, &short, old_caret, (1, 0));
    traverse(
        &mut s,
        &events,
        Direction::Undo,
        &f.document,
        source_only,
        (0, 1),
    );
}

#[test]
fn redo_of_whole_table_removal_captures_cells_only_for_next_undo_and_restores_ids() {
    let f = fixture("abcdef");
    let mut s = session(&f, capture_options());
    let events = listen(&mut s);
    let outside = caret(f.first, 2);
    select(&mut s, outside);
    assert_eq!(
        s.apply(&transaction().with_step(TransactionStep::RemoveNode { node: f.table })),
        Ok(SessionOutcome::DocumentChanged)
    );
    let without_table = s.document().clone();
    for node in std::iter::once(&f.table)
        .chain(&f.cells)
        .chain(&f.cell_paragraphs)
    {
        assert!(s.document().node(*node).is_none());
    }
    traverse(
        &mut s,
        &events,
        Direction::Undo,
        &f.document,
        outside,
        (0, 1),
    );
    let source_cells = cell_range(&f);
    assert!(source_cells.validate(&without_table).is_err());
    select(&mut s, source_cells);
    traverse(
        &mut s,
        &events,
        Direction::Redo,
        &without_table,
        outside,
        (1, 0),
    );
    traverse(
        &mut s,
        &events,
        Direction::Undo,
        &f.document,
        source_cells,
        (0, 1),
    );
    let range = s.selection().active_cell_range().unwrap();
    assert_eq!(range.anchor(), f.cells[3]);
    assert_eq!(range.focus(), f.cells[0]);
    for node in std::iter::once(&f.table)
        .chain(&f.cells)
        .chain(&f.cell_paragraphs)
    {
        assert_eq!(s.document().node(*node), f.document.node(*node));
    }
}
