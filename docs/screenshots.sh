#!/bin/sh
# Window screenshots for the README, rendered inside a hidden nested KWin: nothing appears on screen.
#
#   cargo build --release && docs/screenshots.sh        # English → docs/media/screenshot-*-en.png
#   docs/screenshots.sh uk                              # Ukrainian → docs/media/screenshot-*.png
#
# Needs kwin_wayland and dbus-run-session (KDE Plasma). Plug the keyboard in first so the window shows it
# as connected. A separate service runs for a few seconds next to the usual one, then stops.
set -eu

LANG_CODE=${1:-en}
BIN=$(realpath "${BIN:-target/release/corne-hud}")
OUT=$(realpath "${OUT:-docs/media}")
case "$LANG_CODE" in
    uk) SUFFIX="" ;;
    *) SUFFIX="-$LANG_CODE" ;;
esac

TMP=$(mktemp -d)
chmod 700 "$TMP"
trap 'kill "$DAEMON" 2>/dev/null || true; rm -rf "$TMP"' EXIT

export XDG_RUNTIME_DIR="$TMP" CORNE_HUD_LANG="$LANG_CODE"
"$BIN" daemon 2>/dev/null &
DAEMON=$!
sleep 1

shot() { # command, file, size
    cat >"$TMP/session.sh" <<EOF
#!/bin/sh
unset DISPLAY
export CORNE_HUD_SCREENSHOT="$2" CORNE_HUD_SCREENSHOT_SIZE="$3"
exec "$BIN" $1
EOF
    chmod +x "$TMP/session.sh"
    dbus-run-session -- kwin_wayland --virtual --width 1200 --height 1200 --no-lockscreen \
        --socket corne-hud-shot --exit-with-session "$TMP/session.sh" >/dev/null 2>&1
    echo "$2"
}

shot gui "$OUT/screenshot-screens$SUFFIX.png" 840x1000
shot draw "$OUT/screenshot-draw$SUFFIX.png" 840x992
