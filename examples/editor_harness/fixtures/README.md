# Responsive image smoke fixture

This fixture is synthetic and bounded: ten image nodes reference four PNG/JPEG
files, each under 2 KiB. Copy both the text file and its `.assets` directory to a
new writable directory before a native editing/save/reopen check. Do not save
into this checked-in fixture.

Run the normal harness, opting into the public per-instance image policy:

```sh
cargo run -p xiaomu-editor-harness -- /tmp/image-check/responsive-images.txt --responsive-images
```

Omitting `--responsive-images` retains the original fit-container presentation.
The application still uses the usual asset service, persistence adapter,
selection, input, undo/redo and document scroller. No alternate rendering or
editing path is used for this fixture.

## Checks

- Small PNG 96×64 and JPEG 80×48 keep natural content size when space permits
- Width48 yields a 48px border box and automatic intrinsic-ratio content height
- Height32 alone does not constrain the PNG's natural height
- Mismatched48×200 has the same loaded box as width48 alone
- Explicit width192 may upscale; natural wide images clamp to local content width
- Tall120×720 has a 122×722px natural border box and remains vertically reachable
- Quote/list descendants respect their local containing width
- Narrow→wide window changes reflow image and following text together
- Click selects the whole image without resizing it; following text accepts input
- Undo/redo, Ctrl/Cmd-S and a normal reopen retain canonical attrs and assets

Sizes include a stable 1px border on every side. GPUI physical-pixel rounding
can differ from browser subpixel rounding. Native text styles and margins are
not intended as a browser CSS implementation. Before decode, the view uses a
96px placeholder rather than reserving HTML attribute aspect ratio. Actual
platform checks must be recorded separately from mounted virtual-platform tests.
