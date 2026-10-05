//! Fixed-size strip decomposition; never allocates a span-sized cell array.

use crate::document::{CellPlacement, TableRect};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Fragment {
    pub top: usize,
    pub left: usize,
    pub bottom: usize,
    pub right: usize,
}

pub(super) struct Fragments {
    pieces: [Fragment; 5],
    len: usize,
}

impl Fragments {
    pub fn new(cell: &CellPlacement, rect: TableRect) -> Self {
        // TableGrid checked these sums and positive dimensions already.
        let original = Fragment {
            top: cell.row(),
            left: cell.column(),
            bottom: cell.row() + cell.rowspan(),
            right: cell.column() + cell.colspan(),
        };
        let mut result = Self {
            pieces: [original; 5],
            len: 0,
        };
        if original.top >= rect.bottom()
            || original.bottom <= rect.top()
            || original.left >= rect.right()
            || original.right <= rect.left()
        {
            result.push(original);
            return result;
        }
        let mut middle = original;
        if middle.top < rect.top() {
            result.push(Fragment {
                bottom: rect.top(),
                ..middle
            });
            middle.top = rect.top();
        }
        let bottom = (middle.bottom > rect.bottom()).then_some(Fragment {
            top: rect.bottom(),
            ..middle
        });
        middle.bottom = middle.bottom.min(rect.bottom());
        if middle.left < rect.left() {
            result.push(Fragment {
                right: rect.left(),
                ..middle
            });
            middle.left = rect.left();
        }
        let right = (middle.right > rect.right()).then_some(Fragment {
            left: rect.right(),
            ..middle
        });
        middle.right = middle.right.min(rect.right());
        result.push(middle);
        if let Some(right) = right {
            result.push(right);
        }
        if let Some(bottom) = bottom {
            result.push(bottom);
        }
        result
    }

    pub fn as_slice(&self) -> &[Fragment] {
        &self.pieces[..self.len]
    }

    fn push(&mut self, fragment: Fragment) {
        self.pieces[self.len] = fragment;
        self.len += 1;
    }
}
