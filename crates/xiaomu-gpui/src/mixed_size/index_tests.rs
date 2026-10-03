//! Guard the two projections whose cost is not a number of shaping calls.
//! Long combining sequences deliberately have few EGCs but many scalar runs.
use super::*;
use gpui::{TestAppContext, font, px};

fn input<'a>(text: &'a str, runs: &'a [TextRun]) -> Input<'a> {
    Input {
        text,
        sizes: &[],
        runs,
        base_font: font(".SystemUIFont"),
        base_size: px(12.0),
        base_color: gpui::black(),
        line_height: 1.5,
        wrap_width: px(500.0),
        limits: WorkLimits::default(),
    }
}

#[test]
fn run_clipping_visits_only_indexed_intersections_of_a_large_mark_list() {
    let text = format!("a{} z", "\u{301}".repeat(10_000));
    let run_template = shape::Prepared::new(input(&text, &[])).unwrap().runs[0].clone();
    let runs: Vec<_> = text
        .chars()
        .enumerate()
        .map(|(index, character)| {
            let mut run = run_template.clone();
            run.len = character.len_utf8();
            run.color = if index % 2 == 0 {
                gpui::black()
            } else {
                gpui::white()
            };
            run
        })
        .collect();
    let prepared = shape::Prepared::new(input(&text, &runs)).unwrap();
    let tail = text.len() - 1..text.len();
    assert_eq!(prepared.run_indices(&tail), runs.len() - 1..runs.len());
    assert_eq!(prepared.clip_runs(tail), vec![runs.last().unwrap().clone()]);
    assert_eq!(prepared.run_indices(&(1..5)), 1..3);
    let middle = prepared.clip_runs(1..5);
    assert_eq!(middle, runs[1..3]);
    assert!(prepared.clip_runs(5..5).is_empty());
}

#[gpui::test]
fn linear_scalar_projection_equals_native_mapping_for_large_combining_cluster(
    cx: &mut TestAppContext,
) {
    cx.add_empty_window().update(|window, _| {
        let text = format!("a{}", "\u{301}".repeat(10_000));
        let prepared = shape::Prepared::new(input(&text, &[])).unwrap();
        let line = prepared.shape(window.text_system(), 0..text.len(), px(12.0));
        let fragments = vec![Fragment {
            range: 0..text.len(),
            x: px(5.0),
            line,
        }];
        let carets = rows::caret_positions(&fragments, 0);
        assert_eq!(carets.len(), 10_002);
        // Only sample native's linear scan. Calling it for all 10k positions
        // here would reintroduce the quadratic work the kernel avoids.
        for index in [0, 1, 3, 9_999, 19_999, text.len()] {
            let stop = carets.iter().find(|stop| stop.index == index).unwrap();
            assert_eq!(stop.x, px(5.0) + fragments[0].line.x_for_index(index));
        }
        assert_eq!(clusters::stops(&fragments[0].line, 0).unwrap().len(), 2);
    });
}

#[test]
fn many_paragraphs_with_scalar_marks_fit_budget_without_global_run_rescans() {
    let paragraph = format!("a{}\n", "\u{301}".repeat(125));
    let text = paragraph.repeat(800);
    let template = shape::Prepared::new(input(&text, &[])).unwrap().runs[0].clone();
    let runs: Vec<_> = text
        .chars()
        .map(|character| {
            let mut run = template.clone();
            run.len = character.len_utf8();
            run
        })
        .collect();
    let spans: Vec<_> = (0..800)
        .map(|index| SizeSpan {
            range: index * paragraph.len()..(index + 1) * paragraph.len(),
            size: px(if index % 2 == 0 { 12.0 } else { 24.0 }),
        })
        .collect();
    let mut request = input(&text, &runs);
    request.sizes = &spans;
    let prepared = shape::Prepared::new(request).unwrap();
    assert!(budget::check(&prepared).is_ok());
    let mut visits = 0;
    // Admission, measurement, and final row construction all clip the same
    // ranges here. The actual implementation iterates exactly this slice.
    for _ in 0..3 {
        for index in 0..800 {
            let range = index * paragraph.len()..(index + 1) * paragraph.len() - 1;
            let indices = prepared.run_indices(&range);
            visits += indices.len();
            assert_eq!(indices.len(), 126);
            assert_eq!(prepared.clip_runs(range).len(), indices.len());
        }
    }
    assert_eq!(visits, 3 * 800 * 126);
    assert!(visits <= 3 * runs.len());
}
