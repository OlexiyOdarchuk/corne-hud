#!/usr/bin/env bash
# Збирає Vial-прошивку для Corne v3 (crkbd/rev1) під RP2040 Pro Micro у Docker.
#
#   ./build.sh                          # vial_tzt, sparkfun_pm2040
#   BOOTMAGIC=no ./build.sh             # без входу в бутлоадер через затиснуту Q
#
# Готові .uf2 з'являються в out/.
set -euo pipefail

KEYMAP=${1:-vial_tzt}
CONVERTER=${2:-sparkfun_pm2040}
BOOTMAGIC=${BOOTMAGIC:-yes}

# Перевірена версія vial-qmk; онови, якщо потрібні свіжіші зміни.
VIAL_QMK_REPO=https://github.com/vial-kb/vial-qmk.git
VIAL_QMK_COMMIT=dd43959ae5c08d8a28d38a1acf7b04e86b14a344
DOCKER_IMAGE=docker.io/qmkfm/qmk_cli:latest

HERE=$(cd "$(dirname "$0")" && pwd)
QMK_DIR=$HERE/.vial-qmk
OUT_DIR=$HERE/out

if [[ ! -d $HERE/keymaps/$KEYMAP ]]; then
    echo "Немає розкладки keymaps/$KEYMAP" >&2
    exit 1
fi

if [[ ! -d $QMK_DIR/.git ]]; then
    echo ">> Завантаження vial-qmk ($VIAL_QMK_COMMIT)"
    git init -q "$QMK_DIR"
    git -C "$QMK_DIR" remote add origin "$VIAL_QMK_REPO"
    git -C "$QMK_DIR" fetch -q --depth 1 origin "$VIAL_QMK_COMMIT"
    git -C "$QMK_DIR" checkout -q FETCH_HEAD
    git -C "$QMK_DIR" submodule update -q --init --recursive --depth 1
fi

# Розкладки з цієї папки — основне джерело; в дереві QMK вони лише копія.
rm -rf "$QMK_DIR/keyboards/crkbd/keymaps/$KEYMAP"
cp -r "$HERE/keymaps/$KEYMAP" "$QMK_DIR/keyboards/crkbd/keymaps/$KEYMAP"

mkdir -p "$OUT_DIR"
docker run --rm -e HOME=/tmp \
    -v "$QMK_DIR:/qmk" \
    -v "$HERE/scripts:/scripts:ro" \
    -v "$OUT_DIR:/out" \
    "$DOCKER_IMAGE" /scripts/build-in-docker.sh "$KEYMAP" "$CONVERTER" "$BOOTMAGIC"
