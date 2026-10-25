//! Fixed bounds shared by block and inline parsing and the recursive writer.
use xiaomu_core::document::{NodeContent, XiaomuDocument};

use crate::{MarkdownCodecError, Result};

pub(crate) const MAX_SOURCE_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_NODES: usize = 20_000;
pub(crate) const MAX_DEPTH: usize = 64;
const MAX_WORK: usize = 16 * 1024 * 1024;

pub(crate) fn check(value: usize, resource: &'static str, limit: usize) -> Result<()> {
    if value > limit {
        Err(MarkdownCodecError::ResourceLimit { resource, limit })
    } else {
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct Budget {
    nodes: usize,
    work: usize,
}

impl Budget {
    pub(crate) fn node(&mut self) -> Result<()> {
        self.nodes = self.nodes.saturating_add(1);
        check(self.nodes, "nodes", MAX_NODES)
    }

    pub(crate) fn work(&mut self, amount: usize) -> Result<()> {
        self.work = self.work.saturating_add(amount);
        check(self.work, "work", MAX_WORK)
    }
}

/// Iterative preflight bounds recursion and conservative writer allocations.
/// Unsupported atoms/attributes are still rejected by the writer itself.
pub(crate) fn check_document(document: &XiaomuDocument) -> Result<()> {
    let mut stack = vec![(document.root(), 0usize)];
    let mut budget = Budget::default();
    let mut output_estimate = 0usize;
    while let Some((id, depth)) = stack.pop() {
        check(depth, "nesting", MAX_DEPTH)?;
        budget.node()?;
        let node = document
            .node(id)
            .ok_or(MarkdownCodecError::InvalidDocument(
                xiaomu_core::Error::UnknownNode,
            ))?;
        // Includes indentation, quote/list markers and blank lines.
        output_estimate = output_estimate.saturating_add(16 + depth * 4);
        match node.content() {
            NodeContent::Children(children) => {
                check(
                    stack.len().saturating_add(children.len()),
                    "nodes",
                    MAX_NODES,
                )?;
                stack.extend(children.iter().map(|id| (*id, depth + 1)));
            }
            NodeContent::Inline(inline) => {
                for run in inline.runs() {
                    let bytes = run.text().as_str().len();
                    // Escaping, fences and repeated indentation. Mark attributes
                    // are checked separately below before they are formatted.
                    output_estimate = output_estimate
                        .saturating_add(bytes.saturating_mul(4 + depth * 4).saturating_add(32));
                    for mark in run.marks().as_slice() {
                        if let xiaomu_core::document::Mark::Link(link) = mark
                            && let Some((href, title)) = link.classic_parts()
                        {
                            output_estimate = output_estimate.saturating_add(
                                (href.len().saturating_add(title.map_or(0, str::len)))
                                    .saturating_mul(2),
                            );
                        }
                    }
                }
            }
            _ => {}
        }
        // Attr values supported by this codec are scalar image fields/language.
        for (_, value) in node.attrs().iter() {
            if let xiaomu_core::document::AttrValue::String(value) = value {
                output_estimate = output_estimate.saturating_add(value.len().saturating_mul(2));
            }
        }
        check(output_estimate, "output estimate", MAX_SOURCE_BYTES)?;
    }
    Ok(())
}
