#!/usr/bin/env bash
# Re-copy the endpoint protocol crates from a herdr-gpui checkout.
#
#   scripts/sync-herdr-vendor.sh /home/zero/zero/up/herdr-gpui [--write]
#
# Without --write it only reports what would change: the src trees are compared
# and the endpoint wire file is diffed, because field and variant order there is
# the compatibility contract with the daemon (a reordering is a breaking change
# even when it compiles).
#
# The crates' `Cargo.toml` files are deliberately not touched: this tree's
# copies were rewritten to stand alone (no workspace inheritance), and an
# upstream copy would drag the workspace back in.
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
upstream=${1:?usage: sync-herdr-vendor.sh <herdr-gpui-checkout> [--write]}
write=${2:-}

[[ -d "$upstream/crates/herdr-protocol" ]] || {
  echo "sync-herdr-vendor: $upstream does not look like a herdr-gpui checkout" >&2
  exit 1
}

echo "upstream: $upstream @ $(git -C "$upstream" log -1 --format=%h)"
for crate in herdr-protocol herdr-client; do
  src="$upstream/crates/$crate/src"
  dst="$repo_root/vendor/$crate/src"
  [[ -d "$src" ]] || { echo "sync-herdr-vendor: missing $src" >&2; exit 1; }
  echo "--- $crate"
  diff -rq "$src" "$dst" || true
  if [[ "$write" == "--write" ]]; then
    rm -rf "$dst"
    cp -a "$src" "$dst"
    echo "    written"
  fi
done

echo "--- wire contract (vendor/herdr-protocol/src/wire.rs)"
# Best effort: the wire file may not exist in a future upstream layout, and a
# missing diff tool is not a reason to fail a report.
if command -v diff >/dev/null; then
  diff -u "$upstream/crates/herdr-protocol/src/wire.rs" \
    "$repo_root/vendor/herdr-protocol/src/wire.rs" | head -n 80 || true
fi
