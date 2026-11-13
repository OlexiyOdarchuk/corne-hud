#!/usr/bin/env bash
# Запускається всередині qmkfm/qmk_cli. Аргументи: <keymap> <converter> <bootmagic yes|no>
set -euo pipefail

KEYMAP=$1
CONVERTER=$2
BOOTMAGIC=$3

cd /qmk
KEYMAP_DIR=keyboards/crkbd/keymaps/$KEYMAP
NAME=corne_${KEYMAP}_${CONVERTER}
[[ $BOOTMAGIC == no ]] && NAME=${NAME}_nobootmagic

# EE_HANDS: кожна половина знає свою сторону, тож потрібні два файли.
# Інакше (MASTER_LEFT) один файл на обидві половини.
if grep -qE '^\s*#define\s+EE_HANDS' "$KEYMAP_DIR/config.h"; then
    SIDES=(LEFT RIGHT)
else
    SIDES=(BOTH)
fi

status=0
for side in "${SIDES[@]}"; do
    extra=()
    [[ $side != BOTH ]] && extra+=("EXTRAFLAGS=-DINIT_EE_HANDS_$side")
    [[ $BOOTMAGIC == no ]] && extra+=("BOOTMAGIC_ENABLE=no")

    # Прапорці не входять у залежності make, тож без очистки сторона може не змінитися.
    rm -rf .build
    log=/out/build_${NAME}_${side,,}.log
    if make -j"$(nproc)" "crkbd/rev1:$KEYMAP" CONVERT_TO="$CONVERTER" "${extra[@]}" >"$log" 2>&1; then
        mv "crkbd_rev1_${KEYMAP}_${CONVERTER}.uf2" "/out/${NAME}_${side,,}.uf2"
        rm -f "$log"
        echo "OK   ${NAME}_${side,,}.uf2"
    else
        echo "FAIL ${NAME}_${side,,} (лог: out/$(basename "$log"))"
        status=1
    fi
done
exit $status
