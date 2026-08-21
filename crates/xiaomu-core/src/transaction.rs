//! Typed canonical document mutations.
//!
//! All canonical edits flow through transactions. Applying a transaction must
//! preserve document invariants and return explicit change/mapping data.
