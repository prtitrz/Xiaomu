//! First-class hard-break values share the ordinary atom identity and placement
//! system. Literal LF text and same-name extension kinds remain distinct.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use xiaomu_core::{
    Error,
    document::{
        AtomKind, AttrValue, InlineAtomContent, InlineAtomPlacement, InlineContent, LinkAttributes,
        LinkMark, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
        StringAttribute, TextRun, TextStyleAttributes, TextStyleMark, XiaomuDocument,
    },
    mapping::{MapBias, MappedPosition},
    selection::{CursorAffinity, InlinePoint},
    text::{TextBuffer, TextOffset},
    transaction::{Transaction, TransactionOrigin, TransactionStep},
};

fn exact_marks() -> MarkSet {
    MarkSet::new([
        Mark::Bold,
        Mark::Code,
        Mark::Link(LinkMark::from_attributes(
            LinkAttributes::default()
                .with_href("原值🙂".into())
                .with_target(StringAttribute::Null)
                .with_rel("".into())
                .with_title("中文 e\u{301}".into()),
        )),
        Mark::TextStyle(TextStyleMark::from_attributes(
            TextStyleAttributes::default()
                .with_color(StringAttribute::Null)
                .with_font_family("\"宋体\", serif".into())
                .with_font_size("calc(1em + 2px)".into()),
        )),
    ])
    .unwrap()
}

fn attrs(key: &str, value: AttrValue) -> NodeAttrs {
    NodeAttrs::new(BTreeMap::from([(key.to_owned(), value)])).unwrap()
}

fn inline(document: &XiaomuDocument, paragraph: NodeId) -> &InlineContent {
    document
        .node(paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
}

fn gap(document: &XiaomuDocument, paragraph: NodeId, ordinal: usize) -> InlinePoint {
    InlinePoint::new(
        paragraph,
        inline(document, paragraph).offset_at("中".len()).unwrap(),
        ordinal,
        CursorAffinity::Before,
    )
}

fn fixture() -> (XiaomuDocument, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let extension = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::new("hardBreak").unwrap()),
            attrs("unknown", AttrValue::Null),
            NodeContent::InlineAtom(InlineAtomContent::new("\n").unwrap()),
        )
        .unwrap();
    let text = TextBuffer::from_string("中\n🙂尾".to_owned());
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new(text.clone(), MarkSet::new([Mark::Italic]).unwrap()).unwrap()],
                    [InlineAtomPlacement::new(
                        extension,
                        text.offset_at("中".len()).unwrap(),
                    )],
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        paragraph,
        extension,
    )
}

fn insert_hard_break(at: InlinePoint, marks: MarkSet) -> Transaction {
    Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::InsertInlineAtom {
        at,
        kind: AtomKind::hard_break(),
        attrs: NodeAttrs::empty(),
        content: InlineAtomContent::hard_break().with_marks(marks),
    })
}

#[test]
fn built_in_identity_is_not_a_reserved_extension_string() {
    let builtin = AtomKind::hard_break();
    let extension = AtomKind::new(builtin.as_str()).unwrap();
    assert_eq!(builtin.as_str(), "hardBreak");
    assert!(builtin.is_hard_break());
    assert!(!extension.is_hard_break());
    assert_ne!(builtin, extension);
    assert_eq!(HashSet::from([builtin.clone(), extension.clone()]).len(), 2);
    assert_eq!(BTreeSet::from([builtin, extension]).len(), 2);

    for key in [
        "mention",
        "hardBreak",
        "hard_break",
        "HardBreak",
        " 原值🙂 ",
    ] {
        let kind = AtomKind::new(key).unwrap();
        assert_eq!(kind.as_str(), key);
        assert!(!kind.is_hard_break());
        assert_eq!(AtomKind::new(kind.as_str()).unwrap(), kind);
    }
    assert_eq!(AtomKind::new(""), Err(Error::InvalidAtomKind));
    assert_eq!(AtomKind::new(" \n\t"), Err(Error::InvalidAtomKind));
}

#[test]
fn content_marks_preserve_exact_values_and_remain_host_neutral() {
    let marks = exact_marks();
    let unmarked = InlineAtomContent::hard_break();
    assert_eq!(unmarked.fallback_text(), "\n");
    assert!(unmarked.marks().is_empty());
    assert_eq!(unmarked, InlineAtomContent::new("\n").unwrap());

    let marked = unmarked.clone().with_marks(marks.clone());
    assert_eq!(marked.marks(), &marks);
    assert_eq!(
        marked.marks().len(),
        4,
        "Code does not exclude other Core marks"
    );
    assert_ne!(marked, unmarked);
    assert_eq!(marked.clone().with_marks(MarkSet::empty()), unmarked);
    assert_eq!(marked.clone(), marked);

    let unicode = InlineAtomContent::new("@张🙂 e\u{301}\r\n").unwrap();
    assert!(unicode.marks().is_empty());
    assert_eq!(unicode.fallback_text(), "@张🙂 e\u{301}\r\n");
    assert_eq!(InlineAtomContent::new(""), Err(Error::InvalidAtomFallback));
}

#[test]
fn every_text_style_attribute_state_participates_in_atom_equality() {
    let states = [
        StringAttribute::Missing,
        StringAttribute::Null,
        StringAttribute::Value(String::new()),
        StringAttribute::Value("原值🙂; calc(100% + 2px)".into()),
    ];
    let mut payloads = Vec::new();
    for color in &states {
        for family in &states {
            for size in &states {
                let mark = TextStyleMark::from_attributes(
                    TextStyleAttributes::default()
                        .with_color(color.clone())
                        .with_font_family(family.clone())
                        .with_font_size(size.clone()),
                );
                let marks = MarkSet::new([Mark::TextStyle(mark)]).unwrap();
                let payload = InlineAtomContent::hard_break().with_marks(marks.clone());
                assert_eq!(payload.marks(), &marks);
                assert!(!payloads.contains(&payload));
                payloads.push(payload);
            }
        }
    }
    assert_eq!(payloads.len(), 64);
}

#[test]
fn builder_enforces_exact_lf_and_empty_attrs_only_for_builtin() {
    let mut builder = NodeStoreBuilder::new();
    let expected_id = builder.peek_next_id();
    for fallback in ["\r", "\r\n", "\n\n", " \n", "\u{2028}", "\u{2029}", "中🙂"] {
        assert_eq!(
            builder.insert(
                NodeKind::InlineAtom(AtomKind::hard_break()),
                NodeAttrs::empty(),
                NodeContent::InlineAtom(InlineAtomContent::new(fallback).unwrap()),
            ),
            Err(Error::InvalidHardBreak)
        );
        assert_eq!(builder.peek_next_id(), expected_id);
    }
    for attributes in [
        attrs("unknown", AttrValue::Null),
        attrs("marks", AttrValue::List(Vec::new())),
        attrs("fallback", AttrValue::String("\n".into())),
    ] {
        assert_eq!(
            builder.insert(
                NodeKind::InlineAtom(AtomKind::hard_break()),
                attributes,
                NodeContent::InlineAtom(InlineAtomContent::hard_break()),
            ),
            Err(Error::InvalidHardBreak)
        );
    }
    assert_eq!(builder.peek_next_id(), expected_id);
    assert!(builder.is_empty());
    assert_eq!(
        builder.insert(
            NodeKind::InlineAtom(AtomKind::hard_break()),
            NodeAttrs::empty(),
            NodeContent::Atomic,
        ),
        Err(Error::InvalidNodeContent)
    );
    assert_eq!(
        builder
            .insert(
                NodeKind::InlineAtom(AtomKind::hard_break()),
                NodeAttrs::empty(),
                NodeContent::InlineAtom(InlineAtomContent::hard_break().with_marks(exact_marks())),
            )
            .unwrap(),
        expected_id
    );

    let custom_content = InlineAtomContent::new("\r\n中🙂").unwrap();
    let custom_attrs = attrs("unknown", AttrValue::Null);
    let extension = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::new("hardBreak").unwrap()),
            custom_attrs.clone(),
            NodeContent::InlineAtom(custom_content.clone()),
        )
        .unwrap();
    let store = builder.finish();
    let node = store.get(extension).unwrap();
    assert_eq!(node.attrs(), &custom_attrs);
    assert_eq!(node.content().as_inline_atom(), Some(&custom_content));
}

#[test]
fn builtin_insertion_coexists_with_literal_lf_and_same_name_extension() {
    let (document, paragraph, extension) = fixture();
    let at = gap(&document, paragraph, 1);
    let applied = insert_hard_break(at, exact_marks())
        .apply_with_changes(&document)
        .unwrap();
    let next = applied.document();
    next.validate().unwrap();
    let old_inline = inline(&document, paragraph);
    let new_inline = inline(next, paragraph);
    assert_eq!(new_inline.runs(), old_inline.runs());
    assert_eq!(new_inline.len_bytes(), "中\n🙂尾".len());
    assert_eq!(new_inline.runs()[0].text().as_str(), "中\n🙂尾");
    assert_eq!(new_inline.atoms().len(), 2);
    assert_eq!(new_inline.atom_count_at(at.text_offset()), 2);
    assert_eq!(new_inline.atoms()[0].atom(), extension);
    let atom = new_inline.atoms()[1].atom();
    assert_ne!(atom, extension);
    assert_eq!(new_inline.atoms()[1].text_offset(), at.text_offset());
    assert_eq!(next.parent_of(atom), Some(paragraph));
    assert_eq!(next.node(extension), document.node(extension));
    assert_eq!(
        next.node(atom).unwrap().kind(),
        &NodeKind::InlineAtom(AtomKind::hard_break())
    );
    let payload = next.node(atom).unwrap().content().as_inline_atom().unwrap();
    assert_eq!(payload.fallback_text(), "\n");
    assert_eq!(payload.marks(), &exact_marks());
    assert_ne!(payload.marks(), new_inline.runs()[0].marks());

    assert_eq!(
        applied.changes().map_inline_point(at, MapBias::Start),
        MappedPosition::Mapped(at)
    );
    assert_eq!(
        applied.changes().map_inline_point(at, MapBias::End),
        MappedPosition::Mapped(gap(next, paragraph, 2))
    );
    let undo = applied.inverse().apply_with_changes(next).unwrap();
    assert_eq!(undo.document().store(), document.store());
    let redo = undo.inverse().apply(undo.document()).unwrap();
    assert_eq!(redo.store(), next.store());
    assert_eq!(redo.node(atom), next.node(atom));
}

#[test]
fn consecutive_breaks_keep_distinct_marks_identity_and_inverse_order() {
    let (document, paragraph, extension) = fixture();
    let first = insert_hard_break(gap(&document, paragraph, 0), exact_marks())
        .apply_with_changes(&document)
        .unwrap();
    let second = insert_hard_break(gap(first.document(), paragraph, 1), MarkSet::empty())
        .apply_with_changes(first.document())
        .unwrap();
    let before = second.document();
    let placements = inline(before, paragraph).atoms();
    assert_eq!(placements.len(), 3);
    let atom = placements[0].atom();
    let other = placements[1].atom();
    assert_ne!(atom, other);
    assert_eq!(placements[2].atom(), extension);
    assert_eq!(placements[0].text_offset(), placements[1].text_offset());
    assert!(
        before
            .node(other)
            .unwrap()
            .content()
            .as_inline_atom()
            .unwrap()
            .marks()
            .is_empty()
    );

    let removed = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveInlineAtom { atom })
        .apply_with_changes(before)
        .unwrap();
    assert_eq!(
        inline(removed.document(), paragraph).atoms()[0].atom(),
        other
    );
    assert_eq!(
        removed
            .changes()
            .map_inline_point(gap(before, paragraph, 2), MapBias::End),
        MappedPosition::Mapped(gap(removed.document(), paragraph, 1))
    );
    let restored = removed.inverse().apply(removed.document()).unwrap();
    assert_eq!(restored.store(), before.store());
    assert_eq!(restored.node(atom), before.node(atom));

    let subtree = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveNode { node: paragraph })
        .apply_with_changes(before)
        .unwrap();
    let restored_subtree = subtree.inverse().apply(subtree.document()).unwrap();
    assert_eq!(restored_subtree.store(), before.store());
}

#[test]
fn node_store_equality_includes_atom_marks_and_typed_kind() {
    fn store(kind: AtomKind, content: InlineAtomContent) -> xiaomu_core::document::NodeStore {
        let mut builder = NodeStoreBuilder::new();
        builder
            .insert(
                NodeKind::InlineAtom(kind),
                NodeAttrs::empty(),
                NodeContent::InlineAtom(content),
            )
            .unwrap();
        builder.finish()
    }
    let unmarked = store(AtomKind::hard_break(), InlineAtomContent::hard_break());
    let marked = store(
        AtomKind::hard_break(),
        InlineAtomContent::hard_break().with_marks(exact_marks()),
    );
    let extension = store(
        AtomKind::new("hardBreak").unwrap(),
        InlineAtomContent::hard_break(),
    );
    assert_ne!(unmarked, marked);
    assert_ne!(unmarked, extension);
    assert_eq!(marked.clone(), marked);
}

#[test]
fn insert_step_rejects_invalid_builtin_payload_atomically() {
    let (document, paragraph, _) = fixture();
    let original = document.store().clone();
    for (attributes, content) in [
        (NodeAttrs::empty(), InlineAtomContent::new("\r\n").unwrap()),
        (
            attrs("unknown", AttrValue::Null),
            InlineAtomContent::hard_break(),
        ),
    ] {
        let transaction = Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::InsertInlineAtom {
                at: gap(&document, paragraph, 0),
                kind: AtomKind::hard_break(),
                attrs: NodeAttrs::empty(),
                content: InlineAtomContent::hard_break().with_marks(exact_marks()),
            })
            .with_step(TransactionStep::InsertInlineAtom {
                at: gap(&document, paragraph, 1),
                kind: AtomKind::hard_break(),
                attrs: attributes,
                content,
            });
        assert!(matches!(
            transaction.apply(&document),
            Err(Error::InvalidHardBreak)
        ));
        assert_eq!(document.store(), &original);
    }
}

#[test]
fn existing_builtin_rejects_unknown_attrs_but_extension_stays_extensible() {
    let (document, paragraph, extension) = fixture();
    let inserted = insert_hard_break(gap(&document, paragraph, 0), exact_marks())
        .apply_with_changes(&document)
        .unwrap();
    let atom = inline(inserted.document(), paragraph).atoms()[0].atom();
    let attributes = attrs("future", AttrValue::String("原值🙂".into()));
    let rejected =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::SetNodeAttrs {
            node: atom,
            attrs: attributes.clone(),
        });
    assert!(matches!(
        rejected.apply(inserted.document()),
        Err(Error::InvalidHardBreak)
    ));
    assert!(inserted.document().node(atom).unwrap().attrs().is_empty());
    let accepted = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeAttrs {
            node: extension,
            attrs: attributes.clone(),
        })
        .apply_with_changes(inserted.document())
        .unwrap();
    assert_eq!(
        accepted.document().node(extension).unwrap().attrs(),
        &attributes
    );
    assert_eq!(
        accepted
            .inverse()
            .apply(accepted.document())
            .unwrap()
            .store(),
        inserted.document().store()
    );
}

#[test]
fn hard_break_placement_uses_valid_utf8_boundaries_and_no_fake_text_byte() {
    let (document, paragraph, _) = fixture();
    let invalid_utf8 = TextBuffer::from_string("abc".into()).offset_at(1).unwrap();
    let bad_gap = InlinePoint::new(paragraph, invalid_utf8, 0, CursorAffinity::Before);
    assert!(matches!(
        insert_hard_break(bad_gap, MarkSet::empty()).apply(&document),
        Err(Error::InvalidTextBoundary { offset: 1 })
    ));
    let bad_ordinal = gap(&document, paragraph, 2);
    assert!(matches!(
        insert_hard_break(bad_ordinal, MarkSet::empty()).apply(&document),
        Err(Error::InvalidSelection)
    ));

    let mut builder = NodeStoreBuilder::new();
    let atom = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::hard_break()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::hard_break()),
        )
        .unwrap();
    let content =
        InlineContent::with_atoms([], [InlineAtomPlacement::new(atom, TextOffset::ZERO)]).unwrap();
    assert_eq!(content.len_bytes(), 0);
    assert!(!content.is_empty());
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(content),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    document.validate().unwrap();
    for ordinal in 0..=1 {
        InlinePoint::new(paragraph, TextOffset::ZERO, ordinal, CursorAffinity::Before)
            .validate(&document)
            .unwrap();
    }
}
