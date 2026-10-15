# Read-only user input

Hosts can opt an editor instance into a fixed read-only input mode:

```rust,ignore
let editor = EditorInstance::new_with_policy(document, selection, hooks, policy)?
    .with_read_only_input();
let view = editor.build_view();
assert!(view.is_read_only_input());
```

All existing constructors remain editable by default. The opt-in affects views
built afterward; existing views are unchanged. Each view keeps its mode for its
entire lifetime. Normal, nested/table and All/node/cell-range `ParagraphView`
children inherit it at construction, including children created after host
updates. There is no mounted-mode setter or mutable shared permission flag.

## User-input boundary

Read-only input silently consumes these operations before their editing effects:

- Native typing, explicit UTF-16 replacement, IME preedit and commit/unmark
- Built-in editing keys, formatting, Enter variants and Tab/Shift-Tab
- Public toolbar command paths `apply_edit_intent` and
  `apply_edit_intent_with_selection`, before target selection or policy preflight
- Cut, before either prepared-Cut handling or legacy clipboard publication
- Paste, before clipboard decoding, command routing or image asset import
- Undo/Redo and the built-in Save action
- Task checkbox changes and table-column resizing, including preview/host callbacks

Tab is intentionally consumed: the normal next-cell intent can append a row.
Read-only rejection does not create preedit, advance document revision/history,
clear pending marks, move selection, sample the history clock, import an asset,
write the clipboard, call an editing router or emit an `EditorRejection`.
`selected_text_range(false)` reports disabled native input; queries explicitly
ignoring disabled input and read-only text/geometry queries remain available.

Selection, arrow/Home/End navigation, Select All, Copy, scrolling, reading
highlights and passive reveal keep their normal behavior. Selection may naturally
clear stored marks or close history grouping; read-only input does not freeze
all session state. Selection-only command routers and inline-atom activations
remain enabled. Rendering may still resolve existing image resources.

The current frontend imports image pixels only through clipboard Paste. It has
no native external file/image-drop importer. Host-owned drop handlers must check
`is_read_only_input()` before their own asset import or other external effects;
passing an already imported asset to a blocked edit command cannot undo that
host side effect. Future Xiaomu input/import entry points must apply this same
guard before side effects.

## Separate session policy

This mode is a user-input guard, not an authorization boundary or an immutable
document. Controlled host updates through `apply_edit_transaction`,
`apply_passive_edit_plan` and the shared session remain available. Their existing
validation and history contracts are unchanged. A host must bind an independent
`SessionPolicy` for any restrictions that must also cover host commands, raw
transactions, history or final canonical candidates. The view mode neither
installs nor replaces that policy.

Likewise, host controls, atom-activation callbacks and other adapters retain their
own permissions and lifecycle responsibilities. A read-only view can share a
session with an editable view only by deliberate host choice; updates from that
other view are not blocked by this mode.

## Verification boundary

The `read_only_input` GPUI tests exercise actual virtual-platform keyboard,
clipboard and pointer dispatch, plus direct and retained `EntityInputHandler`
callbacks. They check canonical/history/selection/marks, clipboard preservation,
host callback counts, newly created children, controlled host updates, and
selection/Copy/reading behavior. Existing editable behavior remains in the
workspace regression matrix. These deterministic tests do not establish native
OS IME, clipboard, font or host-application acceptance.

Local Linux verification on 2026-10-15: 11 focused read-only input regressions;
1,696 workspace/all-targets tests in 114 binaries; 6 workspace compile-fail
doctests; strict Clippy, formatting and all four source/dependency/vendor guards.
The separate vendor checks passed 8 XIM tests and 89 published Taffy inline tests
plus 5 doctests; the Taffy crate does not include the full upstream conformance
suite. Local `cargo-deny` was unavailable; the existing CI policy job runs its
bans/licenses/sources check and remains a required publication gate.
