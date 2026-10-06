//! One immutable batch style projection per current document synchronization.

use super::DocumentView;
use crate::block_view::ParagraphView;
use crate::text_size::{TextSizeError, TextSizeErrorKind, TextSizeStyle};
use std::collections::BTreeMap;
use xiaomu_core::document::{DocumentRevision, NodeId};

type Styles = Option<(
    DocumentRevision,
    Result<BTreeMap<NodeId, TextSizeStyle>, TextSizeError>,
)>;

impl DocumentView {
    pub(super) fn prepare_text_size_styles(&self) -> Styles {
        let capability = self.text_size_capability.as_ref()?;
        let session = self.session.borrow();
        Some((
            session.document().revision(),
            capability.prepare_styles(session.document()),
        ))
    }

    pub(super) fn attach_text_size_style(view: &mut ParagraphView, node: NodeId, styles: &Styles) {
        if let Some((revision, styles)) = styles {
            let style = match styles {
                Ok(styles) => styles
                    .get(&node)
                    .cloned()
                    .ok_or_else(|| TextSizeError::new(node, 0..0, TextSizeErrorKind::InvalidStyle)),
                Err(error) => Err(error.clone()),
            };
            view.set_text_size_style(*revision, style);
        }
    }
}
