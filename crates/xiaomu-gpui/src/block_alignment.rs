//! Optional, host-resolved horizontal presentation of inline-bearing blocks.

use xiaomu_core::document::Node;

/// Horizontal alignment of each measured visual line in a block's text box.
///
/// This is presentation only: canonical text, attrs, selection and history are
/// unchanged. Lines wider than their box keep the same signed alignment offset
/// as GPUI's painter. Justification is not supported by this capability.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BlockAlignment {
    /// Start each visual line at the left edge (the unchanged default).
    #[default]
    Left,
    /// Center each visual line, including empty lines, in the text box.
    Center,
    /// End each visual line at the right edge of the text box.
    Right,
}

impl BlockAlignment {
    pub(crate) const fn text_align(self) -> gpui::TextAlign {
        match self {
            Self::Left => gpui::TextAlign::Left,
            Self::Center => gpui::TextAlign::Center,
            Self::Right => gpui::TextAlign::Right,
        }
    }

    pub(crate) fn offset(self, container: gpui::Pixels, width: gpui::Pixels) -> gpui::Pixels {
        match self {
            Self::Left => gpui::Pixels::ZERO,
            Self::Center => (container - width) / 2.0,
            Self::Right => container - width,
        }
    }
}

/// Pure, non-reentrant projection from a current canonical block to alignment.
///
/// Called during the normal render chain for inline-bearing nodes. The host
/// owns attr interpretation, defaults, supported kinds and admission policy.
/// Do not edit the session or perform external effects in this callback.
/// No provider means the original left-aligned presentation. Native range
/// input proxies are not canonical blocks and never invoke this provider.
pub trait BlockAlignmentProvider {
    /// Returns this block's effective visual alignment without changing it.
    fn alignment(&self, node: &Node) -> BlockAlignment;
}
