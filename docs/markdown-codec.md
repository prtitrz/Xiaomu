# Baseline Markdown preservation and resource bounds

The codec imports/exports the P4 built-in subset. Canonical documents remain
immutable; a refusal leaves the caller's document untouched. This is a bounded
codec, not a general CommonMark implementation or a lossless persistence format.

## Export behavior (2026-10-25 maintenance)

- ATX headings preserve Bold, Italic, Strike and Code instead of flattening their
  text. Underline, TextStyle and heading links remain explicit refusals. This
  retains the existing heading-link contract.
- Code spans add one padding space on each side when edge backticks would join
  the delimiter, or when Markdown normalization would remove literal edge spaces.
  All-space code text keeps its exact width. A delimiter of three or more backticks
  at the beginning of a paragraph line is refused because the baseline reader
  treats it as a fenced block; the same code can be represented inside a heading,
  link, or after ordinary text. No invisible characters are inserted.
- Paragraph terminal LF is refused with `HardBreakNotRepresentable`, including
  quote/list descendants and marked runs. Internal LF remains representable;
  list-item output and first-line parsing now use the same hard-break rules.
- Emphasis/link delimiters can interact across adjacent runs. Before returning
  success, inline export re-scans the generated inline form and compares normalized
  text, marks and mark attributes. Unsupported combinations return
  `InlineNotRepresentable`; comparison does not rely on only a stable output string.
- Existing fenced-code import normalization (a nonempty body ends with LF) is
  unchanged. Do not use this codec to preserve arbitrary CodeBlock byte content;
  use canonical snapshot persistence when exact document storage is required.

## Resource contract

Import checks source length **before** normalization/allocation: at most **1 MiB**.
A shared budget across nested quotes/lists permits at most **20,000 nodes**,
including the Document root, and **64 tree levels** below the root. The reader
checks before recursive descent. A shared **16 Mi work units** ceiling bounds
revisited source, inline lookahead, retroactive mark application and unmatched
literal reconstruction; a work unit is a conservative byte/segment operation,
not wall-clock time. The current implementation may conservatively refuse input
before its exact operation count reaches that ceiling.

Export uses an **iterative** preflight to enforce the same node/depth bounds and
a **1 MiB conservative output estimate**, accounting for text, supported scalar
attrs, link destinations, indentation, escapes and delimiters. This estimate is
not the actual output length and can reject a smaller output. Inline validation
also has a bounded scan. Oversized or deeply nested input returns the typed
`ResourceLimit { resource, limit }` error, rather than relying on stack exhaustion.
These are codec-local limits; they do not shrink the Core document model.

## Permanent checks

`export_fidelity` compares kinds, attrs, container structure, text and exact marks
without comparing regenerated NodeIds. It includes heading levels, mixed marks,
Unicode, code padding/fences, adjacent delimiters, terminal and internal LF.
`resource_limits` covers source/node/work/depth limits, a subprocess reproducer
for 8192 nested quotes, nested lists and mixed containers, writer preflight,
malformed short list markers, incomplete destinations and tab-indented fences.
A deterministic punctuation/Unicode corpus supplements these cases; it is not
an exhaustive fuzz campaign.

Run `cargo test --locked -p xiaomu-codec-markdown`.
