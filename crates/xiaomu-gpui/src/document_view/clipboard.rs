//! Clipboard actions preserve transport identity before optional host routing.

use super::{DocumentView, EditorRejectionReason, EditorRejectionStage};
use crate::block_view::{ClipboardCopy, ClipboardCut, ClipboardPaste};
use crate::editor_commands::{CodePasteSource, EditorCommand};
use crate::input::platform_clipboard::{
    PlatformClipboard, PlatformClipboardContent, prepare_lossless_slice,
};
use gpui::{Context, Window};
use xiaomu_core::document::NodeKind;
use xiaomu_runtime::clipboard::{
    ClipboardExportPurpose, normalize_multiline_paste_text, normalize_paste_text,
};
use xiaomu_runtime::session::{EditIntent, SessionError, SessionOutcome};

enum CutRejection {
    Session(SessionError),
    Metadata,
}

impl DocumentView {
    pub(crate) fn copy(&mut self, _: &ClipboardCopy, _: &mut Window, cx: &mut Context<Self>) {
        let slice = self
            .session
            .borrow()
            .clipboard_slice_for(ClipboardExportPurpose::Copy);
        match slice {
            Ok(Some(slice)) => {
                if !PlatformClipboard::new(&*cx).write_slice(&slice) {
                    self.emit_rejection(
                        EditorRejectionStage::ClipboardCopy,
                        EditorRejectionReason::ClipboardMetadata,
                        cx,
                    );
                }
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("xiaomu: clipboard projection failed: {error}");
                self.emit_session_rejection(EditorRejectionStage::ClipboardCopy, &error, cx);
            }
        }
    }

    pub(crate) fn cut(&mut self, _: &ClipboardCut, window: &mut Window, cx: &mut Context<Self>) {
        // Admission precedes every clipboard write, including the legacy path.
        // These presentation/composition guards are deliberately silent.
        if self.selection_has_hidden_table_endpoint() || self.focused_child_composing(window, cx) {
            return;
        }
        let prepared_outcome = {
            let mut session = self.session.borrow_mut();
            let result = match session.prepare_cut() {
                Ok(Some(prepared)) => match prepare_lossless_slice(prepared.clipboard_slice()) {
                    Some(item) => {
                        // Stock GPUI's writer is synchronous, returns unit and
                        // does not call into this session. Keep the same borrow
                        // until the exact prevalidated candidate is published.
                        PlatformClipboard::new(&*cx).write_prepared_item(item);
                        Ok(Some(prepared.publish()))
                    }
                    None => Err(CutRejection::Metadata),
                },
                Ok(None) => Ok(None),
                Err(error) => Err(CutRejection::Session(error)),
            };
            drop(session);
            result
        };
        // Both the opaque guard and RefMut are gone before rejection emission,
        // view synchronization or any other operation that reborrows session.
        match prepared_outcome {
            Ok(Some(outcome)) => {
                self.finish_cut(outcome, window, cx);
                return;
            }
            Ok(None) => {}
            Err(CutRejection::Session(error)) => {
                eprintln!("xiaomu: cut preparation failed: {error}");
                self.emit_session_rejection(EditorRejectionStage::ClipboardCut, &error, cx);
                return;
            }
            Err(CutRejection::Metadata) => {
                self.emit_rejection(
                    EditorRejectionStage::ClipboardCut,
                    EditorRejectionReason::ClipboardMetadata,
                    cx,
                );
                return;
            }
        }

        // No dedicated policy opted in: preserve the historical projection /
        // write / generic Delete route. Its Delete can still reject after a
        // write; prepared-Cut semantic atomicity does not extend to this path.
        let projected = self
            .session
            .borrow()
            .clipboard_slice_for(ClipboardExportPurpose::Cut);
        let slice = match projected {
            Ok(Some(slice)) => slice,
            Ok(None) => return,
            Err(error) => {
                eprintln!("xiaomu: clipboard projection failed: {error}");
                self.emit_session_rejection(EditorRejectionStage::ClipboardCut, &error, cx);
                return;
            }
        };
        if !PlatformClipboard::new(&*cx).write_slice_for_cut(&slice) {
            eprintln!("xiaomu: cut requires lossless structured clipboard metadata");
            self.emit_rejection(
                EditorRejectionStage::ClipboardCut,
                EditorRejectionReason::ClipboardMetadata,
                cx,
            );
            return;
        }
        self.apply_intent(EditIntent::Delete, window, cx);
    }

    fn finish_cut(&mut self, outcome: SessionOutcome, window: &mut Window, cx: &mut Context<Self>) {
        self.desired_x = None;
        if outcome != SessionOutcome::NoChange {
            self.epoch.set(self.epoch.get() + 1);
        }
        if outcome == SessionOutcome::DocumentChanged {
            self.sync_children(cx);
            self.route_focus(window, cx);
            self.request_focus_scroll(cx);
        }
        cx.notify();
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
        let content = match PlatformClipboard::new(&*cx).read_content_checked() {
            Ok(Some(content)) => content,
            Ok(None) => return,
            Err(_) => {
                self.emit_rejection(
                    EditorRejectionStage::ClipboardPaste,
                    EditorRejectionReason::ClipboardMetadata,
                    cx,
                );
                return;
            }
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
                    if slice.allows_default_fitting() {
                        // Preserve the original plain-code default only for
                        // ordinary open slices. Whole roots and CellRange
                        // carriers retain their boundary unless the host
                        // explicitly routes them to text above.
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

#[cfg(test)]
#[path = "cell_carrier_paste_tests.rs"]
mod cell_carrier_paste_tests;
