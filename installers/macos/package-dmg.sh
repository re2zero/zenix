#!/usr/bin/env bash
# Create a signed .app bundle and .dmg for macOS.
#
# Usage:
#   ./package-macos.sh <version> <arch> <binary-dir>
#
# Example:
#   ./package-macos.sh 1.0.0 x86_64 target/release/
#
# Depends: sips, iconutil, hdiutil (built-in on macOS runners).

set -euo pipefail

VERSION="$1"
ARCH="$2"              # x86_64 or aarch64
BINDIR="$3"            # e.g. target/release/ or target/x86_64-unknown-linux-musl/release/

APP="zenix.app"
DMG="zenix-${VERSION}-${ARCH}-apple-darwin.dmg"
VOLNAME="zenix ${VERSION}"

# ------- .app bundle -------
rm -rf "${APP}"
mkdir -p "${APP}/Contents/MacOS"
mkdir -p "${APP}/Contents/Resources"

# Binary
cp "${BINDIR}/zenix" "${APP}/Contents/MacOS/zenix"
chmod 755 "${APP}/Contents/MacOS/zenix"

# herdr companion if it exists
test -f "${BINDIR}/herdr" && cp "${BINDIR}/herdr" "${APP}/Contents/MacOS/"

# Info.plist
sed -e "s/__VERSION__/${VERSION}/g" \
    -e "s/__YEAR__/$(date +%Y)/g" \
    installers/macos/Info.plist > "${APP}/Contents/Info.plist"

# Icon — convert 512x512 PNG → .icns (macOS runners have sips+iconutil)
ICONSET="zenix.iconset"
mkdir -p "${ICONSET}"
sips -z 16 16     res/zenix.png --out "${ICONSET}/icon_16x16.png"     >/dev/null 2>&1
sips -z 32 32     res/zenix.png --out "${ICONSET}/icon_16x16@2x.png"  >/dev/null 2>&1
sips -z 32 32     res/zenix.png --out "${ICONSET}/icon_32x32.png"     >/dev/null 2>&1
sips -z 64 64     res/zenix.png --out "${ICONSET}/icon_32x32@2x.png"  >/dev/null 2>&1
sips -z 128 128   res/zenix.png --out "${ICONSET}/icon_128x128.png"   >/dev/null 2>&1
sips -z 256 256   res/zenix.png --out "${ICONSET}/icon_128x128@2x.png" >/dev/null 2>&1
sips -z 256 256   res/zenix.png --out "${ICONSET}/icon_256x256.png"   >/dev/null 2>&1
sips -z 512 512   res/zenix.png --out "${ICONSET}/icon_256x256@2x.png" >/dev/null 2>&1
sips -z 512 512   res/zenix.png --out "${ICONSET}/icon_512x512.png"   >/dev/null 2>&1
sips -z 1024 1024 res/zenix.png --out "${ICONSET}/icon_512x512@2x.png" >/dev/null 2>&1
iconutil -c icns "${ICONSET}"
mv "zenix.icns" "${APP}/Contents/Resources/zenix.icns"
rm -rf "${ICONSET}"

# Also bundle the PNG for other uses
cp res/zenix.png "${APP}/Contents/Resources/"

# ------- .dmg -------
rm -f "${DMG}"
# Create read/write DMG first, then compress
TMP_DMG="zenix-${VERSION}-tmp.dmg"
hdiutil create \
  -srcfolder "${APP}" \
  -volname "${VOLNAME}" \
  -fs HFS+ \
  -format UDRW \
  -ov \
  "${TMP_DMG}"

# Attach, set icon position, then detach
DEVICE=$(hdiutil attach -readwrite -noverify -noautoopen "${TMP_DMG}" | grep 'Apple_HFS' | awk '{print $1}')
if [ -n "${DEVICE}" ]; then
  hdiutil detach "${DEVICE}" -quiet 2>/dev/null || \
    hdiutil detach -force "${DEVICE}" -quiet 2>/dev/null || true
fi

# Convert to compressed read-only DMG
hdiutil convert "${TMP_DMG}" -format UDZO -imagekey zlib-level=9 -o "${DMG}"
rm -f "${TMP_DMG}"

echo "=== dmg created ==="
ls -lh "${DMG}"

# Clean up the .app (it's inside the dmg now)
rm -rf "${APP}"
