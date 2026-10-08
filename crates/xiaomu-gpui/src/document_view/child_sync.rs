//! Keep per-block native input entities stable across document paints.
use super::*;

impl DocumentView {
    /// Syncs child entities to the current snapshot's block list, dropping
    /// views whose nodes no longer exist.
    pub(super) fn sync_children(&mut self, cx: &mut Context<Self>) {
        self.focus_handle.get_or_insert_with(|| cx.focus_handle());
        self.sync_range_input(cx);
        let nodes: Vec<NodeId> = {
            let session = self.session.borrow();
            self.buildable_text_blocks(session.document())
                .into_iter()
                .map(|block| block.node)
                .collect()
        };

        let mut pool = std::mem::take(&mut self.children);
        let session = self.session.clone();
        let epoch = self.epoch.clone();
        let registry = self.registry.clone();
        self.children = nodes
            .into_iter()
            .map(|node| {
                if let Some(position) = pool.iter().position(|(id, _)| *id == node) {
                    pool.remove(position)
                } else {
                    let view = cx.new(|cx| {
                        ParagraphView::new_with_optional_history_clock(
                            session.clone(),
                            epoch.clone(),
                            registry.clone(),
                            node,
                            self.history_clock.clone(),
                            cx,
                        )
                    });
                    let feedback =
                        cx.subscribe(&view, |this, input, event: &EditorRejection, cx| {
                            this.forward_input_rejection(&input, event, cx);
                        });
                    view.update(cx, |view, _| view.rejection_feedback = Some(feedback));
                    (node, view)
                }
            })
            .collect();

        let scroll_handle = self.scroll_handle.clone();
        let atom_renderers = self.atom_renderers.clone();
        let text_sizes = self.prepare_text_size_styles();
        for (node, child) in &self.children {
            let alignment = self.block_alignment_provider.as_ref().and_then(|provider| {
                let session = self.session.borrow();
                session
                    .document()
                    .node(*node)
                    .map(|node| provider.alignment(node))
            });
            let scroll_handle = scroll_handle.clone();
            let atom_renderers = atom_renderers.clone();
            child.update(cx, |view, _| {
                view.attach_reading_state(self.reading.clone());
                view.attach_scroll_handle(scroll_handle);
                view.attach_atom_renderers(atom_renderers);
                view.attach_table_capability(self.table_capability.clone());
                view.set_code_block_presentation(self.code_block_presentation.clone());
                view.set_block_alignment(alignment);
                view.attach_text_size_capability(self.text_size_capability.clone());
                Self::attach_text_size_style(view, *node, &text_sizes);
            });
        }
        // Stale entries dropped with `pool`.
    }
}
