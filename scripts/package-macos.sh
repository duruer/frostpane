#!/usr/bin/env bash
# Package an unsigned (ad-hoc signed) universal macOS app into dist/:
#   frostpane-vX.Y.Z-macos-universal.dmg and .zip
# Usage: package-macos.sh [VERSION]   (default: workspace version)
# Env: SKIP_BUILD=1 reuses target/<triple>/release/frostpane for both slices.
# Needs macOS: lipo, codesign, hdiutil, ditto.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
[ "$(uname -s)" = "Darwin" ] || { echo "package-macos.sh runs on macOS only" >&2; exit 1; }

ver="${1:-$(awk '/^\[workspace\.package\]/{p=1;next} /^\[/{p=0} p&&/^version[ ]*=/{gsub(/[^0-9.]/,"",$3); print $3; exit}' Cargo.toml)}"
[[ "$ver" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "bad version '$ver'" >&2; exit 1; }
name="frostpane-v${ver}-macos-universal"
tdir="${CARGO_TARGET_DIR:-$root/target}"
arm="aarch64-apple-darwin"
x64="x86_64-apple-darwin"

if [ "${SKIP_BUILD:-0}" != "1" ]; then
  rustup target add "$arm" "$x64" >/dev/null 2>&1 || true
  for t in "$arm" "$x64"; do
    cargo build --release --locked --target "$t" -p frostpane
  done
fi

dist="$root/dist"
stage="$(mktemp -d)"
trap 'rm -rf "${stage:?}"' EXIT
mkdir -p "$dist"

app="$stage/Frostpane.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create -output "$app/Contents/MacOS/frostpane" \
  "$tdir/$arm/release/frostpane" "$tdir/$x64/release/frostpane"
chmod 0755 "$app/Contents/MacOS/frostpane"
lipo -info "$app/Contents/MacOS/frostpane"
sed "s/@VERSION@/$ver/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
cp packaging/macos/Frostpane.icns "$app/Contents/Resources/Frostpane.icns"
cp LICENSE "$app/Contents/Resources/LICENSE"
plutil -lint "$app/Contents/Info.plist"

# Ad-hoc signature (not notarized). Apple Silicon refuses unsigned binaries,
# so sign after the bundle is complete.
codesign --force --deep -s - "$app"
codesign --verify --deep --strict "$app"

ditto -c -k --keepParent "$app" "$dist/$name.zip"

dmg="$stage/dmg"
mkdir -p "$dmg"
cp -R "$app" "$dmg/"
ln -s /Applications "$dmg/Applications"
hdiutil create -volname "Frostpane" -srcfolder "$dmg" -ov -format UDZO "$dist/$name.dmg"
echo "built $dist/$name.dmg and $dist/$name.zip"
