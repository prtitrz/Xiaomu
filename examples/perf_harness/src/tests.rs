use crate::{fixtures, runtime};
use xiaomu_core::document::NodeKind;
use xiaomu_runtime::session::EditIntent;

#[test]
fn complex_table_workloads_really_have_nested_tables_headers_and_spans() {
    for name in ["table-20x20", "table-50x20"] {
        let fixture = fixtures::build(name);
        let tables: Vec<_> = fixture
            .document
            .store()
            .iter()
            .filter(|n| *n.kind() == NodeKind::Table)
            .collect();
        assert!(tables.len() > 1);
        for table in tables {
            let grid = fixture.document.table_grid(table.id()).unwrap();
            assert!(grid.origins().any(|o| o.rowspan() > 1));
            assert!(grid.origins().any(|o| o.colspan() > 1));
        }
        assert!(
            fixture
                .document
                .store()
                .iter()
                .any(|n| *n.kind() == NodeKind::TableHeader)
        );
    }
}

#[test]
fn shared_initial_snapshot_does_not_share_editor_changes_or_history() {
    let fixture = fixtures::build("multi-4x1k");
    let mut sessions = fixture.sessions();
    sessions[2]
        .apply_intent(&EditIntent::InsertText {
            text: "独立".into(),
        })
        .unwrap();
    for (index, session) in sessions.iter().enumerate() {
        assert_eq!(session.history_depths(), (usize::from(index == 2), 0));
        assert_eq!(
            session.document().store() == fixture.document.store(),
            index != 2
        );
    }
    sessions[2].undo().unwrap();
    assert_eq!(sessions[2].document().store(), fixture.document.store());
}

#[test]
fn baseline_smoke_exercises_grouped_and_isolated_history_with_raw_samples() {
    let result = runtime::run("table-20x20", 2, 4);
    assert_eq!(
        result["timings"]["insert"]["raw_us"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let memory = result["memory"].as_array().unwrap();
    for (name, depth) in [("history_full", 1), ("isolated_full", 4)] {
        let stage = memory.iter().find(|v| v["stage"] == name).unwrap();
        assert_eq!(stage["history_depths"][0][0], depth);
    }
}
