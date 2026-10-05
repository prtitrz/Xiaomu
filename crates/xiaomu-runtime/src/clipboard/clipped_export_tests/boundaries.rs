//! Explicit mode admission and v14 provenance stay independent of default paste.

use super::*;
use crate::session::EditIntent;
use std::sync::Arc;

#[test]
fn spec_clone_shares_defaults_and_mode_replacement_drops_unused_payload() {
    use super::super::export::CellRangeExport;

    let original = spec();
    let cloned = original.clone();
    let (CellRangeExport::Clipped(first), CellRangeExport::Clipped(second)) =
        (original.cell_ranges(), cloned.cell_ranges())
    else {
        panic!("clipped mode must own shared immutable defaults")
    };
    assert!(Arc::ptr_eq(first, second));
    assert_eq!(first.as_ref(), &defaults());
    let closed = ClipboardExportSpec::new()
        .with_closed_cell_ranges()
        .with_text_projection(ClipboardTextProjection::TextBetweenLfV1);
    let replaced = cloned.with_closed_cell_ranges();
    assert_eq!(replaced, closed);
    assert_eq!(format!("{replaced:?}"), format!("{closed:?}"));
    assert_eq!(Arc::strong_count(first), 1);
    let new_defaults = attrs(&[("new", AttrValue::Bool(true))]);
    assert_eq!(
        original.with_clipped_cell_ranges(new_defaults.clone()),
        closed.with_clipped_cell_ranges(new_defaults.clone())
    );
    assert_eq!(
        ClipboardExportSpec::new()
            .with_clipped_cell_ranges(new_defaults)
            .text_projection(),
        None
    );
}

#[test]
fn only_explicit_clipped_mode_admits_nonclosed_geometry() {
    let f = fixture(3, 4, vec![CellSpec::new(1, 0, 1, 3)]);
    let closed = ClipboardExportSpec::new()
        .with_closed_cell_ranges()
        .with_text_projection(ClipboardTextProjection::TextBetweenLfV1);
    for options in [
        None,
        Some(ClipboardExportSpec::new()),
        Some(
            ClipboardExportSpec::new()
                .with_text_projection(ClipboardTextProjection::TextBetweenLfV1),
        ),
        Some(closed.clone()),
        Some(spec().with_closed_cell_ranges()),
    ] {
        assert_eq!(
            copied(&f, (0, 1), (2, 2), options),
            Err(SessionError::UnsupportedTableOperation)
        );
    }
    let clipped = copied(&f, (0, 1), (2, 2), Some(spec())).unwrap();
    assert_eq!(
        clipped,
        copied(
            &f,
            (0, 1),
            (2, 2),
            Some(closed.with_clipped_cell_ranges(defaults()))
        )
        .unwrap()
    );
    roundtrip(&clipped);
}

#[test]
fn default_unit_copy_keeps_legacy_text_wire_and_geometry_admission() {
    let unit = fixture(2, 2, vec![]);
    let legacy = copied(&unit, (0, 0), (1, 1), None).unwrap();
    assert_eq!(legacy.plain_text(), "0:0\t0:1\n1:0\t1:1");
    assert_eq!(legacy.source_boundary(), None);
    assert_eq!(legacy.text_projection(), None);
    let metadata = encode_metadata(&legacy).unwrap();
    assert!(!metadata.starts_with("xiaomu.clipboard.v14\n"));
    assert_eq!(
        decode_metadata(legacy.plain_text(), &metadata),
        Some(legacy.clone())
    );
    let projected = copied(&unit, (0, 0), (1, 1), Some(spec())).unwrap();
    assert_eq!(projected.roots(), legacy.roots());
    assert_eq!(projected.plain_text(), "0:0\n0:1\n1:0\n1:1");
    roundtrip(&projected);
    let span = fixture(2, 2, vec![CellSpec::new(0, 0, 2, 2)]);
    assert_eq!(
        copied(&span, (0, 0), (0, 0), None),
        Err(SessionError::UnsupportedTableOperation)
    );
    let closed = ClipboardExportSpec::new()
        .with_closed_cell_ranges()
        .with_text_projection(ClipboardTextProjection::TextBetweenLfV1);
    assert_eq!(
        copied(&span, (0, 0), (0, 0), Some(spec())).unwrap(),
        copied(&span, (0, 0), (0, 0), Some(closed)).unwrap()
    );
}

#[test]
fn rows_and_whole_table_cell_ranges_keep_open_one_one_wire_and_byte_binding() {
    let f = fixture(3, 4, vec![CellSpec::new(1, 1, 1, 3).rich_header()]);
    for (anchor, focus, root_form) in [
        ((0, 1), (2, 2), ClipboardCellRangeRoot::Rows),
        ((0, 0), (2, 3), ClipboardCellRangeRoot::Table),
    ] {
        let slice = copied(&f, anchor, focus, Some(spec())).unwrap();
        assert_eq!(
            slice.source_boundary(),
            Some(ClipboardSourceBoundary::CellRange { root_form })
        );
        assert_eq!(slice.source_boundary().unwrap().open_depths(), Some((1, 1)));
        assert!(!slice.is_closed());
        assert!(!slice.allows_default_fitting());
        roundtrip(&slice);
        let metadata = encode_metadata(&slice).unwrap();
        let mut wire: serde_json::Value =
            serde_json::from_str(metadata.strip_prefix("xiaomu.clipboard.v14\n").unwrap()).unwrap();
        assert_eq!(wire["source_boundary"]["open_start"], 1);
        assert_eq!(wire["source_boundary"]["open_end"], 1);
        assert_eq!(
            wire["source_boundary"]["root_form"],
            if root_form == ClipboardCellRangeRoot::Rows {
                "rows"
            } else {
                "table"
            }
        );
        assert_eq!(wire["closed"], false);
        for stale in [
            slice.plain_text().replace("\r\n", "\n"),
            slice.plain_text().replace('\t', " "),
            format!("{}\n", slice.plain_text()),
        ] {
            assert_ne!(stale, slice.plain_text());
            assert_eq!(
                decode_metadata_checked(&stale, &metadata),
                ClipboardMetadataDecode::RejectedNative
            );
        }
        wire["source_boundary"]["open_start"] = serde_json::json!(0);
        assert_eq!(
            decode_metadata_checked(slice.plain_text(), &format!("xiaomu.clipboard.v14\n{wire}")),
            ClipboardMetadataDecode::RejectedNative
        );
    }
}

#[test]
fn clipping_can_be_enabled_without_selecting_a_new_text_projection() {
    let f = fixture(3, 4, vec![CellSpec::new(1, 0, 1, 3)]);
    let slice = copied(
        &f,
        (0, 1),
        (2, 2),
        Some(ClipboardExportSpec::new().with_clipped_cell_ranges(defaults())),
    )
    .unwrap();
    assert_eq!(slice.text_projection(), None);
    assert_eq!(slice.plain_text(), "0:1\t0:2\n\t\n2:1\t2:2");
    assert_cleared(find_cell(&slice, "1:0"), &defaults());
    roundtrip(&slice);
}

#[test]
fn clipped_cell_carriers_do_not_enable_default_paste_fitting() {
    let source = fixture(3, 4, vec![CellSpec::new(1, 0, 1, 3)]);
    let slice = copied(&source, (0, 1), (2, 2), Some(spec())).unwrap();
    for rectangular in [false, true] {
        let target = fixture(3, 2, vec![]);
        let mut s = session(&target, None);
        if rectangular {
            s.set_cell_range_selection(target.cells[&(0, 0)], target.cells[&(2, 1)])
                .unwrap();
        }
        let before = s.document().clone();
        let selection = s.selection();
        assert_eq!(
            s.apply_intent(&EditIntent::PasteSlice {
                slice: slice.clone()
            }),
            Err(SessionError::UnsupportedTableOperation)
        );
        assert_eq!(s.document().store(), before.store());
        assert_eq!(s.document().revision(), before.revision());
        assert_eq!(s.selection(), selection);
        assert_eq!(s.history_depths(), (0, 0));
    }
}
