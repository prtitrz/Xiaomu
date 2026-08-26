//! Frontend-neutral persistence seam.
//!
//! Hosts implement [`DocumentPersistence`] to move canonical snapshots in
//! and out of the editor. The seam carries Core types only: how a snapshot
//! becomes bytes (format, storage, sync protocol) is entirely the host
//! adapter's business, and the editor never learns it.
//!
//! The P2.6 host-contract harness exercises this seam with a fixture
//! adapter whose on-disk format is harness-internal and explicitly not a
//! codec commitment.

use xiaomu_core::document::XiaomuDocument;

use core::fmt;

/// Typed failure of one persistence operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistenceError(pub String);

impl fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "persistence failed: {}", self.0)
    }
}

impl std::error::Error for PersistenceError {}

/// Load/save adapter between the editing session and a host store.
///
/// The editor treats the adapter as the only bridge to durable state: it
/// never touches files, databases, or network itself.
pub trait DocumentPersistence {
    /// Persists one canonical snapshot.
    ///
    /// Implementations must persist the snapshot as a whole; there is no
    /// incremental delta contract at this seam.
    fn save(&mut self, document: &XiaomuDocument) -> Result<(), PersistenceError>;

    /// Returns the most recently persisted snapshot, if one exists.
    ///
    /// Returning `None` starts the editor from whatever initial document
    /// the host provides instead.
    fn load(&self) -> Option<XiaomuDocument>;
}
