# ADR 0008: Preserve typed text-style attributes separately from rendering

Status: Accepted (local experiment)
Date: 2026-10-03

## Context

Rich-text sources distinguish absent, null and string color, font-family and
font-size attributes. Empty strings and marks with no visual attributes can
still be meaningful source values. A renderer-side annotation or CSS-normalized
number cannot retain those distinctions through editing, history and clipboard.

## Decision

- Add `TextStyleMark` and `TextStyleAttributes`, with three private fields using
  the existing `StringAttribute::{Missing, Null, Value(String)}`. Builders and
  getters preserve exact values, including empty and unrecognized strings.
- `Mark::TextStyle` is an ordinary semantic kind. Equality, conflicts, run
  normalization, AddMark/RemoveMark and their inverses operate on the full value.
  A mark is not removed just because all its fields are missing, null or empty.
- Core does not interpret CSS, resolve fonts, convert units, insert browser
  defaults or implement another editor's mark-exclusion and cleanup rules.
  Host field patches first merge each affected run's attributes, then publish
  one transaction; generic SetMark remains complete-value replacement.
- Clipboard introduces a `text_style` variant in conditional v9. Its three
  `color`, `font_family`, `font_size` slots are required, each an explicit tagged
  missing/null/string value. The strict duplicate-key and unknown-field checks
  remain unchanged. Pre-v9 envelopes cannot carry the new kind. Payloads without
  TextStyle retain the previous v4–v8 feature choice and interpretation.
- Runtime exact insertion, paste and cross-block atom reconstruction share a
  single complete mark-kind list so absent inserted styles do not inherit the
  destination's TextStyle accidentally.
- Markdown and the old harness fixture codec reject every TextStyle, including
  visually empty attributes. Their formats cannot preserve the mark or its exact
  attributes. Heading's plain-text export must reject it too.
- `DocumentSession::effective_input_marks` exposes read-only default replacement
  inheritance for frontend overlays. Explicit pending marks win; otherwise the
  replacement start chooses the same surrounding run as the default commit.
  This is not policy execution or permission to edit a session directly.

Canonical DocumentVersion remains 1: Core has no serialized format whose bytes
need migration. The actual clipboard representation has its explicit v9 boundary.

## Rendering boundary

Canonical support and editable rendering support are separate capabilities.
A color/fontFamily frontend increment can ship while documents carrying a
fontSize value remain visibly read-only at the host boundary. This is a temporary
rollout gate, not a permanent storage restriction. Unsupported style strings must
remain intact; they cannot be rewritten to null/default to make a document seem
editable. Full mixed-size shaping, line metrics, caret/selection/IME geometry and
cache invalidation must be enabled together, not approximated only in paint.

The first GPUI implementation resolves CSS Color3 values and CSS font-family
lists into real TextRuns, preserving the original canonical strings. Invalid or
dynamic unsupported values retain the surrounding native style. Font availability
is platform dependent. GPUI0.2.2 Linux does not consume explicit FontFallbacks
ordering; the selected primary family and existing system glyph fallback remain
available, but cross-platform identical glyphs/fallback order are not promised.
Native family tokenization also differs from the original host's comma-split
then-quote HTML rendering for unusual quoted/escaped family names.

Preedit now projects the same effective default replacement marks as commit,
including existing Bold/Link marks, fixing the previous always-default overlay.
This changes only display styling; platform transport, ordinary unmark/click
ordering and composition lifetime are unchanged. A custom policy that replaces
the commit plan may choose different marks; the read-only query does not execute
that policy. Layout cache identity includes base style and resolved run styles,
even when pending marks change without a canonical revision.

## Verification scope

Source regressions cover independent attribute states, arbitrary strings,
same-kind conflicts, exact inverses/history, pending marks, ordinary and atom
reconstruction, strict conditional clipboard versions, rejection atomicity,
read-only input-mark queries, and Markdown/fixture refusal. Compilation, complete
workspace checks and native rendering acceptance are separate integration gates.

2026-10-03 integration:660 workspace tests, strict all-targets Clippy, source-size,
dependency and decoder provenance guards, and cargo-deny bans/licenses/sources
pass. The host's native GUI acceptance is still pending. Two initially incorrect
new test expectations were corrected: #f008 alpha is0x88, and the custom-draw
cache test must inspect its own frame rather than a subsequent root refresh.
