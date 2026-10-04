//! Clipboard actions preserve transport identity before optional host routing.

use super::DocumentView;
use crate::block_view::{ClipboardCopy, ClipboardCut, ClipboardPaste};
use crate::editor_commands::{CodePasteSource, EditorCommand};
use crate::input::platform_clipboard::{PlatformClipboard, PlatformClipboardContent};
use gpui::{Context, Window};
use xiaomu_core::document::NodeKind;
use xiaomu_runtime::clipboard::{
    ClipboardExportPurpose, normalize_multiline_paste_text, normalize_paste_text,
};
use xiaomu_runtime::session::EditIntent;

impl DocumentView {
    pub(crate) fn copy(&mut self, _: &ClipboardCopy, _: &mut Window, cx: &mut Context<Self>) {
        match self
            .session
            .borrow()
            .clipboard_slice_for(ClipboardExportPurpose::Copy)
        {
            Ok(Some(slice)) => PlatformClipboard::new(&*cx).write_slice(&slice),
            Ok(None) => {}
            Err(error) => eprintln!("xiaomu: clipboard projection failed: {error}"),
        }
    }

    pub(crate) fn cut(&mut self, _: &ClipboardCut, window: &mut Window, cx: &mut Context<Self>) {
        let slice = match self
            .session
            .borrow()
            .clipboard_slice_for(ClipboardExportPurpose::Cut)
        {
            Ok(Some(slice)) => slice,
            Ok(None) => return,
            Err(error) => {
                eprintln!("xiaomu: clipboard projection failed: {error}");
                return;
            }
        };
        if !PlatformClipboard::new(&*cx).write_slice_for_cut(&slice) {
            eprintln!("xiaomu: cut requires lossless structured clipboard metadata");
            return;
        }
        // Clipboard projection is read-only; Delete remains the one history
        // mutation for the whole cut command.
        self.apply_intent(EditIntent::Delete, window, cx);
    }

    pub(crate) fn paste(
        &mut self,
        _: &ClipboardPaste,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.focused_child_composing(window, cx) {
            return;
        }
        let Some(content) = PlatformClipboard::new(&*cx).read_content() else {
            return;
        };
        let code_block = matches!(self.focused_node_kind(), Some(NodeKind::CodeBlock));
        match content {
            PlatformClipboardContent::Image {
                format,
                bytes,
                plain_text,
            } => {
                // Hosts may explicitly prefer nonempty raw text in a code
                // target even when the clipboard also offers image data.
                // Default still reaches the unchanged image path below.
                if code_block
                    && let Some(raw) = plain_text.as_deref().filter(|raw| !raw.is_empty())
                    && self.route_code_paste_command(raw, CodePasteSource::PlatformText, window, cx)
                {
                    return;
                }
                let selection = self.session.borrow().selection();
                if code_block
                    || !selection.is_collapsed()
                    || !matches!(selection.focus(), super::DocumentPosition::Inline(_))
                {
                    eprintln!(
                        "xiaomu: image paste requires a collapsed text caret outside code blocks"
                    );
                    return;
                }
                let Some(service) = &self.asset_service else {
                    eprintln!("xiaomu: image paste requires a host asset service");
                    return;
                };
                match service.import_image(format, &bytes) {
                    Ok(image)
                        if matches!(
                            image.source(),
                            xiaomu_core::document::ImageSource::AssetRef(_)
                        ) =>
                    {
                        self.apply_intent(EditIntent::InsertImage { image }, window, cx);
                    }
                    Ok(_) => eprintln!("xiaomu: image import must return a host asset reference"),
                    Err(error) => eprintln!("xiaomu: image import failed: {error:?}"),
                }
            }
            PlatformClipboardContent::Structured(slice) => {
                if code_block {
                    if self.route_code_slice_command(&slice, window, cx) {
                        return;
                    }
                    if !slice.is_closed() {
                        // Preserve the original plain-code default only for
                        // open slices. Closed whole-root data must keep its
                        // structured meaning unless the host opts into text.
                        let text = normalize_multiline_paste_text(slice.plain_text());
                        if !text.is_empty() {
                            self.apply_intent(EditIntent::PasteText { text }, window, cx);
                        }
                        return;
                    }
                }
                self.apply_intent(EditIntent::PasteSlice { slice }, window, cx);
            }
            PlatformClipboardContent::Text(text) => {
                let consumed = if code_block {
                    self.route_code_paste_command(&text, CodePasteSource::PlatformText, window, cx)
                } else {
                    self.route_editor_command(EditorCommand::PlainTextPaste(&text), window, cx)
                };
                if consumed {
                    return;
                }
                let text = if code_block {
                    normalize_multiline_paste_text(&text)
                } else {
                    normalize_paste_text(&text)
                };
                if !text.is_empty() {
                    self.apply_intent(EditIntent::PasteText { text }, window, cx);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "clipboard_export_tests.rs"]
mod export_tests;

#[cfg(test)]
#[path = "clipboard_native_tests.rs"]
mod native_tests;
