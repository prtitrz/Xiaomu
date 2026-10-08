//! Shared scroll surface and ordinary editor action routing.
use super::DocumentView;
use gpui::{Context, MouseButton, div, prelude::*, px};

impl DocumentView {
    pub(super) fn render_scroll_tree(
        &self,
        tree: gpui::AnyElement,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        super::reading_surface::ReadingSurface::new(
            cx.entity(),
            div()
                .key_context("XiaomuDocument")
                .track_focus(self.focus_handle.as_ref().expect("synced focus handle"))
                .size_full()
                .bg(gpui::white())
                .p_4()
                .line_height(px(28.0))
                .text_size(px(20.0))
                .text_color(gpui::black())
                .cursor(gpui::CursorStyle::IBeam)
                .id("xiaomu-document-scroll")
                .track_scroll(&self.scroll_handle)
                .overflow_y_scroll()
                .when(self.table_capability.borrow().enabled(), |scroll| {
                    scroll.overflow_x_scroll()
                })
                .on_action(cx.listener(Self::backspace))
                .on_action(cx.listener(Self::delete))
                .on_action(cx.listener(Self::left))
                .on_action(cx.listener(Self::right))
                .on_action(cx.listener(Self::up))
                .on_action(cx.listener(Self::down))
                .on_action(cx.listener(Self::select_left))
                .on_action(cx.listener(Self::select_right))
                .on_action(cx.listener(Self::select_up))
                .on_action(cx.listener(Self::select_down))
                .on_action(cx.listener(Self::home))
                .on_action(cx.listener(Self::end))
                .on_action(cx.listener(Self::select_home))
                .on_action(cx.listener(Self::select_end))
                .on_action(cx.listener(Self::select_all))
                .on_action(cx.listener(Self::select_cell))
                .on_action(cx.listener(Self::escape_cell_range))
                .on_action(cx.listener(Self::enter))
                .on_action(cx.listener(Self::hard_break))
                .on_action(cx.listener(Self::primary_modifier_enter))
                .on_action(cx.listener(Self::tab_indent))
                .on_action(cx.listener(Self::shift_tab_indent))
                .on_action(cx.listener(Self::undo_entry))
                .on_action(cx.listener(Self::redo_entry))
                .on_action(cx.listener(Self::save_document))
                .on_action(cx.listener(Self::copy))
                .on_action(cx.listener(Self::cut))
                .on_action(cx.listener(Self::paste))
                .on_action(cx.listener(Self::toggle_bold))
                .on_action(cx.listener(Self::toggle_italic))
                .on_action(cx.listener(Self::toggle_code))
                .on_action(cx.listener(Self::toggle_underline))
                .on_action(cx.listener(Self::toggle_strike))
                .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                .on_mouse_move(cx.listener(Self::on_mouse_move))
                .child(tree)
                .into_any_element(),
        )
        .into_any_element()
    }
}
