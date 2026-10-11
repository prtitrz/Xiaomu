//! These use GPUI's virtual text system; real font evidence is a separate gate.
use super::*;
use gpui::{TestAppContext, font, px};

fn input<'a>(text: &'a str, sizes: &'a [SizeSpan], width: f32, inset: f32) -> Input<'a> {
    Input {
        text,
        sizes,
        runs: &[],
        base_font: font(".SystemUIFont"),
        base_size: px(18.0),
        empty_size: None,
        base_color: gpui::black(),
        line_height: 1.5,
        wrap_width: px(width),
        first_line_inset: px(inset),
        limits: WorkLimits::default(),
    }
}

fn rows(system: &WindowTextSystem, request: Input<'_>) -> MixedLayout {
    let Layout::Mixed(layout) = layout(system, request).unwrap() else {
        panic!("positive inset must use real row shaping even at uniform size");
    };
    layout
}

#[gpui::test]
fn positive_inset_reflows_uniform_and_mixed_glyphs_only_first_visual_row(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let system = window.text_system();
        let text = "甲乙丙丁戊己庚辛\n壬癸甲乙丙丁";
        for spans in [
            vec![],
            vec![
                SizeSpan {
                    range: 0..12,
                    size: px(18.0),
                },
                SizeSpan {
                    range: 12..text.len(),
                    size: px(36.0),
                },
            ],
        ] {
            let result = rows(system, input(text, &spans, 80.0, 36.0));
            let first = &result.rows[0];
            assert!(!first.range.is_empty());
            assert!(first.width <= px(44.0));
            assert!(result.rows.len() > 2);
            assert!(result.rows[1..].iter().all(|row| row.width <= px(80.0)));
            let after_lf = result
                .rows
                .iter()
                .find(|row| row.range.start == 25)
                .unwrap();
            assert!(
                after_lf.width > px(44.0),
                "LF continuation retains full width"
            );
            assert_eq!(result.first_line_inset, px(36.0));
            let expected = result
                .rows
                .iter()
                .enumerate()
                .map(|(i, row)| row.width + if i == 0 { px(36.0) } else { px(0.0) })
                .fold(px(0.0), Pixels::max);
            assert_eq!(result.size.width, expected);
        }
    });
}

#[gpui::test]
fn inset_larger_than_box_overflows_first_cluster_without_empty_row_or_loop(
    cx: &mut TestAppContext,
) {
    cx.add_empty_window().update(|window, _| {
        for first in ["A", "中", "e\u{301}", "👨‍👩‍👧‍👦", "👍🏽"] {
            let text = format!("{first}甲乙丙丁戊己");
            for width in [0.01, 1.0, 20.0, 35.0, 36.0, 40.0] {
                let result = rows(window.text_system(), input(&text, &[], width, 36.0));
                assert_eq!(result.rows[0].range, 0..first.len());
                assert_eq!(result.rows[0].y, px(0.0));
                assert_eq!(result.rows[0].stops.len(), 2);
                assert!(result.rows.len() <= text.chars().count());
                assert_eq!(result.rows.last().unwrap().range.end, text.len());
            }
        }
    });
}

#[gpui::test]
fn empty_and_leading_lf_consume_inset_and_intrinsic_extent_includes_it(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let system = window.text_system();
        let empty = rows(system, input("", &[], 50.0, 36.25));
        assert_eq!(empty.rows.len(), 1);
        assert_eq!(empty.size.width, px(36.25));
        assert_eq!(empty.rows[0].range, 0..0);
        let full = rows(system, input("A", &[], 500.0, 36.25));
        assert_eq!(full.size.width, px(36.25) + full.rows[0].width);
        let leading = rows(system, input("\n甲乙丙丁戊己", &[], 60.0, 36.25));
        assert_eq!(leading.rows[0].range, 0..0);
        assert!(leading.rows[0].hard_break_after);
        assert_eq!(leading.rows[1].range.start, 1);
        assert!(leading.rows[1].width > px(23.75));
        assert_eq!(leading.rows[1].y, leading.rows[0].height);
    });
}

#[gpui::test]
fn inset_uniform_input_uses_conservative_gate_but_zero_preserves_native_unicode(
    cx: &mut TestAppContext,
) {
    cx.add_empty_window().update(|window, _| {
        for text in ["عربي", "אב", "क", "A\tB", "A\u{202e}B"] {
            assert_eq!(
                admission(window.text_system(), input(text, &[], 100.0, 36.0))
                    .unwrap_err()
                    .reason,
                Reason::ComplexScriptOrControl
            );
            for zero in [0.0, -0.0] {
                assert_eq!(
                    admission(window.text_system(), input(text, &[], 100.0, zero)).unwrap(),
                    Admission::Uniform
                );
                assert!(matches!(
                    layout(window.text_system(), input(text, &[], 100.0, zero)).unwrap(),
                    Layout::Uniform(_)
                ));
            }
        }
        for inset in [
            -1.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::MAX,
            65_537.0,
        ] {
            assert_eq!(
                admission(window.text_system(), input("abc", &[], 100.0, inset))
                    .unwrap_err()
                    .reason,
                Reason::InvalidInput
            );
        }
        let result = rows(window.text_system(), input("abc", &[], 1.0, 65_536.0));
        assert!(f32::from(result.size.width).is_finite());
    });
}

#[gpui::test]
fn first_line_inset_admission_is_width_independent_and_work_bounded(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        for width in [0.0, f32::NAN, 1.0, 150.0] {
            assert_eq!(
                admission(
                    window.text_system(),
                    input("Latin 中文 日本 한글 👩‍🚀", &[], width, 36.0)
                )
                .unwrap(),
                Admission::MixedLtr
            );
        }
        let mut bounded = input("abcd", &[], 150.0, 36.0);
        bounded.limits.max_shape_calls = 1;
        assert_eq!(
            admission(window.text_system(), bounded).unwrap_err().reason,
            Reason::WorkBudgetExceeded
        );
    });
}
