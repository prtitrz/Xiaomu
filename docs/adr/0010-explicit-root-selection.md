# ADR 0010: Explicit whole-root selection and closed clipboard boundaries

Status: Accepted for opt-in host integration; native product acceptance pending
Date: 2026-10-04

## Evidence

A native host's Ctrl+A selected the first inline start through the last inline
end. Pasting a document beginning with a heading into that range inside a list
retained the old list wrapper and fitted the heading's text into a paragraph.
A real editor oracle reproduces this for ordinary TextSelection, but its actual
Ctrl+A uses AllSelection and replaces complete root content. Equal text coverage
is not equal structural intent.

## Decision

Reuse structural coordinates. `DocumentSelection::all(document)` is the exact
root NodeGap range 0..child_count. `is_all` recognizes either direction, never
inline endpoints, partial/nested gaps or a cell range. A truly empty root has
coincident gaps, so it is both all and collapsed, and has nothing to copy. An
empty paragraph remains a real selected child. `set_document_selection` validates
before modifying selection, stored marks or history grouping. The new
`SelectionUpdate::AllDocument` recomputes the range after committing.

GPUI's optional `EditorCommandRouter::select_all` defaults to None. Existing
routers and default Ctrl+A keep their previous behavior. Opt-in hosts must plan
root-range edits through SessionPolicy; unsupported generic edits fail closed.
The native range proxy provides visible preedit/candidate bounds and normal
composition cancel/commit/unmark ordering, with no additional IME waiting.
Left/Right collapse to the real edge, including atomic blocks and final inline
atom ordinals. Ordinary partial-gap navigation is not expanded.

## Clipboard provenance

Whole-root Copy preserves complete children, including boundary atomic blocks,
empty containers and marked inline atoms. Unsupported leaves refuse Copy rather
than disappear. `ClipboardSlice::is_closed` records these source boundaries;
root shape never implies provenance. Existing constructors and ordinary inline
range Copy remain open.

Closed slices encode as conditional metadata v11 with explicit `closed:true`.
Older versions reject a closed field, including null; v11 rejects missing, false,
null and nonboolean values. Original duplicate-key/version/resource checks stay
in force. Older readers cannot interpret v11 and may use their plain-text
fallback; preservation across those readers is not promised.

Generic PasteSlice refuses closed roots unless a host policy plans them.
CodeBlock paste cannot flatten them before policy. Closed Copy, like Cut,
requires exact encode/decode equality before updating the platform clipboard.
Failure preserves the previous clipboard and document. Legacy open Copy retains
its existing fallback behavior.

## Verification and boundaries

The checkpoint passes 844 workspace/all-targets tests and strict Clippy, including
seven new Runtime cases. The full GPUI library has 167 passing tests, with real
virtual-key, focus, native composition, default-route and copy-budget coverage.
Independent review is static. Host persistence and real OS clipboard/IME remain
separate acceptance gates.

This is not arbitrary partial-gap editing or full ProseMirror Slice openness
depths. Public selection-update/error enums gain variants; downstream exhaustive
matches need updates. Existing command-router implementations need no new method.
Old frozen GUI evidence must not be attributed to this checkpoint.
