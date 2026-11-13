// Екрани: див. screens.h. Протокол із ПК описано в corne-hud/PROTOCOL.md.
#include "screens.h"

#include <string.h>

#include "oled_ui.h"
#include "raw_hid.h"
#include "split_util.h"
#include "transactions.h"
#include "via.h"

#define SETTINGS_MAGIC 0xC5

#define HUD_CMD 0x70
#define HUD_PROTO 2
#define HUD_LINK_TIMEOUT 5000
#define HUD_IMAGE_CHUNK 28

enum {
    HUD_HELLO      = 0x01, // -> [2] версія, [3] режим USB-половини, [4] режим другої, [5] кількість режимів, [6] екрани вимкнені
    HUD_INFO       = 0x02, // [2] година, [3] хвилина, [4] CPU %, [5] RAM %, [6] значок
    HUD_TEXT       = 0x03, // [2] зсув, [3] довжина (<= 28), [4..] символи
    HUD_IMAGE      = 0x04, // [2] маска екранів, [3] номер шматка 0..18, [4..31] 28 байт кадру
    HUD_COMMIT     = 0x05, // [2] маска: показати зібраний кадр
    HUD_SET_SCREEN = 0x06, // [2] маска, [3] режим, [4] 1 = запам'ятати, [5] 1 = без підказки
    HUD_CLEAR      = 0x07, // [2] маска: прибрати картинку
    HUD_SET_OFF    = 0x08, // [2] 1 = вимкнути екрани, [3] 1 = запам'ятати
};

enum { TARGET_MAIN = 1, TARGET_SIDE = 2 };

// Стан, який головна половина розсилає. На другій половині — отримана копія.
static ui_shared_t shared;

// Тільки на головній половині.
static uint8_t  slave_copy[sizeof(ui_shared_t)]; // що вже точно є на другій половині
static uint8_t  main_image[UI_FB_SIZE];
static bool     main_has_image;
static uint8_t  back_main[UI_FB_SIZE], back_side[UI_FB_SIZE]; // кадри, що збираються з шматків
static uint32_t host_seen;

static uint8_t oled_fb[UI_FB_SIZE];

/* ----------------------------------------------------------- налаштування */

typedef union {
    uint32_t raw;
    struct {
        uint8_t magic, main_screen, side_screen, screens_off;
    };
} settings_t;

static void settings_save(void) {
    settings_t s = {.magic = SETTINGS_MAGIC, .main_screen = shared.main_screen, .side_screen = shared.side_screen, .screens_off = shared.screens_off};
    eeconfig_update_user(s.raw);
}

static void settings_defaults(void) {
    shared.main_screen = UI_SCR_STATUS;
    shared.side_screen = UI_SCR_SHIP;
    shared.screens_off = 0;
}

void eeconfig_init_user(void) {
    settings_defaults();
    settings_save();
}

static void settings_load(void) {
    settings_t s = {.raw = eeconfig_read_user()};
    if (s.magic != SETTINGS_MAGIC || s.main_screen >= UI_SCR_COUNT || s.side_screen >= UI_SCR_COUNT) {
        settings_defaults();
        return;
    }
    shared.main_screen = s.main_screen;
    shared.side_screen = s.side_screen;
    shared.screens_off = s.screens_off ? 1 : 0;
}

/* ----------------------------------------------- синхронізація між половинами */

#define SYNC_CHUNK 60
#define SYNC_CHUNKS ((sizeof(ui_shared_t) + SYNC_CHUNK - 1) / SYNC_CHUNK)

static void sync_receive(uint8_t in_len, const void *in_data, uint8_t out_len, void *out_data) {
    const uint8_t *d   = in_data;
    uint16_t       off = d[0] * SYNC_CHUNK;
    if (in_len < 1 || off >= sizeof(shared)) return;
    uint16_t len = MIN(SYNC_CHUNK, sizeof(shared) - off);
    if (in_len < len + 1) return;
    memcpy((uint8_t *)&shared + off, d + 1, len);
}

static void sync_send(uint8_t chunk) {
    uint16_t off = chunk * SYNC_CHUNK;
    uint16_t len = MIN(SYNC_CHUNK, sizeof(shared) - off);
    uint8_t  buf[SYNC_CHUNK + 1];
    buf[0] = chunk;
    memcpy(buf + 1, (uint8_t *)&shared + off, len);
    if (transaction_rpc_send(USER_SYNC, len + 1, buf)) {
        memcpy(slave_copy + off, (uint8_t *)&shared + off, len);
    }
}

// Раз на 2 мс шле один змінений шматок; якщо змін немає — потроху все підряд,
// щоб друга половина після перепідключення отримала повний стан.
static void sync_task(void) {
    static uint32_t last, last_refresh;
    static uint8_t  next;
    if (!is_transport_connected() || timer_elapsed32(last) < 2) return;
    last = timer_read32();

    for (uint8_t i = 0; i < SYNC_CHUNKS; i++) {
        uint8_t  c   = (next + i) % SYNC_CHUNKS;
        uint16_t off = c * SYNC_CHUNK;
        if (memcmp((uint8_t *)&shared + off, slave_copy + off, MIN(SYNC_CHUNK, sizeof(shared) - off)) != 0) {
            sync_send(c);
            next = (c + 1) % SYNC_CHUNKS;
            return;
        }
    }
    if (timer_elapsed32(last_refresh) >= 100) {
        last_refresh = timer_read32();
        sync_send(next);
        next = (next + 1) % SYNC_CHUNKS;
    }
}

void keyboard_post_init_user(void) {
    shared.magic = SETTINGS_MAGIC;
    settings_load();
    transaction_register_rpc(USER_SYNC, sync_receive);
}

void housekeeping_task_user(void) {
    if (!is_keyboard_master()) return;
    shared.caps = host_keyboard_led_state().caps_lock || is_caps_word_on();
    if (shared.pc_link && timer_elapsed32(host_seen) > HUD_LINK_TIMEOUT) shared.pc_link = 0;
    sync_task();
}

/* ------------------------------------------------------------ клавіші */

static uint8_t cycle(uint8_t screen, bool backwards) {
    return backwards ? (screen + UI_SCR_COUNT - 1) % UI_SCR_COUNT : (screen + 1) % UI_SCR_COUNT;
}

bool process_record_screens(uint16_t keycode, keyrecord_t *record) {
    if (keycode != SCR_MAIN && keycode != SCR_SIDE && keycode != SCR_OFF) return true;
    if (record->event.pressed) {
        bool backwards = (get_mods() | get_oneshot_mods()) & MOD_MASK_SHIFT;
        shared.quiet = 0;
        if (keycode == SCR_MAIN) shared.main_screen = cycle(shared.main_screen, backwards);
        if (keycode == SCR_SIDE) shared.side_screen = cycle(shared.side_screen, backwards);
        if (keycode == SCR_OFF) shared.screens_off = !shared.screens_off;
        settings_save();
    }
    return false;
}

/* ------------------------------------------------------ зв'язок із corne-hud */

void raw_hid_receive_kb(uint8_t *data, uint8_t length) {
    if (length < 32 || data[0] != HUD_CMD) {
        data[0] = id_unhandled;
        return;
    }
    uint8_t mask = data[2];
    switch (data[1]) {
        case HUD_HELLO:
            data[2] = HUD_PROTO;
            data[3] = shared.main_screen;
            data[4] = shared.side_screen;
            data[5] = UI_SCR_COUNT;
            data[6] = shared.screens_off;
            break;
        case HUD_INFO:
            shared.hour    = data[2];
            shared.minute  = data[3];
            shared.cpu     = data[4];
            shared.ram     = data[5];
            shared.icon    = data[6];
            shared.pc_link = 1;
            host_seen      = timer_read32();
            break;
        case HUD_TEXT: {
            uint8_t off = data[2], n = MIN(data[3], 28);
            if (off + n <= UI_TEXT_LEN) memcpy(shared.text + off, data + 4, n);
            break;
        }
        case HUD_IMAGE: {
            uint16_t off = data[3] * HUD_IMAGE_CHUNK;
            if (off >= UI_FB_SIZE) break;
            uint16_t n = MIN(HUD_IMAGE_CHUNK, UI_FB_SIZE - off);
            if (mask & TARGET_MAIN) memcpy(back_main + off, data + 4, n);
            if (mask & TARGET_SIDE) memcpy(back_side + off, data + 4, n);
            break;
        }
        case HUD_COMMIT:
            if (mask & TARGET_MAIN) memcpy(main_image, back_main, UI_FB_SIZE), main_has_image = true;
            if (mask & TARGET_SIDE) memcpy(shared.image, back_side, UI_FB_SIZE), shared.has_image = 1;
            break;
        case HUD_SET_SCREEN:
            if (data[3] >= UI_SCR_COUNT) break;
            shared.quiet = data[5] ? 1 : 0; // [5] 1 = без підказки з назвою режиму
            if (mask & TARGET_MAIN) shared.main_screen = data[3];
            if (mask & TARGET_SIDE) shared.side_screen = data[3];
            if (data[4]) settings_save();
            break;
        case HUD_CLEAR:
            if (mask & TARGET_MAIN) main_has_image = false;
            if (mask & TARGET_SIDE) shared.has_image = 0;
            break;
        case HUD_SET_OFF:
            shared.screens_off = data[2] ? 1 : 0;
            if (data[3]) settings_save();
            break;
        default:
            data[0] = id_unhandled;
            break;
    }
}

/* ---------------------------------------------------------------- малювання */

oled_rotation_t oled_init_user(oled_rotation_t rotation) {
    return is_keyboard_master() ? rotation : OLED_ROTATION_180;
}

bool oled_task_user(void) {
    // Будь-яка зміна буфера вмикає екран, тож поки він вимкнений (таймаут), нічого не малюємо.
    if (!is_oled_on()) return false;

    // На другій половині мітка активності приходить з головної (SPLIT_ACTIVITY_ENABLE):
    // кожна її зміна — натискання чи відпускання.
    static uint32_t last_activity;
    uint32_t        activity = last_matrix_activity_time();
    bool            master   = is_keyboard_master();
    uint8_t         mods     = get_mods() | get_oneshot_mods();

    ui_input_t in = {
        .layer  = get_highest_layer(layer_state),
        .mods   = ((mods & MOD_MASK_CTRL) ? UI_MOD_CTRL : 0) | ((mods & MOD_MASK_SHIFT) ? UI_MOD_SHIFT : 0) | ((mods & MOD_MASK_ALT) ? UI_MOD_ALT : 0) | ((mods & MOD_MASK_GUI) ? UI_MOD_GUI : 0),
        .wpm    = get_current_wpm(),
        .shots  = activity != last_activity,
        .now    = timer_read32(),
        .shared = &shared,
        .image  = master ? (main_has_image ? main_image : NULL) : (shared.has_image ? shared.image : NULL),
    };
    last_activity = activity;

    ui_render(oled_fb, master ? shared.main_screen : shared.side_screen, &in);
    for (uint16_t i = 0; i < UI_FB_SIZE; i++) {
        oled_write_raw_byte(oled_fb[i], i);
    }
    return false;
}
