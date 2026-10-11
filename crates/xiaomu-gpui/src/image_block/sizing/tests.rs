use super::*;

#[test]
fn loaded_width_hints_clamp_both_axes_and_allow_explicit_upscale() {
    let natural = size(px(96.0), px(64.0));
    for (hint, available, width, height) in [
        (None, 600.0, 98.0, 66.0),
        (None, 40.0, 40.0, 27.333334),
        (Some(48), 600.0, 48.0, 32.666668),
        (Some(192), 600.0, 192.0, 128.66667),
        (Some(1600), 80.0, 80.0, 54.0),
        (Some(u32::MAX), 40.0, 40.0, 27.333334),
        (None, 0.0, 0.0, 2.0),
    ] {
        let actual = responsive_size(natural, hint, px(available));
        assert!((f32::from(actual.width) - width).abs() < 0.001);
        assert!((f32::from(actual.height) - height).abs() < 0.001);
    }
}

#[test]
fn tall_images_have_no_legacy_height_cap() {
    assert_eq!(
        responsive_size(size(px(80.0), px(800.0)), None, px(600.0)),
        size(px(82.0), px(802.0))
    );
}

#[test]
fn all_positive_dimensions_keep_finite_nonnegative_geometry() {
    for w in [1.0, 96.0, u32::MAX as f32] {
        for h in [1.0, 64.0, u32::MAX as f32] {
            for available in [0.0, 1.0, 2.0, 40.0, 600.0] {
                let actual = responsive_size(size(px(w), px(h)), Some(u32::MAX), px(available));
                assert!(f32::from(actual.width).is_finite());
                assert!(f32::from(actual.height).is_finite());
                assert!(actual.width >= px(0.0) && actual.width <= px(available));
                assert!(actual.height >= px(0.0));
            }
        }
    }
}

#[test]
fn nonfinite_constraints_never_create_nonfinite_geometry() {
    for available in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -20.0, f32::MAX] {
        let result = responsive_size(size(px(96.0), px(64.0)), Some(u32::MAX), px(available));
        assert!(f32::from(result.width).is_finite());
        assert!(f32::from(result.height).is_finite());
        assert!(result.width >= px(0.0) && result.height >= px(0.0));
    }
    for natural in [
        size(px(f32::NAN), px(64.0)),
        size(px(0.0), px(64.0)),
        size(px(96.0), px(f32::INFINITY)),
    ] {
        assert_eq!(
            responsive_size(natural, None, px(600.0)),
            size(px(0.0), px(0.0))
        );
    }
}
