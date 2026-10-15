//! Immutable bounded extension-kind descriptors, independent of host schemas.

use std::{fmt, sync::Arc};

use xiaomu_core::document::AtomKind;

use super::ClipboardTextProjection;

const MAX_KINDS: usize = 32;
const MAX_KIND_BYTES: usize = 128;

/// Validated exact extension kinds whose clipboard leaf text is LF.
///
/// Values are immutable, sorted by typed kind, and shared when cloned. Create
/// them through [`ClipboardTextProjection::text_between_lf_with_atoms`]. This
/// describes plaintext only; it grants no document, paste or asset permission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardLeafKinds(Arc<[AtomKind]>);

impl ClipboardLeafKinds {
    /// Returns the exact, sorted extension kinds, without typed HardBreak.
    #[must_use]
    pub fn atom_kinds(&self) -> &[AtomKind] {
        &self.0
    }

    pub(crate) fn contains(&self, kind: &AtomKind) -> bool {
        self.0.binary_search(kind).is_ok()
    }
}

/// An empty, duplicate, built-in or over-budget leaf-projection descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardProjectionError {
    message: &'static str,
}

impl fmt::Display for ClipboardProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for ClipboardProjectionError {}

impl ClipboardTextProjection {
    /// Extends LF/LF text-between with exact extension kinds treated as leaves.
    ///
    /// Accepts 1–32 distinct extension kinds, each at most 128 UTF-8 bytes.
    /// Empty, duplicate, oversized and typed HardBreak entries reject; HardBreak
    /// is already implicit. Extension `"hardBreak"` is distinct and may be
    /// explicitly listed. No trimming, case folding or prefix matching occurs.
    /// Iteration stops after at most 33 entries, before an unbounded collection
    /// can be built. Input order is canonicalized; clones share the descriptor.
    ///
    /// This opts into framed v15 transport. Unknown atoms and custom blocks
    /// still reject. The original `TextBetweenLfV1` behavior is unchanged.
    pub fn text_between_lf_with_atoms(
        kinds: impl IntoIterator<Item = AtomKind>,
    ) -> Result<Self, ClipboardProjectionError> {
        let mut collected = Vec::new();
        for kind in kinds {
            if collected.len() == MAX_KINDS {
                return Err(ClipboardProjectionError {
                    message: "clipboard leaf projection allows at most 32 atom kinds",
                });
            }
            if kind.is_hard_break() || kind.as_str().len() > MAX_KIND_BYTES {
                return Err(ClipboardProjectionError {
                    message: "clipboard leaf projection requires extension keys of at most 128 bytes",
                });
            }
            collected.push(kind);
        }
        if collected.is_empty() {
            return Err(ClipboardProjectionError {
                message: "clipboard leaf projection requires at least one atom kind",
            });
        }
        collected.sort();
        if collected.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ClipboardProjectionError {
                message: "clipboard leaf projection contains duplicate atom kinds",
            });
        }
        Ok(Self::TextBetweenLfWithAtomsV1(ClipboardLeafKinds(
            collected.into(),
        )))
    }

    pub(crate) fn emits_lf(&self, kind: &AtomKind) -> bool {
        kind.is_hard_break()
            || match self {
                Self::TextBetweenLfV1 => false,
                Self::TextBetweenLfWithAtomsV1(kinds) => kinds.contains(kind),
            }
    }
}
