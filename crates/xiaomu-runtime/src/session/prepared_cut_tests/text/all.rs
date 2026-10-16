//! Exact All is closed structural content, never a widened inline selection.
use super::*;
use crate::clipboard::ClipboardSourceBoundary;
use xiaomu_core::document::HeadingLevel;
use xiaomu_core::selection::NodeGap;

fn rich_document(list_first: bool) -> (XiaomuDocument, NodeId) {
    let mut b = NodeStoreBuilder::new();
    let heading = b
        .insert(
            NodeKind::Heading(HeadingLevel::new(2).unwrap()),
            attrs("heading", AttrValue::Null),
            NodeContent::Inline(
                InlineContent::new([
                    TextRun::new("é🙂", MarkSet::new([Mark::Underline]).unwrap()).unwrap(),
                ])
                .unwrap(),
            ),
        )
        .unwrap();
    let (mixed, atoms) = mixed(&mut b, "a中z");
    let empty = paragraph(&mut b, "");
    let quote = b
        .insert(
            NodeKind::Quote,
            attrs("quote", AttrValue::String("exact".into())),
            NodeContent::children([mixed, empty]),
        )
        .unwrap();
    let item = b
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([quote]),
        )
        .unwrap();
    let list = b
        .insert(
            NodeKind::OrderedList,
            attrs("start", AttrValue::Integer(7)),
            NodeContent::children([item]),
        )
        .unwrap();
    let children = if list_first {
        vec![list, heading]
    } else {
        vec![heading, list]
    };
    let root = b
        .insert(
            NodeKind::Document,
            attrs("root", AttrValue::String("preserved".into())),
            NodeContent::children(children),
        )
        .unwrap();
    (XiaomuDocument::new(root, b.finish()).unwrap(), atoms[1])
}

fn assert_default_paragraph(s: &DocumentSession, source: &XiaomuDocument) -> NodeId {
    let document = s.document();
    assert_eq!(document.root(), source.root());
    let root = document.node(document.root()).unwrap();
    assert_eq!(root.attrs(), source.node(source.root()).unwrap().attrs());
    let children = root.content().as_children().unwrap();
    assert_eq!(children.len(), 1);
    let paragraph = document.node(children[0]).unwrap();
    assert_eq!(paragraph.kind(), &NodeKind::Paragraph);
    assert_eq!(paragraph.attrs(), &NodeAttrs::empty());
    assert_eq!(paragraph.content(), &NodeContent::empty_inline());
    assert_eq!(document.store().iter().count(), 2);
    assert!(
        source.node(paragraph.id()).is_none(),
        "replacement is newly allocated"
    );
    assert_eq!(
        s.selection(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(paragraph.id()))
    );
    paragraph.id()
}

#[test]
fn exact_all_keeps_closed_unicode_marks_and_wrappers_then_restores_original_history() {
    for list_first in [false, true] {
        for reverse in [false, true] {
            let (document, _) = rich_document(list_first);
            let all = DocumentSelection::all(&document);
            let all = if reverse {
                DocumentSelection::new(all.focus(), all.anchor())
            } else {
                all
            };
            let source = crate::clipboard::slice_selection(&document, all)
                .unwrap()
                .unwrap();
            let mut s = session(&document, all, None);
            sentinels(&mut s);
            let events = listen(&mut s);
            let mut writer = Writer::seeded();
            assert_eq!(
                publish_through_writer(&mut s, &mut writer),
                Ok(Some(SessionOutcome::DocumentChanged))
            );
            assert_eq!(writer.calls, 1);
            assert_eq!(
                writer.text,
                if list_first {
                    "a\n\n\n中z\n\né🙂"
                } else {
                    "é🙂\na\n\n\n中z\n"
                }
            );
            let ClipboardMetadataDecode::Valid(decoded) =
                decode_metadata_checked(&writer.text, &writer.metadata)
            else {
                panic!("closed All metadata must round trip");
            };
            assert!(decoded.is_closed());
            assert_eq!(
                decoded.source_boundary(),
                Some(ClipboardSourceBoundary::WholeRoots)
            );
            assert_eq!(
                decoded.source_boundary().unwrap().open_depths(),
                Some((0, 0))
            );
            assert_eq!(decoded.roots(), source.roots());
            assert_default_paragraph(&s, &document);
            assert_eq!(s.history_depths(), (1, 0));
            let history = history_image(&mut s);
            assert_eq!(history.undo[0].before, all);
            assert_eq!(history.undo[0].group, HistoryGroup::Isolated);
            assert!(s.stored_marks().is_none());
            assert!(!s.input_rule_undo_available());
            assert_eq!(
                events.borrow().as_slice(),
                &[Event::Document(
                    s.document().revision().as_u64(),
                    s.selection()
                )]
            );
            let after = s.document().clone();
            let caret = s.selection();
            s.undo().unwrap();
            assert_eq!(s.document().store(), document.store());
            assert_eq!(s.selection(), all);
            assert!(s.selection().is_all(s.document()));
            assert!(
                s.stored_marks().is_none(),
                "history does not restore pending marks"
            );
            s.redo().unwrap();
            assert_eq!(s.document().store(), after.store());
            assert_eq!(s.selection(), caret);
            insert(&mut s, "next");
            assert_eq!(
                s.history_depths(),
                (2, 0),
                "Cut is isolated from later typing"
            );
        }
    }
}

#[test]
fn empty_paragraph_all_is_structural_but_empty_root_all_is_not_a_cut() {
    let mut b = NodeStoreBuilder::new();
    let original = paragraph(&mut b, "");
    let document = finish(b, vec![original]);
    let all = DocumentSelection::all(&document);
    assert!(!all.is_collapsed());
    let mut s = session(&document, all, None);
    let mut writer = Writer::seeded();
    publish_through_writer(&mut s, &mut writer)
        .unwrap()
        .unwrap();
    assert_eq!(writer.text, "");
    assert_eq!(writer.calls, 1);
    let ClipboardMetadataDecode::Valid(slice) = decode_metadata_checked("", &writer.metadata)
    else {
        panic!("empty text retains nonempty closed structure");
    };
    assert!(slice.is_closed());
    assert_eq!(slice.roots().len(), 1);
    assert_default_paragraph(&s, &document);
    assert_eq!(s.history_depths(), (1, 0));
    s.undo().unwrap();
    assert_eq!(s.document().store(), document.store());
    assert_eq!(s.selection(), all);

    let empty = finish(NodeStoreBuilder::new(), vec![]);
    let all = DocumentSelection::all(&empty);
    assert!(all.is_collapsed());
    super::geometry::assert_rejected(&empty, all);
}

#[test]
fn atom_only_task_content_and_empty_code_keep_complete_closed_payload() {
    for with_extension in [false, true] {
        let mut b = NodeStoreBuilder::new();
        let mut placements = Vec::new();
        for kind in [
            AtomKind::hard_break(),
            if with_extension {
                AtomKind::new("token").unwrap()
            } else {
                AtomKind::hard_break()
            },
            AtomKind::hard_break(),
        ] {
            let (attrs, content) = if kind.is_hard_break() {
                (NodeAttrs::empty(), InlineAtomContent::hard_break())
            } else {
                (
                    attrs("atom", AttrValue::Null),
                    InlineAtomContent::new("fallback").unwrap(),
                )
            };
            let atom = b
                .insert(
                    NodeKind::InlineAtom(kind),
                    attrs,
                    NodeContent::InlineAtom(
                        content.with_marks(MarkSet::new([Mark::Italic]).unwrap()),
                    ),
                )
                .unwrap();
            placements.push(InlineAtomPlacement::new(atom, TextOffset::ZERO));
        }
        let leaf = b
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(InlineContent::with_atoms([], placements).unwrap()),
            )
            .unwrap();
        let item = b
            .insert(
                NodeKind::TaskItem,
                attrs("checked", AttrValue::Bool(true)),
                NodeContent::children([leaf]),
            )
            .unwrap();
        let list = b
            .insert(
                NodeKind::TaskList,
                NodeAttrs::empty(),
                NodeContent::children([item]),
            )
            .unwrap();
        let code = b
            .insert(
                NodeKind::CodeBlock,
                NodeAttrs::empty(),
                NodeContent::empty_inline(),
            )
            .unwrap();
        let document = finish(b, vec![list, code]);
        let all = DocumentSelection::all(&document);
        let expected = crate::clipboard::slice_selection(&document, all)
            .unwrap()
            .unwrap();
        let mut s = session(&document, all, None);
        let mut writer = Writer::seeded();
        publish_through_writer(&mut s, &mut writer)
            .unwrap()
            .unwrap();
        assert_eq!(writer.text, "\n\n\n\n");
        let ClipboardMetadataDecode::Valid(slice) =
            decode_metadata_checked(&writer.text, &writer.metadata)
        else {
            panic!("closed atoms-only metadata");
        };
        assert!(slice.is_closed());
        assert_eq!(slice.roots(), expected.roots());
        assert_default_paragraph(&s, &document);
        s.undo().unwrap();
        assert_eq!(s.document().store(), document.store());
        assert_eq!(s.selection(), all);
    }
}

#[test]
fn all_candidate_rejection_and_guard_drop_preserve_redo_transients_and_allocator() {
    for reject in [false, true] {
        let (document, protected) = rich_document(true);
        let all = DocumentSelection::all(&document);
        let mut s = session(&document, all, reject.then_some(protected));
        let mut control = session(&document, all, reject.then_some(protected));
        for s in [&mut s, &mut control] {
            s.apply(&transaction()).unwrap();
            s.apply(&transaction()).unwrap();
            s.undo().unwrap();
            sentinels(s);
        }
        assert_eq!(s.history_depths(), (1, 1));
        let events = listen(&mut s);
        let before = Snapshot::capture(&mut s, &events);
        let mut writer = Writer::seeded();
        if reject {
            let error = publish_through_writer(&mut s, &mut writer).unwrap_err();
            assert!(error.to_string().contains("protected token"));
        } else {
            let prepared = s.prepare_cut().unwrap().unwrap();
            assert!(prepared.clipboard_slice().is_closed());
            drop(prepared);
        }
        assert_eq!(writer, Writer::seeded());
        before.assert_unchanged(&mut s, &events);
        assert_future_allocation_matches(&mut s, &mut control);
    }
}

#[test]
fn all_rejects_atomic_table_empty_and_unknown_blocks_without_expanding_other_gaps() {
    for kind in [
        NodeKind::Image,
        NodeKind::HorizontalRule,
        NodeKind::custom("opaque").unwrap(),
    ] {
        let mut b = NodeStoreBuilder::new();
        let head = paragraph(&mut b, "head");
        let barrier = b
            .insert(kind, NodeAttrs::empty(), NodeContent::Atomic)
            .unwrap();
        let tail = paragraph(&mut b, "tail");
        let document = finish(b, vec![head, barrier, tail]);
        super::geometry::assert_rejected(&document, DocumentSelection::all(&document));
    }
    for content in [NodeContent::children([]), NodeContent::empty_inline()] {
        let mut b = NodeStoreBuilder::new();
        let unknown = b
            .insert(
                NodeKind::custom("opaque").unwrap(),
                NodeAttrs::empty(),
                content,
            )
            .unwrap();
        let document = finish(b, vec![unknown]);
        super::geometry::assert_rejected(&document, DocumentSelection::all(&document));
    }
    let mut b = NodeStoreBuilder::new();
    let empty = b
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([]),
        )
        .unwrap();
    let document = finish(b, vec![empty]);
    super::geometry::assert_rejected(&document, DocumentSelection::all(&document));
    let f = fixture(false);
    super::geometry::assert_rejected(&f.document, DocumentSelection::all(&f.document));

    let (document, _) = rich_document(true);
    let root = document.root();
    let list = document
        .node(root)
        .unwrap()
        .content()
        .as_children()
        .unwrap()[0];
    for selection in [
        DocumentSelection::new(NodeGap::new(root, 0), NodeGap::new(root, 1)),
        DocumentSelection::new(NodeGap::new(list, 0), NodeGap::new(list, 1)),
        DocumentSelection::node(&document, list).unwrap(),
    ] {
        assert!(!selection.is_all(&document));
        super::geometry::assert_rejected(&document, selection);
    }
}

#[test]
fn all_budget_refusal_precedes_owned_planning() {
    for fault in 0..3 {
        let mut b = NodeStoreBuilder::new();
        let content = if fault == 0 {
            "x".repeat(3 * 1024 * 1024)
        } else {
            "text".into()
        };
        let mut node = paragraph(&mut b, &content);
        for _ in 0..if fault == 1 { 40 } else { 0 } {
            node = b
                .insert(
                    NodeKind::Quote,
                    NodeAttrs::empty(),
                    NodeContent::children([node]),
                )
                .unwrap();
        }
        let root = b
            .insert(
                NodeKind::Document,
                if fault == 2 {
                    attrs(
                        "root-budget",
                        AttrValue::String("x".repeat(3 * 1024 * 1024)),
                    )
                } else {
                    NodeAttrs::empty()
                },
                NodeContent::children([node]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, b.finish()).unwrap();
        let all = DocumentSelection::all(&document);
        let mut s = session(&document, all, None);
        let mut control = session(&document, all, None);
        sentinels(&mut s);
        sentinels(&mut control);
        let events = listen(&mut s);
        let before = Snapshot::capture(&mut s, &events);
        let entries = crate::session::prepared_text_cut::plan_entries();
        let mut writer = Writer::seeded();
        assert!(
            publish_through_writer(&mut s, &mut writer)
                .unwrap_err()
                .to_string()
                .contains("bounded projection budget")
        );
        assert_eq!(crate::session::prepared_text_cut::plan_entries(), entries);
        assert_eq!(writer, Writer::seeded());
        before.assert_unchanged(&mut s, &events);
        assert_future_allocation_matches(&mut s, &mut control);
    }
}

#[test]
fn full_inline_range_stays_open_and_default_all_does_not_opt_in() {
    let mut b = NodeStoreBuilder::new();
    let p = paragraph(&mut b, "é🙂");
    let quote = b
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([p]),
        )
        .unwrap();
    let document = finish(b, vec![quote]);
    let range = selection(&document, (p, 0, 0), (p, 6, 0), false);
    assert!(!range.is_all(&document));
    let mut s = session(&document, range, None);
    let mut writer = Writer::seeded();
    publish_through_writer(&mut s, &mut writer)
        .unwrap()
        .unwrap();
    let ClipboardMetadataDecode::Valid(slice) =
        decode_metadata_checked(&writer.text, &writer.metadata)
    else {
        panic!("metadata");
    };
    assert!(!slice.is_closed());
    assert_eq!(slice.roots()[0].kind(), &NodeKind::Paragraph);
    assert!(s.document().node(quote).is_some());
    assert!(s.document().node(p).is_some());
    let all = DocumentSelection::all(&document);
    let mut legacy = DocumentSession::new(document, all).unwrap();
    let events = listen(&mut legacy);
    let before = Snapshot::capture(&mut legacy, &events);
    assert!(legacy.prepare_cut().unwrap().is_none());
    before.assert_unchanged(&mut legacy, &events);
}
