//! Mounted clipped-table and line-break hits share the ordinary pointer geometry.

use super::*;
use gpui::{ScrollDelta, ScrollWheelEvent, TouchPhase};

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}

fn cell(builder: &mut NodeStoreBuilder, width: i64, child: NodeId) -> NodeId {
    builder
        .insert(
            NodeKind::TableCell,
            NodeAttrs::new(
                [(
                    "colwidth".into(),
                    AttrValue::List(vec![AttrValue::Integer(width)]),
                )]
                .into(),
            )
            .unwrap(),
            NodeContent::children([child]),
        )
        .unwrap()
}

fn table(builder: &mut NodeStoreBuilder, cells: [NodeId; 2]) -> NodeId {
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children(cells),
        )
        .unwrap();
    builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap()
}

pub(super) fn geometry_fixture(hard_break: bool) -> (Fixture, Option<(NodeId, NodeId)>) {
    let mut builder = NodeStoreBuilder::new();
    let atom = builder
        .insert(
            NodeKind::InlineAtom(if hard_break {
                AtomKind::hard_break()
            } else {
                AtomKind::new("reference").unwrap()
            }),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(if hard_break {
                InlineAtomContent::hard_break()
            } else {
                InlineAtomContent::new("[reference]").unwrap()
            }),
        )
        .unwrap();
    let text = if hard_break { "ab" } else { "tail" };
    let run = TextRun::new(text, MarkSet::empty()).unwrap();
    let offset = run.text().offset_at(usize::from(hard_break)).unwrap();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms([run], [InlineAtomPlacement::new(atom, offset)]).unwrap(),
            ),
        )
        .unwrap();
    let selected = self::paragraph(&mut builder, "keep selection");
    let (body, nested) = if hard_break {
        (paragraph, None)
    } else {
        let first = self::paragraph(&mut builder, "inner first");
        let cells = [
            cell(&mut builder, 200, first),
            cell(&mut builder, 170, paragraph),
        ];
        let inner = table(&mut builder, cells);
        let sibling = self::paragraph(&mut builder, "outer sibling");
        let cells = [
            cell(&mut builder, 250, inner),
            cell(&mut builder, 120, sibling),
        ];
        (table(&mut builder, cells), Some((inner, sibling)))
    };
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([selected, body]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let inline = document
        .node(selected)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    let selection = DocumentSelection::new(
        InlinePoint::new(
            selected,
            inline.offset_at(8).unwrap(),
            0,
            CursorAffinity::Before,
        ),
        InlinePoint::new(
            selected,
            inline.offset_at(2).unwrap(),
            0,
            CursorAffinity::After,
        ),
    );
    (
        Fixture {
            document,
            paragraph,
            selected,
            atoms: [atom, atom],
            selection,
        },
        nested,
    )
}

#[gpui::test]
fn atom_pointer_clamped_padding_and_outside_surface_never_activate(cx: &mut TestAppContext) {
    for outside_surface in [false, true] {
        let f = fixture();
        let host = capturing_host();
        let (handle, session) = open(&f, Some(host.clone()), cx);
        let chip = chip_point(handle, f.paragraph, f.atoms[0], cx);
        let position = handle
            .update(cx, |view, _, cx| {
                let surface = view.scroll_handle.bounds();
                let block = view.block_bounds(f.paragraph).unwrap();
                let position = point(
                    chip.x,
                    if outside_surface {
                        surface.top() - px(8.)
                    } else {
                        block.top() - px(4.)
                    },
                );
                assert_eq!(surface.contains(&position), !outside_surface);
                let hit = view
                    .hit_test(position, cx)
                    .expect("nearest caret hit is clamped");
                assert_eq!(hit.chip.unwrap().node, f.atoms[0]);
                assert!(!hit.inside_visible_bounds);
                position
            })
            .unwrap();
        down(handle, position, Modifiers::default(), cx);
        up(handle, position, Modifiers::default(), cx);
        assert_eq!(host.probes.borrow().len(), usize::from(!outside_surface));
        assert!(host.clicks.borrow().is_empty());
        assert!(host.legacy.borrow().is_empty());
        unchanged(&f, &session);
        handle
            .update(cx, |view, _, _| assert!(view.atom_pointer.is_none()))
            .unwrap();
        assert_eq!(session.borrow().document().store(), f.document.store());
        assert_eq!(session.borrow().history_depths(), (0, 0));
        if outside_surface {
            unchanged(&f, &session);
            assert!(host.legacy.borrow().is_empty());
        }
    }
}

#[gpui::test]
fn atom_pointer_nested_table_clips_hidden_chip_then_activates_after_scroll(
    cx: &mut TestAppContext,
) {
    let (f, nested) = geometry_fixture(false);
    let (inner, sibling) = nested.unwrap();
    let host = capturing_host();
    let (handle, session) = open(&f, Some(host.clone()), cx);
    handle
        .update(cx, |view, _, cx| {
            view.set_measured_table_layout(true);
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let chip = chip_point(handle, f.paragraph, f.atoms[0], cx);
    let hidden = handle
        .update(cx, |view, _, cx| {
            let bounds = view.block_bounds(f.paragraph).unwrap();
            let sibling_cell = session.borrow().document().parent_of(sibling).unwrap();
            let sibling_bounds = view
                .cell_registry
                .borrow()
                .iter()
                .find(|(node, _)| *node == sibling_cell)
                .unwrap()
                .1;
            let child = view
                .children
                .iter()
                .find(|(node, _)| *node == f.paragraph)
                .unwrap()
                .1
                .read(cx);
            let projection = child.atom_display_projection().unwrap();
            let span = projection
                .atoms()
                .iter()
                .find(|span| span.atom() == f.atoms[0])
                .unwrap();
            let layout = child.last_layout.as_ref().unwrap();
            let start = layout
                .position_for_caret(span.display_range().start, CursorAffinity::After)
                .unwrap();
            let end = layout
                .position_for_caret(span.display_range().end, CursorAffinity::Before)
                .unwrap();
            // Pick an actual overlap of the painted chip and the sibling cell,
            // rather than assuming the chip midpoint reaches past cell padding.
            let left = (bounds.left() + start.x).max(sibling_bounds.left());
            let right = (bounds.left() + end.x).min(sibling_bounds.right());
            assert!(
                left < right,
                "chip {start:?}..{end:?} in {bounds:?} must overlap {sibling_bounds:?}"
            );
            let hidden = point((left + right) / 2., chip.y);
            assert!(sibling_bounds.contains(&hidden));
            let (raw, _) = child.hit_test_caret_position(hidden).unwrap();
            assert_eq!(
                projection.atom_at_display_offset(raw).unwrap().atom(),
                f.atoms[0]
            );
            let visible = view
                .visible_table_bounds(session.borrow().document(), f.paragraph, bounds)
                .unwrap();
            assert!(
                bounds.contains(&hidden),
                "point lies in canonical chip geometry"
            );
            assert!(
                !visible.contains(&hidden),
                "nested viewport clips that chip geometry"
            );
            let hit = view.hit_test(hidden, cx).unwrap();
            assert_eq!(hit.point.node_id(), sibling);
            assert!(hit.chip.is_none());
            hidden
        })
        .unwrap();
    down(handle, hidden, Modifiers::default(), cx);
    up(handle, hidden, Modifiers::default(), cx);
    assert!(host.probes.borrow().is_empty());
    assert!(host.clicks.borrow().is_empty());
    assert!(host.legacy.borrow().is_empty());
    assert_eq!(
        session
            .borrow()
            .selection()
            .focus()
            .as_inline_point()
            .unwrap()
            .node_id(),
        sibling
    );
    let selection = session.borrow().selection();
    let wheel_point = handle
        .update(cx, |view, _, _| {
            view.table_clips.borrow().get(&inner).unwrap().center()
        })
        .unwrap();
    VisualTestContext::from_window(handle.into(), cx).simulate_event(ScrollWheelEvent {
        position: wheel_point,
        delta: ScrollDelta::Pixels(point(px(-110.), px(0.))),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
    });
    let visible = chip_point(handle, f.paragraph, f.atoms[0], cx);
    handle
        .update(cx, |view, _, cx| {
            assert!(visible.x < hidden.x);
            assert!(
                view.table_clips
                    .borrow()
                    .get(&inner)
                    .unwrap()
                    .contains(&visible)
            );
            let hit = view.hit_test(visible, cx).unwrap();
            assert!(hit.inside_visible_bounds);
            assert_eq!(hit.chip.unwrap().node, f.atoms[0]);
        })
        .unwrap();
    down(handle, visible, Modifiers::default(), cx);
    assert!(host.clicks.borrow().is_empty());
    assert_eq!(session.borrow().selection(), selection);
    up(handle, visible, Modifiers::default(), cx);
    assert_eq!(host.probes.borrow().len(), 1);
    assert_eq!(host.clicks.borrow().len(), 1);
    assert_eq!(host.clicks.borrow()[0].0.node, f.atoms[0]);
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().document().store(), f.document.store());
    assert_eq!(
        session.borrow().document().revision(),
        f.document.revision()
    );
    assert_eq!(session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn atom_pointer_hard_break_is_never_probed_or_activated(cx: &mut TestAppContext) {
    for capture in [false, true] {
        let (f, _) = geometry_fixture(true);
        let host = Rc::new(Host {
            capture,
            ..Default::default()
        });
        let (handle, session) = open(&f, Some(host.clone()), cx);
        let position = handle
            .update(cx, |view, _, cx| {
                let child = view
                    .children
                    .iter()
                    .find(|(node, _)| *node == f.paragraph)
                    .unwrap()
                    .1
                    .read(cx);
                let projection = child.atom_display_projection().unwrap();
                let span = projection
                    .atoms()
                    .iter()
                    .find(|span| span.atom() == f.atoms[0])
                    .unwrap();
                assert!(span.is_hard_break());
                let caret = child
                    .last_layout
                    .as_ref()
                    .unwrap()
                    .position_for_caret(span.display_range().start, CursorAffinity::Before)
                    .unwrap();
                let position = child.last_bounds.unwrap().origin + caret + point(px(2.), px(8.));
                let (raw, _) = child.hit_test_caret_position(position).unwrap();
                assert!(
                    projection
                        .atom_at_display_offset(raw)
                        .is_some_and(|span| span.is_hard_break())
                );
                let hit = view.hit_test(position, cx).unwrap();
                assert!(hit.inside_visible_bounds);
                assert!(hit.chip.is_none());
                position
            })
            .unwrap();
        down(handle, position, Modifiers::default(), cx);
        up(handle, position, Modifiers::default(), cx);
        assert!(host.probes.borrow().is_empty());
        assert!(host.clicks.borrow().is_empty());
        assert!(host.legacy.borrow().is_empty());
        assert_eq!(
            session
                .borrow()
                .selection()
                .focus()
                .as_inline_point()
                .unwrap()
                .node_id(),
            f.paragraph
        );
        assert_eq!(session.borrow().document().store(), f.document.store());
        assert_eq!(session.borrow().history_depths(), (0, 0));
    }
}

#[gpui::test]
fn atom_pointer_single_move_outside_surface_then_return_stays_cancelled(cx: &mut TestAppContext) {
    let f = fixture();
    let host = capturing_host();
    let (handle, session) = open(&f, Some(host.clone()), cx);
    let chip = chip_point(handle, f.paragraph, f.atoms[0], cx);
    let outside = handle
        .update(cx, |view, _, _| {
            let surface = view.scroll_handle.bounds();
            let outside = point(chip.x, surface.top() - px(20.));
            assert!(!surface.contains(&outside));
            outside
        })
        .unwrap();
    down(handle, chip, Modifiers::default(), cx);
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    visual.simulate_mouse_move(outside, Some(MouseButton::Left), Modifiers::default());
    visual.simulate_mouse_move(chip, Some(MouseButton::Left), Modifiers::default());
    up(handle, chip, Modifiers::default(), cx);
    assert_eq!(host.probes.borrow().len(), 1);
    assert!(host.clicks.borrow().is_empty());
    assert!(host.legacy.borrow().is_empty());
    unchanged(&f, &session);
    down(handle, chip, Modifiers::default(), cx);
    up(handle, chip, Modifiers::default(), cx);
    assert_eq!(host.clicks.borrow().len(), 1);
    unchanged(&f, &session);
}
