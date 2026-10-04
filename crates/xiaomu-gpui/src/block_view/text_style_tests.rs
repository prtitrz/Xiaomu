use super::*;
use crate::block_view::project_display_content;
use xiaomu_core::document::{
    InlineContent, LinkAttributes, LinkMark, Mark, MarkSet, TextRun as CoreRun,
    TextStyleAttributes, TextStyleMark,
};

fn style(color: StringAttribute, family: StringAttribute) -> Mark {
    Mark::TextStyle(TextStyleMark::from_attributes(
        TextStyleAttributes::default()
            .with_color(color)
            .with_font_family(family),
    ))
}
fn value(text: &str) -> StringAttribute {
    StringAttribute::Value(text.into())
}

#[test]
fn css_color_parser_accepts_full_color3_values_and_rejects_partial_or_dynamic_values() {
    let red: Hsla = rgba(0xff0000ff).into();
    for raw in [
        "#f00",
        "#ff0000",
        "rgb(255, 0, 0)",
        "hsl(0, 100%, 50%)",
        "RED",
        " red ",
    ] {
        assert_eq!(css_color(raw), Some(red), "{raw}");
    }
    // Four-digit hex repeats the alpha nibble: 8 means 0x88, not 0x80.
    assert_eq!(css_color("#f008"), Some(rgba(0xff000088).into()));
    let translucent = Some(rgba(0xff000080).into());
    for raw in ["#ff000080", "rgba(255,0,0,0.5)", "hsla(0,100%,50%,0.5)"] {
        assert_eq!(css_color(raw), translucent, "{raw}");
    }
    assert_eq!(css_color("transparent"), Some(rgba(0x00000000).into()));
    assert!(css_color("rebeccapurple").is_some());
    for raw in [
        "",
        "currentColor",
        "inherit",
        "unset",
        "var(--ink)",
        "no-such-color",
        "red blue",
        "rgb(1,2,3) trailing",
    ] {
        assert_eq!(css_color(raw), None, "{raw}");
    }
}

#[test]
fn color_and_family_are_real_shaping_runs_and_merge_with_all_existing_marks() {
    let attrs = TextStyleAttributes::default()
        .with_color(value("rgba(255, 0, 0, .5)"))
        .with_font_family(value("Missing, 'Noto Sans SC', emoji"));
    let marks = MarkSet::new([
        Mark::Bold,
        Mark::Italic,
        Mark::Underline,
        Mark::Strike,
        Mark::Code,
        Mark::Link(LinkMark::from_attributes(LinkAttributes::default())),
        Mark::TextStyle(TextStyleMark::from_attributes(attrs.clone())),
    ])
    .unwrap();
    let content = InlineContent::new([
        CoreRun::new("中文🙂e\u{301}", marks).unwrap(),
        CoreRun::new(" plain", MarkSet::empty()).unwrap(),
    ])
    .unwrap();
    let before = content.clone();
    let (text, segments) = project_display_content(&content, None);
    let base = gpui::font("Host UI");
    let fonts = FontCatalog::from_names(&["Noto Sans SC", "Noto Color Emoji"]);
    let color = rgba(0x112233ff).into();
    let runs = text_runs(&segments, base.clone(), color, &fonts);
    assert_eq!(text, "中文🙂e\u{301} plain");
    assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), text.len());
    assert_eq!(runs[0].font.family.as_ref(), "Noto Sans SC");
    assert_eq!(runs[0].font.weight, FontWeight::BOLD);
    assert_eq!(runs[0].font.style, FontStyle::Italic);
    assert_eq!(
        runs[0].font.fallbacks.as_ref().unwrap().fallback_list(),
        ["Noto Color Emoji", "Host UI"]
    );
    assert_eq!(runs[0].color, rgba(0xff000080).into());
    assert_eq!(runs[0].underline.unwrap().color, Some(runs[0].color));
    assert_eq!(runs[0].strikethrough.unwrap().color, Some(runs[0].color));
    assert!(runs[0].background_color.is_some());
    assert_eq!(runs[1].font, base);
    assert_eq!(runs[1].color, color);
    assert_eq!(segments[0].text_style.as_ref(), Some(&attrs));
    assert_eq!(
        content, before,
        "projection preserves exact canonical bytes/marks"
    );
}

#[test]
fn absent_null_invalid_or_inherited_values_preserve_default_and_link_styling() {
    let fonts = FontCatalog::from_names(&[]);
    let base = gpui::font("Host UI");
    let color = rgba(0x112233ff).into();
    for attribute in [
        StringAttribute::Missing,
        StringAttribute::Null,
        value(""),
        value("inherit"),
        value("currentColor"),
        value("not-valid"),
    ] {
        for linked in [false, true] {
            let mut marks = vec![style(attribute.clone(), attribute.clone())];
            if linked {
                marks.push(Mark::Link(LinkMark::from_attributes(
                    LinkAttributes::default(),
                )));
            }
            let segment = DisplaySegment::from_marks(0, "中文🙂", &MarkSet::new(marks).unwrap());
            let runs = text_runs(&[segment], base.clone(), color, &fonts);
            assert_eq!(runs[0].font, base);
            assert_eq!(
                runs[0].color,
                if linked {
                    rgba(0x2563ebff).into()
                } else {
                    color
                }
            );
            assert_eq!(runs[0].underline.is_some(), linked);
        }
    }
}

#[test]
fn styled_preedit_and_neighbors_use_distinct_marks_without_changing_canonical_content() {
    let content = InlineContent::new([CoreRun::new("AB", MarkSet::empty()).unwrap()]).unwrap();
    let marks = MarkSet::new([Mark::Bold, style(value("green"), value("Noto Sans SC"))]).unwrap();
    let (text, segments) = project_display_content(&content, Some((1..1, "中文🙂", &marks)));
    let runs = text_runs(
        &segments,
        gpui::font("Host UI"),
        rgba(0x111111ff).into(),
        &FontCatalog::from_names(&["Noto Sans SC"]),
    );
    assert_eq!(text, "A中文🙂B");
    assert_eq!(runs[1].color, css_color("green").unwrap());
    assert_eq!(runs[1].font.family.as_ref(), "Noto Sans SC");
    assert_eq!(runs[1].font.weight, FontWeight::BOLD);
    assert!(runs[1].underline.is_some());
    assert!(runs[0].underline.is_none() && runs[2].underline.is_none());
    assert_eq!(project_display_content(&content, None).0, "AB");
}

#[test]
fn cache_fingerprint_tracks_host_style_and_resolved_family_without_an_epoch() {
    use crate::document_view::cache_key::style_fingerprint;
    let marks = MarkSet::new([style(value("red"), value("Optional Font, Fallback Font"))]).unwrap();
    let segments = [DisplaySegment::from_marks(0, "中文🙂", &marks)];
    let base = gpui::font("Host UI");
    let color = rgba(0x111111ff).into();
    let runs = text_runs(
        &segments,
        base.clone(),
        color,
        &FontCatalog::from_names(&["Optional Font", "Fallback Font"]),
    );
    let fingerprint = |font: &Font, color, size, height, runs: &[gpui::TextRun]| {
        style_fingerprint("中文🙂", font, color, px(size), px(height), runs)
    };
    let initial = fingerprint(&base, color, 18.0, 30.0, &runs);
    assert_ne!(
        initial,
        fingerprint(&gpui::font("Other Host"), color, 18.0, 30.0, &runs)
    );
    assert_ne!(
        initial,
        fingerprint(&base, rgba(0xffffffff).into(), 18.0, 30.0, &runs)
    );
    assert_ne!(initial, fingerprint(&base, color, 20.0, 30.0, &runs));
    assert_ne!(initial, fingerprint(&base, color, 18.0, 32.0, &runs));
    let changed = text_runs(
        &segments,
        base.clone(),
        color,
        &FontCatalog::from_names(&["Fallback Font"]),
    );
    assert_ne!(runs[0].font, changed[0].font);
    assert_ne!(initial, fingerprint(&base, color, 18.0, 30.0, &changed));
}
