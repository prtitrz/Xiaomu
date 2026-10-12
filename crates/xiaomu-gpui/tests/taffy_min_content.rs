//! Permanent regression for upstream Taffy PR #1119, adapted to the 0.9.0 API.
//!
//! These are library callback/geometry checks, not native host performance tests.
use taffy::prelude::*;
use taffy::{Overflow, Point};

#[derive(Debug, PartialEq)]
struct Measurements {
    total: usize,
    main_min_content: usize,
}

fn measure(
    tree: &mut TaffyTree<()>,
    root: NodeId,
    column: bool,
    intrinsic: Size<f32>,
    require_skip: bool,
) -> Measurements {
    let mut calls = Measurements {
        total: 0,
        main_min_content: 0,
    };
    tree.compute_layout_with_measure(root, Size::MAX_CONTENT, |_, available, _, _, _| {
        calls.total += 1;
        let main_available = if column {
            available.height
        } else {
            available.width
        };
        let is_min_content = main_available == AvailableSpace::MinContent;
        calls.main_min_content += usize::from(is_min_content);
        if require_skip {
            // This assertion fails against the unchanged official 0.9.0 source.
            assert!(!is_min_content, "resolved minimum must skip min-content");
        }
        intrinsic
    })
    .unwrap();
    calls
}

fn layouts(tree: &TaffyTree<()>, child: NodeId, root: NodeId) -> [Layout; 4] {
    [
        *tree.layout(child).unwrap(),
        *tree.layout(root).unwrap(),
        *tree.unrounded_layout(child),
        *tree.unrounded_layout(root),
    ]
}

fn check_case(column: bool, minimum: Option<f32>, overflow: Overflow, cross_min_only: bool) {
    let mut tree = TaffyTree::new();
    let mut style = Style {
        overflow: Point {
            x: overflow,
            y: overflow,
        },
        ..Default::default()
    };
    let main_min = minimum.map(length).unwrap_or_else(auto);
    if column {
        style.min_size.height = main_min;
        if cross_min_only {
            style.min_size.width = length(0.0_f32);
        }
    } else {
        style.min_size.width = main_min;
        if cross_min_only {
            style.min_size.height = length(0.0_f32);
        }
    }
    let child = tree.new_leaf_with_context(style, ()).unwrap();
    let root = tree
        .new_with_children(
            Style {
                flex_direction: if column {
                    FlexDirection::Column
                } else {
                    FlexDirection::Row
                },
                ..Default::default()
            },
            &[child],
        )
        .unwrap();
    let needs_min_content =
        minimum.is_none() && matches!(overflow, Overflow::Visible | Overflow::Clip);
    let intrinsic = Size {
        width: 10.0,
        height: 10.0,
    };
    let calls = measure(&mut tree, root, column, intrinsic, !needs_min_content);
    assert_eq!(calls.main_min_content, usize::from(needs_min_content));
    assert_eq!(calls.total, if needs_min_content { 3 } else { 2 });
    let expected = Layout {
        size: intrinsic,
        content_size: intrinsic,
        ..Layout::new()
    };
    let cold = layouts(&tree, child, root);
    assert_eq!(cold, [expected; 4]);

    // A warm repeat must not invoke the callback and must retain all geometry.
    let warm = measure(&mut tree, root, column, intrinsic, !needs_min_content);
    assert_eq!(warm.total, 0);
    assert_eq!(layouts(&tree, child, root), cold);

    // A real invalidation must reproduce both the needed work and the geometry.
    tree.mark_dirty(child).unwrap();
    let dirty = measure(&mut tree, root, column, intrinsic, !needs_min_content);
    assert_eq!(dirty, calls);
    assert_eq!(layouts(&tree, child, root), cold);
}

#[test]
fn explicit_main_minimum_skips_min_content_for_rows_and_columns() {
    for column in [false, true] {
        for minimum in [0.0, 10.0] {
            for overflow in [
                Overflow::Visible,
                Overflow::Hidden,
                Overflow::Scroll,
                Overflow::Clip,
            ] {
                check_case(column, Some(minimum), overflow, false);
            }
        }
    }
}

#[test]
fn automatic_minimum_still_measures_for_visible_and_clip_overflow() {
    for column in [false, true] {
        for overflow in [Overflow::Visible, Overflow::Clip] {
            check_case(column, None, overflow, false);
            // A cross-axis minimum cannot resolve the main-axis minimum.
            check_case(column, None, overflow, true);
        }
    }
}

#[test]
fn scroll_container_automatic_minimum_skips_unused_measurement() {
    for column in [false, true] {
        for overflow in [Overflow::Hidden, Overflow::Scroll] {
            check_case(column, None, overflow, false);
        }
    }
}

#[test]
fn fractional_geometry_matches_with_explicit_and_automatic_minimums() {
    for column in [false, true] {
        let mut results = Vec::new();
        for explicit in [false, true] {
            let mut tree = TaffyTree::new();
            let mut child_style = Style::default();
            if explicit {
                if column {
                    child_style.min_size.height = length(0.0_f32);
                } else {
                    child_style.min_size.width = length(0.0_f32);
                }
            }
            let child = tree.new_leaf_with_context(child_style, ()).unwrap();
            let root = tree
                .new_with_children(
                    Style {
                        flex_direction: if column {
                            FlexDirection::Column
                        } else {
                            FlexDirection::Row
                        },
                        ..Default::default()
                    },
                    &[child],
                )
                .unwrap();
            let intrinsic = Size {
                width: 10.25,
                height: 20.75,
            };
            let calls = measure(&mut tree, root, column, intrinsic, explicit);
            assert_eq!(calls.main_min_content, usize::from(!explicit));
            let result = layouts(&tree, child, root);
            for layout in &result[..2] {
                assert_eq!(
                    layout.size,
                    Size {
                        width: 10.0,
                        height: 21.0
                    }
                );
                assert_eq!(layout.location, Point::ZERO);
            }
            for layout in &result[2..] {
                assert_eq!(layout.size, intrinsic);
                assert_eq!(layout.content_size, intrinsic);
                assert_eq!(layout.location, Point::ZERO);
            }
            assert_eq!(
                measure(&mut tree, root, column, intrinsic, explicit).total,
                0
            );
            assert_eq!(layouts(&tree, child, root), result);
            results.push(result);
        }
        assert_eq!(results[0], results[1]);
    }
}
