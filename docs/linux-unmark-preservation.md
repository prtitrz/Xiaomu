# Linux stock-GPUI unmark preservation

This narrow fix is based on main `b3d2f982`, using the existing pinned crates.io GPUI 0.2.2 and unchanged Cargo.lock. It does not include the old `gpui-zed` compatibility feature, custom XIM owner/generation contract, deferred pointer-focus barrier, or changes to frozen GPUI/DirectIBus.

## Cause and scope

GPUI `InputHandler::unmark_text` removes composing state. In stock GPUI 0.2.2 X11's mouse-press path, reset/unmark reaches the old input handler before MouseDown changes the caret/focus. The Wayland pointer-press path likewise emits UnmarkText. Xiaomu keeps preedit outside its canonical document, so cancelling that overlay on unmark loses the displayed text instead of just removing its marking. Zed's in-buffer preedit does not have this mismatch: its unmark clears highlighting and ends the IME transaction while text remains in the buffer.

On Linux only, a bare unmark now preserves a nonempty, non-rejected live overlay through existing `commit_composition`, using its original base range and one isolated undo entry. This preserves the preedit already delivered to the application, not a candidate word that the input method has not sent. No MouseDown interception or artificial waiting is introduced. The frozen/stock ResetIC reply path is not modified or appended again.

An explicit result already commits; subsequent unmark is a no-op. Empty mark/empty replacement explicitly cancels; subsequent unmark is also a no-op. Rejected atom-spanning compositions are never committed. `on_focus_out` cancellation remains a separate unchanged path, so programmatic/shortcut focus changes and window deactivation are not automatically treated as mouse unmark. macOS/Windows retain their existing handling pending their own native evidence; this fix does not generalize Linux's callback order to them.

## Automated validation

Linux workspace: **514 tests passed**, plus Clippy with `-D warnings`, formatting, source-size and dependency-boundary guards.

Permanent regressions cover:

- Latest nonempty preedit retained once at all three same-offset atom gaps; exact undo/redo
- Original nonempty and reversed Unicode selection/base range; atom semantics preserved
- Explicit result → unmark/focus-out produces no duplicate or appended raw preedit
- Empty mark, empty replacement, initial empty mark and idle unmark do not mutate document, selection or history
- Rejected atom-spanning preedit cannot commit
- Empty mark followed by an explicit result preserves the original selected-range behavior
- Old handler retention does not edit another editor; focus-out cancellation stays separate
- Rectangular-cell input proxy commits once and undo restores the original range
- Other platforms' existing bare-unmark policy remains covered in their conditional test

The prior geometry test that used bare unmark merely to clean up now uses an explicit empty-mark cancellation. It no longer encodes the old Linux bug as the expected contract.

## Native evidence boundaries

Before-fix standard dual-editor binary SHA256 `304320514c8daadaf6d3cdc4029f9c11ebd114cf88fa60f46f8f164054175849` reproduced left preedit `nihao` disappearing when clicking right, with both canonical revisions still zero. It also survived 100 ordinary focus alternations without the old patched route's 64-context Unavailable condition.

A separate before-fix Chinese result commit encountered a `zed-xim 0.4.0` compound-text decoder `UnsupportedEncoding` panic. That dependency/encoding issue is outside this patch, is not fixed by unmark preservation, and prevents claiming the whole stock IME path is fully accepted. The unmark-only after-fix candidate showed the original field's canonical change before target pointer capture/snapshot, normal focus switching without a wait, no insertion after Escape, one Undo for the preserved preedit, and clean exit/cleanup status 0. The engine-supplied preedit bytes are preserved verbatim, including literal characters emitted by its configured display style; this is not a promise to synthesize an undelivered candidate. Combined Chinese-result and default-style validation, plus the separate audited vendored decoder repair, are recorded in [the decoder evidence](linux-xim-decoder.md). Synthetic input-handler tests are not a substitute for native evidence.
