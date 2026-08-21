use crate::text::TextOffset;

use super::NodeId;

/// 文本节点内的稳定位置。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextPosition {
    pub node_id: NodeId,
    pub offset: TextOffset,
}
