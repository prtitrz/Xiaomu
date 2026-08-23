//! Input translation between GPUI/platform conventions and Core intents.
//!
//! This module owns the UTF-16 boundary conversion used by the platform
//! input path; keyboard actions translate to runtime intents inside the
//! block view.

pub mod utf16;
