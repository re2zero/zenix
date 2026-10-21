#!/usr/bin/env bash
# Fetch the pinned herdr daemon into res/herdr.
#
# The daemon is not built here any more: zenix ships a binary seed that the
# client copies to ~/.local/bin/herdr on first run, and the binary is pinned by
# version and SHA-256 in vendor/herdr-pins.tsv. This script is the only thing
# that writes res/herdr.
#
#   scripts/fetch-herdr.sh              # the pinned version, replaced
#   scripts/fetch-herdr.sh --check      # verify res/herdr without downloading
#   scripts/fetch-herdr.sh --print-url  # just say where it would come from
#
# It verifies the SHA-256 before replacing anything, and it runs the new binary
# with --version so a wrong-platform or broken download fails here rather than
# on a user's first launch.
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
pins="$repo_root/vendor/herdr-pins.tsv"
seed="$repo_root/res/herdr"

case "$(uname -s)" in
  Linux) os=linux ;;
  Darwin) os=macos ;;
  *)
    echo "fetch-herdr: unsupported host $(uname -s); download the release by hand" >&2
    exit 1
    ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch=x86_64 ;;
  aarch64 | arm64) arch=aarch64 ;;
  *)
    echo "fetch-herdr: unsupported machine $(uname -m)" >&2
    exit 1
    ;;
esac
asset="herdr-$os-$arch"

row=$(awk -F'\t' -v asset="$asset" '$2 == asset { print; exit }' "$pins")
[[ -n "$row" ]] || { echo "fetch-herdr: no pin for $asset in $pins" >&2; exit 1; }
IFS=$'\t' read -r version pinned_asset sha <<<"$row"
[[ "$pinned_asset" == "$asset" ]] || { echo "fetch-herdr: malformed pin row" >&2; exit 1; }
# GitHub's release CDN is slow or unreachable from some networks; pointing
# HERDR_DOWNLOAD_BASE at a mirror keeps the same URL shape and the same pin.
base="${HERDR_DOWNLOAD_BASE:-https://github.com/herdrdev/herdr/releases/download}"
url="$base/v$version/$asset"

if [[ "${1:-}" == "--print-url" ]]; then
  echo "$url"
  exit 0
fi

if [[ "${1:-}" == "--check" ]]; then
  [[ -f "$seed" ]] || { echo "fetch-herdr: $seed is missing" >&2; exit 1; }
  printf '%s  %s\n' "$sha" "$seed" | sha256sum -c --status - \
    || { echo "fetch-herdr: $seed does not match the pinned $version ($sha)" >&2; exit 1; }
  echo "fetch-herdr: $seed matches the pinned $version"
  exit 0
fi

echo "fetch-herdr: $version $asset"
echo "  $url"
tmp=$(mktemp "$repo_root/res/.herdr-XXXXXX")
trap 'rm -f "$tmp"' EXIT
# A bound on a hopeless link: GitHub's CDN measured ~13 KB/s from some networks,
# which is 40 minutes for 29 MB and indistinguishable from a hang. Under 4 KB/s
# for a minute is a dead download, and failing there leaves the seed alone.
curl -fL --retry 3 --retry-delay 2 --speed-limit 4096 --speed-time 60 -o "$tmp" "$url"
printf '%s  %s\n' "$sha" "$tmp" | sha256sum -c --status - \
  || { echo "fetch-herdr: the download does not match the pinned SHA-256" >&2; exit 1; }
chmod 0755 "$tmp"
reported=$("$tmp" --version 2>&1 | head -n 1)
[[ "$reported" == "herdr $version" ]] \
  || { echo "fetch-herdr: the binary says '$reported', expected 'herdr $version'" >&2; exit 1; }
mv -f "$tmp" "$seed"
echo "fetch-herdr: res/herdr <- $reported"
