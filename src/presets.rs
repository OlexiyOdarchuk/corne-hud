//! Заготовки: живі анімації, які малюються на ПК і передаються на екран кадр за кадром.

use std::collections::HashMap;
use std::f32::consts::PI;
use std::sync::OnceLock;

use crate::oled::{set_px, Frame, FB, H, W};

pub trait Gen: Send {
    /// Малює кадр на момент `t` (секунди від запуску).
    fn frame(&mut self, t: f32, f: &mut Frame);
    /// Бажана частота кадрів.
    fn fps(&self) -> u32 {
        15
    }
}

pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    /// Чи використовує заготовка свій текст.
    pub uses_text: bool,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        id: "text",
        label: "Свій текст",
        uses_text: true,
    },
    Preset {
        id: "clock",
        label: "Аналоговий годинник",
        uses_text: false,
    },
    Preset {
        id: "matrix",
        label: "Матриця",
        uses_text: false,
    },
    Preset {
        id: "fire",
        label: "Вогонь",
        uses_text: false,
    },
    Preset {
        id: "plasma",
        label: "Плазма",
        uses_text: false,
    },
    Preset {
        id: "life",
        label: "Гра «Життя»",
        uses_text: false,
    },
    Preset {
        id: "snow",
        label: "Сніг",
        uses_text: false,
    },
    Preset {
        id: "warp",
        label: "Зоряний тунель",
        uses_text: false,
    },
    Preset {
        id: "tunnel",
        label: "Тунель",
        uses_text: false,
    },
    Preset {
        id: "waves",
        label: "Осцилограф",
        uses_text: false,
    },
    Preset {
        id: "flag",
        label: "Прапор",
        uses_text: false,
    },
    Preset {
        id: "heart",
        label: "Серце і кардіограма",
        uses_text: false,
    },
    Preset {
        id: "pong",
        label: "Пінг-понг",
        uses_text: false,
    },
    Preset {
        id: "maze",
        label: "Лабіринт",
        uses_text: false,
    },
    Preset {
        id: "bounce",
        label: "Напис, що відбивається",
        uses_text: true,
    },
];

pub const DEFAULT_TEXT: &str = "Привіт, Corne!";

pub fn find(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

pub fn make(id: &str, text: &str) -> Option<Box<dyn Gen>> {
    let text = if text.trim().is_empty() {
        DEFAULT_TEXT.to_string()
    } else {
        text.to_string()
    };
    Some(match id {
        "text" => Box::new(TextScroll { text }),
        "clock" => Box::new(Clock),
        "matrix" => Box::new(Matrix::new()),
        "fire" => Box::new(Fire::new()),
        "plasma" => Box::new(Plasma),
        "life" => Box::new(Life::new()),
        "snow" => Box::new(Snow::new()),
        "warp" => Box::new(Warp::new()),
        "tunnel" => Box::new(Tunnel),
        "waves" => Box::new(Waves),
        "flag" => Box::new(Flag),
        "heart" => Box::new(Heart::new()),
        "pong" => Box::new(Pong::new()),
        "maze" => Box::new(Maze::new()),
        "bounce" => Box::new(Bounce::new(text)),
        _ => return None,
    })
}

/* ------------------------------------------------------------ допоміжне */

struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Self {
        Rng(seed.max(1))
    }
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
    fn unit(&mut self) -> f32 {
        (self.next() % 10_000) as f32 / 10_000.0
    }
}

fn seed() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(7, |d| d.subsec_nanos() | 1)
}

fn clear(f: &mut Frame) {
    *f = [0; FB];
}

fn px(f: &mut Frame, x: i32, y: i32, on: bool) {
    if x >= 0 && y >= 0 && (x as usize) < W && (y as usize) < H {
        set_px(f, x as usize, y as usize, on);
    }
}

fn line(f: &mut Frame, x0: i32, y0: i32, x1: i32, y1: i32) {
    let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let (mut x, mut y, mut err) = (x0, y0, dx + dy);
    loop {
        px(f, x, y, true);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

fn rect(f: &mut Frame, x: i32, y: i32, w: i32, h: i32, fill: bool) {
    for j in y..y + h {
        for i in x..x + w {
            if fill || j == y || j == y + h - 1 || i == x || i == x + w - 1 {
                px(f, i, j, true);
            }
        }
    }
}

fn circle(f: &mut Frame, cx: i32, cy: i32, r: i32) {
    let steps = (r * 8).max(16);
    for k in 0..steps {
        let a = k as f32 / steps as f32 * 2.0 * PI;
        px(
            f,
            cx + (a.cos() * r as f32).round() as i32,
            cy + (a.sin() * r as f32).round() as i32,
            true,
        );
    }
}

const BAYER: [[f32; 4]; 4] = [
    [0.0, 8.0, 2.0, 10.0],
    [12.0, 4.0, 14.0, 6.0],
    [3.0, 11.0, 1.0, 9.0],
    [15.0, 7.0, 13.0, 5.0],
];

/// Півтон через упорядковане розсіювання: яскравість 0..1 → увімкнений піксель чи ні.
fn shade(x: usize, y: usize, v: f32) -> bool {
    v > (BAYER[y % 4][x % 4] + 0.5) / 16.0
}

// Цифри 3x5 для маленьких рахунків.
const DIGITS: [[u8; 5]; 10] = [
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [7, 1, 7, 4, 7],
    [7, 1, 7, 1, 7],
    [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7],
    [7, 4, 7, 5, 7],
    [7, 1, 2, 2, 2],
    [7, 5, 7, 5, 7],
    [7, 5, 7, 1, 7],
];

fn tiny_number(f: &mut Frame, x: i32, y: i32, n: u32) {
    let s = n.to_string();
    for (i, ch) in s.bytes().enumerate() {
        let d = (ch - b'0') as usize;
        for (row, bits) in DIGITS[d].iter().enumerate() {
            for col in 0..3 {
                if bits & (4 >> col) != 0 {
                    px(f, x + i as i32 * 4 + col, y + row as i32, true);
                }
            }
        }
    }
}

/* ------------------------------------------------------------- шрифт */

type GlyphCache = HashMap<(char, u32), (fontdue::Metrics, Vec<u8>)>;

struct Font {
    font: fontdue::Font,
    cache: std::sync::Mutex<GlyphCache>,
}

fn font() -> Option<&'static Font> {
    static FONT: OnceLock<Option<Font>> = OnceLock::new();
    FONT.get_or_init(|| {
        let mut candidates = Vec::new();
        for q in ["DejaVu Sans:style=Bold", "sans-serif:bold"] {
            if let Ok(out) = std::process::Command::new("fc-match")
                .args(["-f", "%{file}", q])
                .output()
            {
                candidates.push(String::from_utf8_lossy(&out.stdout).into_owned());
            }
        }
        candidates.push("/usr/share/fonts/TTF/DejaVuSans-Bold.ttf".into());
        candidates.push("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf".into());
        candidates.iter().find_map(|path| {
            let data = std::fs::read(path).ok()?;
            let font = fontdue::Font::from_bytes(data, fontdue::FontSettings::default()).ok()?;
            Some(Font {
                font,
                cache: Default::default(),
            })
        })
    })
    .as_ref()
}

impl Font {
    fn glyph(&self, c: char, px: f32) -> (fontdue::Metrics, Vec<u8>) {
        let key = (c, px.to_bits());
        let mut cache = self.cache.lock().unwrap();
        cache.entry(key).or_insert_with(|| self.font.rasterize(c, px)).clone()
    }

    fn width(&self, s: &str, px: f32) -> f32 {
        s.chars().map(|c| self.font.metrics(c, px).advance_width).sum()
    }

    fn ascent(&self, px: f32) -> f32 {
        self.font.horizontal_line_metrics(px).map_or(px * 0.8, |m| m.ascent)
    }

    /// Малює рядок: x — лівий край, base — базова лінія.
    fn draw(&self, f: &mut Frame, s: &str, x: f32, base: i32, px_size: f32) {
        let mut pen = x;
        for c in s.chars() {
            let (m, bmp) = self.glyph(c, px_size);
            let gx = pen.round() as i32 + m.xmin;
            let gy = base - m.height as i32 - m.ymin;
            if gx + (m.width as i32) >= 0 && gx < W as i32 {
                for j in 0..m.height {
                    for i in 0..m.width {
                        if bmp[j * m.width + i] >= 110 {
                            px(f, gx + i as i32, gy + j as i32, true);
                        }
                    }
                }
            }
            pen += m.advance_width;
        }
    }
}

/// Малює рядок так, щоб верх великих літер був на `top`. Повертає false, якщо шрифту немає.
pub fn draw_text(f: &mut Frame, s: &str, x: i32, top: i32, size: f32) -> bool {
    let Some(font) = font() else { return false };
    let base = top + (font.ascent(size) * 0.72).round() as i32;
    font.draw(f, s, x as f32, base, size);
    true
}

/* --------------------------------------------------------- заготовки */

struct TextScroll {
    text: String,
}

impl Gen for TextScroll {
    fn frame(&mut self, t: f32, f: &mut Frame) {
        clear(f);
        let Some(font) = font() else { return };
        let size = 26.0;
        let base = ((H as f32 + font.ascent(size) * 0.72) / 2.0).round() as i32;
        let w = font.width(&self.text, size);
        if w <= W as f32 - 4.0 {
            font.draw(f, &self.text, (W as f32 - w) / 2.0, base, size);
        } else {
            let gap = 40.0;
            let off = (t * 45.0) % (w + gap);
            font.draw(f, &self.text, W as f32 - off, base, size);
            font.draw(f, &self.text, W as f32 - off + w + gap, base, size);
        }
    }
    fn fps(&self) -> u32 {
        20
    }
}

struct Clock;

impl Gen for Clock {
    fn frame(&mut self, _t: f32, f: &mut Frame) {
        use chrono::{Datelike, Timelike};
        clear(f);
        let now = chrono::Local::now();
        let (cx, cy, r) = (16, 16, 15);
        circle(f, cx, cy, r);
        for k in 0..12 {
            let a = k as f32 / 12.0 * 2.0 * PI;
            let len = if k % 3 == 0 { 3.0 } else { 1.5 };
            let (s, c) = a.sin_cos();
            line(
                f,
                cx + (s * (r as f32 - 1.0)).round() as i32,
                cy - (c * (r as f32 - 1.0)).round() as i32,
                cx + (s * (r as f32 - len)).round() as i32,
                cy - (c * (r as f32 - len)).round() as i32,
            );
        }
        let secs = now.second() as f32 + now.nanosecond() as f32 / 1e9;
        let mins = now.minute() as f32 + secs / 60.0;
        let hours = (now.hour() % 12) as f32 + mins / 60.0;
        for (frac, len) in [(hours / 12.0, 7.0), (mins / 60.0, 11.0), (secs / 60.0, 13.0)] {
            let (s, c) = (frac * 2.0 * PI).sin_cos();
            line(f, cx, cy, cx + (s * len).round() as i32, cy - (c * len).round() as i32);
        }
        rect(f, cx - 1, cy - 1, 3, 3, true);

        if let Some(font) = font() {
            let time = now.format("%H:%M").to_string();
            font.draw(f, &time, 40.0, 17, 19.0);
            const DAYS: [&str; 7] = ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Нд"];
            let date = format!(
                "{} {:02}.{:02}",
                DAYS[now.weekday().num_days_from_monday() as usize],
                now.day(),
                now.month()
            );
            font.draw(f, &date, 41.0, 30, 11.0);
            // Секунди — смужка під часом.
            let w = (secs / 60.0 * 86.0) as i32;
            rect(f, 40, 20, w.max(1), 1, true);
        }
    }
    fn fps(&self) -> u32 {
        5
    }
}

struct Matrix {
    rng: Rng,
    cols: Vec<(f32, f32, u32)>, // голова (рядок), швидкість, довжина
    glyphs: Vec<u16>,
}

impl Matrix {
    const CW: usize = 4; // ширина клітинки
    const CH: usize = 6; // висота клітинки
    fn new() -> Self {
        let mut rng = Rng::new(seed());
        let ncols = W / Self::CW;
        let cols = (0..ncols)
            .map(|_| (-(rng.below(12) as f32), 0.15 + rng.unit() * 0.35, 3 + rng.below(5)))
            .collect();
        let glyphs = (0..ncols * (H / Self::CH + 1))
            .map(|_| rng.next() as u16 & 0x7FFF)
            .collect();
        Matrix { rng, cols, glyphs }
    }
}

impl Gen for Matrix {
    fn frame(&mut self, _t: f32, f: &mut Frame) {
        clear(f);
        let rows = H / Self::CH + 1;
        for _ in 0..6 {
            let i = self.rng.below(self.glyphs.len() as u32) as usize;
            self.glyphs[i] = self.rng.next() as u16 & 0x7FFF;
        }
        for c in 0..self.cols.len() {
            let (head, speed, len) = self.cols[c];
            let hr = head.floor() as i32;
            for k in 0..=len as i32 {
                let row = hr - k;
                if row < 0 || row >= rows as i32 {
                    continue;
                }
                let g = self.glyphs[c * rows + row as usize];
                let fade = k as f32 / (len as f32 + 1.0);
                for b in 0..15 {
                    if g >> b & 1 == 1 {
                        let (x, y) = (c * Self::CW + b % 3, row as usize * Self::CH + b / 3);
                        if k == 0 || shade(x, y, 1.0 - fade * 0.8) {
                            px(f, x as i32, y as i32, true);
                        }
                    }
                }
            }
            let mut head = head + speed;
            if head - len as f32 > rows as f32 {
                head = -(self.rng.below(8) as f32);
                self.cols[c].1 = 0.15 + self.rng.unit() * 0.35;
                self.cols[c].2 = 3 + self.rng.below(5);
            }
            self.cols[c].0 = head;
        }
    }
}

struct Fire {
    rng: Rng,
    heat: Vec<f32>,
}

impl Fire {
    const HH: usize = H + 2;
    fn new() -> Self {
        Fire {
            rng: Rng::new(seed()),
            heat: vec![0.0; W * Self::HH],
        }
    }
}

impl Gen for Fire {
    fn frame(&mut self, _t: f32, f: &mut Frame) {
        let h = &mut self.heat;
        for x in 0..W {
            for y in Self::HH - 2..Self::HH {
                h[y * W + x] = if self.rng.below(100) < 45 { 1.0 } else { 0.0 };
            }
        }
        for y in 0..Self::HH - 2 {
            for x in 0..W {
                let l = h[(y + 1) * W + (x + W - 1) % W];
                let c = h[(y + 1) * W + x];
                let r = h[(y + 1) * W + (x + 1) % W];
                let d = h[(y + 2) * W + x];
                let v = (l + c + r + d) / 4.0 - 0.012 - self.rng.unit() * 0.016;
                h[y * W + x] = v.max(0.0);
            }
        }
        clear(f);
        for y in 0..H {
            for x in 0..W {
                if shade(x, y, h[y * W + x] * 1.6) {
                    set_px(f, x, y, true);
                }
            }
        }
    }
    fn fps(&self) -> u32 {
        12
    }
}

struct Plasma;

impl Gen for Plasma {
    fn frame(&mut self, t: f32, f: &mut Frame) {
        clear(f);
        for y in 0..H {
            for x in 0..W {
                let (xf, yf) = (x as f32, y as f32 * 2.0);
                let (dx, dy) = (xf - 64.0 - (t * 0.7).sin() * 40.0, yf - 32.0 - (t * 0.9).cos() * 20.0);
                let v = (xf / 14.0 + t).sin()
                    + (yf / 9.0 + t * 1.3).sin()
                    + ((xf + yf) / 18.0 + t * 0.7).sin()
                    + ((dx * dx + dy * dy).sqrt() / 9.0 - t * 1.6).sin();
                if shade(x, y, (v + 4.0) / 8.0) {
                    set_px(f, x, y, true);
                }
            }
        }
    }
    fn fps(&self) -> u32 {
        12
    }
}

struct Life {
    rng: Rng,
    cells: Vec<bool>,
    history: Vec<u64>,
    generation: u32,
}

impl Life {
    fn new() -> Self {
        let mut l = Life {
            rng: Rng::new(seed()),
            cells: vec![false; W * H],
            history: Vec::new(),
            generation: 0,
        };
        l.reseed();
        l
    }
    fn reseed(&mut self) {
        for c in &mut self.cells {
            *c = self.rng.below(100) < 32;
        }
        self.history.clear();
        self.generation = 0;
    }
}

impl Gen for Life {
    fn frame(&mut self, _t: f32, f: &mut Frame) {
        let mut next = vec![false; W * H];
        let mut hash: u64 = 1469598103934665603;
        for y in 0..H {
            for x in 0..W {
                let mut n = 0;
                for dy in [H - 1, 0, 1] {
                    for dx in [W - 1, 0, 1] {
                        if (dx, dy) != (0, 0) && self.cells[(y + dy) % H * W + (x + dx) % W] {
                            n += 1;
                        }
                    }
                }
                let alive = self.cells[y * W + x];
                let v = n == 3 || (alive && n == 2);
                next[y * W + x] = v;
                if v {
                    hash = (hash ^ (y * W + x) as u64).wrapping_mul(1099511628211);
                }
            }
        }
        self.cells = next;
        self.generation += 1;
        // Застигло або зациклилось — новий посів.
        if self.history.contains(&hash) || self.generation > 900 {
            self.reseed();
        } else {
            self.history.push(hash);
            if self.history.len() > 16 {
                self.history.remove(0);
            }
        }
        clear(f);
        for y in 0..H {
            for x in 0..W {
                if self.cells[y * W + x] {
                    set_px(f, x, y, true);
                }
            }
        }
    }
    fn fps(&self) -> u32 {
        10
    }
}

struct Snow {
    rng: Rng,
    flakes: Vec<(f32, f32, f32, f32)>, // x, y, швидкість, фаза
    ground: [f32; W],
}

impl Snow {
    fn new() -> Self {
        let mut rng = Rng::new(seed());
        let flakes = (0..70)
            .map(|_| {
                (
                    rng.unit() * W as f32,
                    rng.unit() * H as f32,
                    0.2 + rng.unit() * 0.6,
                    rng.unit() * 6.0,
                )
            })
            .collect();
        Snow {
            rng,
            flakes,
            ground: [0.0; W],
        }
    }
}

impl Gen for Snow {
    fn frame(&mut self, t: f32, f: &mut Frame) {
        clear(f);
        let total: f32 = self.ground.iter().sum();
        let melting = total > W as f32 * 7.0;
        for g in &mut self.ground {
            if melting {
                *g = (*g - 0.05).max(0.0);
            }
        }
        for fl in &mut self.flakes {
            fl.1 += fl.2;
            fl.0 += (t * 1.5 + fl.3).sin() * 0.3 + 0.1;
            let x = (fl.0.rem_euclid(W as f32)) as usize;
            let floor = H as f32 - self.ground[x];
            if fl.1 >= floor {
                // Сніжинка лягає на кучугуру й трохи розповзається.
                let spread = (x as i32 + self.rng.below(3) as i32 - 1).clamp(0, W as i32 - 1) as usize;
                self.ground[spread] = (self.ground[spread] + 0.6).min(12.0);
                *fl = (
                    self.rng.unit() * W as f32,
                    -2.0,
                    0.2 + self.rng.unit() * 0.6,
                    self.rng.unit() * 6.0,
                );
                continue;
            }
            let (xi, yi) = (x as i32, fl.1 as i32);
            px(f, xi, yi, true);
            if fl.2 > 0.6 {
                px(f, xi - 1, yi, true);
                px(f, xi + 1, yi, true);
                px(f, xi, yi - 1, true);
                px(f, xi, yi + 1, true);
            }
        }
        for (x, &g) in self.ground.iter().enumerate() {
            for y in (H as f32 - g).ceil() as i32..H as i32 {
                px(f, x as i32, y, true);
            }
        }
    }
    fn fps(&self) -> u32 {
        15
    }
}

struct Warp {
    rng: Rng,
    stars: Vec<(f32, f32, f32)>,
}

impl Warp {
    fn new() -> Self {
        let mut rng = Rng::new(seed());
        let stars = (0..90)
            .map(|_| (rng.unit() * 2.0 - 1.0, rng.unit() * 2.0 - 1.0, 0.1 + rng.unit()))
            .collect();
        Warp { rng, stars }
    }
}

impl Gen for Warp {
    fn frame(&mut self, _t: f32, f: &mut Frame) {
        clear(f);
        let project = |x: f32, y: f32, z: f32| (64.0 + x / z * 40.0, 16.0 + y / z * 14.0);
        for s in &mut self.stars {
            let (x0, y0) = project(s.0, s.1, s.2);
            s.2 -= 0.035;
            let (x1, y1) = project(s.0, s.1, s.2.max(0.01));
            if s.2 <= 0.02 || !(0.0..W as f32).contains(&x1) || !(0.0..H as f32).contains(&y1) {
                *s = (
                    self.rng.unit() * 2.0 - 1.0,
                    self.rng.unit() * 2.0 - 1.0,
                    1.0 + self.rng.unit() * 0.2,
                );
                continue;
            }
            line(f, x0 as i32, y0 as i32, x1 as i32, y1 as i32);
        }
    }
    fn fps(&self) -> u32 {
        20
    }
}

struct Tunnel;

impl Gen for Tunnel {
    fn frame(&mut self, t: f32, f: &mut Frame) {
        clear(f);
        for k in 0..8 {
            let z = ((t * 0.6 + k as f32 / 8.0) % 1.0).powf(2.2);
            let (cx, cy) = (
                64.0 + (t * 0.8).sin() * 18.0 * (1.0 - z),
                16.0 + (t * 1.1).cos() * 5.0 * (1.0 - z),
            );
            let (hw, hh) = (2.0 + z * 90.0, 1.0 + z * 24.0);
            rect(
                f,
                (cx - hw) as i32,
                (cy - hh) as i32,
                (hw * 2.0) as i32,
                (hh * 2.0) as i32,
                false,
            );
        }
    }
    fn fps(&self) -> u32 {
        20
    }
}

struct Waves;

impl Gen for Waves {
    fn frame(&mut self, t: f32, f: &mut Frame) {
        clear(f);
        for x in (0..W).step_by(16) {
            for y in (0..H).step_by(4) {
                set_px(f, x, y, true);
            }
        }
        for y in (0..H).step_by(8) {
            for x in (0..W).step_by(4) {
                set_px(f, x, y, true);
            }
        }
        let wave = |x: f32, k: u32| -> f32 {
            match k {
                0 => 16.0 + (x / 10.0 + t * 3.0).sin() * 9.0 * (t * 0.5).sin(),
                _ => 16.0 + ((x / 6.0 - t * 4.0).sin() * 0.6 + (x / 23.0 + t).sin() * 0.4) * 12.0,
            }
        };
        for k in 0..2 {
            let mut prev = wave(0.0, k) as i32;
            for x in 1..W as i32 {
                let y = wave(x as f32, k) as i32;
                line(f, x - 1, prev, x, y);
                prev = y;
            }
        }
    }
    fn fps(&self) -> u32 {
        20
    }
}

struct Flag;

impl Gen for Flag {
    fn frame(&mut self, t: f32, f: &mut Frame) {
        clear(f);
        // Древко.
        for y in 0..H as i32 {
            px(f, 0, y, true);
            px(f, 1, y, true);
        }
        for x in 3..W {
            let k = (x - 3) as f32 / (W - 3) as f32; // біля древка не майорить
            let phase = x as f32 / 15.0 - t * 4.0;
            let dy = phase.sin() * 3.0 * k;
            let light = phase.cos() * 0.18 * k;
            let (top, bottom) = (2.0 + dy, 29.0 + dy);
            let mid = (top + bottom) / 2.0;
            for y in 0..H {
                let yf = y as f32;
                if yf < top || yf > bottom {
                    continue;
                }
                // Синій темніший, жовтий світліший.
                let v = if yf < mid { 0.35 } else { 0.9 } + light;
                if shade(x, y, v) {
                    set_px(f, x, y, true);
                }
            }
        }
    }
    fn fps(&self) -> u32 {
        15
    }
}

struct Heart {
    trace: Vec<f32>,
}

impl Heart {
    fn new() -> Self {
        Heart { trace: vec![0.0; 88] }
    }
}

impl Gen for Heart {
    fn frame(&mut self, t: f32, f: &mut Frame) {
        clear(f);
        // Подвійний удар: тук-тук … пауза.
        let beat = t % 1.0;
        let pulse = (-((beat - 0.05) * 14.0).powi(2)).exp() + 0.6 * (-((beat - 0.28) * 14.0).powi(2)).exp();
        let s = 10.0 + pulse * 3.0;
        for y in 0..H {
            for x in 0..36 {
                let u = (x as f32 - 18.0) / s;
                let v = -(y as f32 - 15.0) / s;
                let a = u * u + v * v - 1.0;
                if a * a * a - u * u * v * v * v <= 0.0 {
                    set_px(f, x, y, true);
                }
            }
        }
        // Кардіограма праворуч.
        let ecg = if beat < 0.03 {
            -8.0
        } else if beat < 0.06 {
            10.0
        } else if beat < 0.09 {
            -4.0
        } else if (0.25..0.33).contains(&beat) {
            -((beat - 0.29) * PI / 0.08).cos() * 3.0
        } else {
            0.0
        };
        self.trace.remove(0);
        self.trace.push(ecg);
        let mut prev = 16 + self.trace[0] as i32;
        for (i, v) in self.trace.iter().enumerate().skip(1) {
            let y = 16 - *v as i32;
            line(f, 39 + i as i32, prev, 40 + i as i32, y);
            prev = y;
        }
    }
    fn fps(&self) -> u32 {
        20
    }
}

struct Pong {
    ball: (f32, f32, f32, f32),
    paddles: [f32; 2],
    score: [u32; 2],
    rng: Rng,
}

impl Pong {
    fn new() -> Self {
        Pong {
            ball: (64.0, 16.0, 2.2, 1.1),
            paddles: [12.0, 12.0],
            score: [0, 0],
            rng: Rng::new(seed()),
        }
    }
}

impl Gen for Pong {
    fn frame(&mut self, _t: f32, f: &mut Frame) {
        let b = &mut self.ball;
        b.0 += b.2;
        b.1 += b.3;
        if b.1 <= 0.0 || b.1 >= H as f32 - 2.0 {
            b.3 = -b.3;
            b.1 = b.1.clamp(0.0, H as f32 - 2.0);
        }
        // Ракетки стежать за м'ячем із запізненням, тож інколи пропускають.
        for (i, p) in self.paddles.iter_mut().enumerate() {
            let coming = if i == 0 { b.2 < 0.0 } else { b.2 > 0.0 };
            let target = if coming { b.1 - 4.0 } else { 12.0 };
            let speed = if coming { 1.0 + self.rng.unit() * 0.6 } else { 0.5 };
            *p += (target - *p).clamp(-speed, speed);
            *p = p.clamp(0.0, H as f32 - 9.0);
        }
        for (i, x) in [(0usize, 3.0f32), (1, W as f32 - 5.0)] {
            let hit = (b.0 - x).abs() < 2.5 && b.1 + 2.0 >= self.paddles[i] && b.1 <= self.paddles[i] + 9.0;
            if hit && (if i == 0 { b.2 < 0.0 } else { b.2 > 0.0 }) {
                b.2 = -b.2 * 1.04;
                b.3 = (b.1 - self.paddles[i] - 4.0) * 0.35 + (self.rng.unit() - 0.5) * 0.4;
            }
        }
        if b.0 < -2.0 || b.0 > W as f32 + 2.0 {
            self.score[if b.0 < 0.0 { 1 } else { 0 }] += 1;
            if self.score.iter().any(|s| *s > 9) {
                self.score = [0, 0];
            }
            *b = (
                64.0,
                16.0,
                if b.0 < 0.0 { 2.2 } else { -2.2 },
                (self.rng.unit() - 0.5) * 2.4,
            );
        }
        clear(f);
        for y in (0..H as i32).step_by(4) {
            px(f, 64, y, true);
            px(f, 64, y + 1, true);
        }
        tiny_number(f, 54, 1, self.score[0]);
        tiny_number(f, 71, 1, self.score[1]);
        rect(f, 2, self.paddles[0] as i32, 2, 9, true);
        rect(f, W as i32 - 4, self.paddles[1] as i32, 2, 9, true);
        rect(f, b.0 as i32, b.1 as i32, 2, 2, true);
    }
    fn fps(&self) -> u32 {
        20
    }
}

struct Maze {
    rng: Rng,
    open: Vec<[bool; 4]>, // для кожної клітинки: прохід вгору, вправо, вниз, вліво
    visited: Vec<bool>,
    stack: Vec<usize>,
    path: Vec<usize>,
    stage: u32, // 0 — будується, 1 — пошук шляху, 2 — пауза
    hold: u32,
}

impl Maze {
    const CW: usize = 31;
    const CH: usize = 7;
    const DIRS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

    fn new() -> Self {
        let mut m = Maze {
            rng: Rng::new(seed()),
            open: vec![],
            visited: vec![],
            stack: vec![],
            path: vec![],
            stage: 0,
            hold: 0,
        };
        m.reset();
        m
    }
    fn reset(&mut self) {
        let n = Self::CW * Self::CH;
        self.open = vec![[false; 4]; n];
        self.visited = vec![false; n];
        self.stack = vec![0];
        self.visited[0] = true;
        self.path.clear();
        self.stage = 0;
        self.hold = 0;
    }
    fn neighbor(c: usize, d: usize) -> Option<usize> {
        let (x, y) = (
            (c % Self::CW) as i32 + Self::DIRS[d].0,
            (c / Self::CW) as i32 + Self::DIRS[d].1,
        );
        (x >= 0 && y >= 0 && (x as usize) < Self::CW && (y as usize) < Self::CH)
            .then(|| y as usize * Self::CW + x as usize)
    }
    fn carve_step(&mut self) {
        while let Some(&c) = self.stack.last() {
            let options: Vec<usize> = (0..4)
                .filter(|&d| Self::neighbor(c, d).is_some_and(|n| !self.visited[n]))
                .collect();
            if options.is_empty() {
                self.stack.pop();
                continue;
            }
            let d = options[self.rng.below(options.len() as u32) as usize];
            let n = Self::neighbor(c, d).unwrap();
            self.open[c][d] = true;
            self.open[n][(d + 2) % 4] = true;
            self.visited[n] = true;
            self.stack.push(n);
            return;
        }
        // Лабіринт готовий: шукаємо шлях з кута в кут.
        self.stage = 1;
        let goal = Self::CW * Self::CH - 1;
        let mut prev = vec![usize::MAX; Self::CW * Self::CH];
        let mut queue = std::collections::VecDeque::from([0usize]);
        prev[0] = 0;
        while let Some(c) = queue.pop_front() {
            for (d, &open) in self.open[c].iter().enumerate() {
                if let Some(n) = Self::neighbor(c, d) {
                    if open && prev[n] == usize::MAX {
                        prev[n] = c;
                        queue.push_back(n);
                    }
                }
            }
        }
        let mut c = goal;
        let mut full = vec![c];
        while c != 0 {
            c = prev[c];
            full.push(c);
        }
        full.reverse();
        self.stack = full; // далі стек зберігає готовий шлях
    }
}

impl Gen for Maze {
    fn frame(&mut self, _t: f32, f: &mut Frame) {
        match self.stage {
            0 => {
                for _ in 0..3 {
                    if self.stage == 0 {
                        self.carve_step();
                    }
                }
            }
            1 => {
                for _ in 0..2 {
                    if self.path.len() < self.stack.len() {
                        self.path.push(self.stack[self.path.len()]);
                    }
                }
                if self.path.len() == self.stack.len() {
                    self.stage = 2;
                }
            }
            _ => {
                self.hold += 1;
                if self.hold > 60 {
                    self.reset();
                }
            }
        }
        clear(f);
        let (ox, oy) = (1, 1);
        for c in 0..Self::CW * Self::CH {
            if !self.visited[c] {
                continue;
            }
            let (x, y) = (ox + (c % Self::CW) as i32 * 4, oy + (c / Self::CW) as i32 * 4);
            // Стіни там, де немає проходу.
            if !self.open[c][0] {
                line(f, x - 1, y - 1, x + 3, y - 1);
            }
            if !self.open[c][1] {
                line(f, x + 3, y - 1, x + 3, y + 3);
            }
            if !self.open[c][2] {
                line(f, x - 1, y + 3, x + 3, y + 3);
            }
            if !self.open[c][3] {
                line(f, x - 1, y - 1, x - 1, y + 3);
            }
        }
        if self.stage == 0 {
            if let Some(&c) = self.stack.last() {
                rect(
                    f,
                    ox + (c % Self::CW) as i32 * 4,
                    oy + (c / Self::CW) as i32 * 4,
                    3,
                    3,
                    true,
                );
            }
        }
        for &c in &self.path {
            px(
                f,
                ox + (c % Self::CW) as i32 * 4 + 1,
                oy + (c / Self::CW) as i32 * 4 + 1,
                true,
            );
        }
    }
    fn fps(&self) -> u32 {
        20
    }
}

struct Bounce {
    text: String,
    pos: (f32, f32),
    vel: (f32, f32),
    flash: u32,
}

impl Bounce {
    fn new(text: String) -> Self {
        Bounce {
            text,
            pos: (10.0, 4.0),
            vel: (1.1, 0.55),
            flash: 0,
        }
    }
}

impl Gen for Bounce {
    fn frame(&mut self, _t: f32, f: &mut Frame) {
        clear(f);
        let Some(font) = font() else { return };
        let size = 12.0;
        let tw = font.width(&self.text, size).min(W as f32 - 8.0);
        let (bw, bh) = (tw + 6.0, 14.0);
        self.pos.0 += self.vel.0;
        self.pos.1 += self.vel.1;
        let mut hits = 0;
        if self.pos.0 <= 0.0 || self.pos.0 + bw >= W as f32 {
            self.vel.0 = -self.vel.0;
            self.pos.0 = self.pos.0.clamp(0.0, W as f32 - bw);
            hits += 1;
        }
        if self.pos.1 <= 0.0 || self.pos.1 + bh >= H as f32 {
            self.vel.1 = -self.vel.1;
            self.pos.1 = self.pos.1.clamp(0.0, H as f32 - bh);
            hits += 1;
        }
        if hits == 2 {
            self.flash = 20; // влучив у кут
        }
        let (x, y) = (self.pos.0 as i32, self.pos.1 as i32);
        rect(f, x, y, bw as i32, bh as i32, false);
        font.draw(f, &self.text, x as f32 + 3.0, y + 11, size);
        if self.flash > 0 {
            self.flash -= 1;
            if self.flash % 4 < 2 {
                for b in f.iter_mut() {
                    *b = !*b;
                }
            }
        }
    }
    fn fps(&self) -> u32 {
        20
    }
}

/// Перевіряє, що кожна заготовка малює щось видиме.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_presets_draw() {
        for p in PRESETS {
            let mut g = make(p.id, "Тест").unwrap();
            let mut f = [0u8; FB];
            let mut lit = 0;
            for i in 0..40 {
                g.frame(i as f32 / 10.0, &mut f);
                lit = lit.max(f.iter().map(|b| b.count_ones()).sum::<u32>());
            }
            assert!(lit > 20, "{} майже порожня: {lit}", p.id);
            let _ = crate::oled::get_px(&f, 0, 0);
        }
    }
}
