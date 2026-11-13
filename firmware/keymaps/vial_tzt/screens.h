// Екрани: режими, клавіші перемикання, зв'язок із corne-hud і синхронізація половин.
#pragma once

#include "quantum.h"

// Порядок збігається з "customKeycodes" у vial.json (вкладка User у Vial).
enum screen_keycodes {
    SCR_MAIN = QK_KB_0, // наступний режим екрана на половині з USB (із Shift — попередній)
    SCR_SIDE,           // те саме для другої половини
    SCR_OFF,            // вимкнути/увімкнути обидва екрани
};

bool process_record_screens(uint16_t keycode, keyrecord_t *record);
