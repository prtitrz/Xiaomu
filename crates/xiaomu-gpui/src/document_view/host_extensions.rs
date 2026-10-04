//! Optional host callbacks; no callback receives mutable session state.

use super::DocumentView;
use crate::editor_commands::{
    CommandRoute, EditorCommand, EditorCommandContext, EditorCommandRouter,
};
use crate::list_marker::ListMarkerLabelProvider;
use gpui::{Context, Window};
use std::rc::Rc;

impl DocumentView {
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

    /// Returns true if consumed or composition blocks the command.
    pub(super) fn route_editor_command(
        &mut self,
        command: EditorCommand<'_>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.focused_child_composing(window, cx) {
            return true;
        }
        let Some(router) = &self.command_router else {
            return false;
        };
        let decision = {
            let session = self.session.borrow();
            router.route(EditorCommandContext::from_session(&session), command)
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
