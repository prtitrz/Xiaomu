# Post-P5 native acceptance matrix

Updated 2026-10-25. This matrix consolidates missing evidence; **no new native
acceptance was performed during the audit repair**. Automated GPUI tests use the
virtual test platform and cannot establish real font, IME, clipboard or GPU behavior.

| Capability | Default / host prerequisite | Automated evidence | Native evidence / outstanding work |
| --- | --- | --- | --- |
| P4/P5 original input, atoms, tables | Recorded P5 harness | Original and P5.6 supplemental suites | Windows Microsoft Pinyin historical PASS in [P5 progress](../phases/p5-table/progress.md); no extension of its scope |
| PNG/JPEG clipboard import and sidecar persistence | Host AssetService; supplied harness uses FixtureAssets | Import failure, undo/redo, save/reopen and stale callbacks | Linux/X11 PNG historical PASS in [image record](../image-import-experiment.md); Windows/macOS and native JPEG pending |
| Mixed font size | Opt-in fixed TextSizeCapability and same final-candidate policy | Shaping/admission/geometry using virtual text system | Real fonts/fallback/DPI, selection and IME pending on all target platforms |
| First-line inset | Opt-in provider and same admission policy; conservative LTR limits | Reflow, geometry and input-related regressions | Real fonts/IME and nested containers pending |
| Measured table column resize | Opt-in measured table capability | Rectangles, candidate checks, commit/undo and cancellation | Real pointer drag/cancel, DPI, scrolling, IME and multi-editor pending |
| Task checkbox | Built-in projection; runtime SetTaskChecked; host task schema/persistence policy | Command, race and UI callback tests | Real click/focus/undo and host persisted checked states pending |
| Framed clipboard / prepared Cut / read-only input | Capabilities remain opt-in where documented | Strict decode, refusal, input and rollback tests | OS clipboard ownership, IME, two-editor side effects pending |

Run each capability in its documented harness with disposable snapshots and
assets. For each target platform record: exact commit, OS/backend, display scale,
fonts, input method, enabled capabilities/policy, action sequence, observed result,
screenshots/logs, and unresolved defect. A pending entry requires actual evidence
before being changed to PASS. Windows/macOS/Linux must be recorded independently.

Minimum scenarios: ordinary use; unsupported/refused input with no unwanted
mutation; undo/redo; focus switch between two editors; IME preedit/commit/cancel;
save/reopen where persistence applies. For image import also exercise PNG and
external correct-MIME JPEG, missing/corrupt sidecars and visible pixels. For resize
exercise drag release, Esc, focus loss and scroll during drag. Reuse capability
contracts ([fonts](../mixed-font-size.md), [inset](../first-line-inset.md),
[resize](../measured-column-resize.md)) instead of inventing wider supported ranges.

Owner: repository maintainers / target-platform host integrator. These gates stay
in [planning](../planning.md#delivery-boundaries) before expanding default support.
