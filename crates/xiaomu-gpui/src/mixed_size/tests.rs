//! GPUI's TestPlatform uses NoopTextSystem, not the OS font shaper. These tests
//! exercise the public WindowTextSystem API, byte/geometry contracts and paint
//! dispatch. Synthetic cluster tests live separately. Actual font ligatures,
//! fallback faces, DPI and native IME still require a later platform checkpoint.

use super::*;
use gpui::prelude::*;
use gpui::{AvailableSpace, TestAppContext, canvas, font, point, px, size};
use std::cell::Cell;
use std::rc::Rc;

fn input<'a>(text: &'a str, sizes: &'a [SizeSpan], width: f32) -> Input<'a> {
    Input {
        text,
        sizes,
        runs: &[],
        base_font: font(".SystemUIFont"),
        base_size: px(12.0),
        base_color: gpui::black(),
        line_height: 1.5,
        wrap_width: px(width),
        limits: WorkLimits::default(),
    }
}

fn pieces(parts: &[(&str, f32)]) -> (String, Vec<SizeSpan>) {
    let mut text = String::new();
    let mut spans = Vec::new();
    for (part, size) in parts {
        let start = text.len();
        text.push_str(part);
        spans.push(SizeSpan {
            range: start..text.len(),
            size: px(*size),
        });
    }
    (text, spans)
}

fn mixed(system: &WindowTextSystem, input: Input<'_>) -> MixedLayout {
    match layout(system, input).unwrap() {
        Layout::Mixed(layout) => layout,
        other => panic!("expected mixed layout, got {other:?}"),
    }
}

fn near(left: Pixels, right: Pixels) {
    assert!(
        f32::from(left - right).abs() < 0.02,
        "{left:?} != {right:?}"
    );
}

#[gpui::test]
fn actual_size_requests_define_widths_height_and_shared_baseline(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let system = window.text_system();
        let (text, spans) = pieces(&[("small ", 12.0), ("medium ", 24.0), ("BIG", 48.0)]);
        let result = mixed(system, input(&text, &spans, 2000.0));
        assert_eq!(result.rows.len(), 1);
        let row = &result.rows[0];
        assert_eq!(row.fragments.len(), 3);
        let mut expected = px(0.0);
        for (fragment, span) in row.fragments.iter().zip(&spans) {
            assert_eq!(fragment.range, span.range);
            assert_eq!(fragment.line.font_size, span.size);
            let prepared = shape::Prepared::new(input(&text, &spans, 2000.0)).unwrap();
            let control = prepared.shape(system, span.range.clone(), span.size);
            near(fragment.line.width, control.width);
            near(fragment.x, expected);
            expected += control.width;
            let (origin, paint_height) =
                row.fragment_paint_geometry(fragment, point(px(7.0), px(9.0)));
            let painted_baseline = origin.y
                + (paint_height - fragment.line.ascent - fragment.line.descent) / 2.0
                + fragment.line.ascent;
            near(painted_baseline, px(9.0) + row.y + row.baseline);
        }
        near(row.width, expected);
        near(result.size.width, expected);
        assert!(row.height >= px(72.0));
        assert!(row.baseline > px(24.0));
    });
}

#[gpui::test]
fn unicode_breaks_are_not_spaces_or_size_mark_boundaries(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let system = window.text_system();
        let (text, spans) = pieces(&[("甲乙丙丁", 12.0), ("戊己庚辛", 24.0)]);
        let narrow = mixed(system, input(&text, &spans, 24.0));
        assert_eq!(narrow.rows[0].range, 0..9);
        assert!(narrow.rows.iter().all(|row| row.width <= px(24.0)));
        let wide = mixed(system, input(&text, &spans, 240.0));
        assert_eq!(wide.rows.len(), 1);
        assert!(narrow.rows.len() > wide.rows.len());
        let (text, spans) = pieces(&[("aa he", 12.0), ("llo", 24.0), (" z", 12.0)]);
        let result = mixed(system, input(&text, &spans, 70.0));
        assert_eq!(result.rows[0].range, 0..3);
        assert!(result.rows[1].range.start < spans[1].range.start);
        assert!(result.rows[1].range.end > spans[1].range.end);
    });
}

#[gpui::test]
fn emergency_wrap_never_splits_combining_or_zwj_graphemes(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let system = window.text_system();
        for grapheme in ["e\u{301}", "👩‍🚀", "🇨🇳", "👍🏽"] {
            let (text, spans) = pieces(&[(grapheme, 12.0), ("xy", 24.0)]);
            let result = mixed(system, input(&text, &spans, 1.0));
            assert_eq!(result.rows[0].range, 0..grapheme.len());
            assert_eq!(result.rows[0].stops.len(), 2);
            assert!(result.rows[0].width > px(1.0));
            for index in 1..grapheme.len() {
                assert_eq!(
                    result
                        .position_for_index(index, CursorAffinity::Before)
                        .is_some(),
                    grapheme.is_char_boundary(index),
                );
            }
        }
    });
}

#[gpui::test]
fn mixed_size_split_grapheme_and_complex_script_fail_closed(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let system = window.text_system();
        for (text, boundary) in [("e\u{301}x", 1), ("👩‍🚀x", "👩".len())] {
            let spans = vec![
                SizeSpan {
                    range: 0..boundary,
                    size: px(12.0),
                },
                SizeSpan {
                    range: boundary..text.len(),
                    size: px(24.0),
                },
            ];
            assert_eq!(
                layout(system, input(text, &spans, 100.0))
                    .unwrap_err()
                    .reason,
                Reason::SizeBoundaryInsideGrapheme
            );
        }
        for text in ["Aאב", "Aعربي", "Aक", "A\u{202e}B", "A\tB"] {
            let spans = vec![
                SizeSpan {
                    range: 0..1,
                    size: px(12.0),
                },
                SizeSpan {
                    range: 1..text.len(),
                    size: px(24.0),
                },
            ];
            assert_eq!(
                layout(system, input(text, &spans, 100.0))
                    .unwrap_err()
                    .reason,
                Reason::ComplexScriptOrControl
            );
            assert!(matches!(
                layout(system, input(text, &[], 100.0)).unwrap(),
                Layout::Uniform(_)
            ));
        }
    });
}

#[gpui::test]
fn caret_hits_selections_and_soft_wrap_use_identical_geometry(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let (text, spans) = pieces(&[("abc def ", 12.0), ("ghi jkl", 48.0)]);
        let result = mixed(window.text_system(), input(&text, &spans, 90.0));
        assert!(result.rows.len() >= 3);
        for row in &result.rows {
            near(result.row_for_y(row.y + row.height / 2.0).unwrap().y, row.y);
            for stop in &row.stops {
                let point = point(stop.x, row.y + row.height / 2.0);
                let hit = result.closest_index_for_point(point).unwrap();
                assert_eq!(hit.index, stop.index);
                let caret = result.caret_rect(hit.index, hit.affinity, px(1.0)).unwrap();
                near(caret.origin.x, stop.x);
                near(caret.origin.y, row.y);
                near(caret.size.height, row.height);
            }
        }
        let boundary = result.rows[0].range.end;
        let before = result
            .position_for_index(boundary, CursorAffinity::Before)
            .unwrap();
        let after = result
            .position_for_index(boundary, CursorAffinity::After)
            .unwrap();
        assert!(after.y > before.y);
        near(after.x, px(0.0));
        let forward = result.selection_rects(0, text.len());
        assert_eq!(forward, result.selection_rects(text.len(), 0));
        assert_eq!(forward.len(), result.rows.len());
        for (rect, row) in forward.iter().zip(&result.rows) {
            near(rect.size.width, row.width);
            near(rect.size.height, row.height);
            near(rect.origin.y, row.y);
        }
        assert!(result.selection_rects(2, 2).is_empty());
        assert!(
            result
                .position_for_index(text.len() + 1, CursorAffinity::Before)
                .is_none()
        );
    });
}

#[gpui::test]
fn lf_empty_rows_trailing_lf_and_newline_selection_are_preserved(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let (text, spans) = pieces(&[("a\n\n", 12.0), ("B\n", 48.0)]);
        let result = mixed(window.text_system(), input(&text, &spans, 500.0));
        assert_eq!(
            result
                .rows
                .iter()
                .map(|row| row.range.clone())
                .collect::<Vec<_>>(),
            vec![0..1, 2..2, 3..4, 5..5]
        );
        assert!(result.rows[..3].iter().all(|row| row.hard_break_after));
        assert!(!result.rows[3].hard_break_after);
        assert!(result.rows[2].height > result.rows[0].height);
        assert_eq!(result.selection_rects(1, 3).len(), 2);
        for row in &result.rows {
            assert!(row.height > px(0.0));
            assert!(
                result
                    .caret_rect(row.range.start, CursorAffinity::After, px(1.0))
                    .is_some()
            );
        }
    });
}

#[gpui::test]
fn uniform_path_matches_stock_gpui_and_coalesces_equal_sizes(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let system = window.text_system();
        let text = "AV ffi عربي 中文\n";
        let spans = vec![
            SizeSpan {
                range: 0..2,
                size: px(24.0),
            },
            SizeSpan {
                range: 2..text.len(),
                size: px(24.0),
            },
        ];
        let prepared = shape::Prepared::new(input(text, &spans, 90.0)).unwrap();
        assert_eq!(prepared.sizes.len(), 1);
        let native = system
            .shape_text(
                text.to_owned().into(),
                px(24.0),
                &prepared.runs,
                Some(px(90.0)),
                None,
            )
            .unwrap();
        let Layout::Uniform(result) = layout(system, input(text, &spans, 90.0)).unwrap() else {
            panic!("equal sizes must use the Unicode-complete native route");
        };
        assert_eq!(result.font_size, px(24.0));
        assert_eq!(result.line_height, px(36.0));
        assert_eq!(result.lines.len(), native.len());
        for (actual, expected) in result.lines.iter().zip(native.iter()) {
            assert_eq!(actual.text, expected.text);
            assert_eq!(
                actual.size(result.line_height),
                expected.size(result.line_height)
            );
        }
    });
}

#[gpui::test]
fn admission_ignores_width_and_reflow_keeps_source_sizes(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let system = window.text_system();
        let (text, spans) = pieces(&[("abc ", 12.0), ("def", 48.0)]);
        for width in [1.0, 50.0, 500.0, f32::NAN] {
            assert_eq!(
                admission(system, input(&text, &spans, width)).unwrap(),
                Admission::MixedLtr
            );
        }
        assert_eq!(
            admission(system, input("عربي", &[], 0.0)).unwrap(),
            Admission::Uniform
        );
        for width in [1.0, 50.0, 500.0] {
            let result = mixed(system, input(&text, &spans, width));
            let rendered: String = result
                .rows
                .iter()
                .flat_map(|row| &row.fragments)
                .map(|fragment| fragment.line.text.as_ref())
                .collect();
            assert_eq!(rendered, text);
            assert!(
                result
                    .rows
                    .iter()
                    .all(|row| row.width <= px(width) || row.stops.len() == 2)
            );
        }
    });
}

#[gpui::test]
fn same_size_marks_stay_in_text_runs_and_do_not_create_fragments(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let (text, spans) = pieces(&[("ab", 12.0), ("cd ", 12.0), ("ef", 24.0)]);
        let mut runs = shape::Prepared::new(input(&text, &spans, 500.0))
            .unwrap()
            .runs;
        let mut red = runs[0].clone();
        red.len = 1;
        red.color = gpui::rgba(0xff0000ff).into();
        runs[0].len = text.len() - 1;
        runs.insert(0, red.clone());
        let mut request = input(&text, &spans, 500.0);
        request.runs = &runs;
        let prepared = shape::Prepared::new(request.clone()).unwrap();
        assert_eq!(prepared.sizes.len(), 2);
        assert_eq!(prepared.clip_runs(0..3)[0], red);
        let result = mixed(window.text_system(), request);
        assert_eq!(result.rows[0].fragments.len(), 2);
        assert_eq!(result.rows[0].fragments[0].line.text.as_ref(), "abcd ");
    });
}

#[gpui::test]
fn virtual_paint_dispatches_only_final_row_fragments(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let painted = Rc::new(Cell::new(false));
    let observed = painted.clone();
    cx.draw(
        point(px(0.0), px(0.0)),
        size(
            AvailableSpace::Definite(px(300.0)),
            AvailableSpace::Definite(px(300.0)),
        ),
        |_, _| {
            canvas(
                |_, window, _| {
                    let (text, spans) = pieces(&[("abc ", 12.0), ("DEF", 48.0)]);
                    mixed(window.text_system(), input(&text, &spans, 90.0))
                },
                move |bounds, result, window, cx| {
                    result.paint(bounds.origin, window, cx).unwrap();
                    observed.set(true);
                },
            )
            .w(px(300.0))
            .h(px(300.0))
        },
    );
    assert!(painted.get());
}

#[gpui::test]
fn malformed_input_is_rejected_without_slicing_panics(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let system = window.text_system();
        for spans in [
            vec![
                SizeSpan {
                    range: 0..1,
                    size: px(12.0),
                },
                SizeSpan {
                    range: 1..3,
                    size: px(24.0),
                },
            ],
            vec![SizeSpan {
                range: 0..3,
                size: px(f32::NAN),
            }],
            vec![SizeSpan {
                range: 1..3,
                size: px(24.0),
            }],
        ] {
            assert_eq!(
                layout(system, input("中", &spans, 100.0))
                    .unwrap_err()
                    .reason,
                Reason::InvalidInput
            );
        }
        let mut overflowing = input("abc", &[], 100.0);
        overflowing.line_height = f32::MAX;
        assert_eq!(
            layout(system, overflowing).unwrap_err().reason,
            Reason::InvalidInput
        );
        assert_eq!(
            layout(system, input("abc", &[], 0.0)).unwrap_err().reason,
            Reason::InvalidInput
        );
    });
}
