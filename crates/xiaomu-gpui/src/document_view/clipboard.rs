//! Clipboard actions retain platform transport and route only ordinary text.

use super::DocumentView;
use crate::block_view::{ClipboardCopy, ClipboardCut, ClipboardPaste};
use crate::editor_commands::EditorCommand;
use crate::input::platform_clipboard::{PlatformClipboard, PlatformClipboardContent};
use gpui::{Context, Window};
use xiaomu_core::document::NodeKind;
use xiaomu_runtime::clipboard::{normalize_multiline_paste_text, normalize_paste_text};
use xiaomu_runtime::session::EditIntent;

impl DocumentView {
    pub(crate) fn copy(&mut self, _: &ClipboardCopy, _: &mut Window, cx: &mut Context<Self>) {
        match self.session.borrow().clipboard_slice() {
            Ok(Some(slice)) => PlatformClipboard::new(&*cx).write_slice(&slice),
            Ok(None) => {}
            Err(error) => eprintln!("xiaomu: clipboard projection failed: {error}"),
        }
    }

    pub(crate) fn cut(&mut self, _: &ClipboardCut, window: &mut Window, cx: &mut Context<Self>) {
        let slice = match self.session.borrow().clipboard_slice() {
            Ok(Some(slice)) => slice,
            Ok(None) => return,
            Err(error) => {
                eprintln!("xiaomu: clipboard projection failed: {error}");
                return;
            }
        };
        PlatformClipboard::new(&*cx).write_slice(&slice);
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
            PlatformClipboardContent::Image { format, bytes } => {
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
            PlatformClipboardContent::Structured(slice) if code_block => {
                // CodeBlock is a plain-code surface. Xiaomu-native rich
                // structure is flattened to the same interoperable text the
                // system clipboard exposes, preserving canonical LF while
                // discarding paragraph/list/mark semantics.
                let text = normalize_multiline_paste_text(slice.plain_text());
                if !text.is_empty() {
                    self.apply_intent(EditIntent::PasteText { text }, window, cx);
                }
            }
            PlatformClipboardContent::Structured(slice) => {
                self.apply_intent(EditIntent::PasteSlice { slice }, window, cx);
            }
            PlatformClipboardContent::Text(text) => {
                if !code_block
                    && self.route_editor_command(EditorCommand::PlainTextPaste(&text), window, cx)
                {
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
