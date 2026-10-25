#![forbid(unsafe_code)]

//! Baseline Markdown import / export over the canonical Xiaomu document model.
//!
//! The codec owns the Markdown boundary: Markdown is never canonical editing
//! state and Markdown source offsets are not document positions (architecture
//! "Codec 边界"). It covers the built-in semantics that exist on `main` at the
//! P4.9 closeout:
//!
//! ```text
//! paragraph / ATX heading / quote
//! bullet / ordered list (tight, nested)
//! bold / italic / inline code / strikethrough / link
//! code block (fenced, `language` attr) / hard break
//! horizontal rule
//! image with external URL source
//! ```
//!
//! The contract refuses instead of dropping: a document that carries content
//! the baseline cannot represent (host `AssetRef` images, unknown node kinds,
//! inline atoms, underline, unknown attrs, empty paragraphs) fails export with
//! a [`MarkdownCodecError`], and the canonical document is never modified.
//! Hosts that need Markdown export for extension content write an adapter or
//! extension codec; the baseline invents no policy.
//!
//! Import is bounded to 1 MiB source, 20,000 nodes (including the root),
//! 64 levels below the root, and a shared 16 Mi conservative work units.
//! Export checks node/depth limits iteratively before recursive writing and
//! bounds a conservative output estimate to 1 MiB. Resource refusals use
//! [`MarkdownCodecError::ResourceLimit`]. Paragraph terminal LF is refused;
//! supported internal LF remains representable. Heading links remain unsupported.
//! Inline export validates exact text/marks; ambiguous delimiter combinations
//! fail rather than silently changing canonical content.

mod error;
mod escape;
mod inline;
mod limits;
mod parse;
mod serialize;
mod syntax;

pub use error::{MarkdownCodecError, Result};
pub use parse::from_markdown;
pub use serialize::to_markdown;

/// Bootstrap marker kept for host compatibility probes.
pub const CRATE_NAME: &str = "xiaomu-codec-markdown";

/// Canonical attribute key the codec reads and writes for fenced code blocks.
pub const CODE_BLOCK_ATTR_LANGUAGE: &str = "language";
