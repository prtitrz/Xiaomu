//! Read-only CSS font-size resolution for native rendering capability checks.
//!
//! This module interprets a canonical string without rewriting it. It is not a
//! CSS cascade or a persistence validator. Unsupported values must remain intact
//! and must not be silently replaced by the parent size to enable editing.
//!
//! Absolute unit ratios follow [CSS Values and Units](https://www.w3.org/TR/css-values-4/#absolute-lengths).
//! The parent/root contexts and keyword distinction follow
//! [CSS Fonts](https://www.w3.org/TR/css-fonts-4/#font-size-prop). A successful
//! resolution is a computed logical CSS-pixel size, not proof of browser glyph,
//! line-height, physical-DPI, minimum-font-size or native layout parity.

use std::fmt;

use cssparser::{Parser, ParserInput, Token};
use xiaomu_core::document::StringAttribute;

/// Largest computed font size accepted by this rendering capability, in CSS px.
///
/// This is a resource budget, not a canonical schema constraint or a CSS maximum.
/// Zero, non-finite values, conversion underflow and values above this bound are
/// also unsupported. Values are rejected rather than clamped or normalized.
pub const MAX_FONT_SIZE_PX: f32 = 512.0;

/// Explicit computed-font context for resolving one non-root inline style.
///
/// These values are supplied by the host's inheritance/theme policy, never
/// guessed from a font-size string. Root-element self-referential `rem` rules
/// belong to that policy; `root_px` here is already the appropriate resolved base.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontSizeContext {
    parent_px: f32,
    root_px: f32,
    absolute_medium_px: f32,
}

impl FontSizeContext {
    /// Creates a context from finite positive logical CSS-pixel sizes.
    ///
    /// Parent and root values may exceed this renderer's budget: for example,
    /// a small relative multiplier may still produce a supported final size.
    /// The budget is enforced on the result of every resolution.
    pub fn new(
        parent_px: f32,
        root_px: f32,
        absolute_medium_px: f32,
    ) -> Result<Self, FontSizeError> {
        if [parent_px, root_px, absolute_medium_px]
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
        {
            return Err(FontSizeError::InvalidContext);
        }
        Ok(Self {
            parent_px,
            root_px,
            absolute_medium_px,
        })
    }

    /// Returns the computed parent size used by `em`, percentages and inheritance.
    #[must_use]
    pub const fn parent_px(&self) -> f32 {
        self.parent_px
    }

    /// Returns the explicitly supplied `rem` basis.
    #[must_use]
    pub const fn root_px(&self) -> f32 {
        self.root_px
    }

    /// Returns the host's explicit `medium`/initial-size basis.
    #[must_use]
    pub const fn absolute_medium_px(&self) -> f32 {
        self.absolute_medium_px
    }
}

/// Why a font-size value cannot be resolved by this rendering capability.
///
/// Every error means unsupported for this renderer. Callers must retain the
/// canonical attribute; these distinctions are diagnostic, never repair rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FontSizeError {
    /// A context basis is non-finite, zero or negative.
    InvalidContext,
    /// The simple value is invalid for CSS font-size, or contains extra tokens.
    InvalidCss,
    /// A recognized CSS form requires unsupported metrics, cascade or evaluation.
    ///
    /// Function bodies are intentionally not evaluated or certified as valid.
    /// This includes complex/future functions as well as valid `calc`/`var`.
    UnsupportedCss,
    /// A computed value is zero, non-finite, underflows or exceeds the budget.
    ///
    /// CSS permits zero and arbitrarily large nonnegative values; this renderer
    /// does not. Negative simple values instead report `InvalidCss`.
    OutOfBudget,
}

impl fmt::Display for FontSizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidContext => "font-size context must contain finite positive pixel sizes",
            Self::InvalidCss => "value is not a supported valid single CSS font-size value",
            Self::UnsupportedCss => {
                "CSS font-size requires unsupported rendering context or evaluation"
            }
            Self::OutOfBudget => {
                "font-size is outside the native renderer's positive finite 512px budget"
            }
        })
    }
}

impl std::error::Error for FontSizeError {}

/// Resolves an exact canonical font-size attribute to logical CSS pixels.
///
/// Supports `px`, `pt`, `pc`, `in`, `cm`, `mm`, `q`, `em`, `rem` and `%`.
/// Unit/keyword matching is ASCII-case-insensitive; CSS tokenization handles
/// decimals, exponents, whitespace, comments and escapes. The whole input must
/// be one property value, without a declaration, priority or trailing tokens.
///
/// Missing/null/empty values inherit the parent size, as do `inherit` and
/// `unset`. `initial` and `medium` use the explicit medium basis. Other absolute
/// keywords and `larger`/`smaller` need a user-agent mapping, so no ratio is
/// invented. Viewport/container/font-metric units, cascade rollback keywords,
/// math sizing and all functions (including `calc`/`var`) remain unsupported.
///
/// No source string is mutated, no font is loaded, and no layout is performed.
/// A caller must not turn an error into an editable default-sized document.
pub fn resolve_font_size(
    attribute: &StringAttribute,
    context: &FontSizeContext,
) -> Result<f32, FontSizeError> {
    let StringAttribute::Value(source) = attribute else {
        return checked_size(f64::from(context.parent_px));
    };
    let mut input = ParserInput::new(source);
    let mut parser = Parser::new(&mut input);
    if parser.is_exhausted() {
        return checked_size(f64::from(context.parent_px));
    }
    let token = parser
        .next()
        .map_err(|_| FontSizeError::InvalidCss)?
        .clone();
    parser
        .expect_exhausted()
        .map_err(|_| FontSizeError::InvalidCss)?;
    let pixels = match token {
        Token::Dimension { value, unit, .. } => {
            nonnegative(value)?;
            let factor = unit_factor(&unit, context)?;
            f64::from(value) * factor
        }
        Token::Percentage { unit_value, .. } => {
            nonnegative(unit_value)?;
            f64::from(unit_value) * f64::from(context.parent_px)
        }
        Token::Number { value: 0.0, .. } => return Err(FontSizeError::OutOfBudget),
        Token::Ident(keyword) => match keyword.to_ascii_lowercase().as_str() {
            "inherit" | "unset" => f64::from(context.parent_px),
            "initial" | "medium" => f64::from(context.absolute_medium_px),
            "xx-small" | "x-small" | "small" | "large" | "x-large" | "xx-large" | "xxx-large"
            | "larger" | "smaller" | "math" | "revert" | "revert-layer" => {
                return Err(FontSizeError::UnsupportedCss);
            }
            _ => return Err(FontSizeError::InvalidCss),
        },
        Token::Function(_) => return Err(FontSizeError::UnsupportedCss),
        _ => return Err(FontSizeError::InvalidCss),
    };
    checked_size(pixels)
}

fn nonnegative(value: f32) -> Result<(), FontSizeError> {
    if value < 0.0 {
        Err(FontSizeError::InvalidCss)
    } else if !value.is_finite() {
        Err(FontSizeError::OutOfBudget)
    } else {
        Ok(())
    }
}

fn unit_factor(unit: &str, context: &FontSizeContext) -> Result<f64, FontSizeError> {
    match unit.to_ascii_lowercase().as_str() {
        "px" => Ok(1.0),
        "pt" => Ok(96.0 / 72.0),
        "pc" => Ok(16.0),
        "in" => Ok(96.0),
        "cm" => Ok(96.0 / 2.54),
        "mm" => Ok(96.0 / 25.4),
        "q" => Ok(96.0 / 101.6),
        "em" => Ok(f64::from(context.parent_px)),
        "rem" => Ok(f64::from(context.root_px)),
        "ex" | "rex" | "cap" | "rcap" | "ch" | "rch" | "ic" | "ric" | "lh" | "rlh" | "vw"
        | "vh" | "vi" | "vb" | "vmin" | "vmax" | "svw" | "svh" | "svi" | "svb" | "svmin"
        | "svmax" | "lvw" | "lvh" | "lvi" | "lvb" | "lvmin" | "lvmax" | "dvw" | "dvh" | "dvi"
        | "dvb" | "dvmin" | "dvmax" | "cqw" | "cqh" | "cqi" | "cqb" | "cqmin" | "cqmax" => {
            Err(FontSizeError::UnsupportedCss)
        }
        _ => Err(FontSizeError::InvalidCss),
    }
}

fn checked_size(pixels: f64) -> Result<f32, FontSizeError> {
    if !pixels.is_finite() || pixels <= 0.0 || pixels > f64::from(MAX_FONT_SIZE_PX) {
        return Err(FontSizeError::OutOfBudget);
    }
    let pixels = pixels as f32;
    if pixels.is_finite() && pixels > 0.0 {
        Ok(pixels)
    } else {
        Err(FontSizeError::OutOfBudget)
    }
}

#[cfg(test)]
#[path = "font_size/tests.rs"]
mod tests;
