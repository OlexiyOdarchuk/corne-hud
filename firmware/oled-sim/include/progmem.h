// Заглушка для збірки oled_ui.c на ПК.
#pragma once
#define PROGMEM
#define pgm_read_byte(p) (*(const unsigned char *)(p))
