//! Temporary view boundary until logical span geometry has a renderer.
//!
//! Canonical traversal and Runtime inline editing stay unchanged. Only this
//! renderer's editable surfaces and navigation omit placeholder descendants.
use std::collections::HashMap;

use xiaomu_core::document::{NodeId, XiaomuDocument};
use xiaomu_runtime::session::{DocumentPosition, DocumentSelection};

use super::{
    DocumentView,
    navigation::{self, NavUnit, TextBlock},
};

pub(super) fn rendered_nav_units(document: &XiaomuDocument) -> Vec<NavUnit> {
    fn visible(document: &XiaomuDocument, node: NodeId, cache: &mut HashMap<NodeId, bool>) -> bool {
        if let Some(visible) = cache.get(&node) {
            return *visible;
        }
        let result = !navigation::table_needs_placeholder(document, node)
            && document
                .parent_of(node)
                .is_none_or(|parent| visible(document, parent, cache));
        cache.insert(node, result);
        result
    }
    // Keep canonical ordering, caching ancestry so each table grid is checked
    // once rather than rebuilding its occupancy for every descendant block.
    let mut visibility = HashMap::new();
    let mut units = navigation::nav_units(document);
    units.retain(|unit| {
        visible(
            document,
            match unit {
                NavUnit::Text(block) => block.node,
                NavUnit::Atomic(node) => *node,
            },
            &mut visibility,
        )
    });
    units
}

pub(super) fn rendered_text_blocks(document: &XiaomuDocument) -> Vec<TextBlock> {
    rendered_nav_units(document)
        .into_iter()
        .filter_map(|unit| match unit {
            NavUnit::Text(block) => Some(block),
            NavUnit::Atomic(_) => None,
        })
        .collect()
}

/// Shared only by this view and its native input handlers. Runtime remains
/// able to edit canonical text inside a legal spanning table without GPUI.
pub(crate) fn selection_has_hidden_table_endpoint(
    document: &XiaomuDocument,
    selection: DocumentSelection,
) -> bool {
    if selection.active_cell_range().is_some_and(|range| {
        [range.anchor(), range.focus()]
            .into_iter()
            .any(|cell| navigation::spanning_table_ancestor(document, cell).is_some())
    }) {
        return true;
    }
    [selection.anchor(), selection.focus()]
        .into_iter()
        .any(|position| {
            let node = match position {
                DocumentPosition::Inline(point) => point.node_id(),
                DocumentPosition::Atomic(node) => node,
                DocumentPosition::Gap(gap) => gap.parent(),
            };
            navigation::spanning_table_ancestor(document, node).is_some()
        })
}

impl DocumentView {
    /// A host-restored endpoint can still refer to canonical hidden content.
    /// Consume legacy gestures there without giving an unpainted editor focus.
    pub(super) fn selection_has_hidden_table_endpoint(&self) -> bool {
        let session = self.session.borrow();
        crate::table_capability::selection_has_hidden_table_endpoint(
            session.document(),
            session.selection(),
            &self.table_capability.borrow(),
        )
    }

    pub(super) fn hidden_table_ancestor(
        &self,
        document: &XiaomuDocument,
        node: NodeId,
    ) -> Option<NodeId> {
        self.table_capability
            .borrow()
            .hidden_ancestor(document, node)
    }

    pub(super) fn rendered_nav_units(&self, document: &XiaomuDocument) -> Vec<NavUnit> {
        if !self.table_capability.borrow().enabled() {
            return rendered_nav_units(document);
        }
        self.table_visible_units(document, false)
    }

    pub(super) fn rendered_text_blocks(&self, document: &XiaomuDocument) -> Vec<TextBlock> {
        if !self.table_capability.borrow().enabled() {
            return rendered_text_blocks(document);
        }
        self.rendered_nav_units(document)
            .into_iter()
            .filter_map(|unit| match unit {
                NavUnit::Text(block) => Some(block),
                NavUnit::Atomic(_) => None,
            })
            .collect()
    }

    /// Construction must precede measurement admission, otherwise a new/Undo
    /// restored table could never materialize the children it needs to measure.
    pub(super) fn buildable_text_blocks(&self, document: &XiaomuDocument) -> Vec<TextBlock> {
        if !self.table_capability.borrow().enabled() {
            return rendered_text_blocks(document);
        }
        self.table_visible_units(document, true)
            .into_iter()
            .filter_map(|unit| match unit {
                NavUnit::Text(block) => Some(block),
                NavUnit::Atomic(_) => None,
            })
            .collect()
    }

    fn table_visible_units(&self, document: &XiaomuDocument, building: bool) -> Vec<NavUnit> {
        fn visible(
            document: &XiaomuDocument,
            node: NodeId,
            capability: &crate::table_capability::TableCapability,
            building: bool,
            cache: &mut HashMap<NodeId, bool>,
        ) -> bool {
            if let Some(visible) = cache.get(&node) {
                return *visible;
            }
            let admitted = document.node(node).is_some_and(|value| {
                value.kind() != &xiaomu_core::document::NodeKind::Table
                    || if building {
                        capability.can_build(document, node)
                    } else {
                        capability.permits(document, node)
                    }
            }) && capability
                .parent_of(document, node)
                .is_none_or(|parent| visible(document, parent, capability, building, cache));
            cache.insert(node, admitted);
            admitted
        }
        let capability = self.table_capability.borrow();
        let mut cache = HashMap::new();
        navigation::nav_units(document)
            .into_iter()
            .filter(|unit| {
                let node = match unit {
                    NavUnit::Text(block) => block.node,
                    NavUnit::Atomic(node) => *node,
                };
                visible(document, node, &capability, building, &mut cache)
            })
            .collect()
    }
}
