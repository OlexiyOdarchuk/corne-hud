// Симулятор екранів: проганяє сценарій друку для кожного режиму і пише кадри в stdout
// (на кадр: 1 байт номер режиму + 512 байт буфера OLED). 20 кадрів/с.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "oled_ui.h"

#define FRAMES 240

int main(void) {
    static ui_shared_t sh;
    sh.pc_link = 1, sh.hour = 21, sh.minute = 47, sh.cpu = 37, sh.ram = 62, sh.icon = UI_ICON_PLAY;
    strcpy(sh.text, "Okean Elzy - Obijmy (live at Olimpiyskiy)");
    // Картинка-приклад: концентричні кола.
    for (int x = 0; x < UI_W; x++)
        for (int y = 0; y < UI_H; y++) {
            int dx = x - 64, dy = (y - 16) * 2;
            if ((dx * dx + dy * dy) / 90 % 2) sh.image[x + (y / 8) * UI_W] |= 1 << (y % 8);
        }
    sh.has_image = 1;

    uint8_t fb[UI_FB_SIZE];
    for (int scr = 0; scr < UI_SCR_COUNT; scr++) {
        srand(7);
        for (int f = 0; f < FRAMES; f++) {
            // Розгін, швидкий друк, тиша наприкінці.
            int wpm = f < 20 ? 0 : f < 80 ? (f - 20) * 90 / 60 : f < 160 ? 90 + (f % 37) / 4 : 90 - (f - 160) * 90 / 40;
            if (wpm < 0) wpm = 0;
            int typing = f >= 20 && f < 170;
            sh.caps    = f > 40 && f < 70;
            ui_input_t in = {
                .layer  = f < 100 ? 0 : f < 130 ? 1 : f < 150 ? 2 : 3,
                .mods   = f > 60 && f < 90 ? UI_MOD_SHIFT : 0,
                .wpm    = wpm,
                .shots  = typing && (rand() % 100 < 20 + wpm / 2) ? 1 + (rand() % 3 == 0) : 0,
                .now    = f * 50,
                .shared = &sh,
                .image  = sh.image,
            };
            ui_render(fb, scr, &in);
            fputc(scr, stdout);
            fwrite(fb, 1, UI_FB_SIZE, stdout);
        }
    }
    return 0;
}
