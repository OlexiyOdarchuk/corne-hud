VIA_ENABLE          = yes
VIAL_ENABLE         = yes
LTO_ENABLE          = yes

RGBLIGHT_ENABLE     = no
RGB_MATRIX_ENABLE   = yes
VIALRGB_ENABLE      = yes
MOUSEKEY_ENABLE     = yes
OLED_ENABLE         = yes
OLED_DRIVER         = ssd1306
EXTRAKEY_ENABLE     = yes
COMBO_ENABLE        = yes
TAP_DANCE_ENABLE    = yes
KEY_OVERRIDE_ENABLE = yes

QMK_SETTINGS        = yes

CAPS_WORD_ENABLE    = yes

WPM_ENABLE          = yes
SRC                += oled_ui.c screens.c
