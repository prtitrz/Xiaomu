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

mod error;
mod escape;
mod inline;
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
