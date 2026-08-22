#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Canonical document semantics for the Xiaomu native structured rich-text
//! editor.
//!
//! `xiaomu-core` owns document structure, Unicode-safe text coordinates,
//! selections, typed transactions, position mapping, and inverse generation
//! for undo. It has no dependency on GPUI or a host application.
//!
//! All text offsets are UTF-8 byte offsets validated to fall on Unicode
//! scalar boundaries (see [`text`]); positions that no longer exist after an
//! edit surface as explicit deleted results instead of being clamped (see
//! [`mapping`]).

pub mod commands;
pub mod document;
pub mod history;
pub mod mapping;
pub mod selection;
pub mod text;
pub mod transaction;

mod error;

pub use error::{Error, Result};
