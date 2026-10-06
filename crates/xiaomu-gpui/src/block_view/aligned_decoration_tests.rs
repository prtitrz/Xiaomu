use super::super::{
    BlockAlignment,
    alignment_tests::{runs, unequal_rows},
};
use super::*;
use gpui::{TestAppContext, TextRun, px};
use std::sync::Arc;

fn decorated() -> Vec<TextRun> {
    let mut runs = runs("abcd");
    runs[0].underline = Some(UnderlineStyle {
        thickness: px(2.),
        color: Some(gpui::red()),
        wavy: true,
    });
    runs[0].strikethrough = Some(StrikethroughStyle {
        thickness: px(1.5),
        color: None,
    });
    runs
}

fn carrier(window: &gpui::Window) -> Vec<gpui::WrappedLine> {
    window
        .text_system()
        .shape_text("abcd".into(), px(12.), &runs("abcd"), Some(px(5.)), None)
        .unwrap()
        .into_iter()
        .collect()
}

#[gpui::test]
fn aligned_decoration_carrier_keeps_exact_original_glyph_and_wrap_arcs(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let original = unequal_rows(window);
        let shared = original.lines[0].clone();
        let layout = original.with_decoration_carrier(carrier(window), decorated());
        assert!(Arc::ptr_eq(&shared, &layout.lines[0]));
        assert!(Arc::ptr_eq(&layout.lines[0], &layout.paint_lines()[0]));
        assert_eq!(
            layout.lines[0].wrap_boundaries(),
            layout.paint_lines()[0].wrap_boundaries()
        );
        // Replacing the carrier's Arc did not mutate its separately held source.
        assert_eq!(shared.wrap_boundaries().len(), 1);
    });
}

#[gpui::test]
fn wrapped_wavy_and_strike_use_each_row_offset_and_stock_baselines(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        for (alignment, offsets) in [
            (BlockAlignment::Center, [15., 35.]),
            (BlockAlignment::Right, [30., 70.]),
        ] {
            let layout = unequal_rows(window)
                .with_decoration_carrier(carrier(window), decorated())
                .aligned(alignment, px(100.));
            assert_eq!(layout.decorations.len(), 4);
            for (ix, stroke) in layout.decorations.iter().enumerate() {
                let row = ix % 2;
                assert_eq!(stroke.row, row);
                assert_eq!(stroke.origin.x + layout.rows[row].x, px(offsets[row]));
                assert_eq!(stroke.width, px(if row == 0 { 70. } else { 30. }));
                if ix < 2 {
                    assert_eq!(stroke.origin.y, px(row as f32 * 20. + 15.236));
                    assert_eq!(
                        stroke.decoration,
                        Decoration::Underline(UnderlineStyle {
                            thickness: px(2.),
                            color: Some(gpui::red()),
                            wavy: true
                        })
                    );
                } else {
                    assert_eq!(stroke.origin.y, px(row as f32 * 20. + 9.5));
                    assert_eq!(
                        stroke.decoration,
                        Decoration::Strike(StrikethroughStyle {
                            thickness: px(1.5),
                            color: Some(gpui::black())
                        })
                    );
                }
            }
            let old = layout.decorations.clone();
            let resized = layout.aligned(alignment, px(140.));
            assert_eq!(resized.decorations.len(), old.len());
            assert_eq!(
                resized.decorations[0].origin, old[0].origin,
                "cached source spans do not need reshaping after width-only placement"
            );
        }
    });
}

#[gpui::test]
fn cluster_interior_marks_do_not_create_fake_one_pixel_decoration(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let mut layout = unequal_rows(window);
        let mut glyph_runs = layout.lines[0].runs().to_vec();
        glyph_runs[0].glyphs.remove(1); // one ligature covers bytes 0..2
        super::super::alignment_tests::replace_runs(&mut layout.lines[0], glyph_runs, 1);
        layout.rows = layout.measure_rows();
        let mut styles = runs("a");
        let mut interior = runs("b").remove(0);
        interior.underline = decorated()[0].underline;
        styles.push(interior);
        styles.extend(runs("cd"));
        let layout = layout.with_decoration_carrier(carrier(window), styles);
        assert!(
            layout.decorations.is_empty(),
            "no glyph starts inside the decorated byte range"
        );
    });
}

#[gpui::test]
fn mixed_run_decorations_keep_color_gaps_and_do_not_mark_empty_lf(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let mut styles = decorated();
        styles[0].len = 1;
        styles[0].strikethrough = None;
        styles.extend(runs("b"));
        let mut tail = decorated().remove(0);
        tail.len = 2;
        tail.underline.as_mut().unwrap().color = None;
        tail.color = gpui::blue();
        styles.push(tail);
        let layout = unequal_rows(window).with_decoration_carrier(carrier(window), styles);
        assert_eq!(layout.decorations.len(), 3);
        assert_eq!(layout.decorations[0].width, px(10.));
        assert_eq!(layout.decorations[1].width, px(30.));
        assert!(matches!(&layout.decorations[1].decoration, Decoration::Underline(style) if style.color == Some(gpui::blue())));
        let mut blank_runs = decorated();
        blank_runs[0].len = 2;
        let original: Vec<_> = window.text_system().shape_text("\n\n".into(), px(12.), &blank_runs, Some(px(100.)), None).unwrap().into_iter().collect();
        let plain: Vec<_> = window.text_system().shape_text("\n\n".into(), px(12.), &runs("\n\n"), Some(px(100.)), None).unwrap().into_iter().collect();
        let layout = BlockTextLayout::new(original, px(20.)).with_decoration_carrier(plain, blank_runs);
        assert!(layout.decorations.is_empty());
    });
}

#[gpui::test]
fn positioned_glyphs_inside_one_cluster_do_not_expand_or_overlap_strokes(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let mut layout = unequal_rows(window);
        let mut glyph_runs = layout.lines[0].runs().to_vec();
        glyph_runs[0].glyphs[1].index = 0;
        glyph_runs[0].glyphs[1].position.x = px(-2.);
        super::super::alignment_tests::replace_runs(&mut layout.lines[0], glyph_runs, 2);
        layout.rows = layout.measure_rows();
        let layout = layout
            .with_decoration_carrier(carrier(window), decorated())
            .aligned(BlockAlignment::Center, px(100.));
        assert_eq!(layout.decorations.len(), 4);
        for (index, stroke) in layout.decorations.iter().enumerate() {
            assert_eq!(stroke.origin.x, px(0.));
            assert_eq!(stroke.width, px(if index % 2 == 0 { 70. } else { 30. }));
        }
    });
}

#[gpui::test]
fn decoration_state_resets_on_lf_and_keeps_each_nonempty_lines_marked_run(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let mut styles = decorated();
        styles[0].len = 2;
        styles[0].strikethrough = None;
        styles.extend(runs("\n"));
        let mut tail = decorated().remove(0);
        tail.len = 2;
        tail.underline = None;
        tail.color = gpui::blue();
        styles.push(tail);
        let original = window.text_system().shape_text("ab\ncd".into(), px(12.), &styles, Some(px(100.)), None).unwrap();
        let mut stripped = styles.clone();
        for run in &mut stripped { run.underline = None; run.strikethrough = None; }
        let carrier = window.text_system().shape_text("ab\ncd".into(), px(12.), &stripped, Some(px(100.)), None).unwrap();
        let layout = BlockTextLayout::new(original.into_iter().collect(), px(20.)).with_decoration_carrier(carrier.into_iter().collect(), styles).aligned(BlockAlignment::Right, px(100.));
        assert_eq!(layout.decorations.len(), 2);
        assert_eq!(layout.decorations[0].row, 0);
        assert_eq!(layout.decorations[1].row, 1);
        assert_eq!(layout.decorations[0].width, layout.lines[0].unwrapped_layout.width);
        assert_eq!(layout.decorations[1].width, layout.lines[1].unwrapped_layout.width);
        assert!(matches!(&layout.decorations[0].decoration, Decoration::Underline(style) if style.color == Some(gpui::red())));
        assert!(matches!(&layout.decorations[1].decoration, Decoration::Strike(style) if style.color == Some(gpui::blue())));
    });
}
