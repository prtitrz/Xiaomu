//! The All-only default paragraph seam is borrowed, bounded and transactional.
use super::*;
use std::cell::Cell;

struct Defaults {
    attrs: NodeAttrs,
    calls: Rc<Cell<usize>>,
    enabled: bool,
    reject: bool,
    protected: Option<NodeId>,
}

impl SessionPolicy for Defaults {
    fn prepare_text_cut(&self, _: SessionContext<'_>) -> Result<bool, PolicyError> {
        Ok(self.enabled)
    }
    fn clipboard_export_spec(
        &self,
        _: SessionContext<'_>,
        purpose: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        assert_eq!(purpose, ClipboardExportPurpose::Cut);
        Ok(Some(ClipboardExportSpec::new().with_text_projection(
            ClipboardTextProjection::TextBetweenLfV1,
        )))
    }
    fn text_cut_all_paragraph_attrs(
        &self,
        context: SessionContext<'_>,
    ) -> Result<Option<&NodeAttrs>, PolicyError> {
        assert!(context.selection().is_all(context.document()));
        assert_eq!(context.selection(), context.original_selection());
        self.calls.set(self.calls.get() + 1);
        if self.reject {
            Err(PolicyError::new("synthetic paragraph defaults refusal"))
        } else {
            Ok(Some(&self.attrs))
        }
    }
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if self.protected.is_some_and(|id| document.node(id).is_none()) {
            Err(PolicyError::new("synthetic protected source"))
        } else {
            Ok(())
        }
    }
}

fn defaults() -> NodeAttrs {
    NodeAttrs::new(
        [
            ("custom-null".into(), AttrValue::Null),
            ("custom-flag".into(), AttrValue::Bool(false)),
            (
                "custom-values".into(),
                AttrValue::List(vec![
                    AttrValue::Integer(5),
                    AttrValue::String("default".into()),
                ]),
            ),
        ]
        .into(),
    )
    .unwrap()
}

fn configured(
    document: &XiaomuDocument,
    selection: DocumentSelection,
    attrs: NodeAttrs,
    reject: bool,
    protected: Option<NodeId>,
    enabled: bool,
) -> (DocumentSession, Rc<Cell<usize>>) {
    let calls = Rc::new(Cell::new(0));
    let s = DocumentSession::new_with_policy(
        document.clone(),
        selection,
        Box::new(Defaults {
            attrs,
            calls: calls.clone(),
            enabled,
            reject,
            protected,
        }),
    )
    .unwrap();
    (s, calls)
}

fn source() -> (XiaomuDocument, NodeId) {
    let mut b = NodeStoreBuilder::new();
    let head = paragraph(&mut b, "é🙂");
    let tail = paragraph(&mut b, "tail");
    (finish(b, vec![head, tail]), head)
}

#[test]
fn custom_all_defaults_apply_once_only_to_replacement_and_round_trip_history() {
    let (document, _) = source();
    let all = DocumentSelection::all(&document);
    let original = crate::clipboard::slice_selection(&document, all)
        .unwrap()
        .unwrap();
    let attrs = defaults();
    let (mut s, calls) = configured(&document, all, attrs.clone(), false, None, true);
    sentinels(&mut s);
    let events = listen(&mut s);
    let mut writer = Writer::seeded();
    publish_through_writer(&mut s, &mut writer)
        .unwrap()
        .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(writer.calls, 1);
    let ClipboardMetadataDecode::Valid(slice) =
        decode_metadata_checked(&writer.text, &writer.metadata)
    else {
        panic!("metadata");
    };
    assert!(slice.is_closed());
    assert_eq!(
        slice.roots(),
        original.roots(),
        "replacement defaults must not rewrite source"
    );
    let caret = s.selection();
    let (point, end) = caret.as_same_node_inline().unwrap();
    assert_eq!(point, end);
    let paragraph = s.document().node(point.node_id()).unwrap();
    assert_eq!(paragraph.kind(), &NodeKind::Paragraph);
    assert_eq!(paragraph.attrs(), &attrs);
    assert_eq!(paragraph.content(), &NodeContent::empty_inline());
    assert_eq!(s.document().node_count(), 2);
    assert_eq!(s.document().root(), document.root());
    assert_eq!(s.history_depths(), (1, 0));
    assert_eq!(events.borrow().len(), 1);
    assert!(s.stored_marks().is_none());
    assert!(!s.input_rule_undo_available());
    let after = s.document().clone();
    s.undo().unwrap();
    assert_eq!(s.document().store(), document.store());
    assert_eq!(s.selection(), all);
    s.redo().unwrap();
    assert_eq!(s.document().store(), after.store());
    assert_eq!(s.selection(), caret);
    assert_eq!(
        calls.get(),
        1,
        "publication/history do not call the defaults hook again"
    );
}

#[test]
fn all_defaults_refusal_candidate_refusal_and_drop_preserve_every_live_state() {
    for fault in 0..3 {
        let (document, head) = source();
        let all = DocumentSelection::all(&document);
        let (mut s, calls) = configured(
            &document,
            all,
            defaults(),
            fault == 0,
            (fault == 1).then_some(head),
            true,
        );
        let (mut control, _) = configured(
            &document,
            all,
            defaults(),
            fault == 0,
            (fault == 1).then_some(head),
            true,
        );
        for s in [&mut s, &mut control] {
            s.apply(&transaction()).unwrap();
            s.apply(&transaction()).unwrap();
            s.undo().unwrap();
            sentinels(s);
        }
        let events = listen(&mut s);
        let before = Snapshot::capture(&mut s, &events);
        let mut writer = Writer::seeded();
        if fault == 2 {
            drop(s.prepare_cut().unwrap().unwrap());
        } else {
            assert!(publish_through_writer(&mut s, &mut writer).is_err());
        }
        assert_eq!(calls.get(), 1);
        assert_eq!(writer, Writer::seeded());
        before.assert_unchanged(&mut s, &events);
        assert_future_allocation_matches(&mut s, &mut control);
    }
}

#[test]
fn all_defaults_are_bounded_with_source_before_owned_plan_or_clone() {
    for fault in 0..3 {
        let mut b = NodeStoreBuilder::new();
        let source_text = if fault == 2 {
            "x".repeat(1_500_000)
        } else {
            "text".into()
        };
        let head = paragraph(&mut b, &source_text);
        let document = finish(b, vec![head]);
        let mut value = AttrValue::String("x".repeat(if fault == 0 {
            3 * 1024 * 1024
        } else if fault == 2 {
            1_500_000
        } else {
            1
        }));
        if fault == 1 {
            for _ in 0..40 {
                value = AttrValue::List(vec![value]);
            }
        }
        let all = DocumentSelection::all(&document);
        let attrs = attrs("oversized-defaults", value);
        let (mut s, calls) = configured(&document, all, attrs.clone(), false, None, true);
        let (mut control, _) = configured(&document, all, attrs, false, None, true);
        sentinels(&mut s);
        sentinels(&mut control);
        let events = listen(&mut s);
        let before = Snapshot::capture(&mut s, &events);
        let plans = crate::session::prepared_text_cut::all_plan_entries();
        let mut writer = Writer::seeded();
        let error = publish_through_writer(&mut s, &mut writer).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("paragraph exceeds bounded projection budget")
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(crate::session::prepared_text_cut::all_plan_entries(), plans);
        assert_eq!(writer, Writer::seeded());
        before.assert_unchanged(&mut s, &events);
        assert_future_allocation_matches(&mut s, &mut control);
    }
}

#[test]
fn all_defaults_hook_never_runs_for_inline_disabled_or_unadmitted_source() {
    for fault in 0..5 {
        let mut b = NodeStoreBuilder::new();
        let text = if fault == 4 {
            "x".repeat(3 * 1024 * 1024)
        } else {
            "text".into()
        };
        let head = paragraph(&mut b, &text);
        let mut children = vec![head];
        if fault == 3 {
            children.push(
                b.insert(
                    NodeKind::HorizontalRule,
                    NodeAttrs::empty(),
                    NodeContent::Atomic,
                )
                .unwrap(),
            );
        }
        let document = finish(b, children);
        let selected = match fault {
            0 => selection(&document, (head, 0, 0), (head, 2, 0), false),
            2 => DocumentSelection::node(&document, head).unwrap(),
            _ => DocumentSelection::all(&document),
        };
        let (mut s, calls) = configured(&document, selected, defaults(), true, None, fault != 1);
        match fault {
            0 => {
                drop(s.prepare_cut().unwrap().unwrap());
            }
            1 => {
                assert!(s.prepare_cut().unwrap().is_none());
            }
            _ => {
                assert!(s.prepare_cut().is_err());
            }
        }
        assert_eq!(calls.get(), 0);
    }
}
