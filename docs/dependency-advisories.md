# Dependency advisory disposition — 2026-10-25

Baseline: main `7015166`. Owner: repository maintainers. This maintenance batch
keeps GPUI pinned to **0.2.2** and changes only compatible lockfile resolutions.
It does not replace GPUI, alter host trust policy, or suppress advisory IDs.

| Dependency | Before → after | Disposition |
| --- | --- | --- |
| rustls | 0.23.43 → 0.23.46 | Patched versions for [0285](https://rustsec.org/advisories/RUSTSEC-2026-0285.html), [0346](https://rustsec.org/advisories/RUSTSEC-2026-0346.html), [0348](https://rustsec.org/advisories/RUSTSEC-2026-0348.html), [0349](https://rustsec.org/advisories/RUSTSEC-2026-0349.html) |
| rustls-webpki | 0.103.15 → 0.103.16 | Patched for [0382](https://rustsec.org/advisories/RUSTSEC-2026-0382.html) |
| tokio | 1.53.1 → 1.53.3 | Patched for [0386](https://rustsec.org/advisories/RUSTSEC-2026-0386.html) |
| git2 | 0.20.4 unchanged | [0183](https://rustsec.org/advisories/RUSTSEC-2026-0183.html) / [0184](https://rustsec.org/advisories/RUSTSEC-2026-0184.html) remain scanner findings; reviewed test-only exposure, upstream resolution pending |

The three downloaded archives were verified against the official crates.io index
SHA256 values. Cargo.lock retains the crates.io source; no mirror/source override
or locally modified third-party source is introduced.

## Remaining git2 constraint

`gpui_util 0.2.2` declares optional `git2 = "0.20.1"`, enabled by test-support.
A compatible Cargo update cannot select the patched **0.21.0** line. The current
all-feature/all-target **normal + build** graph has no git2; it enters through the
GPUI dev/test feature used by virtual tests. The upstream test helper
`gpui_util/src/test.rs::write_tree` uses `Repository::init` for temporary fixtures.
No use of affected `Remote::list`, `Blame::blame_buffer` or BlameHunk signatures
was found in Xiaomu or that helper. This is a source/graph reachability review,
not proof that the git2 crate itself is fixed or generally safe.

Retain visible findings (`deny.toml` still has an empty ignore list), while keeping
the existing nonblocking advisory policy. `check_dependency_advisory_scope.py`
now fails the Linux CI check if git2 enters the production graph or its version
changes without review. It does **not** accept new RustSec advisories and does not
replace `cargo deny check advisories`.

Maintainers must renew this review **by 2026-11-25**, or earlier when GPUI/git2,
Cargo features, target support, or fixture helpers change. Resolve the version
constraint in a dedicated GPUI dependency maintenance PR when a compatible
upstream is available; do not force 0.21 across a 0.20 API contract or vendor a
large unused production dependency solely to hide a test-only scanner result.
Downstream hosts enabling GPUI test-support outside tests must assess their own
resolved graph and call sites.

## Verification

```sh
cargo tree --locked --all-features --target all -e normal,build -i git2
python3 tools/check_dependency_advisory_scope.py
cargo deny check advisories
cargo deny check bans licenses sources
```

After these updates the advisory scan reports **2**, down from **8**, and still
returns a failure status for the two git2 findings. Do not describe advisory CI
as clean. Local tests and platform CI evidence belong in the
[maintenance record](maintenance/2026-10-main-audit.md).
