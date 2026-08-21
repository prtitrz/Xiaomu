//! Typed canonical document mutations.
//!
//! All canonical edits flow through transactions. Applying a transaction
//! validates the resulting document and returns a new immutable snapshot;
//! there is no public direct-mutation path into `XiaomuDocument`.
//!
//! P0.4 covers the first batch of typed steps. Position mapping (P0.5) and
//! inverse generation (P0.6) build on the same step vocabulary.

mod apply;
mod inline;
mod step;

use std::collections::BTreeMap;

use crate::document::XiaomuDocument;
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

    /// Applies this transaction to `document`.
    ///
    /// Returns a fully validated snapshot at the next revision. The input
    /// snapshot is never modified.
    pub fn apply(&self, document: &XiaomuDocument) -> Result<XiaomuDocument> {
        apply::apply_steps(document, &self.steps)
    }
}
