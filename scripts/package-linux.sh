#!/usr/bin/env bash
# Package a Linux release: tar.gz, .deb and AppImage into dist/.
# Usage: package-linux.sh [x64|arm64] [VERSION]
#   arch defaults to the host arch, VERSION to the workspace version.
# Env:
#   SKIP_BUILD=1      use the existing release binary instead of cargo build
#   BIN=path          binary to package (implies SKIP_BUILD)
#   FORMATS="tar.gz deb appimage"   subset to build
#   APPIMAGETOOL=path appimagetool binary; otherwise downloaded and cached
#   CARGO_TARGET_DIR  honoured
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

arch="${1:-}"
if [ -z "$arch" ]; then
  case "$(uname -m)" in
    x86_64) arch=x64 ;;
    aarch64|arm64) arch=arm64 ;;
    *) echo "unsupported host arch $(uname -m)" >&2; exit 1 ;;
  esac
fi
case "$arch" in
  x64)   triple=x86_64-unknown-linux-gnu;  deb_arch=amd64; ai_arch=x86_64 ;;
  arm64) triple=aarch64-unknown-linux-gnu; deb_arch=arm64; ai_arch=aarch64 ;;
  *) echo "arch must be x64 or arm64" >&2; exit 2 ;;
esac

ver="${2:-$(awk '/^\[workspace\.package\]/{p=1;next} /^\[/{p=0} p&&/^version[ ]*=/{gsub(/[^0-9.]/,"",$3); print $3; exit}' Cargo.toml)}"
[[ "$ver" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "bad version '$ver'" >&2; exit 1; }
formats="${FORMATS:-tar.gz deb appimage}"
name="frostpane-v${ver}-linux-${arch}"
tdir="${CARGO_TARGET_DIR:-$root/target}"

if [ -z "${BIN:-}" ]; then
  # Build for the host triple when it matches, else cross by --target.
  host="$(rustc -vV | sed -n 's/^host: //p')"
  if [ "$host" = "$triple" ]; then
    [ "${SKIP_BUILD:-0}" = "1" ] || cargo build --release --locked -p frostpane
    BIN="$tdir/release/frostpane"
  else
    [ "${SKIP_BUILD:-0}" = "1" ] || cargo build --release --locked --target "$triple" -p frostpane
    BIN="$tdir/$triple/release/frostpane"
  fi
fi
[ -x "$BIN" ] || { echo "binary not found: $BIN" >&2; exit 1; }

dist="$root/dist"
stage="$(mktemp -d)"
trap 'rm -rf "${stage:?}"' EXIT
mkdir -p "$dist"
have() { case " $formats " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

strip_copy() {  # strip_copy SRC DST
  cp "$1" "$2"
  strip "$2" 2>/dev/null || true
  chmod 0755 "$2"
}

# --- tar.gz -------------------------------------------------------------
if have tar.gz; then
  d="$stage/$name"
  mkdir -p "$d"
  strip_copy "$BIN" "$d/frostpane"
  cp LICENSE README.md "$d/"
  cp packaging/linux/frostpane.desktop "$d/"
  cp packaging/linux/icons/frostpane-512.png "$d/frostpane.png"
  tar -C "$stage" --owner=0 --group=0 --numeric-owner -czf "$dist/$name.tar.gz" "$name"
  echo "built $dist/$name.tar.gz"
fi

# --- deb ----------------------------------------------------------------
if have deb; then
  command -v dpkg-deb >/dev/null || { echo "dpkg-deb missing" >&2; exit 1; }
  p="$stage/deb"
  mkdir -p "$p/DEBIAN" "$p/usr/bin" "$p/usr/share/applications" \
    "$p/usr/share/icons/hicolor/256x256/apps" "$p/usr/share/icons/hicolor/512x512/apps" \
    "$p/usr/share/doc/frostpane"
  strip_copy "$BIN" "$p/usr/bin/frostpane"
  install -m 0644 packaging/linux/frostpane.desktop "$p/usr/share/applications/frostpane.desktop"
  install -m 0644 packaging/linux/icons/frostpane-256.png "$p/usr/share/icons/hicolor/256x256/apps/frostpane.png"
  install -m 0644 packaging/linux/icons/frostpane-512.png "$p/usr/share/icons/hicolor/512x512/apps/frostpane.png"
  install -m 0644 LICENSE "$p/usr/share/doc/frostpane/copyright"
  sed -e "s/@VERSION@/$ver/" -e "s/@DEB_ARCH@/$deb_arch/" packaging/deb/control.in > "$p/DEBIAN/control"
  size_kb="$(du -sk "$p/usr" | cut -f1)"
  echo "Installed-Size: $size_kb" >> "$p/DEBIAN/control"
  dpkg-deb --root-owner-group -Zxz --build "$p" "$stage/frostpane.deb" >/dev/null
  # Release asset naming keeps the platform in the name like the other assets.
  mv "$stage/frostpane.deb" "$dist/$name.deb"
  echo "built $dist/$name.deb"
fi

# --- AppImage -----------------------------------------------------------
if have appimage; then
  a="$stage/Frostpane.AppDir"
  mkdir -p "$a/usr/bin" "$a/usr/share/applications" "$a/usr/share/icons/hicolor/256x256/apps"
  strip_copy "$BIN" "$a/usr/bin/frostpane"
  cp packaging/linux/frostpane.desktop "$a/frostpane.desktop"
  cp packaging/linux/frostpane.desktop "$a/usr/share/applications/frostpane.desktop"
  cp packaging/linux/icons/frostpane-512.png "$a/frostpane.png"
  cp packaging/linux/icons/frostpane-256.png "$a/usr/share/icons/hicolor/256x256/apps/frostpane.png"
  cp packaging/linux/icons/frostpane-512.png "$a/.DirIcon"
  cat > "$a/AppRun" <<'APPRUN'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/frostpane" "$@"
APPRUN
  chmod 0755 "$a/AppRun"

  tool="${APPIMAGETOOL:-}"
  if [ -z "$tool" ]; then
    ait_ver="${APPIMAGETOOL_VERSION:-1.9.0}"
    tool="${XDG_CACHE_HOME:-$HOME/.cache}/frostpane-appimagetool/appimagetool-${ait_ver}-${ai_arch}.AppImage"
    if [ ! -x "$tool" ]; then
      mkdir -p "$(dirname "$tool")"
      curl -fsSL -o "$tool" \
        "https://github.com/AppImage/appimagetool/releases/download/${ait_ver}/appimagetool-${ai_arch}.AppImage"
      chmod +x "$tool"
    fi
    if [ -n "${APPIMAGETOOL_SHA256:-}" ]; then
      echo "$APPIMAGETOOL_SHA256  $tool" | sha256sum -c -
    fi
  fi
  # No FUSE on CI runners: run the tool by self-extraction.
  ARCH="$ai_arch" APPIMAGE_EXTRACT_AND_RUN=1 "$tool" "$a" "$dist/$name.AppImage"
  chmod 0755 "$dist/$name.AppImage"
  echo "built $dist/$name.AppImage"
fi
