#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Xiaomu runtime: editing-session orchestration on top of `xiaomu-core`.
//!
//! The runtime owns the [`session::DocumentSession`]: current snapshot,
//! current selection, basic undo/redo, and frontend-neutral change
//! notifications. Editing flows through typed intents that become Core
//! transactions plus intent-specific after-selection policies. Frontends
//! (GPUI first) sit on top of this crate and their types never leak into it.

mod session_clipboard;

pub mod clipboard;
pub mod persistence;
pub mod session;
