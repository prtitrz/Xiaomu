//! Canonical line-break command construction.
//!
//! ADR 0004 makes LF the canonical inline line-break scalar. The public
//! constructor deliberately hides the current isolated-text implementation so
//! frontends do not couple line-break semantics to the `PasteText` variant.

use crate::session::EditIntent;

impl EditIntent {
    /// Builds an isolated canonical line-break command.
    ///
    /// In ordinary rich-text inline nodes the LF is a HardBreak; in a
    /// `CodeBlock` it is a code newline. The command owns one history entry
    /// and inherits Runtime StoredMarks exactly like other isolated text
    /// replacement. Soft-wrap never uses this command.
    #[must_use]
    pub fn insert_line_break() -> Self {
        Self::PasteText {
            text: "\n".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructor_compiles_to_one_canonical_lf() {
        assert_eq!(
            EditIntent::insert_line_break(),
            EditIntent::PasteText {
                text: "\n".to_owned()
            }
        );
    }
}
