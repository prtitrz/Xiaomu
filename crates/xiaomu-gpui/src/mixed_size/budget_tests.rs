use super::*;
use gpui::{TestAppContext, font, px};

fn input<'a>(text: &'a str, spans: &'a [SizeSpan], width: f32) -> Input<'a> {
    Input {
        text,
        sizes: spans,
        runs: &[],
        base_font: font(".SystemUIFont"),
        base_size: px(12.0),
        base_color: gpui::black(),
        line_height: 1.5,
        wrap_width: px(width),
        limits: WorkLimits::default(),
    }
}

#[test]
fn estimate_uses_checked_arithmetic_and_counts_empty_rows() {
    assert!(budget::paragraph_estimate(usize::MAX, 2, 2, 2).is_none());
    assert!(budget::paragraph_estimate(1, usize::MAX, 2, 2).is_none());
    assert!(budget::paragraph_estimate(1, 1, usize::MAX, 2).is_none());
    assert_eq!(
        budget::paragraph_estimate(0, 0, 0, 0),
        Some(budget::Estimate {
            shaped_bytes: 1,
            shape_calls: 1,
        })
    );
    // N=4, G=4, S=K=2: seam=16 bytes/6 calls, rest=28 bytes/16 calls.
    assert_eq!(
        budget::paragraph_estimate(4, 4, 2, 2),
        Some(budget::Estimate {
            shaped_bytes: 44,
            shape_calls: 22,
        })
    );
}

#[gpui::test]
fn exact_reserved_budget_covers_every_width_without_new_rejection(cx: &mut TestAppContext) {
    cx.add_empty_window().update(|window, _| {
        let spans = vec![
            SizeSpan {
                range: 0..2,
                size: px(12.0),
            },
            SizeSpan {
                range: 2..4,
                size: px(24.0),
            },
        ];
        for width in [0.01, 1.0, 7.0, 14.0, 28.0, 100.0, 1000.0] {
            let mut request = input("abcd", &spans, width);
            request.limits = WorkLimits {
                max_shaped_bytes: 44,
                max_shape_calls: 22,
            };
            assert_eq!(
                admission(window.text_system(), request.clone()).unwrap(),
                Admission::MixedLtr
            );
            let Layout::Mixed(result) = layout(window.text_system(), request).unwrap() else {
                panic!("mixed");
            };
            assert_eq!(result.rows.first().unwrap().range.start, 0);
            assert_eq!(result.rows.last().unwrap().range.end, 4);
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
fn exhausted_bytes_or_calls_reject_before_shaping_but_uniform_stays_native(
    cx: &mut TestAppContext,
) {
    cx.add_empty_window().update(|window, _| {
        let spans = vec![
            SizeSpan {
                range: 0..2,
                size: px(12.0),
            },
            SizeSpan {
                range: 2..4,
                size: px(24.0),
            },
        ];
        for limits in [
            WorkLimits {
                max_shaped_bytes: 43,
                max_shape_calls: 22,
            },
            WorkLimits {
                max_shaped_bytes: 44,
                max_shape_calls: 21,
            },
        ] {
            let mut request = input("abcd", &spans, 100.0);
            request.limits = limits;
            assert_eq!(
                admission(window.text_system(), request.clone())
                    .unwrap_err()
                    .reason,
                Reason::WorkBudgetExceeded
            );
            assert_eq!(
                layout(window.text_system(), request).unwrap_err().reason,
                Reason::WorkBudgetExceeded
            );
        }
        let mut uniform = input("عربي", &[], 100.0);
        uniform.limits = WorkLimits {
            max_shaped_bytes: 0,
            max_shape_calls: 0,
        };
        assert_eq!(
            admission(window.text_system(), uniform.clone()).unwrap(),
            Admission::Uniform
        );
        assert!(matches!(
            layout(window.text_system(), uniform).unwrap(),
            Layout::Uniform(_)
        ));
    });
}

#[test]
fn default_budget_admits_normal_paragraph_but_bounds_long_mixed_input() {
    for (length, expected) in [(500, true), (1100, false)] {
        let text = "a".repeat(length);
        let spans = vec![
            SizeSpan {
                range: 0..length / 2,
                size: px(12.0),
            },
            SizeSpan {
                range: length / 2..length,
                size: px(24.0),
            },
        ];
        let prepared = shape::Prepared::new(input(&text, &spans, 100.0)).unwrap();
        assert_eq!(budget::check(&prepared).is_ok(), expected);
    }
}
