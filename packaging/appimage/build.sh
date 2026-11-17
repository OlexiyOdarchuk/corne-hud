#!/usr/bin/env bash
# Build an AppImage from a prebuilt release binary.
#
#   packaging/appimage/build.sh [BINARY] [OUTDIR]
#
#   BINARY      default: target/release/corne-hud
#   OUTDIR      default: dist
#   ARCH        env, default: uname -m (x86_64, aarch64)
#   VERSION     env, default: version from Cargo.toml
#   APPIMAGETOOL env, default: appimagetool from PATH
#
# The binary links only to glibc (winit/wayland/xkbcommon/GL are loaded with dlopen),
# so no linuxdeploy step is needed. Build it on an old distro (Ubuntu 22.04) for wide glibc compatibility.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BIN="$(realpath "${1:-$ROOT/target/release/corne-hud}")"
OUT="$(mkdir -p "${2:-$ROOT/dist}" && realpath "${2:-$ROOT/dist}")"
ARCH="${ARCH:-$(uname -m)}"
VERSION="${VERSION:-$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n1)}"
TOOL="${APPIMAGETOOL:-appimagetool}"
ID=io.github.OlexiyOdarchuk.CorneHud

[ -x "$BIN" ] || { echo "binary not found: $BIN" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
APPDIR="$WORK/CorneHud.AppDir"

install -Dm755 "$BIN" "$APPDIR/usr/bin/corne-hud"
install -Dm644 "$ROOT/packaging/corne-hud.desktop" "$APPDIR/usr/share/applications/$ID.desktop"
install -Dm644 "$ROOT/packaging/$ID.metainfo.xml" "$APPDIR/usr/share/metainfo/$ID.metainfo.xml"
install -Dm644 "$ROOT/packaging/icons/corne-hud.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/$ID.svg"
for s in 256 128 64 48 32; do
    install -Dm644 "$ROOT/packaging/icons/corne-hud-$s.png" "$APPDIR/usr/share/icons/hicolor/${s}x${s}/apps/$ID.png"
done
install -Dm644 "$ROOT/packaging/70-corne-hud.rules" "$APPDIR/usr/lib/udev/rules.d/70-corne-hud.rules"

# appimagetool wants the desktop file and the icon at the AppDir root.
cp "$APPDIR/usr/share/applications/$ID.desktop" "$APPDIR/$ID.desktop"
cp "$ROOT/packaging/icons/corne-hud-256.png" "$APPDIR/$ID.png"
ln -s "$ID.png" "$APPDIR/.DirIcon"

cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
# No arguments opens the window; any corne-hud subcommand works too (e.g. `daemon`, `status`).
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/corne-hud" "$@"
APPRUN
chmod 755 "$APPDIR/AppRun"

OUTFILE="$OUT/corne-hud-$VERSION-$ARCH.AppImage"
ARCH="$ARCH" VERSION="$VERSION" "$TOOL" --no-appstream "$APPDIR" "$OUTFILE"
echo "$OUTFILE"
