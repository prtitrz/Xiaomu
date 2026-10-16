//! Geometry that must reject before touching the clipboard or session.
use super::*;
use xiaomu_core::selection::NodeGap;

fn assert_rejected(document: &XiaomuDocument, range: DocumentSelection) {
    let mut s = session(document, range, None);
    sentinels(&mut s);
    let events = listen(&mut s);
    let before = Snapshot::capture(&mut s, &events);
    let mut writer = Writer::seeded();
    assert!(publish_through_writer(&mut s, &mut writer).is_err());
    assert_eq!(writer, Writer::seeded());
    before.assert_unchanged(&mut s, &events);
}

#[test]
fn intervening_atomic_and_unknown_leaves_are_never_silently_skipped() {
    for kind in [
        NodeKind::HorizontalRule,
        NodeKind::Image,
        NodeKind::custom("unknown-leaf").unwrap(),
    ] {
        let mut builder = NodeStoreBuilder::new();
        let head = paragraph(&mut builder, "abc");
        let barrier = builder
            .insert(kind, NodeAttrs::empty(), NodeContent::Atomic)
            .unwrap();
        let tail = paragraph(&mut builder, "xyz");
        let document = finish(builder, vec![head, barrier, tail]);
        for reverse in [false, true] {
            assert_rejected(
                &document,
                selection(&document, (head, 1, 0), (tail, 1, 0), reverse),
            );
        }
    }
}

#[test]
fn tables_are_barriers_including_outside_endpoints_and_same_cell() {
    let mut builder = NodeStoreBuilder::new();
    let head = paragraph(&mut builder, "abc");
    let left = paragraph(&mut builder, "cell one");
    let extra = paragraph(&mut builder, "same cell");
    let right = paragraph(&mut builder, "cell two");
    let cell1 = builder
        .insert(
            NodeKind::TableCell,
            NodeAttrs::empty(),
            NodeContent::children([left, extra]),
        )
        .unwrap();
    let cell2 = builder
        .insert(
            NodeKind::TableCell,
            NodeAttrs::empty(),
            NodeContent::children([right]),
        )
        .unwrap();
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([cell1, cell2]),
        )
        .unwrap();
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap();
    let tail = paragraph(&mut builder, "xyz");
    let document = finish(builder, vec![head, table, tail]);
    for (a, b) in [
        (head, tail),
        (head, left),
        (left, tail),
        (left, right),
        (left, extra),
        (left, left),
    ] {
        for reverse in [false, true] {
            assert_rejected(
                &document,
                selection(&document, (a, 0, 0), (b, 2, 0), reverse),
            );
        }
    }
    let range = DocumentSelection::cell_range(
        cell1,
        cell2,
        DocumentPosition::Inline(point(&document, left, 0, 0)),
    );
    assert_rejected(&document, range);
}

#[test]
fn all_gap_collapsed_and_node_sources_do_not_fall_back() {
    let mut builder = NodeStoreBuilder::new();
    let id = paragraph(&mut builder, "abc");
    let document = finish(builder, vec![id]);
    for range in [
        DocumentSelection::all(&document),
        DocumentSelection::node(&document, id).unwrap(),
        DocumentSelection::collapsed(DocumentPosition::Inline(point(&document, id, 1, 0))),
        DocumentSelection::new(NodeGap::new(document.root(), 0), point(&document, id, 2, 0)),
    ] {
        assert_rejected(&document, range);
    }
}

#[test]
fn plain_unicode_ranges_and_plain_cross_block_ranges_are_exact() {
    for cross in [false, true] {
        for reverse in [false, true] {
            let mut builder = NodeStoreBuilder::new();
            let head = paragraph(&mut builder, "a中z");
            let tail = paragraph(&mut builder, "qéw");
            let document = finish(builder, vec![head, tail]);
            let range = selection(
                &document,
                (head, 1, 0),
                if cross { (tail, 1, 0) } else { (head, 4, 0) },
                reverse,
            );
            let mut s = session(&document, range, None);
            let mut writer = Writer::seeded();
            publish_through_writer(&mut s, &mut writer)
                .unwrap()
                .unwrap();
            assert_eq!(writer.text, if cross { "中z\nq" } else { "中" });
            assert_eq!(text(s.document(), head), if cross { "aéw" } else { "az" });
            let after = s.document().clone();
            s.undo().unwrap();
            assert_eq!(s.document().store(), document.store());
            assert_eq!(s.selection(), range);
            s.redo().unwrap();
            assert_eq!(s.document().store(), after.store());
        }
    }
}

#[test]
fn source_policy_error_is_not_fallback() {
    let mut builder = NodeStoreBuilder::new();
    let id = paragraph(&mut builder, "abc");
    let document = finish(builder, vec![id]);
    let range = selection(&document, (id, 0, 0), (id, 2, 0), false);
    let mut s = DocumentSession::new_with_policy(
        document,
        range,
        Box::new(TextPolicy {
            enabled: true,
            protected: None,
            refuse_source: true,
        }),
    )
    .unwrap();
    let events = listen(&mut s);
    let snapshot = Snapshot::capture(&mut s, &events);
    assert!(s.prepare_cut().is_err());
    snapshot.assert_unchanged(&mut s, &events);
}

#[test]
fn structural_cut_plan_has_priority_over_text_hook() {
    struct StructuralFirst;
    impl SessionPolicy for StructuralFirst {
        fn prepare_cut(
            &self,
            context: SessionContext<'_>,
        ) -> Result<Option<EditPlan>, PolicyError> {
            CutPolicy(Fault::None).prepare_cut(context)
        }
        fn prepare_text_cut(&self, _: SessionContext<'_>) -> Result<bool, PolicyError> {
            panic!("structural plan must win without consulting text hook")
        }
        fn clipboard_export_spec(
            &self,
            context: SessionContext<'_>,
            purpose: ClipboardExportPurpose,
        ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
            CutPolicy(Fault::None).clipboard_export_spec(context, purpose)
        }
    }
    let f = fixture(false);
    let mut s = super::super::session(&f, Some(Box::new(StructuralFirst)));
    select_cells(&mut s, &f, false);
    let mut writer = Writer::seeded();
    assert_eq!(
        publish_through_writer(&mut s, &mut writer),
        Ok(Some(SessionOutcome::DocumentChanged))
    );
    assert_eq!(writer.calls, 1);
}

#[test]
fn default_task_text_cut_remains_legacy() {
    let mut builder = NodeStoreBuilder::new();
    let id = paragraph(&mut builder, "task");
    let item = builder
        .insert(
            NodeKind::TaskItem,
            NodeAttrs::empty(),
            NodeContent::children([id]),
        )
        .unwrap();
    let list = builder
        .insert(
            NodeKind::TaskList,
            NodeAttrs::empty(),
            NodeContent::children([item]),
        )
        .unwrap();
    let document = finish(builder, vec![list]);
    let range = selection(&document, (id, 0, 0), (id, 2, 0), false);
    let mut s = DocumentSession::new(document, range).unwrap();
    assert!(s.prepare_cut().unwrap().is_none());
    assert_eq!(
        s.clipboard_slice_for(ClipboardExportPurpose::Cut)
            .unwrap()
            .unwrap()
            .plain_text(),
        "ta"
    );
}

#[test]
fn cross_list_cut_keeps_unselected_item_and_exact_selected_structure() {
    let mut builder = NodeStoreBuilder::new();
    let head = paragraph(&mut builder, "head");
    let tail = paragraph(&mut builder, "tail");
    let untouched = paragraph(&mut builder, "untouched");
    let selected_item = builder
        .insert(
            NodeKind::ListItem,
            attrs("item", AttrValue::String("selected".into())),
            NodeContent::children([tail]),
        )
        .unwrap();
    let remaining_item = builder
        .insert(
            NodeKind::ListItem,
            attrs("item", AttrValue::String("remaining".into())),
            NodeContent::children([untouched]),
        )
        .unwrap();
    let list = builder
        .insert(
            NodeKind::BulletList,
            attrs("list", AttrValue::Null),
            NodeContent::children([selected_item, remaining_item]),
        )
        .unwrap();
    let document = finish(builder, vec![head, list]);
    let range = selection(&document, (head, 2, 0), (tail, 2, 0), true);
    let mut s = session(&document, range, None);
    let mut writer = Writer::seeded();
    publish_through_writer(&mut s, &mut writer)
        .unwrap()
        .unwrap();
    assert_eq!(writer.text, "ad\nta");
    assert_eq!(text(s.document(), head), "heil");
    assert_eq!(
        s.document().node(remaining_item),
        document.node(remaining_item)
    );
    assert_eq!(s.document().node(untouched), document.node(untouched));
    assert_eq!(
        s.document().node(list).unwrap().attrs(),
        document.node(list).unwrap().attrs()
    );
    assert_eq!(
        s.document()
            .node(list)
            .unwrap()
            .content()
            .as_children()
            .unwrap(),
        &[remaining_item]
    );
    let crate::clipboard::ClipboardMetadataDecode::Valid(slice) =
        decode_metadata_checked(&writer.text, &writer.metadata)
    else {
        panic!("lossless metadata")
    };
    assert_eq!(slice.roots().len(), 2);
    assert_eq!(slice.roots()[1].kind(), &NodeKind::BulletList);
    assert_eq!(
        slice.roots()[1].attrs(),
        document.node(list).unwrap().attrs()
    );
    let after = s.document().clone();
    s.undo().unwrap();
    assert_eq!(s.document().store(), document.store());
    assert_eq!(s.selection(), range);
    s.redo().unwrap();
    assert_eq!(s.document().store(), after.store());
}

#[test]
fn empty_containers_are_not_skipped_by_text_cut() {
    let mut builder = NodeStoreBuilder::new();
    let head = paragraph(&mut builder, "head");
    let empty = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([]),
        )
        .unwrap();
    let tail = paragraph(&mut builder, "tail");
    let document = finish(builder, vec![head, empty, tail]);
    assert_rejected(
        &document,
        selection(&document, (head, 2, 0), (tail, 2, 0), false),
    );
}

#[test]
fn export_policy_missing_spec_and_budget_fail_without_live_mutation() {
    struct ExportFailure(u8);
    impl SessionPolicy for ExportFailure {
        fn prepare_text_cut(&self, _: SessionContext<'_>) -> Result<bool, PolicyError> {
            Ok(true)
        }
        fn clipboard_export_spec(
            &self,
            _: SessionContext<'_>,
            _: ClipboardExportPurpose,
        ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
            match self.0 {
                0 => Err(PolicyError::new("export refused")),
                1 => Ok(None),
                _ => Ok(Some(ClipboardExportSpec::new())),
            }
        }
    }
    for fault in 0..3 {
        let mut builder = NodeStoreBuilder::new();
        let source_text = if fault == 2 {
            "x".repeat(3 * 1024 * 1024)
        } else {
            "abc".into()
        };
        let id = paragraph(&mut builder, &source_text);
        let document = finish(builder, vec![id]);
        let range = selection(&document, (id, 0, 0), (id, 2, 0), false);
        let mut s = DocumentSession::new_with_policy(
            document.clone(),
            range,
            Box::new(ExportFailure(fault)),
        )
        .unwrap();
        let mut control =
            DocumentSession::new_with_policy(document, range, Box::new(ExportFailure(fault)))
                .unwrap();
        sentinels(&mut s);
        sentinels(&mut control);
        let events = listen(&mut s);
        let before = Snapshot::capture(&mut s, &events);
        let mut writer = Writer::seeded();
        assert!(publish_through_writer(&mut s, &mut writer).is_err());
        assert_eq!(writer, Writer::seeded());
        before.assert_unchanged(&mut s, &events);
        assert_future_allocation_matches(&mut s, &mut control);
    }
}
