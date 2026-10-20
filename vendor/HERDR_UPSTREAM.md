# herdr upstream: what this tree pins

zenix talks to a herdr **daemon** it does not build. Two artefacts stand in for
the upstream source tree, and both are pinned here:

| What | Where | Pinned by |
|---|---|---|
| the daemon binary (first-run seed) | `res/herdr` (built artefact, not in git) | `vendor/herdr-pins.tsv` + `scripts/fetch-herdr.sh` |
| the endpoint protocol implementation | `vendor/herdr-protocol`, `vendor/herdr-client` | the herdr-gpui commit recorded below |

There is **no herdr submodule** and there has not been one since the tree was
cleaned in M10. The submodule used to supply the seed by compiling upstream
source with Zig; it also drifted — at the time it was removed the tree pinned
v0.9.3 while the daemon actually running on the development machine was 0.9.5.
A binary pinned by version and SHA-256 says the same thing without two
gigabytes of checkout.

## Daemon pin

`vendor/herdr-pins.tsv` is the single source of truth: `<version>\t<asset>\t<sha256>`.
The SHA-256 values are GitHub's own asset digests (`sha256:` field of the
release API), not numbers computed from a local download.

Current pin: **0.9.5**. Refresh with `scripts/fetch-herdr.sh`; bump by editing
the TSV (all rows must carry the same version — `build.rs` fails the build if
`res/herdr` does not match the row for this platform).

## Protocol pin

`vendor/herdr-protocol` and `vendor/herdr-client` are copies of the
corresponding crates from **herdr-gpui** (the upstream GPUI client),
`crates/herdr-protocol` and `crates/herdr-client`:

- source: `https://github.com/penso/herdr-gpui` (local read-only reference
  checkout used while this tree was built: `/home/zero/zero/up/herdr-gpui`)
- upstream commit at copy time: `87f4dc0` (2026-10-08, `Merge pull request #344 …`)
- endpoint protocol generation: `1` (`ENDPOINT_PROTOCOL_GENERATION` in
  `crates/herdr-protocol/src/endpoint.rs`)

These are **local source, not a read-only mirror**: they may be edited when a
daemon change needs it. Two consequences:

1. `NOTICE.md` in each crate states the provenance; Apache-2.0 §4(b) requires a
   conspicuous notice on modified files, so any edit keeps that file honest.
2. `scripts/sync-herdr-vendor.sh <herdr-gpui-path>` re-copies the upstream
   `src/` trees and prints the diff summary, so a real resync is one command
   rather than a manual merge. The crates' `Cargo.toml` files stay local: they
   were rewritten to stand alone (no workspace, no git dependency).

## What to do when the daemon is upgraded

1. `scripts/fetch-herdr.sh` after editing the TSV row(s) — this proves the asset
   runs on this machine (`--version`) before it replaces `res/herdr`.
2. `cargo run --bin zenix-probe` — it compares the running daemon's `--version`
   with the pin and prints `match=true|false`.
3. If the endpoint wire changed, re-sync the protocol crates (see above) and
   diff `wire.rs` yourself: field and variant order are the compatibility
   contract, so a silent reordering is a breaking change.
