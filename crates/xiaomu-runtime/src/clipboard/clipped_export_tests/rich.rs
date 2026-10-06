//! Complete cell subtrees survive right/bottom clipping and disappear at left/top.

use super::*;
use xiaomu_core::document::{
    AtomKind, HeadingLevel, InlineAtomContent, InlineAtomPlacement, Mark, MarkSet,
};
use xiaomu_core::text::TextOffset;

pub(super) fn children(builder: &mut NodeStoreBuilder) -> Vec<NodeId> {
    let hard_break = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::hard_break()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(
                InlineAtomContent::hard_break().with_marks(MarkSet::new([Mark::Italic]).unwrap()),
            ),
        )
        .unwrap();
    let heading = builder
        .insert(
            NodeKind::Heading(HeadingLevel::new(2).unwrap()),
            attrs(&[("headingOpaque", opaque())]),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new("甲\r\n乙\t🙂", MarkSet::new([Mark::Bold]).unwrap()).unwrap()],
                    [InlineAtomPlacement::new(hard_break, TextOffset::ZERO)],
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let image = builder
        .insert(
            NodeKind::Image,
            attrs(&[
                (
                    "src",
                    AttrValue::String("https://example.test/image".into()),
                ),
                ("alt", AttrValue::String("unprojected image alt".into())),
                ("imageOpaque", opaque()),
            ]),
            NodeContent::Atomic,
        )
        .unwrap();
    let inner_leaf = paragraph(builder, "nested");
    let inner_cell = builder
        .insert(
            NodeKind::TableHeader,
            attrs(&[("innerCell", AttrValue::Null)]),
            NodeContent::children([inner_leaf]),
        )
        .unwrap();
    let inner_row = builder
        .insert(
            NodeKind::TableRow,
            attrs(&[("innerRow", opaque())]),
            NodeContent::children([inner_cell]),
        )
        .unwrap();
    let inner_table = builder
        .insert(
            NodeKind::Table,
            attrs(&[("innerTable", opaque())]),
            NodeContent::children([inner_row]),
        )
        .unwrap();
    let tail = paragraph(builder, "tail");
    vec![heading, image, inner_table, tail]
}

#[test]
fn all_four_boundaries_clear_only_origins_crossing_top_or_left() {
    let f = fixture(
        5,
        5,
        vec![
            CellSpec::new(0, 2, 2, 1).rich_header(),
            CellSpec::new(2, 0, 1, 2).rich_header(),
            CellSpec::new(2, 3, 1, 2).rich_header(),
            CellSpec::new(3, 2, 2, 1).rich_header(),
        ],
    );
    let supplied_defaults = defaults();
    let slice = copied(
        &f,
        (1, 1),
        (3, 3),
        Some(spec_with(supplied_defaults.clone())),
    )
    .unwrap();
    for label in ["0:2", "2:0"] {
        let cell = find_cell(&slice, label);
        assert_eq!(cell.kind(), &NodeKind::TableHeader);
        assert_cleared(cell, &supplied_defaults);
    }
    for coordinate in [(2, 3), (3, 2)] {
        let expected =
            super::super::projection::whole_fragment(&f.document, f.cells[&coordinate]).unwrap();
        let cell = find_cell(&slice, &format!("{}:{}", coordinate.0, coordinate.1));
        assert_eq!(cell.kind(), expected.kind());
        assert_eq!(cell.content(), expected.content());
        let children = cell.content().as_children().unwrap();
        assert_eq!(children.len(), 4);
        assert_eq!(
            children[0].kind(),
            &NodeKind::Heading(HeadingLevel::new(2).unwrap())
        );
        let inline = children[0].content().as_inline().unwrap();
        assert_eq!(inline.text(), "甲\r\n乙\t🙂");
        assert_eq!(
            inline.runs()[0].marks(),
            &MarkSet::new([Mark::Bold]).unwrap()
        );
        assert_eq!(inline.atoms().len(), 1);
        assert!(inline.atoms()[0].kind().is_hard_break());
        assert_eq!(
            inline.atoms()[0].content().marks(),
            &MarkSet::new([Mark::Italic]).unwrap()
        );
        assert_eq!(children[1].kind(), &NodeKind::Image);
        assert_eq!(children[1].attrs().get("imageOpaque"), Some(&opaque()));
        assert_eq!(children[2].kind(), &NodeKind::Table);
        let ClipboardNodeContent::Table { rows, row_attrs } = children[2].content() else {
            panic!("nested table lost its table content")
        };
        assert_eq!(rows[0][0].kind(), &NodeKind::TableHeader);
        assert_eq!(row_attrs, &[attrs(&[("innerRow", opaque())])]);
    }
    assert!(!slice.plain_text().contains("unprojected image alt"));
    assert_eq!(slice.plain_text().matches("甲\r\n乙\t🙂").count(), 2);
    assert_eq!(slice.plain_text().matches("nested").count(), 2);
    roundtrip(&slice);
}

#[test]
fn retained_unclipped_cell_keeps_missing_explicit_unit_and_null_attrs_exactly() {
    for raw in [
        vec![("opaque", opaque())],
        vec![
            ("colspan", AttrValue::Integer(1)),
            ("colwidth", AttrValue::Null),
            ("opaque", opaque()),
        ],
        vec![
            ("rowspan", AttrValue::Integer(1)),
            ("colwidth", widths(&[0])),
            ("opaque", opaque()),
        ],
    ] {
        let f = fixture(
            2,
            2,
            vec![CellSpec::new(0, 0, 1, 1).with_attrs(&raw).rich_header()],
        );
        let slice = copied(&f, (0, 0), (0, 0), Some(spec())).unwrap();
        let expected =
            super::super::projection::whole_fragment(&f.document, f.cells[&(0, 0)]).unwrap();
        assert_eq!(find_cell(&slice, "0:0"), &expected);
        roundtrip(&slice);
    }
}
