//! Deterministic full-fragment text-between projection with LF/LF semantics.

use xiaomu_core::document::NodeKind;

use super::{ClipboardInline, ClipboardNode, ClipboardNodeContent};

#[derive(Default)]
struct TextBetween {
    text: String,
    first: bool,
}

impl TextBetween {
    fn block(&mut self) {
        if self.first {
            self.first = false;
        } else {
            self.text.push('\n');
        }
    }

    fn node(&mut self, node: &ClipboardNode) -> Result<(), ()> {
        match (node.kind(), node.content()) {
            (
                NodeKind::Paragraph | NodeKind::Heading(_) | NodeKind::CodeBlock,
                ClipboardNodeContent::Inline(inline),
            ) => {
                self.block();
                self.inline(inline)?;
            }
            (NodeKind::Image | NodeKind::HorizontalRule, ClipboardNodeContent::Atomic) => {
                self.block();
                self.text.push('\n');
            }
            (
                NodeKind::Quote
                | NodeKind::BulletList
                | NodeKind::OrderedList
                | NodeKind::ListItem
                | NodeKind::TaskList
                | NodeKind::TaskItem
                | NodeKind::Table
                | NodeKind::TableRow
                | NodeKind::TableCell
                | NodeKind::TableHeader,
                ClipboardNodeContent::Children(children),
            ) => {
                for child in children {
                    self.node(child)?;
                }
            }
            (NodeKind::Table, ClipboardNodeContent::Table { rows, .. }) => {
                for cell in rows.iter().flatten() {
                    self.node(cell)?;
                }
            }
            // Do not silently omit or reinterpret custom leaf/atom semantics.
            _ => return Err(()),
        }
        Ok(())
    }

    fn inline(&mut self, inline: &ClipboardInline) -> Result<(), ()> {
        let mut atoms = inline.atoms().iter().peekable();
        let mut cursor = 0;
        for run in inline.runs() {
            let text = run.text().as_str();
            let end = cursor + text.len();
            let mut offset = 0;
            while let Some(atom) = atoms.peek().filter(|atom| atom.anchor().as_usize() <= end) {
                if !atom.kind().is_hard_break() {
                    return Err(());
                }
                let anchor = atom.anchor().as_usize().checked_sub(cursor).ok_or(())?;
                self.text.push_str(text.get(offset..anchor).ok_or(())?);
                self.text.push('\n');
                offset = anchor;
                atoms.next();
            }
            self.text.push_str(text.get(offset..).ok_or(())?);
            cursor = end;
        }
        for atom in atoms {
            if !atom.kind().is_hard_break() || atom.anchor().as_usize() != cursor {
                return Err(());
            }
            self.text.push('\n');
        }
        Ok(())
    }
}

pub(super) fn project(roots: &[ClipboardNode]) -> Result<String, ()> {
    super::export_budget::roots(roots)?;
    let mut projection = TextBetween {
        text: String::new(),
        first: true,
    };
    for root in roots {
        projection.node(root)?;
    }
    Ok(projection.text)
}
