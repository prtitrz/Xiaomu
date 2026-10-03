use super::*;

fn context() -> FontSizeContext {
    FontSizeContext::new(24.0, 16.0, 18.0).unwrap()
}
fn resolve(value: &str) -> Result<f32, FontSizeError> {
    resolve_font_size(&StringAttribute::Value(value.into()), &context())
}
fn near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
}

#[test]
fn toolbar_sizes_resolve_to_the_requested_pixels() {
    for size in [12, 14, 16, 18, 20, 24, 28, 32, 36, 48] {
        assert_eq!(resolve(&format!("{size}px")), Ok(size as f32));
    }
}

#[test]
fn absolute_length_units_use_css_reference_pixel_ratios() {
    for value in ["96px", "72pt", "6pc", "1in", "2.54cm", "25.4mm", "101.6q"] {
        near(resolve(value).unwrap(), 96.0);
    }
    near(resolve(".5PT").unwrap(), 2.0 / 3.0);
    near(resolve("1Q").unwrap(), 96.0 / 101.6);
}

#[test]
fn relative_units_use_distinct_explicit_parent_and_root_contexts() {
    assert_eq!(resolve("1.5em"), Ok(36.0));
    assert_eq!(resolve("1.5rem"), Ok(24.0));
    assert_eq!(resolve("150%"), Ok(36.0));
    let other = FontSizeContext::new(40.0, 10.0, 12.0).unwrap();
    for (value, expected) in [("1.5em", 60.0), ("1.5rem", 15.0), ("150%", 60.0)] {
        assert_eq!(resolve_font_size(&value.into(), &other), Ok(expected));
    }
}

#[test]
fn absence_null_empty_and_css_inheritance_preserve_the_original_attribute() {
    for attribute in [
        StringAttribute::Missing,
        StringAttribute::Null,
        "".into(),
        " \t\r\n".into(),
        "/* comment */".into(),
        "inherit".into(),
        "UnSeT".into(),
    ] {
        let original = attribute.clone();
        assert_eq!(resolve_font_size(&attribute, &context()), Ok(24.0));
        assert_eq!(attribute, original);
    }
}

#[test]
fn initial_and_medium_use_the_supplied_medium_not_parent_or_root() {
    assert_eq!(resolve("initial"), Ok(18.0));
    assert_eq!(resolve("MEDIUM"), Ok(18.0));
    let ctx = context();
    assert_eq!(
        (ctx.parent_px(), ctx.root_px(), ctx.absolute_medium_px()),
        (24.0, 16.0, 18.0)
    );
}

#[test]
fn css_tokens_allow_case_space_decimals_exponents_and_escapes() {
    for (value, expected) in [
        ("  +12.5PX  ", 12.5),
        (".5em", 12.0),
        ("1.2e1px", 12.0),
        ("12px/**/", 12.0),
        ("/**/12px", 12.0),
        ("12p\\78", 12.0),
        ("in\\68 erit", 24.0),
    ] {
        near(resolve(value).unwrap(), expected);
    }
}

#[test]
fn absolute_and_relative_keywords_do_not_invent_ua_mapping_ratios() {
    for value in [
        "xx-small",
        "x-small",
        "small",
        "large",
        "x-large",
        "xx-large",
        "xxx-large",
        "larger",
        "smaller",
        "math",
        "revert",
        "revert-layer",
    ] {
        assert_eq!(
            resolve(value),
            Err(FontSizeError::UnsupportedCss),
            "{value}"
        );
    }
}

#[test]
fn valid_unavailable_units_are_unsupported_rather_than_invalid() {
    for unit in [
        "ex", "rex", "cap", "rcap", "ch", "rch", "ic", "ric", "lh", "rlh", "vw", "vh", "vi", "vb",
        "vmin", "vmax", "svw", "svh", "svi", "svb", "svmin", "svmax", "lvw", "lvh", "lvi", "lvb",
        "lvmin", "lvmax", "dvw", "dvh", "dvi", "dvb", "dvmin", "dvmax", "cqw", "cqh", "cqi", "cqb",
        "cqmin", "cqmax",
    ] {
        assert_eq!(
            resolve(&format!("2{unit}")),
            Err(FontSizeError::UnsupportedCss),
            "{unit}"
        );
    }
}

#[test]
fn complex_functions_are_not_evaluated_or_silently_inherited() {
    for value in [
        "calc(12px + 1em)",
        "var(--size)",
        "min(12px, 2em)",
        "max(12px, 2em)",
        "clamp(12px, 1vw, 30px)",
        "env(size)",
        "future(1px)",
        "calc(infinity * 1px)",
        "calc(NaN * 1px)",
    ] {
        assert_eq!(
            resolve(value),
            Err(FontSizeError::UnsupportedCss),
            "{value}"
        );
    }
}

#[test]
fn invalid_property_values_are_distinct_from_missing_capability() {
    for value in [
        "12",
        "1.5",
        "12 px",
        "12/**/px",
        "12px 14px",
        "12px;",
        "font-size:12px",
        "12px!important",
        "red",
        "auto",
        "normal",
        "12degrees",
        "12s",
        "12foo",
        "'12px'",
        "#12px",
        "[12px]",
        "{12px}",
        ")",
        "NaN",
        "infinity",
        "12px;background:red",
    ] {
        assert_eq!(resolve(value), Err(FontSizeError::InvalidCss), "{value}");
    }
}

#[test]
fn zero_is_valid_css_but_outside_the_positive_rendering_budget() {
    for value in ["0", "-0", "+0", "0px", "-0em", "0%", "0pt", "1e-999px"] {
        assert_eq!(resolve(value), Err(FontSizeError::OutOfBudget), "{value}");
    }
}

#[test]
fn negative_simple_lengths_and_percentages_are_invalid_css() {
    for value in ["-1px", "-1em", "-1rem", "-0.5%", "-1Q", "-1vw", "-1e999px"] {
        assert_eq!(resolve(value), Err(FontSizeError::InvalidCss), "{value}");
    }
}

#[test]
fn upper_budget_is_inclusive_and_applies_after_conversion() {
    for value in ["512px", "384pt", "32pc", "32rem"] {
        assert_eq!(resolve(value), Ok(MAX_FONT_SIZE_PX), "{value}");
    }
    for value in [
        "512.01px", "385pt", "6in", "33rem", "2200%", "1e39px", "1e999px",
    ] {
        assert_eq!(resolve(value), Err(FontSizeError::OutOfBudget), "{value}");
    }
    let tiny = resolve(".00001px").unwrap();
    assert!(tiny.is_finite() && tiny > 0.0);
}

#[test]
fn contexts_reject_non_finite_and_nonpositive_bases_in_every_position() {
    for index in 0..3 {
        for invalid in [0.0, -0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut values = [24.0, 16.0, 18.0];
            values[index] = invalid;
            assert_eq!(
                FontSizeContext::new(values[0], values[1], values[2]),
                Err(FontSizeError::InvalidContext)
            );
        }
    }
}

#[test]
fn budget_checks_inherited_sizes_without_rejecting_useful_large_contexts() {
    let context = FontSizeContext::new(1024.0, 2048.0, 768.0).unwrap();
    for attribute in [
        StringAttribute::Missing,
        StringAttribute::Null,
        "".into(),
        "inherit".into(),
        "unset".into(),
        "initial".into(),
        "medium".into(),
    ] {
        assert_eq!(
            resolve_font_size(&attribute, &context),
            Err(FontSizeError::OutOfBudget)
        );
    }
    for (value, expected) in [
        (".25em", 256.0),
        (".125rem", 256.0),
        ("25%", 256.0),
        ("12px", 12.0),
    ] {
        assert_eq!(resolve_font_size(&value.into(), &context), Ok(expected));
    }
    let huge = FontSizeContext::new(f32::MAX, f32::MAX, f32::MAX).unwrap();
    assert_eq!(
        resolve_font_size(&"2em".into(), &huge),
        Err(FontSizeError::OutOfBudget)
    );
    let tiny = FontSizeContext::new(f32::from_bits(1), 16.0, 18.0).unwrap();
    assert_eq!(
        resolve_font_size(&".1em".into(), &tiny),
        Err(FontSizeError::OutOfBudget)
    );
}

#[test]
fn unsupported_values_and_diagnostics_never_rewrite_canonical_strings() {
    for value in [
        "48 PX",
        "calc(2em + 1px)",
        "var(--size)",
        "513px",
        "  -2px ",
        "1e999px",
        "未知🙂",
    ] {
        let attribute = StringAttribute::Value(value.into());
        let before = attribute.clone();
        let error = resolve_font_size(&attribute, &context()).unwrap_err();
        assert!(!error.to_string().is_empty());
        assert_eq!(attribute, before);
    }
}
