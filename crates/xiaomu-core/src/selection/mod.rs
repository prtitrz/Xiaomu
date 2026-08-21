//! 文档选择模型。
//!
//! P0.3 只定义核心语义，不负责 UI 光标渲染。

mod range;

pub use range::{Cursor, SelectionRange};
