//! Canonical structured document values and invariants.
//!
//! The document module is implemented in small semantic layers. P0.2A defines
//! stable value types; P0.2B adds canonical nodes, storage, validation, and
//! immutable document snapshots.

mod atom;
mod attrs;
mod content;
mod input_marks;
mod kind;
mod link;
mod marks;
mod node;
mod node_id;
mod snapshot;
mod store;
mod string_attribute;
mod table_attrs;
mod table_grid;
mod text_run;
mod text_style;
mod version;

pub use atom::{AtomKind, InlineAtomContent, InlineAtomPlacement};
pub use attrs::{AttrValue, NodeAttrs};
pub use content::{InlineContent, NodeContent};
pub use image::{
    IMAGE_ATTR_ALT, IMAGE_ATTR_ASSET, IMAGE_ATTR_HEIGHT, IMAGE_ATTR_SRC, IMAGE_ATTR_TITLE,
    IMAGE_ATTR_WIDTH, ImageAttrs, ImageSource,
};
pub use kind::{HeadingLevel, NodeKind};
mod image;
pub use link::{LinkAttributes, LinkMark};
pub use marks::{Mark, MarkKind, MarkSet};
pub use node::Node;
pub use node_id::NodeId;
pub use snapshot::XiaomuDocument;
pub use store::{NodeStore, NodeStoreBuilder};
pub use string_attribute::StringAttribute;
pub use table_attrs::{TableAttribute, TableCellAttrs, TableColumnWidths};
pub(crate) use table_grid::TableGridBudget;
pub use table_grid::{
    CellPlacement, TABLE_MAX_GRID_BYTES, TABLE_MAX_LOGICAL_SLOTS, TABLE_MAX_PHYSICAL_CELLS,
    TableGrid, TableRect,
};
pub use text_run::TextRun;
pub use text_style::{TextStyleAttributes, TextStyleMark};
pub use version::{DocumentRevision, DocumentVersion};

pub(crate) use store::allows_child;

pub(crate) use input_marks::inherited_marks_with_store;
