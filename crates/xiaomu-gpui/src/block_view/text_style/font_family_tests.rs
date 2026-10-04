use super::*;

#[test]
fn css_tokens_preserve_quotes_commas_escapes_and_normalize_only_projection_whitespace() {
    assert_eq!(
        parse_families("  'ACME, Text' , Noto   Sans  SC, sAnS-SeRiF "),
        Some(vec![
            Family::Named("ACME, Text".into()),
            Family::Named("Noto Sans SC".into()),
            Family::Generic("sans-serif".into()),
        ])
    );
    assert_eq!(
        parse_families(r#""Noto\20 Sans" , 'serif'"#),
        Some(vec![
            Family::Named("Noto Sans".into()),
            Family::Named("serif".into()),
        ])
    );
    for invalid in [
        "",
        " ",
        ",Arial",
        "Arial,",
        "Arial,,serif",
        "'Arial' extra",
        "12px",
        "var(--font)",
        "inherit",
        "inherit, Arial",
    ] {
        assert!(parse_families(invalid).is_none(), "{invalid}");
    }
}

#[test]
fn first_available_family_wins_and_remaining_families_keep_the_host_glyph_fallbacks() {
    let fonts = FontCatalog::from_names(&[
        "Noto Sans SC",
        "ACME, Text",
        "Noto Color Emoji",
        "DejaVu Serif",
    ]);
    let mut base = gpui::font("Host UI").bold().italic();
    base.fallbacks = Some(FontFallbacks::from_fonts(vec![
        "Noto Color Emoji".into(),
        "Host CJK".into(),
    ]));
    let result = fonts.apply("Missing, 'ACME, Text', noto sans sc, emoji", &base);
    assert_eq!(result.family.as_ref(), "ACME, Text");
    assert_eq!(result.weight, base.weight);
    assert_eq!(result.style, base.style);
    assert_eq!(result.features, base.features);
    assert_eq!(
        result.fallbacks.unwrap().fallback_list(),
        ["Noto Sans SC", "Noto Color Emoji", "Host UI", "Host CJK"]
    );
    assert_eq!(fonts.apply("serif", &base).family.as_ref(), "DejaVu Serif");
    assert_eq!(
        fonts.apply("'serif'", &base),
        base,
        "quoted generic is a literal family"
    );
}

#[test]
fn missing_invalid_and_css_wide_families_leave_the_entire_host_font_unchanged() {
    let fonts = FontCatalog::from_names(&["Noto Sans SC"]);
    let mut base = gpui::font("Host UI").bold();
    base.fallbacks = Some(FontFallbacks::from_fonts(vec!["Emoji".into()]));
    for value in [
        "Missing",
        "'Missing', AlsoMissing",
        "inherit",
        "initial",
        "unset",
        "revert",
        "Arial,",
        "",
        "'Noto Sans SC' garbage",
    ] {
        assert_eq!(fonts.apply(value, &base), base, "{value}");
    }
}
