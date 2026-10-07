//! A whole-node plan can allocate an inline replacement without consuming live IDs.

use super::*;

#[test]
fn allocating_node_cut_drop_and_rejection_preserve_next_id_and_full_state() {
    for fault in [NodeFault::Allocate, NodeFault::AllocateAdmission] {
        for atomic in [false, true] {
            let f = node_fixture(NodeKind::Image, false);
            let mut s = node_session(&f, atomic, fault);
            let mut control = node_session(&f, atomic, fault);
            sentinels(&mut s);
            sentinels(&mut control);
            let events = listen(&mut s);
            let before = Snapshot::capture(&mut s, &events);
            let mut writer = Writer::seeded();
            match fault {
                NodeFault::Allocate => drop(s.prepare_cut().unwrap().unwrap()),
                NodeFault::AllocateAdmission => {
                    assert!(matches!(
                        publish_through_writer(&mut s, &mut writer),
                        Err(SessionError::Policy(_))
                    ));
                }
                _ => unreachable!(),
            }
            assert_eq!(writer, Writer::seeded());
            before.assert_unchanged(&mut s, &events);
            assert_future_allocation_matches(&mut s, &mut control);
        }
    }
}

#[test]
fn allocating_node_cut_redo_reuses_exact_prepared_replacement_identity() {
    for atomic in [false, true] {
        let f = node_fixture(NodeKind::Image, false);
        let mut s = node_session(&f, atomic, NodeFault::Allocate);
        let before = s.document().clone();
        let before_selection = s.selection();
        let events = listen(&mut s);
        let mut writer = Writer::seeded();
        assert_eq!(
            publish_through_writer(&mut s, &mut writer),
            Ok(Some(SessionOutcome::DocumentChanged))
        );
        assert_eq!(writer.calls, 1);
        assert_eq!(events.borrow().len(), 1);
        let (point, _) = s.selection().as_same_node_inline().unwrap();
        let replacement = point.node_id();
        assert!(before.node(replacement).is_none());
        assert!(
            s.document()
                .node(replacement)
                .unwrap()
                .attrs()
                .get("cut-replacement")
                .is_some()
        );
        assert!(s.document().node(f.selected).is_none());
        let after = s.document().clone();
        let after_selection = s.selection();
        s.undo().unwrap();
        assert_eq!(s.document().store(), before.store());
        assert_eq!(s.selection(), before_selection);
        s.redo().unwrap();
        assert_eq!(s.document().store(), after.store());
        assert_eq!(s.selection(), after_selection);
        assert_eq!(
            s.selection().as_same_node_inline().unwrap().0.node_id(),
            replacement
        );
        assert_eq!(writer.calls, 1);
    }
}
