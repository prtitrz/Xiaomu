//! Cropped geometry keeps physical origins, raw attributes and row provenance.

use super::*;

#[test]
fn simultaneous_horizontal_and_vertical_crops_keep_width_slices_and_direction() {
    let f = fixture(
        6,
        6,
        vec![
            CellSpec::new(0, 3, 3, 3)
                .with_attrs(&[("colwidth", widths(&[0, 80, 120])), ("opaque", opaque())])
                .rich_header(),
            CellSpec::new(3, 0, 3, 3)
                .with_attrs(&[("colwidth", widths(&[0, 0, 90])), ("opaque", opaque())])
                .rich_header(),
        ],
    );
    let slice = copied(&f, (1, 1), (4, 4), Some(spec())).unwrap();
    assert_eq!(slice, copied(&f, (4, 4), (1, 1), Some(spec())).unwrap());
    assert_eq!(
        rows(&slice)
            .0
            .iter()
            .map(|row| row.iter().map(origin).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        vec![
            vec!["1:1", "1:2", "0:3"],
            vec!["2:1", "2:2"],
            vec!["3:0", "3:3", "3:4"],
            vec!["4:3", "4:4"],
        ]
    );
    for (coordinate, label, expected_widths) in [
        ((0, 3), "0:3", widths(&[0, 80])),
        ((3, 0), "3:0", widths(&[0, 90])),
    ] {
        let cell = find_cell(&slice, label);
        let original = f.document.node(f.cells[&coordinate]).unwrap();
        assert_eq!(cell.kind(), &NodeKind::TableHeader);
        assert_eq!(
            cell.attrs(),
            &extended(
                original.attrs(),
                &[
                    ("rowspan", AttrValue::Integer(2)),
                    ("colspan", AttrValue::Integer(2)),
                    ("colwidth", expected_widths),
                ],
            )
        );
        assert_cleared(cell, &defaults());
    }
    assert_eq!(rows(&slice).1, &f.row_attrs[1..5]);
    assert_eq!(
        slice.roots()[0].attrs(),
        f.document.node(f.table).unwrap().attrs()
    );
    roundtrip(&slice);
}

#[test]
fn carried_origin_is_sorted_between_native_origins_and_covered_empty_row_is_kept() {
    let f = fixture(
        3,
        3,
        vec![
            CellSpec::new(0, 1, 3, 1)
                .with_attrs(&[("colwidth", AttrValue::Null), ("opaque", opaque())])
                .rich_header(),
            CellSpec::new(1, 0, 2, 1),
            CellSpec::new(1, 2, 2, 1)
                .with_attrs(&[("colspan", AttrValue::Integer(1)), ("opaque", opaque())]),
        ],
    );
    let slice = copied(&f, (1, 0), (1, 2), Some(spec())).unwrap();
    let (rows, row_attrs) = rows(&slice);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].iter().map(origin).collect::<Vec<_>>(),
        ["1:0", "0:1", "1:2"]
    );
    assert!(rows[1].is_empty());
    assert_eq!(row_attrs, &f.row_attrs[1..3]);
    let carried = &rows[0][1];
    assert_eq!(
        carried.attrs(),
        &extended(
            f.document.node(f.cells[&(0, 1)]).unwrap().attrs(),
            &[("rowspan", AttrValue::Integer(2))],
        )
    );
    assert_eq!(carried.attrs().get("colspan"), None);
    assert_eq!(carried.attrs().get("colwidth"), Some(&AttrValue::Null));
    assert_cleared(carried, &defaults());
    for (cell, coordinate) in [(&rows[0][0], (1, 0)), (&rows[0][2], (1, 2))] {
        assert_eq!(
            cell.attrs(),
            f.document.node(f.cells[&coordinate]).unwrap().attrs()
        );
    }
    roundtrip(&slice);
}

#[test]
fn horizontally_cropped_widths_become_null_only_when_no_positive_width_remains() {
    for (source, expected) in [
        (Some(widths(&[90, 0, 0])), Some(AttrValue::Null)),
        (Some(widths(&[0, 0, 0])), Some(AttrValue::Null)),
        (Some(widths(&[0, 0, 70])), Some(widths(&[0, 70]))),
        (Some(AttrValue::Null), Some(AttrValue::Null)),
        (None, None),
    ] {
        let mut span = CellSpec::new(1, 0, 1, 3).with_attrs(&[("opaque", opaque())]);
        if let Some(source) = source {
            span = span.with_attrs(&[("colwidth", source)]);
        }
        let f = fixture(3, 4, vec![span]);
        let slice = copied(&f, (0, 1), (2, 2), Some(spec())).unwrap();
        let cell = find_cell(&slice, "1:0");
        assert_eq!(cell.attrs().get("colspan"), Some(&AttrValue::Integer(2)));
        assert_eq!(cell.attrs().get("rowspan"), None);
        assert_eq!(cell.attrs().get("colwidth"), expected.as_ref());
        assert_eq!(cell.attrs().get("opaque"), Some(&opaque()));
        assert_cleared(cell, &defaults());
        roundtrip(&slice);
    }
}

#[test]
fn right_crop_keeps_zero_entries_but_nulls_an_all_zero_remainder() {
    for (source, expected) in [
        (widths(&[0, 30, 90]), widths(&[0, 30])),
        (widths(&[0, 0, 90]), AttrValue::Null),
    ] {
        let f = fixture(
            3,
            4,
            vec![CellSpec::new(1, 1, 1, 3).with_attrs(&[("colwidth", source)])],
        );
        let slice = copied(&f, (0, 1), (2, 2), Some(spec())).unwrap();
        let cell = find_cell(&slice, "1:1");
        assert_eq!(cell.attrs().get("colwidth"), Some(&expected));
        assert_eq!(cell.attrs().get("colspan"), Some(&AttrValue::Integer(2)));
        assert_eq!(
            cell.content(),
            super::super::projection::whole_fragment(&f.document, f.cells[&(1, 1)])
                .unwrap()
                .content()
        );
        roundtrip(&slice);
    }
}

#[test]
fn vertical_only_crops_preserve_all_zero_widths_without_normalization() {
    for (origin_row, anchor, focus, clears) in
        [(0, (1, 0), (2, 2), true), (1, (0, 0), (2, 2), false)]
    {
        let f = fixture(
            4,
            3,
            vec![
                CellSpec::new(origin_row, 1, 3, 1)
                    .with_attrs(&[("colwidth", widths(&[0]))])
                    .rich_header(),
            ],
        );
        let slice = copied(&f, anchor, focus, Some(spec())).unwrap();
        let cell = find_cell(&slice, &format!("{origin_row}:1"));
        assert_eq!(cell.attrs().get("rowspan"), Some(&AttrValue::Integer(2)));
        assert_eq!(cell.attrs().get("colspan"), None);
        assert_eq!(cell.attrs().get("colwidth"), Some(&widths(&[0])));
        if clears {
            assert_cleared(cell, &defaults());
        } else {
            assert_eq!(
                cell.content(),
                super::super::projection::whole_fragment(&f.document, f.cells[&(origin_row, 1)])
                    .unwrap()
                    .content()
            );
        }
        roundtrip(&slice);
    }
}
