#![forbid(unsafe_code)]

//! Canonical document semantics for the Xiaomu native structured rich-text
//! editor.
//!
//! `xiaomu-core` owns document structure, Unicode-safe text coordinates,
//! selections, typed transactions, position mapping, and history primitives.
//! It has no dependency on GPUI or a host application.

pub mod commands;
pub mod document;
pub mod history;
pub mod mapping;
pub mod selection;
pub mod text;
pub mod transaction;

mod error;

pub use error::{Error, Result};
