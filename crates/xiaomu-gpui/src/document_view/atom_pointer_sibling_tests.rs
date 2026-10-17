//! Non-text children own their painted hitboxes before clamped atom policy.

use super::*;
#[path = "atom_pointer_control_tests.rs"]
mod controls;
use crate::{
    block_alignment::{BlockAlignment, BlockAlignmentProvider},
    editor_commands::{CommandRoute, EditorCommand, EditorCommandContext, EditorCommandRouter},
    image_block::ImageSizingPolicy,
};
use xiaomu_core::document::{ImageAttrs, ImageSource};
use xiaomu_runtime::session::PolicyError;

#[derive(Default)]
struct CapturingFallbackHost {
    probes: RefCell<Vec<AtomAction>>,
    actions: RefCell<Vec<AtomAction>>,
}

// Deliberately inherit atom_pointer_click: a completed gesture would reach
// atom_action even without a separate completed-click implementation.
impl InlineAtomHostCapability for CapturingFallbackHost {
    fn atom_action(&self, action: AtomAction) {
        self.actions.borrow_mut().push(action);
    }

    fn capture_pointer_click(&self, action: &AtomAction, _: Modifiers) -> bool {
        self.probes.borrow_mut().push(action.clone());
        true
    }
}

struct LeftAligned;

impl BlockAlignmentProvider for LeftAligned {
    fn alignment(&self, _: &xiaomu_core::document::Node) -> BlockAlignment {
        BlockAlignment::Left
    }
}

struct AtomicRouter {
    explicit_node: bool,
    calls: Rc<RefCell<Vec<(DocumentSelection, NodeId)>>>,
}

impl EditorCommandRouter for AtomicRouter {
    fn route(
        &self,
        _: EditorCommandContext<'_>,
        _: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError> {
        Ok(CommandRoute::Default)
    }

    fn select_atomic(
        &self,
        context: EditorCommandContext<'_>,
        node: NodeId,
    ) -> Result<Option<DocumentSelection>, PolicyError> {
        self.calls.borrow_mut().push((context.selection(), node));
        if self.explicit_node {
            DocumentSelection::node(context.document(), node)
                .map(Some)
                .map_err(|error| PolicyError::new(error.to_string()))
        } else {
            Ok(None)
        }
    }
}

fn sibling_fixture(image: bool) -> (Fixture, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let atom = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::new("reference").unwrap()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::new("[reference]").unwrap()),
        )
        .unwrap();
    let text = TextRun::new("leading text ", MarkSet::empty()).unwrap();
    let atom_offset = text.text().offset_at(text.text().as_str().len()).unwrap();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms([text], [InlineAtomPlacement::new(atom, atom_offset)])
                    .unwrap(),
            ),
        )
        .unwrap();
    let selected = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("keep selection", MarkSet::empty()).unwrap()])
                    .unwrap(),
            ),
        )
        .unwrap();
    let sibling = builder
        .insert(
            if image {
                NodeKind::Image
            } else {
                NodeKind::HorizontalRule
            },
            if image {
                ImageAttrs::new(
                    ImageSource::AssetRef("sibling-image".into()),
                    "sibling image".into(),
                    None,
                    None,
                    None,
                )
                .unwrap()
                .to_attrs()
                .unwrap()
            } else {
                NodeAttrs::empty()
            },
            NodeContent::Atomic,
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([selected, paragraph, sibling]),
        )
        .unwrap();
    (
        Fixture {
            document: XiaomuDocument::new(root, builder.finish()).unwrap(),
            paragraph,
            selected,
            atoms: [atom, atom],
            selection: DocumentSelection::collapsed(InlinePoint::at_start_of(selected)),
        },
        sibling,
    )
}

fn sibling_click_owns_dispatch(image: bool, cx: &mut TestAppContext) {
    // Exercise the untouched default, a host that defers to it, and a host
    // explicitly requesting whole-node selection from the same native press.
    for route in 0..3 {
        let (f, sibling) = sibling_fixture(image);
        let host = Rc::new(CapturingFallbackHost::default());
        let (handle, session) = open(&f, Some(host.clone()), cx);
        let calls = Rc::new(RefCell::new(Vec::new()));
        handle
            .update(cx, |view, _, cx| {
                // Responsive images expose the actual painted content bounds,
                // including the normal unresolved-asset placeholder.
                view.set_image_sizing_policy(ImageSizingPolicy::ResponsiveIntrinsic);
                // Use the supported alignment layout, whose ordinary nearest
                // caret keeps the painted x while clamping to the final row.
                view.set_block_alignment_provider(Some(Rc::new(LeftAligned)));
                if route != 0 {
                    view.set_command_router(Some(Rc::new(AtomicRouter {
                        explicit_node: route == 2,
                        calls: calls.clone(),
                    })));
                }
                cx.notify();
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        let chip = chip_point(handle, f.paragraph, f.atoms[0], cx);
        let prefix = if image {
            "image-content"
        } else {
            "atomic-block"
        };
        let selector = Box::leak(format!("{prefix}-{sibling:?}").into_boxed_str());
        let bounds = VisualTestContext::from_window(handle.into(), cx)
            .debug_bounds(selector)
            .expect("sibling native pointer target is painted");
        let position = point(chip.x, bounds.center().y);
        assert!(
            bounds.contains(&position),
            "click lies inside the actual child hitbox"
        );
        handle
            .update(cx, |view, _, cx| {
                assert!(view.scroll_handle.bounds().contains(&position));
                let hit = view
                    .hit_test(position, cx)
                    .expect("nearest text hit exists");
                assert_eq!(hit.point.node_id(), f.paragraph);
                assert_eq!(hit.chip.unwrap().node, f.atoms[0]);
                assert!(
                    !hit.inside_visible_bounds,
                    "fixture must reproduce the clamped chip hit"
                );
            })
            .unwrap();

        down(handle, position, Modifiers::default(), cx);
        assert!(host.actions.borrow().is_empty());
        let selected = session.borrow().selection();
        if route == 2 {
            assert_eq!(selected.as_node_selection(), Some(sibling));
            assert_eq!(selected.as_atomic_node(), None);
        } else {
            assert_eq!(selected.as_atomic_node(), Some(sibling));
            assert_eq!(selected.as_node_selection(), None);
        }
        assert!(
            host.probes.borrow().is_empty(),
            "sibling press must not probe the clamped chip"
        );
        if route == 0 {
            assert!(calls.borrow().is_empty());
        } else {
            assert_eq!(calls.borrow().as_slice(), &[(f.selection, sibling)]);
        }
        handle
            .update(cx, |view, _, _| {
                assert!(view.atom_pointer.is_none());
                assert!(!view.is_dragging);
            })
            .unwrap();
        up(handle, position, Modifiers::default(), cx);
        assert!(host.probes.borrow().is_empty());
        assert!(host.actions.borrow().is_empty());
        assert_eq!(session.borrow().selection(), selected);
        assert_eq!(session.borrow().document().store(), f.document.store());
        assert_eq!(
            session.borrow().document().revision(),
            f.document.revision()
        );
        assert_eq!(session.borrow().history_depths(), (0, 0));

        // Positive control: the same mounted opt-in host still captures a
        // genuine chip and inherits completed-click forwarding to atom_action.
        let chip = chip_point(handle, f.paragraph, f.atoms[0], cx);
        down(handle, chip, Modifiers::default(), cx);
        assert_eq!(host.probes.borrow().len(), 1);
        assert!(host.actions.borrow().is_empty());
        up(handle, chip, Modifiers::default(), cx);
        assert_eq!(host.actions.borrow().len(), 1);
        assert_eq!(host.actions.borrow()[0].node, f.atoms[0]);
        assert_eq!(session.borrow().selection(), selected);
    }
}

#[gpui::test]
fn atom_pointer_image_child_owns_clamped_chip_hit(cx: &mut TestAppContext) {
    sibling_click_owns_dispatch(true, cx);
}

#[gpui::test]
fn atom_pointer_horizontal_rule_child_owns_clamped_chip_hit(cx: &mut TestAppContext) {
    sibling_click_owns_dispatch(false, cx);
}
