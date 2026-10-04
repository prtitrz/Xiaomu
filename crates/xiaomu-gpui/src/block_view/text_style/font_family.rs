//! CSS family-list parsing and native family resolution, without canonical edits.
//!
//! The host's web renderer splits on commas then quotes each piece. Here CSS
//! tokens preserve quoted commas and escaped names, and unquoted generic names
//! select native families. This is intentionally CSS interpretation, not a byte
//! rewrite of that web serialization. Font availability and glyphs are platform
//! dependent. Stock GPUI 0.2.2 Linux ignores explicit FontFallbacks during font
//! selection; its system glyph fallback remains active (macOS/Windows consume
//! the supplied chain). Do not promise identical fallback order across systems.

use cssparser::{ParseError, Parser, ParserInput, Token};
use gpui::{Font, FontFallbacks, TextSystem};

#[derive(Debug, PartialEq, Eq)]
enum Family {
    Named(String),
    Generic(String),
}

fn generic_candidates(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "system-ui" | "ui-sans-serif" => &[".SystemUIFont"],
        "sans-serif" => &[
            "Arial",
            "Noto Sans",
            "DejaVu Sans",
            "Liberation Sans",
            "Helvetica",
            ".SystemUIFont",
        ],
        "serif" | "ui-serif" => &[
            "Times New Roman",
            "Noto Serif",
            "DejaVu Serif",
            "Liberation Serif",
            "Times",
        ],
        "monospace" | "ui-monospace" => &[
            "Menlo",
            "Consolas",
            "DejaVu Sans Mono",
            "Liberation Mono",
            "Noto Sans Mono",
            "Courier New",
        ],
        "cursive" => &[
            "Apple Chancery",
            "Segoe Script",
            "Comic Sans MS",
            "URW Chancery L",
        ],
        "fantasy" => &["Papyrus", "Impact", "Copperplate"],
        "ui-rounded" => &["SF Pro Rounded", "Arial Rounded MT Bold"],
        "emoji" => &["Apple Color Emoji", "Segoe UI Emoji", "Noto Color Emoji"],
        "math" => &["STIX Two Math", "Cambria Math", "Noto Sans Math"],
        "fangsong" => &["FangSong", "STFangsong", "Noto Serif CJK SC"],
        _ => return None,
    })
}

fn parse_family<'i>(input: &mut Parser<'i, '_>) -> Result<Family, ParseError<'i, ()>> {
    let first = input.next()?.clone();
    if let Token::QuotedString(name) = first {
        input.expect_exhausted()?;
        return if name.is_empty() {
            Err(input.new_custom_error(()))
        } else {
            Ok(Family::Named(name.to_string()))
        };
    }
    let Token::Ident(first) = first else {
        return Err(input.new_custom_error(()));
    };
    let mut words = vec![first.to_string()];
    while !input.is_exhausted() {
        words.push(input.expect_ident()?.to_string());
    }
    if words.iter().any(|word| {
        matches!(
            word.to_ascii_lowercase().as_str(),
            "inherit" | "initial" | "unset" | "revert" | "revert-layer" | "default"
        )
    }) {
        return Err(input.new_custom_error(()));
    }
    let name = words.join(" ");
    let lower = name.to_ascii_lowercase();
    if words.len() == 1 && generic_candidates(&lower).is_some() {
        Ok(Family::Generic(lower))
    } else {
        Ok(Family::Named(name))
    }
}

fn parse_families(value: &str) -> Option<Vec<Family>> {
    let mut input = ParserInput::new(value);
    Parser::new(&mut input)
        .parse_entirely(|input| input.parse_comma_separated(parse_family))
        .ok()
}

/// One render-pass catalog, so newly added fonts are not hidden by stale state.
pub(in crate::block_view) struct FontCatalog<'a> {
    names: Vec<String>,
    system: Option<&'a TextSystem>,
}

impl<'a> FontCatalog<'a> {
    pub(in crate::block_view) fn from_system(system: &'a TextSystem) -> Self {
        Self {
            names: system.all_font_names(),
            system: Some(system),
        }
    }

    #[cfg(test)]
    pub(in crate::block_view) fn from_names(names: &[&str]) -> Self {
        Self {
            names: names.iter().map(|name| (*name).to_owned()).collect(),
            system: None,
        }
    }

    fn available(&self, name: &str) -> Option<String> {
        let name = self
            .names
            .iter()
            .find(|candidate| candidate.eq_ignore_ascii_case(name))?;
        if let Some(system) = self.system {
            // all_font_names also includes GPUI's fallback-stack aliases, even
            // if absent locally. Reject an alias that merely resolves to a
            // different fallback family, then try the next CSS candidate.
            let id = system.resolve_font(&gpui::font(name.clone()));
            if !system
                .get_font_for_id(id)
                .is_some_and(|font| font.family.as_ref() == name.as_str())
            {
                return None;
            }
        }
        Some(name.clone())
    }

    pub(in crate::block_view) fn apply(&self, value: &str, base: &Font) -> Font {
        let Some(families) = parse_families(value) else {
            return base.clone();
        };
        let mut selected = Vec::new();
        for family in families {
            let name = match family {
                Family::Named(name) => self.available(&name),
                Family::Generic(name) => generic_candidates(&name)
                    .and_then(|names| names.iter().find_map(|name| self.available(name))),
            };
            if let Some(name) = name {
                push_unique(&mut selected, name);
            }
        }
        if selected.is_empty() {
            return base.clone();
        }
        let primary = selected.remove(0);
        // Retain the host's known family and fallback chain after explicit
        // alternatives, including CJK and emoji fonts configured by the host.
        if !base.family.eq_ignore_ascii_case(&primary) {
            push_unique(&mut selected, base.family.to_string());
        }
        if let Some(fallbacks) = &base.fallbacks {
            for name in fallbacks.fallback_list() {
                if !name.eq_ignore_ascii_case(&primary) {
                    push_unique(&mut selected, name.clone());
                }
            }
        }
        let mut font = base.clone();
        font.family = primary.into();
        font.fallbacks = (!selected.is_empty()).then(|| FontFallbacks::from_fonts(selected));
        font
    }
}

fn push_unique(names: &mut Vec<String>, name: String) {
    if !names
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&name))
    {
        names.push(name);
    }
}

#[cfg(test)]
#[path = "font_family_tests.rs"]
mod tests;
