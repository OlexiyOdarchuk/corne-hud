// Графіка для OLED 128x32. Без залежностей від QMK: малює в масив кадру,
// тому той самий код збирається і в прошивку, і в симулятор на ПК (qmk/oled-sim).
#pragma once

#include <stdbool.h>
#include <stdint.h>

#define UI_W 128
#define UI_H 32
#define UI_FB_SIZE (UI_W * UI_H / 8)
#define UI_TEXT_LEN 48

enum {
    UI_MOD_CTRL  = 1 << 0,
    UI_MOD_SHIFT = 1 << 1,
    UI_MOD_ALT   = 1 << 2,
    UI_MOD_GUI   = 1 << 3,
};

// Режими екрана. Порядок = порядок перемикання клавішею.
enum {
    UI_SCR_STATUS, // шар, WPM, графік, модифікатори
    UI_SCR_SHIP,   // корабель і астероїди
    UI_SCR_PET,    // вихованець, що бігає, коли друкуєш
    UI_SCR_EQ,     // еквалайзер від натискань
    UI_SCR_PC,     // годинник, CPU/RAM, музика (з corne-hud)
    UI_SCR_IMAGE,  // своя картинка або потік кадрів (з corne-hud)
    UI_SCR_COUNT,
};

enum { UI_ICON_NONE, UI_ICON_PLAY, UI_ICON_PAUSE };

// Стан, спільний для обох половин: головна половина його змінює і по шматках
// пересилає другій. Тут лише те, чого друга половина не знає сама.
typedef struct __attribute__((packed)) {
    uint8_t magic;
    uint8_t main_screen; // режим екрана на половині з USB
    uint8_t side_screen; // режим на другій половині
    uint8_t screens_off;
    uint8_t quiet;   // режим змінено з ПК без підказки з назвою (тимчасовий показ)
    uint8_t caps;    // Caps Lock або Caps Word
    uint8_t pc_link; // corne-hud на зв'язку
    uint8_t hour, minute;
    uint8_t cpu, ram; // відсотки
    uint8_t icon;     // UI_ICON_*
    char    text[UI_TEXT_LEN]; // нижній рядок: трек або дата, ASCII
    uint8_t has_image; // картинка для другої половини (для головної вона лежить локально)
    uint8_t image[UI_FB_SIZE];
} ui_shared_t;

typedef struct {
    uint8_t            layer; // найвищий активний шар, 0..3
    uint8_t            mods;  // UI_MOD_*
    uint8_t            wpm;
    uint8_t            shots; // скільки разів змінилась матриця з минулого кадру
    uint32_t           now;   // мс
    const ui_shared_t *shared;
    const uint8_t     *image; // картинка цієї половини або NULL
} ui_input_t;

// Малює один кадр вибраного режиму. Викликати ~20 разів на секунду.
void ui_render(uint8_t *fb, uint8_t screen, const ui_input_t *in);

// Назва режиму для короткої підказки після перемикання.
const char *ui_screen_name(uint8_t screen);
