//! Width-independent upper bounds, reserved before any experimental shaping.
//! This intentionally trades admission breadth for bounded synchronous work.
//! A future cached/backend implementation may relax the conservative bound.

use super::shape::Prepared;
use super::{Reason, Unsupported, WorkLimits};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Estimate {
    pub(super) shaped_bytes: usize,
    pub(super) shape_calls: usize,
}

impl Estimate {
    fn add(self, other: Self) -> Option<Self> {
        Some(Self {
            shaped_bytes: self.shaped_bytes.checked_add(other.shaped_bytes)?,
            shape_calls: self.shape_calls.checked_add(other.shape_calls)?,
        })
    }

    fn within(self, limits: WorkLimits) -> bool {
        self.shaped_bytes <= limits.max_shaped_bytes && self.shape_calls <= limits.max_shape_calls
    }
}

/// Pure checked arithmetic kept separate so overflow regressions need not
/// allocate a huge document. One empty paragraph reserves one overhead token.
pub(super) fn paragraph_estimate(
    bytes: usize,
    graphemes: usize,
    spans: usize,
    distinct_sizes: usize,
) -> Option<Estimate> {
    if bytes == 0 {
        return Some(Estimate {
            shaped_bytes: 1,
            shape_calls: 1,
        });
    }
    let boundaries = spans.checked_sub(1)?;
    let seam = if boundaries == 0 {
        Estimate::default()
    } else {
        Estimate {
            shaped_bytes: distinct_sizes
                .checked_mul(boundaries.checked_add(1)?)?
                .checked_mul(bytes)?,
            shape_calls: distinct_sizes.checked_mul(boundaries.checked_mul(2)?.checked_add(1)?)?,
        }
    };
    // Admission and initial measure each shape every span once (2N, 2S).
    // There are at most G final-row candidates, each at most N bytes / S calls.
    // An overflowing candidate gets just one first-cluster fallback: <= N
    // total extra bytes / G extra calls across the paragraph, never retries.
    seam.add(Estimate {
        shaped_bytes: bytes.checked_mul(graphemes.checked_add(3)?)?,
        shape_calls: spans
            .checked_mul(2)?
            .checked_add(graphemes.checked_mul(spans.checked_add(1)?)?)?,
    })
}

pub(super) fn check(input: &Prepared<'_>) -> Result<Estimate, Unsupported> {
    let exhausted = || Unsupported::at(0..input.text.len(), Reason::WorkBudgetExceeded);
    // These are also lower bounds on the combined admission/reflow reservation,
    // and bound estimator input before walking paragraphs or unique sizes.
    if input.text.len() > input.limits.max_shaped_bytes
        || input.sizes.len() > input.limits.max_shape_calls
    {
        return Err(exhausted());
    }
    let mut total = Estimate::default();
    let mut start = 0;
    for text in input.text.split('\n') {
        let end = start + text.len();
        let spans = input.spans_for(&(start..end));
        let mut distinct = Vec::new();
        for span in spans {
            if !distinct.contains(&span.size) {
                distinct.push(span.size);
            }
        }
        let estimate = paragraph_estimate(
            text.len(),
            text.graphemes(true).count(),
            spans.len(),
            distinct.len(),
        )
        .ok_or_else(exhausted)?;
        total = total.add(estimate).ok_or_else(exhausted)?;
        if !total.within(input.limits) {
            return Err(exhausted());
        }
        start = end + 1;
    }
    Ok(total)
}
