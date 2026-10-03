# Image import experiment (2026-10-03)

Independent branch based on `8ed83c4` (code baseline `a28003f`). No GPUI dependency or DirectIBus patch changes. This is a bounded host-contract slice, not a production persistence format or a P6 milestone.

## Contract

- Valid Xiaomu structured metadata wins, then PNG/JPEG clipboard image bytes, then text. Unsupported image encodings and file lists are not imported.
- Ctrl+V requires a collapsed inline caret outside CodeBlock and no IME composition. Paste inserts a sibling block after that text block, preserving the caret, as one isolated history entry. Selection replacement, atomic targets and table rectangles are intentionally rejected.
- `AssetService::import_image` defaults to `ImportUnsupported`. Hosts validate bytes, choose canonical attrs and persist before returning an opaque AssetRef. Import is synchronous and should be bounded; an asynchronous host flow is future work.
- Harness sidecar is `<snapshot-path>.assets`. Imports are append-only, bounded PNG/JPEG decodes. Missing/corrupt bytes fail resolution without changing the document. Undo/Redo and other references keep bytes alive. There is no automatic garbage collector; failed insertions or unsaved imports can leave unused assets.
- Snapshot is still harness-internal fixture v5. Moving a document requires moving its sidecar too. Clipboard AssetRef values are not portable across unrelated asset stores.
- Files are written through a same-directory temporary file, synced, then renamed; partial writes cannot truncate the previous snapshot. No multi-file transaction, concurrent-writer arbitration or power-loss durability across directory metadata is claimed.

## Automated checks

- GPUI clipboard extraction without text, unsupported image fallback
- Actual simulated Ctrl+V → host import → canonical image → Ctrl+Z → Ctrl+Shift+Z
- Import error leaves document/selection/history unchanged; code and atomic targets rejected before import
- PNG/JPEG bytes resolve after creating a fresh host, snapshot save/load preserves image semantics
- Undo → save → reopen does not resurrect a node; Redo still resolves original bytes
- Corrupt/missing assets and path traversal references reject; byte/dimension bounds reject
- Failed same-destination partial write/publish preserves old snapshot; temporary files removed
- Stale different-source callback and wrong returned AssetRef cannot overwrite current render source
- Two nodes sharing one AssetRef retain their bytes when one insertion is undone, saved/reopened and redone

Cache limitation: mutable/asynchronous hosts still need request-generation and same-source revision coordination; A→B→A late replies are not fully ordered by this slice. The fixture host uses immutable references and synchronous resolve.

## Native operator gate (pending)

Run `cargo run -p xiaomu-editor-harness -- /tmp/xiaomu-image-gate.txt` in a real desktop session. Do not use a production document.

1. Click a paragraph and copy a visible, non-transparent screenshot through the OS clipboard (PNG MIME). Ctrl+V must show actual pixels, not the neutral fixture image placeholder. Record screenshot and logs.
2. Ctrl+Z removes the image; Ctrl+Shift+Z restores its pixels. Repeat paste/undo/redo without losing text focus.
3. Ctrl+S; close and reopen the same snapshot path. Pixels must remain visible from the sidecar.
4. Undo a new paste, Ctrl+S, close and reopen: the undone node must remain absent although its sidecar exists.
5. With a disposable copied fixture, remove/corrupt a referenced sidecar and reopen: failure placeholder, original image node and surrounding text retained.

Record platform, display/clipboard backend, exact commit and actual outcome. Automated GPUI tests are not OS clipboard or GPU texture evidence. PNG native acceptance does not establish native JPEG, Windows or macOS support. GPUI 0.2.2's X11 image writer labels clipboard data PNG, so JPEG native input must come from an external correct-MIME producer rather than the same writer.

## Linux automated result

On 2026-10-03, Rust 1.97.1, frozen crates.io GPUI 0.2.2: `cargo test --workspace --all-targets` passed 504 tests; `cargo clippy --workspace --all-targets -- -D warnings`, format, source-size and dependency-boundary guards passed. Harness binary built successfully. Build used `CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_PROFILE_TEST_DEBUG=0` to reuse the shared experiment target without excessive debug artifacts. Native desktop gate above remains pending and must be recorded separately.
