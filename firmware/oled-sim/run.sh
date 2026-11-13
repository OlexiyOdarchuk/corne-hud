#!/usr/bin/env bash
# Збирає симулятор і малює превʼю всіх режимів у ./out (або в каталог з аргументу).
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=${1:-$HERE/out}
KM=$HERE/../keymaps/vial_tzt
mkdir -p "$OUT"
gcc -std=gnu11 -O2 -Wall -Wextra -I"$HERE/include" -I"$KM" "$HERE/sim.c" "$KM/oled_ui.c" \
    "$HERE/../.vial-qmk/keyboards/crkbd/lib/glcdfont.c" -o "$OUT/sim"
"$OUT/sim" > "$OUT/frames.bin"
python3 "$HERE/render.py" "$OUT/frames.bin" "$OUT" "${@:2}"
