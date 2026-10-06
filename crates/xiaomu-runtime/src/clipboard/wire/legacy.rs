//! Historical unframed v4-v13 decoding keeps its original fallback contract.

use super::*;

/// Decodes Xiaomu metadata when it matches `plain_text` exactly.
///
/// Unknown versions, malformed/foreign metadata, unsupported canonical
/// values, invalid fragment trees, and stale metadata whose computed fallback
/// differs from the platform text all return `None`. The caller should then
/// paste the supplied plain text normally. An older envelope carrying a
/// newer feature (v4 tables, v5 row attributes, pre-v7 null attributes, or
/// pre-v8 extended link marks, pre-v9 text-style marks, pre-v10 typed/marked atoms,
/// pre-v12 task nodes, or pre-v13 header/table geometry)
/// is also rejected. Unknown attribute variants reject the entire fragment
/// rather than silently dropping values. Historical v1-v3 envelopes remain
/// unsupported, as before the null-attribute extension. All versions are
/// subject to the resource limits documented on [`encode_metadata`].
#[must_use]
pub(super) fn decode_legacy(plain_text: &str, metadata: &str) -> Option<ClipboardSlice> {
    // serde's map deserializer keeps the last duplicate key. Reject that
    // ambiguity before DTO parsing can overwrite an unsupported/null value.
    if !strict_json::validate(metadata) {
        return None;
    }
    let envelope: WireEnvelope = serde_json::from_str(metadata).ok()?;
    if envelope.format != FORMAT
        || !matches!(
            envelope.version,
            VERSION
                | VERSION_TABLE
                | VERSION_TABLE_ROW_ATTRS
                | VERSION_NULL_ATTRS
                | VERSION_LINK_ATTRIBUTES
                | VERSION_TEXT_STYLE
                | VERSION_TYPED_ATOMS
                | VERSION_CLOSED_ROOTS
                | VERSION_TASK_LISTS
                | VERSION_TABLE_GEOMETRY
        )
    {
        return None;
    }
    if (envelope.version >= VERSION_TASK_LISTS && envelope.closed.is_none())
        || (envelope.version == VERSION_CLOSED_ROOTS && envelope.closed != Some(true))
        || (envelope.version < VERSION_CLOSED_ROOTS && envelope.closed.is_some())
    {
        return None;
    }
    if envelope.version < VERSION_TABLE_GEOMETRY
        && envelope.roots.iter().any(WireNode::carries_table_geometry)
    {
        return None;
    }
    if envelope.version < VERSION_TASK_LISTS && envelope.roots.iter().any(WireNode::carries_tasks) {
        return None;
    }
    if envelope.version < VERSION_TYPED_ATOMS
        && envelope.roots.iter().any(WireNode::carries_typed_atoms)
    {
        return None;
    }
    if envelope.version < VERSION_TEXT_STYLE
        && envelope.roots.iter().any(WireNode::carries_text_style)
    {
        return None;
    }
    if envelope.version < VERSION_LINK_ATTRIBUTES
        && envelope.roots.iter().any(WireNode::carries_link_attributes)
    {
        return None;
    }
    if envelope.version < VERSION_NULL_ATTRS && envelope.roots.iter().any(WireNode::carries_null) {
        return None;
    }
    let roots = envelope
        .roots
        .into_iter()
        .map(WireNode::into_node)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if roots.is_empty() || validate_roots(&roots).is_err() {
        return None;
    }
    if (envelope.version < VERSION_TABLE && roots.iter().any(WireNode::carries_table))
        || (envelope.version < VERSION_TABLE_ROW_ATTRS
            && roots.iter().any(WireNode::carries_row_attrs))
    {
        return None;
    }
    let slice = if envelope.closed == Some(true) {
        ClipboardSlice::from_closed_roots(roots)
    } else {
        match &roots[..] {
            [root] if root.content().as_table().is_some() => {
                ClipboardSlice::from_table(root.clone()).ok()?
            }
            _ => ClipboardSlice::from_roots(roots),
        }
    };
    (slice.plain_text() == plain_text).then_some(slice)
}
