//! Runtime edit/paste/history measurement with semantic assertions outside timers.
use crate::{
    fixtures::{self, leaf_text},
    metrics::{Timings, checkpoint},
};
use serde_json::{Value, json};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{EditIntent, SessionOutcome};

/// Runs one fixed scenario. Samples exclude warmup; history retention is separate.
pub fn run(name: &str, samples: usize, history_edits: usize) -> Value {
    assert!((1..=1000).contains(&samples) && (1..=4096).contains(&history_edits));
    let mut memory = vec![checkpoint("process_start", &[])];
    let mut times = Timings::default();
    let fixture = times.measure("fixture_build", true, || fixtures::build(name));
    memory.push(checkpoint("fixture", &[]));
    let mut sessions = times.measure("session_mount", true, || fixture.sessions());
    memory.push(checkpoint("mounted", &sessions));
    let before = leaf_text(&fixture.document, fixture.target);
    let paste = "paste 中文 🌲\n".repeat(256);
    for sample in 0..samples + 5 {
        let record = sample >= 5;
        for (index, session) in sessions.iter_mut().enumerate() {
            for (label, intent) in [
                ("insert", EditIntent::InsertText { text: "界".into() }),
                (
                    "paste_4k",
                    EditIntent::PasteText {
                        text: paste.clone(),
                    },
                ),
            ] {
                let after = times
                    .measure(label, record, || session.apply_intent(&intent))
                    .unwrap();
                assert_eq!(after, SessionOutcome::DocumentChanged);
                assert!(leaf_text(session.document(), fixture.target).len() > before.len());
                let selection_after = session.selection();
                times
                    .measure(&format!("{label}_undo"), record, || session.undo())
                    .unwrap();
                assert_eq!(session.document().store(), fixture.document.store());
                times
                    .measure(&format!("{label}_redo"), record, || session.redo())
                    .unwrap();
                assert_eq!(session.selection(), selection_after);
                session.undo().unwrap();
            }
            assert_eq!(
                session.document().store(),
                fixture.document.store(),
                "editor {index}"
            );
            if let Some(table) = fixture.table {
                let transaction = Transaction::new(TransactionOrigin::UserInput).with_step(
                    TransactionStep::InsertTableColumnLogical {
                        table,
                        index: 1,
                        cell_kinds: vec![
                            xiaomu_core::document::NodeKind::TableCell;
                            if name == "table-20x20" { 20 } else { 50 }
                        ],
                    },
                );
                times
                    .measure("table_insert_column", record, || {
                        session.apply(&transaction)
                    })
                    .unwrap();
                times
                    .measure("table_column_undo", record, || session.undo())
                    .unwrap();
                assert_eq!(session.document().store(), fixture.document.store());
            }
        }
    }
    memory.push(checkpoint("latency_cycles", &sessions));
    drop(sessions);
    let mut sessions = fixture.sessions();
    memory.push(checkpoint("history_empty", &sessions));
    for edit in 0..history_edits {
        for session in &mut sessions {
            times
                .measure("grouped_typing", true, || {
                    session.apply_intent(&EditIntent::InsertText { text: "界".into() })
                })
                .unwrap();
        }
        if edit + 1 == history_edits / 4 {
            memory.push(checkpoint("history_quarter", &sessions));
        }
    }
    memory.push(checkpoint("history_full", &sessions));
    for session in &mut sessions {
        assert_eq!(session.history_depths(), (1, 0));
        assert_eq!(
            leaf_text(session.document(), fixture.target),
            format!("{}{before}", "界".repeat(history_edits))
        );
        times
            .measure("grouped_undo", true, || session.undo())
            .unwrap();
        assert_eq!(session.document().store(), fixture.document.store());
    }
    memory.push(checkpoint("history_undone", &sessions));
    for session in &mut sessions {
        times
            .measure("grouped_redo", true, || session.redo())
            .unwrap();
        assert_eq!(
            leaf_text(session.document(), fixture.target),
            format!("{}{before}", "界".repeat(history_edits))
        );
    }
    memory.push(checkpoint("history_redone", &sessions));
    // Destroy editor owners. RSS can stay high because the allocator retains pages.
    drop(sessions);
    memory.push(checkpoint("sessions_dropped", &[]));
    // Isolated paste owns one undo entry per operation, unlike grouped typing.
    let mut isolated = fixture.sessions();
    memory.push(checkpoint("isolated_empty", &isolated));
    for _ in 0..history_edits {
        for session in &mut isolated {
            session
                .apply_intent(&EditIntent::PasteText { text: "界".into() })
                .unwrap();
        }
    }
    memory.push(checkpoint("isolated_full", &isolated));
    for session in &mut isolated {
        assert_eq!(session.history_depths(), (history_edits, 0));
        for _ in 0..history_edits {
            session.undo().unwrap();
        }
        assert_eq!(session.document().store(), fixture.document.store());
        for _ in 0..history_edits {
            session.redo().unwrap();
        }
        assert_eq!(
            leaf_text(session.document(), fixture.target),
            format!("{}{before}", "界".repeat(history_edits))
        );
    }
    drop(isolated);
    memory.push(checkpoint("isolated_dropped", &[]));
    let result = json!({"schema": 1, "kind": "runtime", "scenario": name, "samples_per_editor": samples,
        "history_edits_per_editor": history_edits, "editors": fixture.editors,
        "nodes_per_document": fixture.document.store().len(), "target_text_bytes": before.len(), "paste_bytes": paste.len(),
        "timings": times.json(), "memory": memory});
    drop(fixture);
    result
}
