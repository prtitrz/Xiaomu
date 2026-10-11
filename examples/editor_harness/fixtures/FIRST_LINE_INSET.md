# First-line inset native fixture

This is an opt-in synthetic fixture in the existing `xiaomu-editor-harness`
binary, not a product host or browser-parity claim. The engine provider reads
only harness-local `harnessInset` and `harnessAlign` attributes. The same fixed
base-18px `TextSizeCapability` is shared by the view and final session policy.

## Launch the sealed binary

Use a fresh writable path; never save into the checked-in source fixture:

```sh
/path/to/xiaomu-editor-harness /tmp/first-line-check.txt --first-line-inset --first-line-width=360
```

A missing store loads `first-line-inset.txt`, embedded at build time. An existing
store is loaded unchanged; corrupt snapshots abort. Ctrl+S uses the ordinary
FixtureStore. Close and relaunch the same command to check cold persistence.
The static source is `examples/editor_harness/fixtures/first-line-inset.txt`.

Ctrl+Alt+W cycles the actual text box through 360, 150, 60, 40, 36, 35, 20, 1px,
and back to 360px, without changing the document. Each width is logged. The pane
adds the existing DocumentView's 16px padding on each side; rem is fixed to 16px.
Changing window width alone does not change the fixed fixture text box.

To inspect narrow cases without scrolling through labels that wrap to hundreds
of lines, start with a separate missing path and one selected case:

```sh
/path/to/xiaomu-editor-harness /tmp/first-line-cjk.txt --first-line-inset --first-line-width=35 --first-line-case=11
```

`--first-line-case=1..12` seeds just that case's body, removing its label. This
option applies only when the store does not exist; cold reopen uses saved bytes.

## Declared cases

1. Baseline 0px, left
2. Inset 36px, left, wrapping text
3. Inset 36px, center, wrapping text
4. Inset 36px, right, wrapping text
5. Internal and trailing canonical LF
6. Leading canonical LF
7. Empty paragraph
8. Mixed 30px/18px text, inset based on base 18px
9. Underline and strikethrough
10. Unbroken ASCII ABCDEF
11. CJK
12. Indivisible emoji-family grapheme

The fixture uses the native system font and no CSS word-spacing. Compare row
geometry and input behavior, not exact Chromium glyph coordinates or line ends.
The oracle's leading-BR extra empty ink row has not been isolated as an inset
behavior and is not an acceptance requirement.

## Native observation checklist

- Record title, binary hash, source manifest and actual launch arguments
- Cases 1/2: first row shifts and wraps in its reduced box; continuations start
  at full-width origin; cycle narrow then wide and verify reflow restoration
- Cases 3/4: alignment uses the reduced first-row box, including wrapping
- Cases 5/6: canonical LF does not restart the inset
- Case 8: actual mixed glyphs, row baselines and caret/selection heights
- Cases 10/11/12 at 35px: first indivisible cluster overflows at 36px;
  no synthetic empty first row; following rows begin at zero inset
- Use actual mouse/keyboard input for click caret, wrap-boundary Home/End and
  Up/Down, selection, inserted marker, Undo and Redo
- Ctrl+S, close, restart same path, verify marker and fixture attrs survive
- Do not claim native IME acceptance, all-script support or empty-caret browser
  parity from screenshots. Unsupported positive-inset scripts fail admission

FixtureStore now has a lossless `size:<escaped-value>` mark token for a
TextStyle containing only a string fontSize. Other TextStyle presence/fields
still reject saves instead of being lost. This is harness persistence, not a
public document codec.

Cases 5/6 contain canonical text LF, not typed builtin hardBreak atoms. They
exercise native displayed-line continuation only. Typed hardBreak is covered
separately by mounted engine tests; original browser hardBreak observations
remain an independent source and must not be reported as reproduced by these
LF-only native cases.
