use crate::document::NodeId;
use crate::text::TextOffset;

/// 文本节点内的光标位置。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cursor {
    pub node_id: NodeId,
    pub offset: TextOffset,
}

/// 文档内范围选择。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionRange {
    pub anchor: Cursor,
    pub head: Cursor,
}

impl SelectionRange {
    pub fn collapsed(cursor: Cursor) -> Self {
        Self {
            anchor: cursor,
            head: cursor,
        }
    }

    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.head
    }
}
