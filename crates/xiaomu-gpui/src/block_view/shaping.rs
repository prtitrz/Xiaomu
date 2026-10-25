//! Bound the temporary native line array for uniformly styled multiline text.
use gpui::{Pixels, SharedString, TextRun, Window, WrappedLine};

pub(super) fn shape_text(
    window: &Window,
    text: SharedString,
    font_size: Pixels,
    runs: &[TextRun],
    width: Option<Pixels>,
) -> gpui::Result<Vec<WrappedLine>> {
    // Stock GPUI grows SmallVec<WrappedLine> geometrically. Each WrappedLine
    // embeds 32 decoration slots, so a long CodeBlock creates multi-MiB buffers.
    // With one run, splitting exactly as GPUI does preserves LF/empty-line and
    // decoration semantics without reimplementing mixed-run slicing.
    if let [run] = runs
        && run.len == text.len()
        && text.contains('\n')
    {
        let count = text.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let mut lines = Vec::with_capacity(count);
        for line in text.split('\n') {
            let mut run = run.clone();
            run.len = line.len();
            lines.extend(window.text_system().shape_text(
                line.to_owned().into(),
                font_size,
                &[run],
                width,
                None,
            )?);
        }
        Ok(lines)
    } else {
        Ok(window
            .text_system()
            .shape_text(text, font_size, runs, width, None)?
            .into_iter()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, px};

    #[gpui::test]
    fn uniform_multiline_shaping_matches_stock_empty_lines_unicode_and_decorations(
        cx: &mut TestAppContext,
    ) {
        cx.add_empty_window().update(|window, _| {
            for text in [
                "",
                "plain",
                "\n",
                "\n\n",
                "a\n",
                "中文 e\u{301} 🌲\n\nend",
                "a\r\nb",
            ] {
                for width in [None, Some(px(15.)), Some(px(300.))] {
                    let runs = [TextRun {
                        len: text.len(),
                        font: gpui::font(".SystemUIFont"),
                        color: gpui::red(),
                        background_color: Some(gpui::blue()),
                        underline: Some(gpui::UnderlineStyle {
                            thickness: px(1.),
                            color: None,
                            wavy: true,
                        }),
                        strikethrough: Some(gpui::StrikethroughStyle {
                            thickness: px(1.),
                            color: None,
                        }),
                    }];
                    let expected = window
                        .text_system()
                        .shape_text(text.to_owned().into(), px(16.), &runs, width, None)
                        .unwrap();
                    let actual =
                        shape_text(window, text.to_owned().into(), px(16.), &runs, width).unwrap();
                    assert_eq!(actual.len(), expected.len());
                    for (actual, expected) in actual.iter().zip(&expected) {
                        // Pinned GPUI's Debug includes private decoration runs as
                        // well as glyph/cluster/wrap geometry and exact line text.
                        assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
                    }
                }
            }
        });
    }
}
