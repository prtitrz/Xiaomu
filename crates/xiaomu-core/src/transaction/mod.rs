//! Typed canonical document mutations.
//!
//! All canonical edits flow through transactions. Applying a transaction
//! validates the resulting document and returns a new immutable snapshot;
//! there is no public direct-mutation path into `XiaomuDocument`.
//!
//! P0.4 covers the first batch of typed steps. Application also produces the
//! explicit change data used for position mapping (P0.5) and the inverse
//! transaction used for undo round-trips (P0.6).

mod apply;
mod inline;
mod inline_atom;
mod inverse;
mod step;

use std::collections::BTreeMap;

use crate::document::XiaomuDocument;
use crate::mapping::ChangeMap;
use crate::{Error, Result};

pub use step::TransactionStep;

/// Where a transaction came from.
///
/// The origin is semantic bookkeeping for history grouping and future
/// collaboration seams; it does not change application behavior.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransactionOrigin {
    /// Direct user editing intent.
    UserInput,
    /// Engine-internal change.
    System,
    /// Extension-issued change identified by a stable name.
    Extension(String),
}

/// A sequence of typed steps applied atomically to one snapshot.
///
/// Application is all-or-nothing: if any step fails, the original document is
/// left untouched and no partial state escapes the engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transaction {
    origin: TransactionOrigin,
    metadata: BTreeMap<String, String>,
    steps: Vec<TransactionStep>,
}

/// One applied transaction: the next snapshot plus its explicit change data.
#[derive(Clone, Debug)]
pub struct AppliedTransaction {
    document: XiaomuDocument,
    changes: ChangeMap,
    inverse: Transaction,
}

impl AppliedTransaction {
    /// Returns the fully validated new snapshot.
    #[must_use]
    pub const fn document(&self) -> &XiaomuDocument {
        &self.document
    }

    /// Returns the mapping data produced by application.
    #[must_use]
    pub const fn changes(&self) -> &ChangeMap {
        &self.changes
    }

    /// Returns a transaction that undoes this application.
    ///
    /// Applying the inverse against [`Self::document`] reproduces the exact
    /// node store and root of the snapshot the original transaction was
    /// applied to; only the revision moves forward. Inverse steps are
    /// recorded while the engine still sees each step's before-state, so
    /// text, mark, and attribute changes round-trip exactly, and restored
    /// subtrees keep their original node identities.
    #[must_use]
    pub const fn inverse(&self) -> &Transaction {
        &self.inverse
    }

    /// Consumes the outcome and returns the new snapshot.
    #[must_use]
    pub fn into_document(self) -> XiaomuDocument {
        self.document
    }
}

impl Transaction {
    /// Creates an empty transaction with the given origin.
    #[must_use]
    pub fn new(origin: TransactionOrigin) -> Self {
        Self {
            origin,
            metadata: BTreeMap::new(),
            steps: Vec::new(),
        }
    }

    /// Adds one step, returning the transaction for chaining.
    #[must_use]
    pub fn with_step(mut self, step: TransactionStep) -> Self {
        self.steps.push(step);
        self
    }

    /// Adds one step in place.
    pub fn push_step(&mut self, step: TransactionStep) {
        self.steps.push(step);
    }

    /// Assembles a transaction from already-ordered steps.
    pub(crate) fn from_steps(origin: TransactionOrigin, steps: Vec<TransactionStep>) -> Self {
        Self {
            origin,
            metadata: BTreeMap::new(),
            steps,
        }
    }

    /// Attaches one metadata entry; empty keys are rejected.
    ///
    /// Metadata is host-neutral string data. Host-specific types stay outside
    /// Core by contract.
    pub fn set_metadata(&mut self, key: impl Into<String>, value: impl Into<String>) -> Result<()> {
        let key = key.into();
        if key.trim().is_empty() {
            return Err(Error::InvalidNodeAttrKey);
        }
        self.metadata.insert(key, value.into());
        Ok(())
    }

    /// Returns the transaction origin.
    #[must_use]
    pub const fn origin(&self) -> &TransactionOrigin {
        &self.origin
    }

    /// Returns metadata entries in deterministic key order.
    #[must_use]
    pub fn metadata(&self) -> impl ExactSizeIterator<Item = (&str, &str)> {
        self.metadata
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }

    /// Returns the typed steps in application order.
    #[must_use]
    pub fn steps(&self) -> &[TransactionStep] {
        &self.steps
    }

    /// Applies this transaction to `document` and returns the new snapshot
    /// together with explicit change data.
    ///
    /// The returned [`ChangeMap`] translates positions of the input snapshot
    /// into coordinates of the returned snapshot; deleted targets surface as
    /// [`MappedPosition::Deleted`](crate::mapping::MappedPosition::Deleted)
    /// instead of being clamped.
    pub fn apply_with_changes(&self, document: &XiaomuDocument) -> Result<AppliedTransaction> {
        let (document, changes, inverse_steps) = apply::apply_steps(document, &self.steps)?;
        let inverse = Transaction::from_steps(TransactionOrigin::System, inverse_steps);
        Ok(AppliedTransaction {
            document,
            changes,
            inverse,
        })
    }

    /// Applies this transaction to `document`.
    ///
    /// Returns a fully validated snapshot at the next revision. The input
    /// snapshot is never modified. Use [`Transaction::apply_with_changes`]
    /// when the change data is needed for position mapping.
    pub fn apply(&self, document: &XiaomuDocument) -> Result<XiaomuDocument> {
        self.apply_with_changes(document)
            .map(AppliedTransaction::into_document)
    }
}
