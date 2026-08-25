//! GPUI platform binding for the runtime text-clipboard seam.
//!
//! This is the only place where the clipboard touches GPUI; the editing
//! layer sees only [`TextClipboard`] (see `xiaomu_runtime::clipboard`).
//! Non-text clipboard content reads as `None`.

use gpui::App;

use xiaomu_runtime::clipboard::TextClipboard;

/// [`TextClipboard`] implementation backed by the GPUI app clipboard.
pub(crate) struct PlatformClipboard<'a> {
    app: &'a App,
}

impl<'a> PlatformClipboard<'a> {
    /// Creates a clipboard adapter over the running GPUI app.
    pub(crate) fn new(app: &'a App) -> Self {
        Self { app }
    }
}

impl TextClipboard for PlatformClipboard<'_> {
    fn write_text(&mut self, text: String) {
        self.app
            .write_to_clipboard(gpui::ClipboardItem::new_string(text));
    }

    fn read_text(&self) -> Option<String> {
        self.app.read_from_clipboard()?.text()
    }
}
