//! Cross-block copy must preserve every covered atomic leaf in tree order.

use xiaomu_core::document::{
    AttrValue, InlineAtomContent, InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId,
    NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, NodeGap};
use xiaomu_runtime::clipboard::{
    ClipboardNode, ClipboardNodeContent, ClipboardSlice, decode_metadata, encode_metadata,
};
use xiaomu_runtime::session::{
    DocumentPosition, DocumentSelection, DocumentSession, EditIntent, SessionError,
};

fn attrs(label: &str) -> NodeAttrs {
    NodeAttrs::new([("data-keep".to_owned(), AttrValue::String(label.to_owned()))].into()).unwrap()
}

fn paragraph(builder: &mut NodeStoreBuilder, text: &str, mark: Mark) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            attrs(text),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::new([mark]).unwrap()).unwrap()])
                    .unwrap(),
            ),
        )
        .unwrap()
}

struct Fixture {
    document: XiaomuDocument,
    first: NodeId,
    image: NodeId,
    rule: NodeId,
    custom: NodeId,
    last: NodeId,
}

fn fixture() -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let before = builder
        .insert(
            NodeKind::Image,
            attrs("outside-before"),
            NodeContent::Atomic,
        )
        .unwrap();
    let first = paragraph(&mut builder, "前A", Mark::Bold);
    let image = builder
        .insert(
            NodeKind::Image,
            NodeAttrs::new(
                [
                    (
                        "src".to_owned(),
                        AttrValue::String("https://example.invalid/range.png".to_owned()),
                    ),
                    ("alt".to_owned(), AttrValue::String("范围图".to_owned())),
                    (
                        "host".to_owned(),
                        AttrValue::Object([("width".to_owned(), AttrValue::Integer(320))].into()),
                    ),
                ]
                .into(),
            )
            .unwrap(),
            NodeContent::Atomic,
        )
        .unwrap();
    let rule = builder
        .insert(NodeKind::HorizontalRule, attrs("rule"), NodeContent::Atomic)
        .unwrap();
    let custom = builder
        .insert(
            NodeKind::custom("host-card").unwrap(),
            attrs("opaque"),
            NodeContent::Atomic,
        )
        .unwrap();
    let last = paragraph(&mut builder, "B后", Mark::Italic);
    let item = builder
        .insert(
            NodeKind::ListItem,
            attrs("item"),
            NodeContent::children([first, image, rule, custom, last]),
        )
        .unwrap();
    let list = builder
        .insert(
            NodeKind::BulletList,
            attrs("list"),
            NodeContent::children([item]),
        )
        .unwrap();
    let after = builder
        .insert(NodeKind::Image, attrs("outside-after"), NodeContent::Atomic)
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([before, list, after]),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        first,
        image,
        rule,
        custom,
        last,
    }
}

fn point(document: &XiaomuDocument, node: NodeId, byte: usize) -> DocumentPosition {
    let inline = document.node(node).unwrap().content().as_inline().unwrap();
    DocumentPosition::Inline(InlinePoint::new(
        node,
        inline.offset_at(byte).unwrap(),
        0,
        CursorAffinity::Before,
    ))
}

fn copy(f: &Fixture, anchor: DocumentPosition, focus: DocumentPosition) -> ClipboardSlice {
    DocumentSession::new(f.document.clone(), DocumentSelection::new(anchor, focus))
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap()
}

fn list_children(slice: &ClipboardSlice) -> &[ClipboardNode] {
    assert_eq!(
        slice.roots().len(),
        1,
        "unselected sibling images must stay out"
    );
    let list = &slice.roots()[0];
    assert_eq!(list.kind(), &NodeKind::BulletList);
    assert_eq!(list.attrs(), &attrs("list"));
    let items = list.content().as_children().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind(), &NodeKind::ListItem);
    assert_eq!(items[0].attrs(), &attrs("item"));
    items[0].content().as_children().unwrap()
}

fn round_trip(slice: &ClipboardSlice) -> ClipboardSlice {
    let metadata = encode_metadata(slice).unwrap();
    assert!(
        metadata.contains("\"version\":4"),
        "existing Atomic wire version suffices"
    );
    let decoded = decode_metadata(slice.plain_text(), &metadata).unwrap();
    assert_eq!(&decoded, slice);
    decoded
}

#[test]
fn inline_range_preserves_intervening_atomic_blocks_marks_attrs_and_containers() {
    let f = fixture();
    let head = point(&f.document, f.first, "前".len());
    let tail = point(&f.document, f.last, "B".len());
    for (anchor, focus) in [(head, tail), (tail, head)] {
        let slice = round_trip(&copy(&f, anchor, focus));
        let children = list_children(&slice);
        assert_eq!(children.len(), 5);
        assert_eq!(children[0].content().as_inline().unwrap().text(), "A");
        assert_eq!(children[4].content().as_inline().unwrap().text(), "B");
        assert_eq!(
            children[0].content().as_inline().unwrap().runs()[0].marks(),
            &MarkSet::new([Mark::Bold]).unwrap()
        );
        assert_eq!(
            children[4].content().as_inline().unwrap().runs()[0].marks(),
            &MarkSet::new([Mark::Italic]).unwrap()
        );
        for (fragment, id) in children[1..4].iter().zip([f.image, f.rule, f.custom]) {
            let source = f.document.node(id).unwrap();
            assert_eq!(fragment.kind(), source.kind());
            assert_eq!(fragment.attrs(), source.attrs());
            assert!(matches!(fragment.content(), ClipboardNodeContent::Atomic));
        }
    }
}

#[test]
fn atomic_range_endpoints_are_inclusive_and_never_shorten_the_range() {
    let f = fixture();
    let first = point(&f.document, f.first, "前".len());
    let last = point(&f.document, f.last, "B".len());
    let image = DocumentPosition::Atomic(f.image);
    let custom = DocumentPosition::Atomic(f.custom);
    for (head, tail, kinds) in [
        (
            image,
            last,
            vec![
                NodeKind::Image,
                NodeKind::HorizontalRule,
                NodeKind::custom("host-card").unwrap(),
                NodeKind::Paragraph,
            ],
        ),
        (
            first,
            custom,
            vec![
                NodeKind::Paragraph,
                NodeKind::Image,
                NodeKind::HorizontalRule,
                NodeKind::custom("host-card").unwrap(),
            ],
        ),
        (
            image,
            custom,
            vec![
                NodeKind::Image,
                NodeKind::HorizontalRule,
                NodeKind::custom("host-card").unwrap(),
            ],
        ),
    ] {
        for (anchor, focus) in [(head, tail), (tail, head)] {
            let slice = round_trip(&copy(&f, anchor, focus));
            assert_eq!(
                list_children(&slice)
                    .iter()
                    .map(|node| node.kind().clone())
                    .collect::<Vec<_>>(),
                kinds
            );
        }
    }
}

#[test]
fn collapsed_atomic_and_single_inline_copy_keep_their_existing_shapes() {
    let f = fixture();
    let atomic = DocumentPosition::Atomic(f.image);
    let slice = round_trip(&copy(&f, atomic, atomic));
    assert_eq!(slice.roots().len(), 1);
    assert_eq!(slice.roots()[0].kind(), &NodeKind::Image);
    let slice = copy(
        &f,
        point(&f.document, f.first, 3),
        point(&f.document, f.first, 4),
    );
    assert_eq!(slice.roots()[0].kind(), &NodeKind::Paragraph);
    assert_eq!(slice.blocks()[0].inline().text(), "A");
    let caret = point(&f.document, f.first, 3);
    assert!(
        DocumentSession::new(f.document, DocumentSelection::collapsed(caret))
            .unwrap()
            .clipboard_slice()
            .unwrap()
            .is_none()
    );
}

#[test]
fn mixed_atomic_paste_fails_closed_without_changing_document_selection_or_history() {
    let f = fixture();
    let slice = round_trip(&copy(
        &f,
        point(&f.document, f.first, 0),
        point(&f.document, f.last, 4),
    ));
    let selection = DocumentSelection::collapsed(point(&f.document, f.first, 0));
    let mut target = DocumentSession::new(f.document.clone(), selection).unwrap();
    target
        .apply_intent(&EditIntent::InsertText {
            text: "kept".to_owned(),
        })
        .unwrap();
    let before_document = format!("{:?}", target.document());
    let before_selection = target.selection();
    let before_history = target.history_depths();
    assert_eq!(
        target.apply_intent(&EditIntent::PasteSlice { slice }),
        Err(SessionError::ClipboardAtomicUnsupported)
    );
    assert_eq!(format!("{:?}", target.document()), before_document);
    assert_eq!(target.selection(), before_selection);
    assert_eq!(target.history_depths(), before_history);
}

#[test]
fn unsupported_covered_content_fails_copy_instead_of_disappearing() {
    let mut builder = NodeStoreBuilder::new();
    let first = paragraph(&mut builder, "A", Mark::Bold);
    // Host custom nodes may legally use inline-atom content, but detached
    // clipboard blocks have no representation for that shape.
    let unknown = builder
        .insert(
            NodeKind::custom("opaque-block").unwrap(),
            attrs("unknown"),
            NodeContent::InlineAtom(InlineAtomContent::new("payload").unwrap()),
        )
        .unwrap();
    let last = paragraph(&mut builder, "B", Mark::Italic);
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, unknown, last]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let selection = DocumentSelection::new(point(&document, first, 0), point(&document, last, 1));
    let session = DocumentSession::new(document, selection).unwrap();
    let before = format!("{:?}", session.document());
    assert_eq!(
        session.clipboard_slice(),
        Err(SessionError::SelectionInvalid)
    );
    assert_eq!(format!("{:?}", session.document()), before);
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), (0, 0));
}

#[test]
fn gap_endpoints_still_reject_even_when_the_other_endpoint_is_atomic() {
    let f = fixture();
    let atomic = DocumentPosition::Atomic(f.image);
    let gap = DocumentPosition::Gap(NodeGap::new(f.document.root(), 3));
    for selection in [
        DocumentSelection::new(atomic, gap),
        DocumentSelection::new(gap, atomic),
    ] {
        let session = DocumentSession::new(f.document.clone(), selection).unwrap();
        assert_eq!(
            session.clipboard_slice(),
            Err(SessionError::SelectionInvalid)
        );
    }
}

#[test]
fn unselected_unsupported_leaf_does_not_block_copy_and_covered_empty_container_survives() {
    let mut builder = NodeStoreBuilder::new();
    let unsupported = builder
        .insert(
            NodeKind::custom("unselected-opaque").unwrap(),
            attrs("outside"),
            NodeContent::InlineAtom(InlineAtomContent::new("unselected").unwrap()),
        )
        .unwrap();
    let first = paragraph(&mut builder, "A", Mark::Bold);
    let empty = builder
        .insert(
            NodeKind::custom("empty-host-container").unwrap(),
            attrs("keep-empty"),
            NodeContent::children([]),
        )
        .unwrap();
    let last = paragraph(&mut builder, "B", Mark::Italic);
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([unsupported, first, empty, last]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let selection = DocumentSelection::new(point(&document, first, 0), point(&document, last, 1));
    let slice = DocumentSession::new(document, selection)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap();
    let slice = round_trip(&slice);
    assert_eq!(slice.roots().len(), 3);
    assert_eq!(
        slice.roots()[1].kind(),
        &NodeKind::custom("empty-host-container").unwrap()
    );
    assert_eq!(slice.roots()[1].attrs(), &attrs("keep-empty"));
    assert!(slice.roots()[1].content().as_children().unwrap().is_empty());
}
