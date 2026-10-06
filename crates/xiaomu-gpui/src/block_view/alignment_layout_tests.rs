//! Deterministic geometry checks. Virtual glyphs are not real-font evidence.
use super::*;
use gpui::{LineLayout, TestAppContext, TextRun, WrapBoundary, WrappedLineLayout, font};
use std::sync::Arc;

pub(super) fn runs(text: &str) -> Vec<TextRun> {
    vec![TextRun {
        len: text.len(),
        font: font(".SystemUIFont"),
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    }]
}

fn shape(window: &gpui::Window, text: &str, width: f32) -> BlockTextLayout {
    let lines = window
        .text_system()
        .shape_text(
            text.to_owned().into(),
            px(12.0),
            &runs(text),
            Some(px(width)),
            None,
        )
        .unwrap();
    BlockTextLayout::new(lines.into_iter().collect(), px(20.0))
}

pub(super) fn unequal_rows(window: &gpui::Window) -> BlockTextLayout {
    let mut lines = shape(window, "abcd", 100.0).lines;
    let line = &mut lines[0];
    let mut glyph_runs = line.runs().to_vec();
    for (glyph, x) in glyph_runs
        .iter_mut()
        .flat_map(|run| &mut run.glyphs)
        .zip([0., 10., 70., 90.])
    {
        glyph.position.x = px(x);
    }
    **line = Arc::new(WrappedLineLayout {
        unwrapped_layout: Arc::new(LineLayout {
            font_size: px(12.0),
            width: px(100.0),
            ascent: px(10.0),
            descent: px(2.0),
            runs: glyph_runs,
            len: 4,
        }),
        wrap_boundaries: [WrapBoundary {
            run_ix: 0,
            glyph_ix: 2,
        }]
        .into(),
        wrap_width: Some(px(100.0)),
    });
    BlockTextLayout::new(lines, px(20.0))
}

#[gpui::test]
fn alignment_uses_each_stock_glyph_boundary_width_and_soft_wrap_affinity(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        for (alignment, starts, first_end) in [
            (BlockAlignment::Left, [0., 0.], 70.),
            (BlockAlignment::Center, [15., 35.], 85.),
            (BlockAlignment::Right, [30., 70.], 100.),
        ] {
            let layout = unequal_rows(window).aligned(alignment, px(100.));
            assert_eq!(
                layout.position_for_caret(0, CursorAffinity::Before),
                Some(point(px(starts[0]), px(0.)))
            );
            assert_eq!(
                layout.position_for_caret(2, CursorAffinity::Before),
                Some(point(px(first_end), px(0.)))
            );
            assert_eq!(
                layout.position_for_caret(2, CursorAffinity::After),
                Some(point(px(starts[1]), px(20.)))
            );
            for (index, affinity) in [
                (0, CursorAffinity::Before),
                (1, CursorAffinity::Before),
                (2, CursorAffinity::Before),
                (2, CursorAffinity::After),
                (3, CursorAffinity::Before),
                (4, CursorAffinity::Before),
            ] {
                let p = layout.position_for_caret(index, affinity).unwrap();
                assert_eq!(
                    layout.caret_for_position(p + point(px(0.), px(10.))),
                    (index, affinity)
                );
            }
            let selection = layout.selection_rects(1..4);
            assert_eq!(
                selection[0],
                Bounds::new(point(px(starts[0] + 10.), px(0.)), size(px(60.), px(20.)))
            );
            assert_eq!(
                selection[1],
                Bounds::new(point(px(starts[1]), px(20.)), size(px(30.), px(20.)))
            );
            let x = px(starts[1]);
            assert_eq!(
                layout.vertical_target(0, CursorAffinity::Before, x, true),
                Some((2, CursorAffinity::After))
            );
            assert_eq!(
                layout.edge_row_target(x, true),
                Some((2, CursorAffinity::After))
            );
            assert_eq!(
                layout.visual_line_edge(3, CursorAffinity::Before, false),
                Some((2, CursorAffinity::After))
            );
        }
    });
}

#[gpui::test]
fn empty_consecutive_and_trailing_lf_rows_align_without_invented_text(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        for alignment in [
            BlockAlignment::Left,
            BlockAlignment::Center,
            BlockAlignment::Right,
        ] {
            let empty_x = match alignment {
                BlockAlignment::Left => 0.,
                BlockAlignment::Center => 50.,
                BlockAlignment::Right => 100.,
            };
            for text in ["", "\n", "a\n\n"] {
                let layout = shape(window, text, 100.).aligned(alignment, px(100.));
                let p = layout.position_for_index(text.len()).unwrap();
                assert_eq!(p.x, px(empty_x));
                assert_eq!(p.y, px(20. * text.matches('\n').count() as f32));
                assert_eq!(
                    layout.closest_index_for_position(p + point(px(0.), px(10.))),
                    text.len()
                );
                assert!(layout.selection_rects(text.len()..text.len()).is_empty());
                for byte in text.match_indices('\n').map(|(byte, _)| byte) {
                    let rects = layout.selection_rects(byte..byte + 1);
                    assert_eq!(rects.len(), 1);
                    assert_eq!(rects[0].origin, layout.position_for_index(byte).unwrap());
                    assert_eq!(rects[0].size.width, px(4.));
                }
            }
        }
    });
}

#[gpui::test]
fn aligned_unicode_layout_keeps_stock_cluster_mapping_and_source(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        for text in ["中文🙂 e\u{301} family 👨‍👩‍👧‍👦  尾", "short trailing  ", "🙂"]
        {
            for width in [1., 47., 100.25] {
                let baseline = shape(window, text, width);
                for alignment in [BlockAlignment::Center, BlockAlignment::Right] {
                    let layout = baseline.clone().aligned(alignment, px(width));
                    assert_eq!(
                        layout
                            .lines
                            .iter()
                            .map(|line| line.text.as_ref())
                            .collect::<Vec<_>>()
                            .join("\n"),
                        text
                    );
                    for byte in text
                        .char_indices()
                        .map(|(byte, _)| byte)
                        .chain([text.len()])
                    {
                        let before = baseline.position_for_index(byte).unwrap();
                        let after = layout.position_for_index(byte).unwrap();
                        let row =
                            row_for_caret(&layout.rows, byte, CursorAffinity::Before).unwrap();
                        assert_eq!(after.y, before.y);
                        assert!(
                            (f32::from(after.x - before.x - layout.rows[row].x)).abs() < 0.0001
                        );
                    }
                    assert_eq!(layout.rows.len(), baseline.rows.len());
                }
            }
        }
    });
}

#[gpui::test]
fn nonzero_first_glyph_origin_and_exact_lf_top_use_the_painted_row(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let mut original = unequal_rows(window);
        let mut glyph_runs = original.lines[0].runs().to_vec();
        glyph_runs[0].glyphs[0].position.x = px(1.);
        replace_runs(&mut original.lines[0], glyph_runs, 2);
        original.rows = original.measure_rows();
        assert_eq!(
            original.position_for_index(0).unwrap().x,
            px(0.),
            "no provider keeps the legacy origin"
        );
        for alignment in [
            BlockAlignment::Left,
            BlockAlignment::Center,
            BlockAlignment::Right,
        ] {
            let layout = original.clone().aligned(alignment, px(100.));
            let expected = px(match alignment {
                BlockAlignment::Left => 1.,
                BlockAlignment::Center => 16.,
                BlockAlignment::Right => 31.,
            });
            assert_eq!(layout.position_for_index(0).unwrap().x, expected);
            assert_eq!(layout.selection_rects(0..1)[0].origin.x, expected);
            assert_eq!(
                layout.closest_index_for_position(point(expected, px(10.))),
                0
            );
            let hard = shape(window, "a\nb", 100.).aligned(alignment, px(100.));
            let start = hard.position_for_index(2).unwrap();
            assert_eq!(start.y, px(20.));
            assert_eq!(hard.closest_index_for_position(start), 2);
        }
    });
}

pub(super) fn replace_runs(line: &mut WrappedLine, runs: Vec<gpui::ShapedRun>, boundary: usize) {
    **line = Arc::new(WrappedLineLayout {
        unwrapped_layout: Arc::new(LineLayout {
            font_size: line.font_size(),
            width: line.unwrapped_layout.width,
            ascent: line.ascent(),
            descent: line.descent(),
            runs,
            len: line.len(),
        }),
        wrap_boundaries: [WrapBoundary {
            run_ix: 0,
            glyph_ix: boundary,
        }]
        .into(),
        wrap_width: line.wrap_width,
    });
}

#[gpui::test]
fn actual_box_width_repositions_cached_rows_including_fractional_and_overwide_rows(
    cx: &mut TestAppContext,
) {
    cx.add_empty_window().update(|window, _| {
        let source = unequal_rows(window);
        let center = source.clone().aligned(BlockAlignment::Center, px(100.1));
        let resized = center.clone().aligned(BlockAlignment::Center, px(100.4));
        let dx = f32::from(
            resized.position_for_index(0).unwrap().x - center.position_for_index(0).unwrap().x,
        );
        assert!((dx - 0.15).abs() < 0.0001);
        let overwide = source.aligned(BlockAlignment::Right, px(40.));
        assert_eq!(overwide.position_for_index(0).unwrap().x, px(-30.));
        assert_eq!(
            overwide
                .position_for_caret(2, CursorAffinity::Before)
                .unwrap()
                .x,
            px(40.)
        );
    });
}
