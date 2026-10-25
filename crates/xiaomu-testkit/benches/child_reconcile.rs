//! CPU-only child reuse baseline; no claim about native layout/paint latency.
#[path = "../../xiaomu-gpui/src/document_view/child_pool.rs"]
mod child_pool;
use std::{hint::black_box, time::Instant};
use xiaomu_core::document::{NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder};

fn main() {
    println!("scenario,blocks,samples,p50_us,p95_us");
    for count in [1_000, 10_000] {
        let mut builder = NodeStoreBuilder::new();
        let ids: Vec<_> = (0..count)
            .map(|_| {
                builder
                    .insert(
                        NodeKind::Paragraph,
                        NodeAttrs::empty(),
                        NodeContent::empty_inline(),
                    )
                    .unwrap()
            })
            .collect();
        for reversed in [false, true] {
            for legacy in [true, false] {
                let mut samples = Vec::new();
                for iteration in 0..35 {
                    let mut previous: Vec<_> = ids
                        .iter()
                        .copied()
                        .enumerate()
                        .map(|(i, id)| (id, i))
                        .collect();
                    if reversed {
                        previous.reverse();
                    }
                    let start = Instant::now();
                    let output = if legacy {
                        ids.iter()
                            .map(|id| {
                                let position =
                                    previous.iter().position(|(old, _)| old == id).unwrap();
                                previous.remove(position)
                            })
                            .collect::<Vec<_>>()
                    } else {
                        child_pool::reconcile(previous, ids.iter().copied(), |_| {
                            panic!("reuse expected")
                        })
                    };
                    black_box(&output);
                    let elapsed = start.elapsed().as_secs_f64() * 1e6;
                    assert_eq!(output.len(), count);
                    if iteration >= 5 {
                        samples.push(elapsed);
                    }
                }
                samples.sort_by(f64::total_cmp);
                println!(
                    "{}_{},{count},{},{:.3},{:.3}",
                    if legacy { "legacy_vec" } else { "node_map" },
                    if reversed { "reverse" } else { "stable" },
                    samples.len(),
                    samples[14],
                    samples[28]
                );
            }
        }
    }
}
