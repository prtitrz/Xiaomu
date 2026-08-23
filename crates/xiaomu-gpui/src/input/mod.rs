//! Input translation for the platform text pipeline.
//!
//! - [`utf16`]: UTF-16 ↔ UTF-8 offset conversion at the adapter boundary.
//! - [`composition`]: IME composition state machine (marked text stays a
//!   frontend transient and never enters the canonical document).

pub(crate) mod composition;
pub mod utf16;
