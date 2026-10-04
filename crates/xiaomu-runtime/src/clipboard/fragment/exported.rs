//! Export descriptors and validated slice construction.

use super::*;

impl ClipboardSlice {
    /// Returns explicitly captured provenance, absent for historical exports.
    /// CellRange remains open even when its carrier covers an entire table.
    #[must_use]
    pub const fn source_boundary(&self) -> Option<ClipboardSourceBoundary> {
        self.source_boundary
    }

    /// Returns the deterministic text algorithm, independently of provenance.
    #[must_use]
    pub const fn text_projection(&self) -> Option<ClipboardTextProjection> {
        self.text_projection
    }

    /// Whether Copy must preserve the complete native structure and metadata.
    /// A transport failure must leave the previous clipboard unchanged.
    #[must_use]
    pub const fn requires_lossless_transport(&self) -> bool {
        self.closed || self.source_boundary.is_some() || self.text_projection.is_some()
    }

    /// Whether source provenance permits the default generic paste fitter.
    ///
    /// This is an admission check, not a promise that a payload fits a target.
    /// Historical open fragments and explicit ordinary Open fragments retain
    /// their existing fitting rules. WholeRoots needs a host-aware whole-block
    /// plan. CellRange is open 1/1 and its Table DTO is only a transport carrier;
    /// neither Rows nor Table root form grants ordinary table-root semantics.
    /// Frontends must apply this same check before default text flattening.
    /// An explicit host policy/command route may handle a rejected boundary.
    #[must_use]
    pub const fn allows_default_fitting(&self) -> bool {
        match self.source_boundary {
            None | Some(ClipboardSourceBoundary::Open) => !self.closed,
            Some(
                ClipboardSourceBoundary::WholeRoots | ClipboardSourceBoundary::CellRange { .. },
            ) => false,
        }
    }

    pub(crate) fn set_export(
        &mut self,
        boundary: ClipboardSourceBoundary,
        projection: Option<ClipboardTextProjection>,
    ) -> std::result::Result<(), ()> {
        crate::clipboard::export_budget::roots(&self.roots)?;
        if self.closed != matches!(boundary, ClipboardSourceBoundary::WholeRoots) {
            return Err(());
        }
        if matches!(boundary, ClipboardSourceBoundary::CellRange { .. })
            && !matches!(&self.roots[..], [node] if node.kind() == &NodeKind::Table && node.content().as_table().is_some())
        {
            return Err(());
        }
        if projection.is_some() {
            self.plain_text = crate::clipboard::text_projection::project(&self.roots)?;
        }
        self.source_boundary = Some(boundary);
        self.text_projection = projection;
        Ok(())
    }

    pub(crate) fn from_export_roots(
        roots: Vec<ClipboardNode>,
        boundary: ClipboardSourceBoundary,
        projection: Option<ClipboardTextProjection>,
    ) -> std::result::Result<Self, ()> {
        crate::clipboard::export_budget::roots(&roots)?;
        validate_roots(&roots).map_err(|_| ())?;
        if roots.is_empty() {
            return Err(());
        }
        if projection.is_some() {
            let mut blocks = Vec::new();
            flatten_blocks(&roots, &mut blocks);
            let mut slice = Self {
                plain_text: String::new(),
                roots,
                blocks,
                closed: matches!(boundary, ClipboardSourceBoundary::WholeRoots),
                source_boundary: None,
                text_projection: None,
            };
            slice.set_export(boundary, projection)?;
            return Ok(slice);
        }
        let mut slice = match boundary {
            ClipboardSourceBoundary::WholeRoots => Self::from_closed_roots(roots),
            ClipboardSourceBoundary::CellRange { .. } => {
                if !matches!(&roots[..], [node] if node.kind() == &NodeKind::Table && node.content().as_table().is_some())
                {
                    return Err(());
                }
                Self::from_table(roots.into_iter().next().ok_or(())?).map_err(|_| ())?
            }
            ClipboardSourceBoundary::Open => match &roots[..] {
                [node] if node.content().as_table().is_some() => {
                    Self::from_table(roots.into_iter().next().ok_or(())?).map_err(|_| ())?
                }
                _ => Self::from_roots(roots),
            },
        };
        slice.set_export(boundary, projection)?;
        Ok(slice)
    }
}
