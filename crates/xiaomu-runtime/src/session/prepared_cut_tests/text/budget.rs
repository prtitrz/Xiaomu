//! Whole-document admission must precede geometry and owned edit planning.
use super::*;

#[test]
fn oversized_tail_payload_or_deep_unselected_tree_never_enters_text_planner() {
    for fault in 0..3 {
        let mut builder = NodeStoreBuilder::new();
        let head = paragraph(&mut builder, "head");
        let tail = if fault == 0 {
            builder
                .insert(
                    NodeKind::Paragraph,
                    attrs("opaque", AttrValue::String("x".repeat(3 * 1024 * 1024))),
                    NodeContent::Inline(
                        InlineContent::new([TextRun::new("tail", MarkSet::empty()).unwrap()])
                            .unwrap(),
                    ),
                )
                .unwrap()
        } else if fault == 1 {
            paragraph(&mut builder, &"x".repeat(3 * 1024 * 1024))
        } else {
            paragraph(&mut builder, "tail")
        };
        let mut children = vec![head, tail];
        if fault == 2 {
            let mut nested = paragraph(&mut builder, "outside selection");
            for _ in 0..40 {
                nested = builder
                    .insert(
                        NodeKind::Quote,
                        NodeAttrs::empty(),
                        NodeContent::children([nested]),
                    )
                    .unwrap();
            }
            children.push(nested);
        }
        let document = finish(builder, children);
        let range = selection(&document, (head, 2, 0), (tail, 2, 0), false);
        let mut s = session(&document, range, None);
        let mut control = session(&document, range, None);
        sentinels(&mut s);
        sentinels(&mut control);
        let events = listen(&mut s);
        let before = Snapshot::capture(&mut s, &events);
        let entries = crate::session::prepared_text_cut::plan_entries();
        let mut writer = Writer::seeded();
        let error = publish_through_writer(&mut s, &mut writer).unwrap_err();
        assert!(error.to_string().contains("bounded projection budget"));
        assert_eq!(
            crate::session::prepared_text_cut::plan_entries(),
            entries,
            "budget refusal must happen before the recursive geometry and owned planner"
        );
        assert_eq!(writer, Writer::seeded());
        before.assert_unchanged(&mut s, &events);
        assert_future_allocation_matches(&mut s, &mut control);
    }
}

#[test]
fn affinity_only_range_rejects_but_a_cross_block_boundary_is_real_content() {
    let mut builder = NodeStoreBuilder::new();
    let head = paragraph(&mut builder, "abc");
    let tail = paragraph(&mut builder, "DEF");
    let document = finish(builder, vec![head, tail]);
    let before = point(&document, head, 1, 0);
    let after = InlinePoint::new(head, before.text_offset(), 0, CursorAffinity::After);
    let range = DocumentSelection::new(
        DocumentPosition::Inline(before),
        DocumentPosition::Inline(after),
    );
    let mut s = session(&document, range, None);
    let events = listen(&mut s);
    let snapshot = Snapshot::capture(&mut s, &events);
    let mut writer = Writer::seeded();
    assert!(publish_through_writer(&mut s, &mut writer).is_err());
    assert_eq!(writer, Writer::seeded());
    snapshot.assert_unchanged(&mut s, &events);

    for reverse in [false, true] {
        let range = selection(&document, (head, 3, 0), (tail, 0, 0), reverse);
        let mut s = session(&document, range, None);
        let mut writer = Writer::seeded();
        publish_through_writer(&mut s, &mut writer)
            .unwrap()
            .unwrap();
        assert_eq!(writer.text, "\n");
        assert_eq!(writer.calls, 1);
        assert_eq!(text(s.document(), head), "abcDEF");
        let after = s.document().clone();
        s.undo().unwrap();
        assert_eq!(s.document().store(), document.store());
        assert_eq!(s.selection(), range);
        s.redo().unwrap();
        assert_eq!(s.document().store(), after.store());
    }
}
