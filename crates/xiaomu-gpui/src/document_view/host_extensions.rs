//! Optional host callbacks; no callback receives mutable session state.

use super::DocumentView;
use crate::code_presentation::CodeBlockPresentation;
use crate::editor_commands::{
    CodePasteSource, CommandRoute, EditorCommand, EditorCommandContext, EditorCommandRouter,
    EnterSource,
};
use crate::list_marker::ListMarkerLabelProvider;
use gpui::{Context, Window};
use std::rc::Rc;

impl DocumentView {
    /// Offers a native atomic-block pointer gesture without first changing
    /// canonical selection or marks. Rejection never falls through to Atomic.
    pub(super) fn route_atomic_selection(
        &mut self,
        node: xiaomu_core::document::NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(router) = &self.command_router else {
            return false;
        };
        if self.focused_child_composing(window, cx) {
            return true;
        }
        let result = {
            let session = self.session.borrow();
            // A rendered pointer target may have disappeared since paint.
            // Do not offer a stale/non-atomic source to a host callback.
            if let Err(error) = xiaomu_runtime::session::DocumentSelection::collapsed(
                xiaomu_runtime::session::DocumentPosition::Atomic(node),
            )
            .validate(session.document())
            {
                eprintln!("xiaomu: atomic pointer target rejected: {error}");
                return true;
            }
            router.select_atomic(EditorCommandContext::from_session(&session), node)
        };
        match result {
            Ok(None) => return false,
            Ok(Some(selection)) => {
                let outcome = self.session.borrow_mut().set_document_selection(selection);
                match outcome {
                    Ok(_) => {
                        self.desired_x = None;
                        self.sync_children(cx);
                        self.route_focus(window, cx);
                        self.request_focus_scroll(cx);
                        cx.notify();
                    }
                    Err(error) => eprintln!("xiaomu: atomic pointer selection rejected: {error}"),
                }
            }
            Err(error) => eprintln!("xiaomu: host atomic pointer rejected: {error}"),
        }
        true
    }

    /// Sets an optional pure command router; `None` restores default routing.
    ///
    /// Changes only this view. Existing sibling views remain independent.
    pub fn set_command_router(&mut self, router: Option<Rc<dyn EditorCommandRouter>>) {
        self.command_router = router;
    }

    /// Sets visual list labels; `None` restores original bullets and ordinals.
    ///
    /// Hosts changing a mounted view should notify its GPUI context afterward.
    /// Labels are projected from fresh canonical attrs during normal rendering.
    pub fn set_list_marker_provider(&mut self, provider: Option<Rc<dyn ListMarkerLabelProvider>>) {
        self.list_marker_provider = provider;
    }

    /// Sets optional code-block presentation for this view only.
    ///
    /// `None` restores Xiaomu's original appearance. Hosts changing a mounted
    /// view should notify its GPUI context; the next child sync invalidates
    /// affected layout caches without editing the canonical document.
    pub fn set_code_block_presentation(&mut self, presentation: Option<CodeBlockPresentation>) {
        self.code_block_presentation = presentation;
    }

    pub(super) fn route_arrow_down(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.focused_child_composing(window, cx) {
            return true;
        }
        let Some(router) = &self.command_router else {
            return false;
        };
        let result = {
            let session = self.session.borrow();
            router.route_arrow_down(EditorCommandContext::from_session(&session))
        };
        match result {
            Ok(None) => return false,
            Ok(Some(selection)) => {
                let outcome = self.session.borrow_mut().set_document_selection(selection);
                match outcome {
                    Ok(_) => {
                        self.desired_x = None;
                        self.sync_children(cx);
                        self.route_focus(window, cx);
                        self.request_focus_scroll(cx);
                        cx.notify();
                    }
                    Err(error) => eprintln!("xiaomu: ArrowDown selection rejected: {error}"),
                }
            }
            Err(error) => eprintln!("xiaomu: host ArrowDown rejected: {error}"),
        }
        true
    }

    /// Root-range selection uses the same native range input proxy as cells.
    pub(super) fn route_select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.focused_child_composing(window, cx) {
            return true;
        }
        let Some(router) = &self.command_router else {
            return false;
        };
        let result = {
            let session = self.session.borrow();
            router.select_all(EditorCommandContext::from_session(&session))
        };
        match result {
            Ok(None) => return false,
            Ok(Some(selection)) => {
                let outcome = self.session.borrow_mut().set_document_selection(selection);
                match outcome {
                    Ok(_) => {
                        self.desired_x = None;
                        self.sync_children(cx);
                        self.route_focus(window, cx);
                        cx.notify();
                    }
                    Err(error) => eprintln!("xiaomu: Select All rejected: {error}"),
                }
            }
            Err(error) => eprintln!("xiaomu: host Select All rejected: {error}"),
        }
        true
    }

    /// Returns true if consumed or composition blocks the command.
    pub(super) fn route_editor_command(
        &mut self,
        command: EditorCommand<'_>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.route_host_command(window, cx, |router, context| router.route(context, command))
    }

    pub(super) fn route_enter_command(
        &mut self,
        source: EnterSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.route_host_command(window, cx, |router, context| {
            router.route_enter(context, source)
        })
    }

    pub(super) fn route_code_paste_command(
        &mut self,
        raw: &str,
        source: CodePasteSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.route_host_command(window, cx, |router, context| {
            router.route_code_paste(context, raw, source)
        })
    }

    pub(super) fn route_code_slice_command(
        &mut self,
        slice: &xiaomu_runtime::clipboard::ClipboardSlice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.route_host_command(window, cx, |router, context| {
            router.route_code_slice(context, slice)
        })
    }

    fn route_host_command(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        route: impl FnOnce(
            &dyn EditorCommandRouter,
            EditorCommandContext<'_>,
        ) -> Result<CommandRoute, xiaomu_runtime::session::PolicyError>,
    ) -> bool {
        if self.focused_child_composing(window, cx) {
            return true;
        }
        let Some(router) = &self.command_router else {
            return false;
        };
        let decision = {
            let session = self.session.borrow();
            route(
                router.as_ref(),
                EditorCommandContext::from_session(&session),
            )
        };
        match decision {
            Ok(CommandRoute::Default) => return false,
            Ok(CommandRoute::NoChange) => {}
            Ok(CommandRoute::Intent(intent)) => self.apply_intent(intent, window, cx),
            Err(error) => eprintln!("xiaomu: host command rejected: {error}"),
        }
        true
    }
}
