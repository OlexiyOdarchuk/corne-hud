// Графіка для OLED 128x32 (див. oled_ui.h).
// Кадр: 512 байт, байт = стовпчик із 8 пікселів, біт 0 — верхній. Індекс: x + (y / 8) * 128.
#include "oled_ui.h"

#include <string.h>

#include "progmem.h"

// Шрифт 6x8 із keyboards/crkbd/lib/glcdfont.c (той самий, що й у драйвера OLED).
extern const unsigned char font[];

/* ------------------------------------------------------------------ малювання */

static uint8_t *g_fb;

static void px(int x, int y, bool on) {
    if ((unsigned)x >= UI_W || (unsigned)y >= UI_H) return;
    uint8_t *b = &g_fb[x + (y >> 3) * UI_W];
    uint8_t  m = 1 << (y & 7);
    if (on)
        *b |= m;
    else
        *b &= ~m;
}

static void fill_rect(int x, int y, int w, int h, bool on) {
    for (int j = y; j < y + h; j++)
        for (int i = x; i < x + w; i++) px(i, j, on);
}

static void hline(int x0, int x1, int y, bool on) {
    for (int x = x0; x <= x1; x++) px(x, y, on);
}

static void draw_char(int x, int y, char c, bool on) {
    for (int col = 0; col < 6; col++) {
        uint8_t bits = pgm_read_byte(&font[(uint8_t)c * 6 + col]);
        for (int row = 0; row < 8; row++)
            if (bits & (1 << row)) px(x + col, y + row, on);
    }
}

static void draw_text(int x, int y, const char *s, bool on) {
    for (; *s; s++, x += 6) draw_char(x, y, *s, on);
}

static int text_width(const char *s) {
    return (int)strlen(s) * 6 - 1;
}

// Жирні цифри 6x10, рядок — 6 біт (старший — лівий піксель).
static const uint8_t digits6x10[10][10] = {
    {0x1E, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x1E},
    {0x0C, 0x1C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x1E},
    {0x1E, 0x33, 0x03, 0x03, 0x06, 0x0C, 0x18, 0x30, 0x30, 0x3F},
    {0x1E, 0x33, 0x03, 0x03, 0x0E, 0x03, 0x03, 0x03, 0x33, 0x1E},
    {0x06, 0x0E, 0x1E, 0x36, 0x36, 0x3F, 0x06, 0x06, 0x06, 0x06},
    {0x3F, 0x30, 0x30, 0x3E, 0x03, 0x03, 0x03, 0x03, 0x33, 0x1E},
    {0x1E, 0x30, 0x30, 0x3E, 0x33, 0x33, 0x33, 0x33, 0x33, 0x1E},
    {0x3F, 0x03, 0x03, 0x06, 0x06, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C},
    {0x1E, 0x33, 0x33, 0x33, 0x1E, 0x33, 0x33, 0x33, 0x33, 0x1E},
    {0x1E, 0x33, 0x33, 0x33, 0x33, 0x1F, 0x03, 0x03, 0x03, 0x1E},
};

static void draw_big_digit_scaled(int x, int y, uint8_t d, int scale, bool on) {
    for (int row = 0; row < 10; row++)
        for (int col = 0; col < 6; col++)
            if (digits6x10[d][row] & (0x20 >> col)) fill_rect(x + col * scale, y + row * scale, scale, scale, on);
}

static void draw_big_digit(int x, int y, uint8_t d, bool on) {
    draw_big_digit_scaled(x, y, d, 1, on);
}

// Картинка 16xN, рядок — 16 біт, старший біт — лівий піксель.
static void draw_bitmap16(int x, int y, const uint16_t *rows, int h, bool on) {
    for (int row = 0; row < h; row++)
        for (int col = 0; col < 16; col++)
            if (rows[row] & (0x8000 >> col)) px(x + col, y + row, on);
}

static void fill_circle(int cx, int cy, int r, bool on) {
    for (int dy = -r; dy <= r; dy++)
        for (int dx = -r; dx <= r; dx++)
            if (dx * dx + dy * dy <= r * r + r) px(cx + dx, cy + dy, on);
}

/* ---------------------------------------------------------- екран статусу */

static const uint16_t icon_base[16] = {
    0x0000, 0x0180, 0x03C0, 0x0660, 0x0C30, 0x1818, 0x300C, 0x7FFE,
    0x2004, 0x2004, 0x23C4, 0x2244, 0x2244, 0x2244, 0x3FFC, 0x0000,
};
static const uint16_t icon_lower[16] = {
    0x03C0, 0x03C0, 0x03C0, 0x03C0, 0x03C0, 0x03C0, 0x03C0, 0x3FFC,
    0x1FF8, 0x0FF0, 0x07E0, 0x03C0, 0x0180, 0x0000, 0x3FFC, 0x0000,
};
static const uint16_t icon_raise[16] = {
    0x0000, 0x3FFC, 0x0000, 0x0180, 0x03C0, 0x07E0, 0x0FF0, 0x1FF8,
    0x3FFC, 0x03C0, 0x03C0, 0x03C0, 0x03C0, 0x03C0, 0x03C0, 0x03C0,
};
static const uint16_t icon_adjust[16] = {
    0x0000, 0x0180, 0x1188, 0x0FF0, 0x1FF8, 0x1C38, 0x3818, 0x781E,
    0x781E, 0x3818, 0x1C38, 0x1FF8, 0x0FF0, 0x1188, 0x0180, 0x0000,
};

static const char *const layer_names[] = {"BASE", "LOWER", "RAISE", "ADJUST"};
static const uint16_t *const layer_icons[] = {icon_base, icon_lower, icon_raise, icon_adjust};
static const char *const mod_names[] = {"CTL", "SFT", "ALT", "GUI"};

#define GRAPH_X 46
#define GRAPH_W (UI_W - GRAPH_X)
#define GRAPH_TOP 12
#define GRAPH_BOTTOM 22
#define GRAPH_SAMPLE_MS 500

static uint8_t  wpm_hist[GRAPH_W];
static uint8_t  hist_pos;
static uint32_t hist_last;

// Історію WPM пишемо завжди, навіть коли на екрані інший режим.
static void sample_wpm(const ui_input_t *in) {
    if (in->now - hist_last >= GRAPH_SAMPLE_MS) {
        hist_last          = in->now;
        wpm_hist[hist_pos] = in->wpm;
        hist_pos           = (hist_pos + 1) % GRAPH_W;
    }
}

static void render_status(const ui_input_t *in) {

    // Картка шару: світла плашка з назвою та значком.
    uint8_t layer = in->layer < 4 ? in->layer : 3;
    fill_rect(0, 0, 42, 32, true);
    px(0, 0, false), px(41, 0, false), px(0, 31, false), px(41, 31, false);
    const char *name = layer_names[layer];
    draw_text((42 - text_width(name)) / 2, 2, name, false);
    draw_bitmap16(13, 13, layer_icons[layer], 16, false);

    // WPM великими цифрами.
    uint8_t w = in->wpm;
    draw_big_digit(GRAPH_X, 0, w / 100, true);
    draw_big_digit(GRAPH_X + 8, 0, w / 10 % 10, true);
    draw_big_digit(GRAPH_X + 16, 0, w % 10, true);
    draw_text(GRAPH_X + 26, 3, "WPM", true);

    if (in->shared->caps) {
        fill_rect(101, 0, 27, 10, true);
        draw_text(103, 1, "CAPS", false);
    }

    // Графік WPM за останні ~40 с, масштаб підлаштовується під максимум.
    int scale = 40;
    for (int i = 0; i < GRAPH_W; i++)
        if (wpm_hist[i] > scale) scale = wpm_hist[i];
    const int gh = GRAPH_BOTTOM - GRAPH_TOP + 1;
    for (int i = 0; i < GRAPH_W; i++) {
        uint8_t v = wpm_hist[(hist_pos + i) % GRAPH_W];
        int     h = (v * gh + scale - 1) / scale;
        int     x = GRAPH_X + i;
        if (h == 0) {
            px(x, GRAPH_BOTTOM, i % 2 == 0);
        } else {
            for (int y = GRAPH_BOTTOM - h + 1; y <= GRAPH_BOTTOM; y++) px(x, y, true);
        }
    }

    // Модифікатори: затиснутий — інвертована плашка.
    for (int i = 0; i < 4; i++) {
        int  bx     = GRAPH_X + i * 21;
        bool active = in->mods & (1 << i);
        if (active) fill_rect(bx, 24, 19, 8, true);
        draw_text(bx + 1, 24, mod_names[i], !active);
    }
}

/* ---------------------------------------------------- корабель і астероїди */

#define N_STARS 30
#define N_ROCKS 5
#define N_BOLTS 8
#define N_BOOMS 6

#define SHIP_X 5
#define SHIP_W 16
#define SHIP_H 9

typedef struct {
    uint16_t x; // 1/16 пікселя
    uint8_t  y, depth;
} star_t;

typedef struct {
    int16_t x; // 1/16 пікселя
    int8_t  y;
    uint8_t r, speed, spin;
    bool    alive;
} rock_t;

typedef struct {
    int16_t x;
    int8_t  y;
    bool    alive;
} bolt_t;

typedef struct {
    int16_t x;
    int8_t  y;
    uint8_t age;
    bool    alive;
} boom_t;

static const uint16_t ship_rows[SHIP_H] = {
    0xE000, 0x7C00, 0x3F80, 0x7F30, 0xFF9E, 0x7FF0, 0x3F80, 0x7C00, 0xE000,
};

static bool ship_ink(int row, int col) {
    return row >= 0 && row < SHIP_H && col >= 0 && col < SHIP_W && (ship_rows[row] & (0x8000 >> col));
}

static star_t   stars[N_STARS];
static rock_t   rocks[N_ROCKS];
static bolt_t   bolts[N_BOLTS];
static boom_t   booms[N_BOOMS];
static bool     scene_ready;
static uint16_t rng = 0xACE1;
static uint32_t frame;
static uint8_t  warp;         // згладжений WPM
static int16_t  ship_cy = 128; // центр корабля, 1/8 пікселя
static bool     gun_alt;
static uint8_t  shield;
static uint16_t score;
static uint16_t idle_frames;

static uint16_t rnd(void) {
    rng ^= rng << 7;
    rng ^= rng >> 9;
    rng ^= rng << 8;
    return rng;
}

static void spawn_boom(int x, int y) {
    for (int i = 0; i < N_BOOMS; i++)
        if (!booms[i].alive) {
            booms[i] = (boom_t){.x = x, .y = y, .age = 0, .alive = true};
            return;
        }
}

static void spawn_rock(void) {
    for (int i = 0; i < N_ROCKS; i++)
        if (!rocks[i].alive) {
            uint8_t r = 2 + rnd() % 3;
            rocks[i]  = (rock_t){
                 .x     = (UI_W + r) * 16,
                 .y     = r + 1 + rnd() % (UI_H - 2 * r - 2),
                 .r     = r,
                 .speed = 8 + rnd() % 12 + warp / 4,
                 .spin  = rnd(),
                 .alive = true,
            };
            return;
        }
}

static void scene_step(uint8_t wpm, uint8_t shots) {
    if (!scene_ready) {
        scene_ready = true;
        for (int i = 0; i < N_STARS; i++)
            stars[i] = (star_t){.x = (rnd() % UI_W) * 16, .y = rnd() % UI_H, .depth = 1 + i % 3};
    }
    frame++;

    uint8_t target = wpm > 150 ? 150 : wpm;
    if (warp < target)
        warp += (target - warp + 3) / 4;
    else if (warp > target)
        warp -= (warp - target + 7) / 8;

    idle_frames = shots ? 0 : (idle_frames < 0xFFFF ? idle_frames + 1 : idle_frames);
    bool typing = idle_frames < 40; // ~2 с після останнього натискання

    // Зорі: що ближче, то швидше; на швидкості тягнуться в смуги.
    int base = 3 + warp / 3;
    for (int i = 0; i < N_STARS; i++) {
        star_t *s = &stars[i];
        int     v = base * s->depth;
        if (s->x < v) {
            s->x = (UI_W + rnd() % 16) * 16;
            s->y = rnd() % UI_H;
        } else {
            s->x -= v;
        }
    }

    // Астероїди з'являються, лише поки друкуєш.
    if (typing) {
        int alive = 0;
        for (int i = 0; i < N_ROCKS; i++) alive += rocks[i].alive;
        int chance = alive ? 3 + warp / 8 : 25;
        if (rnd() % 100 < chance) spawn_rock();
    }
    for (int i = 0; i < N_ROCKS; i++) {
        rock_t *r = &rocks[i];
        if (!r->alive) continue;
        r->x -= r->speed;
        if (r->x < -(r->r + 1) * 16) r->alive = false;
    }

    // Корабель тримає курс на найближчий астероїд попереду, інакше плавно гойдається.
    int target_cy = 16 + ((frame / 6) % 16 < 8 ? (frame / 6) % 8 : 8 - (frame / 6) % 8) - 4;
    int best      = 0x7FFF;
    for (int i = 0; i < N_ROCKS; i++) {
        rock_t *r = &rocks[i];
        if (r->alive && r->x / 16 > SHIP_X + SHIP_W && r->x < best) {
            best      = r->x;
            target_cy = r->y;
        }
    }
    if (target_cy < 4) target_cy = 4;
    if (target_cy > UI_H - 5) target_cy = UI_H - 5;
    int diff = target_cy * 8 - ship_cy;
    if (diff > 6) diff = 6;
    if (diff < -6) diff = -6;
    ship_cy += diff;
    int cy = (ship_cy + 4) / 8;

    // Кожна зміна матриці — постріл, гармати по черзі.
    for (uint8_t n = 0; n < shots && n < 2; n++) {
        for (int i = 0; i < N_BOLTS; i++)
            if (!bolts[i].alive) {
                bolts[i] = (bolt_t){.x = SHIP_X + 10 + n * 3, .y = cy + (gun_alt ? -2 : 2), .alive = true};
                gun_alt  = !gun_alt;
                break;
            }
    }
    for (int i = 0; i < N_BOLTS; i++) {
        bolt_t *b = &bolts[i];
        if (!b->alive) continue;
        b->x += 7;
        if (b->x >= UI_W) {
            b->alive = false;
            continue;
        }
        for (int j = 0; j < N_ROCKS; j++) {
            rock_t *r  = &rocks[j];
            int     rx = r->x / 16;
            if (r->alive && b->x + 4 >= rx - r->r && b->x - 7 <= rx + r->r && b->y >= r->y - r->r && b->y <= r->y + r->r) {
                r->alive = b->alive = false;
                spawn_boom(rx, r->y);
                if (score < 9999) score++;
                break;
            }
        }
    }

    // Астероїд, що долетів до корабля, розбивається об щит.
    for (int i = 0; i < N_ROCKS; i++) {
        rock_t *r  = &rocks[i];
        int     rx = r->x / 16;
        if (r->alive && rx - r->r <= SHIP_X + SHIP_W - 2 && rx + r->r >= SHIP_X && r->y + r->r >= cy - 4 && r->y - r->r <= cy + 4) {
            r->alive = false;
            spawn_boom(rx, r->y);
            shield = 8;
        }
    }
    if (shield) shield--;

    for (int i = 0; i < N_BOOMS; i++) {
        boom_t *b = &booms[i];
        if (!b->alive) continue;
        b->x--;
        if (++b->age > 7) b->alive = false;
    }
}

static void draw_rock(const rock_t *r) {
    int x = r->x / 16, y = r->y, rad = r->r;
    fill_circle(x, y, rad, true);
    if (rad >= 3) {
        static const int8_t cdx[4] = {-1, 1, 1, -1}, cdy[4] = {-1, -1, 1, 1};
        uint8_t             k = (r->spin + frame / 3) % 4;
        px(x + cdx[k], y + cdy[k], false);
        px(x + cdx[(k + 2) % 4] * (rad - 1), y + cdy[(k + 2) % 4], false);
    }
}

static void draw_boom(const boom_t *b) {
    static const int8_t dx[8] = {2, 1, 0, -1, -2, -1, 0, 1}, dy[8] = {0, 1, 2, 1, 0, -1, -2, -1};
    if (b->age == 0) {
        fill_circle(b->x, b->y, 2, true);
    } else if (b->age == 1) {
        fill_circle(b->x, b->y, 3, true);
        fill_circle(b->x, b->y, 1, false);
    } else {
        int d = b->age + 1;
        for (int k = 0; k < 8; k++)
            if (b->age < 5 || (k + b->age) % 2) px(b->x + dx[k] * d / 2, b->y + dy[k] * d / 2, true);
    }
}

static void render_ship(const ui_input_t *in) {
    scene_step(in->wpm, in->shots);

    int base = 3 + warp / 3;
    for (int i = 0; i < N_STARS; i++) {
        const star_t *s   = &stars[i];
        int           x   = s->x / 16;
        int           len = base * s->depth / 16;
        if (len > 10) len = 10;
        hline(x, x + (len > 1 ? len - 1 : 0), s->y, true);
    }

    for (int i = 0; i < N_ROCKS; i++)
        if (rocks[i].alive) draw_rock(&rocks[i]);

    for (int i = 0; i < N_BOLTS; i++)
        if (bolts[i].alive) hline(bolts[i].x, bolts[i].x + 4, bolts[i].y, true);

    for (int i = 0; i < N_BOOMS; i++)
        if (booms[i].alive) draw_boom(&booms[i]);

    // Корабель на чорній підкладці, щоб зорі не пролітали крізь нього.
    int cy  = (ship_cy + 4) / 8;
    int top = cy - SHIP_H / 2;
    for (int row = -1; row <= SHIP_H; row++)
        for (int col = -1; col <= SHIP_W; col++) {
            bool near = false;
            for (int dr = -1; dr <= 1; dr++)
                for (int dc = -1; dc <= 1; dc++) near |= ship_ink(row + dr, col + dc);
            if (near) px(SHIP_X + col, top + row, ship_ink(row, col));
        }

    // Вогонь двигуна: довший на швидкості, мерехтить.
    int flame = 1 + (frame & 1) + warp / 30;
    if (flame > SHIP_X) flame = SHIP_X;
    hline(SHIP_X - flame, SHIP_X - 1, cy, true);
    if (flame > 2) {
        hline(SHIP_X - flame + 2, SHIP_X - 1, cy - 1, (frame & 2) != 0);
        hline(SHIP_X - flame + 2, SHIP_X - 1, cy + 1, (frame & 2) == 0);
    }

    // Щит після зіткнення: дуга перед носом.
    if (shield && (shield & 1)) {
        for (int dy = -6; dy <= 6; dy++) {
            int dx = 6 - (dy * dy) / 7;
            px(SHIP_X + SHIP_W + dx - 2, cy + dy, true);
        }
    }

    // Рахунок збитих астероїдів у правому верхньому куті.
    if (score) {
        char buf[6];
        int  n = 0;
        for (uint16_t v = score; v; v /= 10) buf[n++] = '0' + v % 10;
        int w = n * 6;
        fill_rect(UI_W - w - 1, 0, w + 1, 8, false);
        for (int i = 0; i < n; i++) draw_char(UI_W - w + i * 6, 0, buf[n - 1 - i], true);
    }
}

/* ------------------------------------------------------------- вихованець */

#define PET_X 52

static uint16_t pet_idle = 0xFFFF; // кадрів без натискань
static uint16_t pet_phase;         // фаза кроку, 1/8 кадру анімації
static uint16_t pet_scroll;        // зсув землі, 1/16 пікселя
static uint8_t  pet_hop;           // підскок від натискання, коли стоїть
static uint32_t pet_frame;

static void fill_ellipse(int cx, int cy, int rx, int ry, bool on) {
    for (int dy = -ry; dy <= ry; dy++)
        for (int dx = -rx; dx <= rx; dx++)
            if (dx * dx * ry * ry + dy * dy * rx * rx <= rx * rx * ry * ry + rx * ry) px(cx + dx, cy + dy, on);
}

static void render_pet(const ui_input_t *in) {
    static const uint8_t hop_tab[8] = {0, 1, 2, 3, 3, 2, 1, 0};

    pet_frame++;
    pet_idle      = in->shots ? 0 : (pet_idle < 0xFFFF ? pet_idle + 1 : pet_idle);
    bool running  = pet_idle < 40;      // ~2 с після натискання
    bool sleeping = pet_idle > 20 * 20; // ~20 с тиші
    if (running) {
        pet_scroll += 8 + in->wpm / 2;
        pet_phase += 3 + in->wpm / 15;
    } else if (in->shots) {
        pet_hop = 3;
    }
    int scroll = pet_scroll / 16;
    int step   = (pet_phase / 8) % 8;

    // Хмари повільніше за землю — глибина.
    for (int k = 0; k < 3; k++) {
        int span = UI_W + 24;
        int x    = ((k * 53 + 20 - scroll / 4) % span + span) % span - 12;
        int y    = 1 + k;
        hline(x + 3, x + 6, y, true);
        hline(x + 1, x + 9, y + 1, true);
        hline(x, x + 11, y + 2, true);
    }

    // Земля: лінія з вибоїнами, камінці, травинки.
    for (int x = 0; x < UI_W; x++) {
        int wx = x + scroll;
        px(x, 30, wx % 13 != 0);
        if (wx % 7 == 0) px(x, 31, true);
        if (wx % 29 == 0) px(x, 29, true), px(x + 1, 28, true);
    }

    int hop = running ? hop_tab[step] : 0;
    if (pet_hop) hop += pet_hop--;
    int rx = 7, ry = 6;
    if (running && hop == 0) rx = 8, ry = 5; // приземлення — сплющується
    if (sleeping) ry = 5 + ((pet_frame / 20) & 1); // дихає
    int bottom = sleeping ? 29 : 27 - hop;
    int cx = PET_X, cy = bottom - ry;

    // Смуги швидкості.
    if (running && in->wpm > 40) {
        int len = in->wpm / 20;
        for (int k = 0; k < 3; k++) {
            int x1 = cx - rx - 3 - (int)((pet_frame + k * 2) % 3);
            hline(x1 - len, x1, cy - 2 + k * 3, true);
        }
    }

    // Лапки.
    if (!sleeping) {
        int sw = running ? (step < 4 ? 1 : -1) : 0;
        fill_rect(cx - 4 + sw, bottom + 1, 2, 2, true);
        fill_rect(cx + 2 - sw, bottom + 1, 2, 2, true);
    }

    fill_ellipse(cx, cy, rx, ry, true);

    // Антена, що гойдається на бігу.
    int sway = running ? (step < 4 ? -1 : 1) : 0;
    px(cx - 1, cy - ry - 1, true);
    px(cx - 1, cy - ry - 2, true);
    px(cx - 1 + sway, cy - ry - 3, true);
    fill_rect(cx - 2 + sway, cy - ry - 5, 2, 2, true);

    // Очі: кліпає, озирається, спить.
    int look = (!running && !sleeping && (pet_frame / 40) % 3 == 1) ? -3 : 0;
    if (sleeping) {
        hline(cx + 1, cx + 2, cy - 1, false);
        hline(cx + 4, cx + 5, cy - 1, false);
    } else if (pet_frame % 70 < 3) {
        hline(cx + 1 + look, cx + 2 + look, cy - 1, false);
        hline(cx + 4 + look, cx + 5 + look, cy - 1, false);
    } else {
        fill_rect(cx + 1 + look, cy - 2, 2, 2, false);
        fill_rect(cx + 4 + look, cy - 2, 2, 2, false);
    }
    if (running && in->wpm > 60) fill_rect(cx + 3, cy + 1, 2, 2, false); // захекався

    if (sleeping) {
        for (int k = 0; k < 3; k++) {
            int ph = (int)((pet_frame / 4 + k * 12) % 36);
            int x = cx + 7 + ph / 4, y = cy - ry - 1 - ph / 3;
            if (y < 0) continue;
            hline(x, x + 2, y, true);
            px(x + 1, y + 1, true);
            hline(x, x + 2, y + 2, true);
        }
    }
}

/* -------------------------------------------------------------- еквалайзер */

#define EQ_BARS 21

static uint16_t eq_h[EQ_BARS]; // висота, 1/8 пікселя
static uint8_t  eq_peak[EQ_BARS];
static uint8_t  eq_hold[EQ_BARS];
static uint32_t eq_frame;

static void eq_kick(int i, int v) {
    if (i < 0 || i >= EQ_BARS) return;
    int h   = eq_h[i] + v;
    eq_h[i] = h > 248 ? 248 : h;
}

static void render_eq(const ui_input_t *in) {
    eq_frame++;
    for (uint8_t n = 0; n < in->shots && n < 3; n++) {
        int c = rnd() % EQ_BARS;
        eq_kick(c, 140 + in->wpm);
        eq_kick(c - 1, 90), eq_kick(c + 1, 90);
        eq_kick(c - 2, 40), eq_kick(c + 2, 40);
    }
    if (in->wpm) eq_kick(rnd() % EQ_BARS, rnd() % (in->wpm + 1));

    for (int i = 0; i < EQ_BARS; i++) {
        int drop = eq_h[i] / 10 + 3;
        eq_h[i]  = eq_h[i] > drop ? eq_h[i] - drop : 0;
        int h    = eq_h[i] / 8;
        if (h >= eq_peak[i]) {
            eq_peak[i] = h;
            eq_hold[i] = 8;
        } else if (eq_hold[i]) {
            eq_hold[i]--;
        } else if (eq_peak[i] && (eq_frame & 1)) {
            eq_peak[i]--;
        }

        int x = 1 + i * 6;
        for (int k = 0; k < h; k++)
            if (k % 3 != 2) hline(x, x + 4, UI_H - 1 - k, true);
        if (eq_peak[i]) hline(x, x + 4, UI_H - 2 - eq_peak[i], true);
    }
}

/* ------------------------------------------------------- дані з комп'ютера */

static void draw_centered(int y, const char *s, bool on) {
    draw_text((UI_W - text_width(s)) / 2, y, s, on);
}

static void draw_bar(int x, int y, int w, int h, uint8_t pct) {
    if (pct > 100) pct = 100;
    hline(x, x + w - 1, y, true);
    hline(x, x + w - 1, y + h - 1, true);
    for (int j = y; j < y + h; j++) px(x, j, true), px(x + w - 1, j, true);
    fill_rect(x + 2, y + 2, (w - 4) * pct / 100, h - 4, true);
}

static void render_pc(const ui_input_t *in) {
    const ui_shared_t *sh = in->shared;
    if (!sh->pc_link) {
        draw_centered(6, "NO PC LINK", true);
        draw_centered(18, "run corne-hud", true);
        return;
    }

    // Годинник 12x20, двокрапка блимає раз на секунду.
    uint8_t h = sh->hour % 24, m = sh->minute % 60;
    draw_big_digit_scaled(0, 0, h / 10, 2, true);
    draw_big_digit_scaled(14, 0, h % 10, 2, true);
    if ((in->now / 500) % 2 == 0) {
        fill_rect(29, 5, 2, 2, true);
        fill_rect(29, 13, 2, 2, true);
    }
    draw_big_digit_scaled(33, 0, m / 10, 2, true);
    draw_big_digit_scaled(47, 0, m % 10, 2, true);

    draw_text(66, 0, "CPU", true);
    draw_bar(86, 0, 42, 8, sh->cpu);
    draw_text(66, 11, "RAM", true);
    draw_bar(86, 11, 42, 8, sh->ram);

    // Нижній рядок: трек біжить, якщо не влазить.
    char text[UI_TEXT_LEN + 1];
    memcpy(text, sh->text, UI_TEXT_LEN);
    text[UI_TEXT_LEN] = 0;
    int x0 = sh->icon ? 8 : 0;
    int tw = (int)strlen(text) * 6;
    if (tw <= UI_W - x0) {
        draw_text(x0, 24, text, true);
    } else {
        int gap = 24, off = (int)((in->now / 40) % (uint32_t)(tw + gap));
        draw_text(x0 - off, 24, text, true);
        draw_text(x0 - off + tw + gap, 24, text, true);
        fill_rect(0, 24, x0, 8, false);
    }
    if (sh->icon == UI_ICON_PLAY) {
        for (int i = 0; i < 4; i++)
            for (int y = 24 + i; y <= 30 - i; y++) px(1 + i, y, true);
    } else if (sh->icon == UI_ICON_PAUSE) {
        fill_rect(0, 24, 2, 7, true);
        fill_rect(3, 24, 2, 7, true);
    }
}

static void render_image(const ui_input_t *in) {
    if (in->image) {
        memcpy(g_fb, in->image, UI_FB_SIZE);
    } else {
        draw_centered(6, "NO IMAGE", true);
        draw_centered(18, "corne-hud image", true);
    }
}

/* ----------------------------------------------------------------- загальне */

static const char *const screen_names[UI_SCR_COUNT] = {"STATUS", "SHIP", "PET", "EQ", "PC", "IMAGE"};

const char *ui_screen_name(uint8_t screen) {
    return screen < UI_SCR_COUNT ? screen_names[screen] : "?";
}

void ui_render(uint8_t *fb, uint8_t screen, const ui_input_t *in) {
    static uint8_t  last_screen = 0xFF;
    static uint32_t toast_at;
    static bool     toast;

    g_fb = fb;
    memset(fb, 0, UI_FB_SIZE);
    sample_wpm(in);
    if (in->shared->screens_off) return;

    if (screen != last_screen) {
        toast       = last_screen != 0xFF && !in->shared->quiet;
        toast_at    = in->now;
        last_screen = screen;
    }

    switch (screen) {
        case UI_SCR_SHIP: render_ship(in); break;
        case UI_SCR_PET: render_pet(in); break;
        case UI_SCR_EQ: render_eq(in); break;
        case UI_SCR_PC: render_pc(in); break;
        case UI_SCR_IMAGE: render_image(in); break;
        default: render_status(in); break;
    }

    // Назва режиму на секунду після перемикання.
    if (toast && in->now - toast_at < 900) {
        const char *name = ui_screen_name(screen);
        int         w    = text_width(name) + 8;
        int         x    = (UI_W - w) / 2;
        fill_rect(x - 1, 10, w + 2, 13, false);
        fill_rect(x, 11, w, 11, true);
        draw_text(x + 4, 13, name, false);
    } else {
        toast = false;
    }
}
