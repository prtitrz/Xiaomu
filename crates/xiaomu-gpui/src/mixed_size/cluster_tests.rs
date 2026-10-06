//! Synthetic missing-metadata regressions, not evidence of real font shaping.
//! Start with public ShapedLine data from virtual WindowTextSystem and replace
//! only its public DerefMut LineLayout to represent ligatures and kerning that
//! the NoopTextSystem cannot produce itself.

use super::*;
use gpui::{LineLayout, ShapedLine, ShapedRun, TestAppContext, font, point, px};
use std::sync::Arc;

fn shaped(system: &WindowTextSystem, text: &str) -> ShapedLine {
    system.shape_line(
        text.to_owned().into(),
        px(12.0),
        &[TextRun {
            len: text.len(),
            font: font(".SystemUIFont"),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    )
}

fn replace(line: &mut ShapedLine, runs: Vec<ShapedRun>, width: Pixels) {
    **line = Arc::new(LineLayout {
        font_size: line.font_size,
        width,
        ascent: line.ascent,
        descent: line.descent,
        runs,
        len: line.text.len(),
    });
}

#[gpui::test]
fn inferred_ligature_cluster_is_indivisible_and_a_size_seam_is_rejected(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let system = window.text_system();
        let mut full = shaped(system, "ffi");
        let mut runs = full.runs.clone();
        runs[0].glyphs.truncate(1);
        replace(&mut full, runs, px(18.0));
        let stops = clusters::stops(&full, 10).unwrap();
        assert_eq!(
            stops.iter().map(|stop| stop.index).collect::<Vec<_>>(),
            vec![10, 13]
        );
        assert_eq!(
            clusters::validate_split(&full, &shaped(system, "f"), &shaped(system, "fi"), 10)
                .unwrap_err(),
            Unsupported {
                range: 11..11,
                reason: Reason::SizeBoundaryInsideShapedCluster,
            }
        );
    });
}

#[gpui::test]
fn av_cross_size_kerning_is_rejected_even_when_both_glyph_starts_exist(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let system = window.text_system();
        let left = shaped(system, "A");
        let right = shaped(system, "V");
        let mut full = shaped(system, "AV");
        let mut runs = full.runs.clone();
        runs[0].glyphs[1].position.x -= px(1.0);
        let width = full.width - px(1.0);
        replace(&mut full, runs, width);
        assert_eq!(clusters::stops(&full, 0).unwrap().len(), 3);
        assert_eq!(
            clusters::validate_split(&full, &left, &right, 0)
                .unwrap_err()
                .reason,
            Reason::ContextAtSizeBoundary
        );
        assert!(clusters::validate_split(&shaped(system, "AV"), &left, &right, 0).is_ok());
    });
}

#[gpui::test]
fn reordered_or_zero_advance_clusters_are_explicitly_unsupported(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let system = window.text_system();
        let mut reversed = shaped(system, "AB");
        let mut runs = reversed.runs.clone();
        runs[0].glyphs.swap(0, 1);
        let width = reversed.width;
        replace(&mut reversed, runs, width);
        assert_eq!(
            clusters::stops(&reversed, 0).unwrap_err().reason,
            Reason::UnreliableClusterGeometry
        );
        let mut zero = shaped(system, "AB");
        let mut runs = zero.runs.clone();
        runs[0].glyphs[1].position = point(px(0.0), px(0.0));
        let width = zero.width;
        replace(&mut zero, runs, width);
        assert_eq!(
            clusters::stops(&zero, 0).unwrap_err().reason,
            Reason::UnreliableClusterGeometry
        );
    });
}

fn layout_for_line(line: ShapedLine) -> MixedLayout {
    let len = line.text.len();
    let width = line.width;
    let stops = clusters::stops(&line, 0).unwrap();
    let fragments = vec![Fragment {
        range: 0..len,
        x: px(0.0),
        line: NativeLine::synthetic(line),
    }];
    let carets = rows::caret_positions(&fragments, 0);
    MixedLayout {
        rows: vec![Row {
            range: 0..len,
            y: px(0.0),
            height: px(20.0),
            baseline: px(14.0),
            width,
            hard_break_after: false,
            fragments,
            stops,
            carets,
        }],
        size: gpui::size(width, px(20.0)),
        text_len: len,
    }
}

#[gpui::test]
fn ffi_combining_and_zwj_interior_scalar_positions_keep_caret_and_ime_bounds(
    cx: &mut TestAppContext,
) {
    cx.add_empty_window().update(|window, _| {
        let system = window.text_system();
        for text in ["ffi", "e\u{301}", "👩‍🚀"] {
            let mut full = shaped(system, text);
            let mut runs = full.runs.clone();
            runs[0].glyphs.truncate(1);
            replace(&mut full, runs, px(18.0));
            let result = layout_for_line(full);
            assert_eq!(result.rows[0].stops.len(), 2);
            assert_eq!(result.rows[0].carets.len(), text.chars().count() + 1);
            for index in 0..=text.len() {
                let caret = result.caret_rect(index, CursorAffinity::Before, px(1.0));
                assert_eq!(caret.is_some(), text.is_char_boundary(index));
                if let Some(caret) = caret {
                    assert_eq!(
                        caret.origin.x,
                        result.rows[0].fragments[0].line.x_for_index(index)
                    );
                    assert_eq!(caret.size.height, px(20.0));
                    let bounds = result
                        .bounds_for_range(index, index, CursorAffinity::Before, px(1.0))
                        .unwrap();
                    assert_eq!(bounds, caret);
                }
            }
            let scalar_boundaries: Vec<_> = text
                .char_indices()
                .map(|(index, _)| index)
                .chain(std::iter::once(text.len()))
                .collect();
            let interior = scalar_boundaries[1];
            let next = scalar_boundaries[2];
            let selection = result.selection_rects(interior, next);
            assert_eq!(selection.len(), 1);
            assert_eq!(selection[0].size.width, px(0.0));
            let bounds = result
                .bounds_for_range(interior, next, CursorAffinity::Before, px(1.0))
                .unwrap();
            assert_eq!(bounds.origin.x, px(18.0));
            assert_eq!(bounds.size.width, px(1.0));
            assert_eq!(bounds.size.height, px(20.0));
            let hit = result
                .closest_index_for_point(point(px(9.0), px(10.0)))
                .unwrap();
            assert!(hit.index == 0 || hit.index == text.len());
        }
    });
}

#[gpui::test]
fn reflow_cannot_add_hit_edges_inside_original_cluster_but_keeps_scalar_carets(
    cx: &mut TestAppContext,
) {
    cx.add_empty_window().update(|window, _| {
        let mut result = layout_for_line(shaped(window.text_system(), "ffi"));
        assert_eq!(result.rows[0].stops.len(), 4);
        rows::restrict_hit_stops(&mut result.rows[0], |index| index == 3);
        assert_eq!(
            result.rows[0]
                .stops
                .iter()
                .map(|stop| stop.index)
                .collect::<Vec<_>>(),
            vec![0, 3]
        );
        assert_eq!(result.rows[0].carets.len(), 4);
        assert!(
            result
                .caret_rect(1, CursorAffinity::Before, px(1.0))
                .is_some()
        );
        assert!(
            result
                .caret_rect(2, CursorAffinity::Before, px(1.0))
                .is_some()
        );
    });
}

#[gpui::test]
fn linear_cursor_matches_stock_for_duplicate_and_reordered_indices_inside_an_egc(
    cx: &mut TestAppContext,
) {
    cx.add_empty_window().update(|window, _| {
        let text = "a\u{301}\u{302}\u{303}";
        let mut line = shaped(window.text_system(), text);
        let mut runs = line.runs.clone();
        runs[0].glyphs[1].index = 3;
        runs[0].glyphs[2].index = 1;
        runs[0].glyphs[3].index = 3;
        let width = line.width;
        replace(&mut line, runs, width);
        // These glyph starts all belong to the same EGC and do not establish
        // separate safe wrapping edges, but stock's first >= mapping remains.
        assert_eq!(clusters::stops(&line, 0).unwrap().len(), 2);
        let fragments = vec![Fragment {
            range: 0..text.len(),
            x: px(4.0),
            line: NativeLine::synthetic(line),
        }];
        let carets = rows::caret_positions(&fragments, 0);
        for stop in carets {
            assert_eq!(stop.x, px(4.0) + fragments[0].line.x_for_index(stop.index));
        }
    });
}

#[gpui::test]
fn nonzero_first_glyph_offset_keeps_advance_origin_hit_caret_and_seams_consistent(
    cx: &mut TestAppContext,
) {
    cx.add_empty_window().update(|window, _| {
        let system = window.text_system();
        let mut first = shaped(system, "AB");
        let mut runs = first.runs.clone();
        runs[0].glyphs[0].position.x = px(1.0);
        let width = first.width;
        replace(&mut first, runs, width);
        let result = layout_for_line(first.clone());
        assert_eq!(result.rows[0].stops[0].x, px(0.0));
        assert_eq!(result.rows[0].carets[0].x, px(0.0));
        for stop in &result.rows[0].stops {
            let hit = result
                .closest_index_for_point(point(stop.x, px(10.0)))
                .unwrap();
            let caret = result.caret_rect(hit.index, hit.affinity, px(1.0)).unwrap();
            assert_eq!(caret.origin.x, stop.x);
        }
        let mut second = shaped(system, "CD");
        let mut runs = second.runs.clone();
        runs[0].glyphs[0].position.x = px(2.0);
        let second_width = second.width;
        replace(&mut second, runs, second_width);
        let fragments = vec![
            Fragment {
                range: 0..2,
                x: px(0.0),
                line: NativeLine::synthetic(first),
            },
            Fragment {
                range: 2..4,
                x: width,
                line: NativeLine::synthetic(second),
            },
        ];
        let carets = rows::caret_positions(&fragments, 0);
        assert_eq!(carets.iter().find(|stop| stop.index == 2).unwrap().x, width);
        assert_eq!(carets.len(), 5);
    });
}
