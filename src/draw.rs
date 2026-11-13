//! Малювання: фігури на кадрі, документ із кількох кадрів і ефекти, що оживляють малюнок.

use std::f32::consts::PI;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ipc;
use crate::oled::{get_px, set_px, Anim, Frame, FB, H, W};

/* ------------------------------------------------------------ фігури */

fn put(f: &mut Frame, x: i32, y: i32, on: bool) {
    if x >= 0 && y >= 0 && (x as usize) < W && (y as usize) < H {
        set_px(f, x as usize, y as usize, on);
    }
}

fn lit(f: &Frame, x: i32, y: i32) -> bool {
    x >= 0 && y >= 0 && (x as usize) < W && (y as usize) < H && get_px(f, x as usize, y as usize)
}

/// Квадратний пензель розміру `size` із центром у (x, y).
pub fn dab(f: &mut Frame, x: i32, y: i32, size: i32, on: bool) {
    let a = (size - 1) / 2;
    for dy in -a..size - a {
        for dx in -a..size - a {
            put(f, x + dx, y + dy, on);
        }
    }
}

pub fn line(f: &mut Frame, (x0, y0): (i32, i32), (x1, y1): (i32, i32), size: i32, on: bool) {
    let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let (mut x, mut y, mut err) = (x0, y0, dx + dy);
    loop {
        dab(f, x, y, size, on);
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

pub fn rect(f: &mut Frame, a: (i32, i32), b: (i32, i32), fill: bool, size: i32, on: bool) {
    let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
    let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
    if fill {
        for y in y0..=y1 {
            for x in x0..=x1 {
                put(f, x, y, on);
            }
        }
    } else {
        line(f, (x0, y0), (x1, y0), size, on);
        line(f, (x1, y0), (x1, y1), size, on);
        line(f, (x1, y1), (x0, y1), size, on);
        line(f, (x0, y1), (x0, y0), size, on);
    }
}

/// Овал, вписаний у прямокутник між двома точками.
pub fn ellipse(f: &mut Frame, a: (i32, i32), b: (i32, i32), fill: bool, size: i32, on: bool) {
    let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
    let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
    let (cx, cy) = ((x0 + x1) as f32 / 2.0, (y0 + y1) as f32 / 2.0);
    let (rx, ry) = ((x1 - x0) as f32 / 2.0 + 0.5, (y1 - y0) as f32 / 2.0 + 0.5);
    let inside = |x: i32, y: i32| {
        let (u, v) = ((x as f32 - cx) / rx, (y as f32 - cy) / ry);
        u * u + v * v <= 1.0
    };
    for y in y0..=y1 {
        for x in x0..=x1 {
            if !inside(x, y) {
                continue;
            }
            let edge = !inside(x - 1, y) || !inside(x + 1, y) || !inside(x, y - 1) || !inside(x, y + 1);
            if fill {
                put(f, x, y, on);
            } else if edge {
                dab(f, x, y, size, on);
            }
        }
    }
}

/// Заливка області однакового кольору, починаючи з (x, y).
pub fn flood(f: &mut Frame, x: i32, y: i32, on: bool) {
    if x < 0 || y < 0 || x as usize >= W || y as usize >= H || lit(f, x, y) == on {
        return;
    }
    let from = !on;
    let mut stack = vec![(x, y)];
    while let Some((x, y)) = stack.pop() {
        if x < 0 || y < 0 || x as usize >= W || y as usize >= H || lit(f, x, y) != from {
            continue;
        }
        put(f, x, y, on);
        stack.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
    }
}

/// Зсув без загортання: те, що виходить за край, зникає.
pub fn shifted(f: &Frame, dx: i32, dy: i32) -> Frame {
    map(|x, y| lit(f, x - dx, y - dy))
}

pub fn flipped(f: &Frame, horizontal: bool) -> Frame {
    map(|x, y| {
        if horizontal {
            lit(f, W as i32 - 1 - x, y)
        } else {
            lit(f, x, H as i32 - 1 - y)
        }
    })
}

pub fn inverted(f: &Frame) -> Frame {
    let mut out = *f;
    out.iter_mut().for_each(|b| *b = !*b);
    out
}

/// Кадр, кожен піксель якого дає функція.
fn map(mut px: impl FnMut(i32, i32) -> bool) -> Frame {
    let mut out = [0u8; FB];
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            if px(x, y) {
                set_px(&mut out, x as usize, y as usize, true);
            }
        }
    }
    out
}

/* ------------------------------------------------------------ ефекти */

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    #[default]
    None,
    ScrollLeft,
    ScrollRight,
    ScrollUp,
    ScrollDown,
    Swing,
    Bounce,
    Shake,
    Wave,
    Pulse,
    Blink,
    Invert,
    Reveal,
    Dissolve,
    Sparkle,
    Glitch,
}

impl Effect {
    pub const ALL: [Effect; 16] = [
        Effect::None,
        Effect::ScrollLeft,
        Effect::ScrollRight,
        Effect::ScrollUp,
        Effect::ScrollDown,
        Effect::Swing,
        Effect::Bounce,
        Effect::Shake,
        Effect::Wave,
        Effect::Pulse,
        Effect::Blink,
        Effect::Invert,
        Effect::Reveal,
        Effect::Dissolve,
        Effect::Sparkle,
        Effect::Glitch,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Effect::None => "Без ефекту",
            Effect::ScrollLeft => "Біжить ліворуч",
            Effect::ScrollRight => "Біжить праворуч",
            Effect::ScrollUp => "Пливе вгору",
            Effect::ScrollDown => "Пливе вниз",
            Effect::Swing => "Гойдається",
            Effect::Bounce => "Підстрибує",
            Effect::Shake => "Тремтить",
            Effect::Wave => "Хвиля",
            Effect::Pulse => "Пульсує",
            Effect::Blink => "Блимає",
            Effect::Invert => "Мигає інверсією",
            Effect::Reveal => "З'являється і зникає",
            Effect::Dissolve => "Розсипається на точки",
            Effect::Sparkle => "Іскри навколо",
            Effect::Glitch => "Глюк",
        }
    }

    /// Скільки кроків триває один цикл ефекту.
    pub fn period(self) -> usize {
        match self {
            Effect::None => 1,
            Effect::ScrollLeft | Effect::ScrollRight => W / 2,
            Effect::ScrollUp | Effect::ScrollDown => H,
            Effect::Swing => 36,
            Effect::Bounce => 16,
            Effect::Shake => 12,
            Effect::Wave => 24,
            Effect::Pulse => 20,
            Effect::Blink => 6,
            Effect::Invert => 2,
            Effect::Reveal => 88,
            Effect::Dissolve => 48,
            Effect::Sparkle => 24,
            Effect::Glitch => 30,
        }
    }

    /// Крок `k` ефекту для кадру `src`.
    pub fn apply(self, src: &Frame, k: usize) -> Frame {
        let p = self.period();
        let k = k % p;
        let phase = 2.0 * PI * k as f32 / p as f32;
        let (w, h) = (W as i32, H as i32);
        match self {
            Effect::None => *src,
            Effect::ScrollLeft => map(|x, y| lit(src, (x + 2 * k as i32).rem_euclid(w), y)),
            Effect::ScrollRight => map(|x, y| lit(src, (x - 2 * k as i32).rem_euclid(w), y)),
            Effect::ScrollUp => map(|x, y| lit(src, x, (y + k as i32).rem_euclid(h))),
            Effect::ScrollDown => map(|x, y| lit(src, x, (y - k as i32).rem_euclid(h))),
            Effect::Swing => shifted(src, (phase.sin() * 10.0).round() as i32, 0),
            Effect::Bounce => shifted(src, 0, -((PI * k as f32 / p as f32).sin() * 7.0).round() as i32),
            Effect::Shake => {
                let r = hash(k as u32 * 7 + 1);
                shifted(src, (r % 5) as i32 - 2, (r / 5 % 3) as i32 - 1)
            }
            Effect::Wave => map(|x, y| {
                let dy = ((x as f32 * 2.0 * PI / 40.0 + phase).sin() * 3.0).round() as i32;
                lit(src, x, y - dy)
            }),
            Effect::Pulse => {
                let (cx, cy) = center(src);
                let s = 1.0 + 0.18 * phase.sin().max(0.0);
                map(|x, y| {
                    lit(
                        src,
                        (cx + (x as f32 - cx) / s).round() as i32,
                        (cy + (y as f32 - cy) / s).round() as i32,
                    )
                })
            }
            Effect::Blink => {
                if k < 4 {
                    *src
                } else {
                    [0; FB]
                }
            }
            Effect::Invert => {
                if k == 0 {
                    *src
                } else {
                    inverted(src)
                }
            }
            Effect::Reveal => {
                let k = k as i32;
                map(|x, y| {
                    let shown = match k {
                        0..32 => x < (k + 1) * 4,
                        32..48 => true,
                        48..80 => x >= (k - 47) * 4,
                        _ => false,
                    };
                    shown && lit(src, x, y)
                })
            }
            Effect::Dissolve => {
                let k = k as f32;
                map(|x, y| {
                    let r = (hash((y * w + x) as u32) % 1000) as f32 / 1000.0;
                    let shown = if k < 20.0 {
                        r < (k + 1.0) / 20.0
                    } else if k < 32.0 {
                        true
                    } else {
                        r >= (k - 31.0) / 16.0
                    };
                    shown && lit(src, x, y)
                })
            }
            Effect::Sparkle => sparkle(src, k),
            Effect::Glitch => {
                if k % 10 > 1 {
                    return *src;
                }
                let seed = hash(k as u32 + 99);
                map(|x, y| {
                    let band = y / 4;
                    let r = hash(seed ^ band as u32);
                    let dx = if r.is_multiple_of(3) {
                        (r / 3 % 13) as i32 - 6
                    } else {
                        0
                    };
                    lit(src, x - dx, y) != r.is_multiple_of(11)
                })
            }
        }
    }
}

fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// Межі світлих пікселів: (x0, y0, x1, y1).
fn bounds(f: &Frame) -> Option<(i32, i32, i32, i32)> {
    let mut b: Option<(i32, i32, i32, i32)> = None;
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            if lit(f, x, y) {
                b = Some(b.map_or((x, y, x, y), |(a, c, d, e)| (a.min(x), c.min(y), d.max(x), e.max(y))));
            }
        }
    }
    b
}

fn center(f: &Frame) -> (f32, f32) {
    bounds(f).map_or((W as f32 / 2.0, H as f32 / 2.0), |(x0, y0, x1, y1)| {
        ((x0 + x1) as f32 / 2.0, (y0 + y1) as f32 / 2.0)
    })
}

/// Малюнок і зірочки, що спалахують у темних місцях поруч із ним.
fn sparkle(src: &Frame, k: usize) -> Frame {
    let mut out = *src;
    // Місце для зірочки: темне, без світлих сусідів, ближче до малюнка.
    let free = |x: i32, y: i32| (-2..=2).all(|dy| (-2..=2).all(|dx| !lit(src, x + dx, y + dy)));
    let (x0, y0, x1, y1) = bounds(src).map_or((0, 0, W as i32 - 1, H as i32 - 1), |(a, b, c, d)| {
        (a - 10, b - 6, c + 10, d + 6)
    });
    for slot in 0..16u32 {
        let age = (k + slot as usize * 3 / 2) % 24;
        if age >= 6 {
            continue;
        }
        let spot = (0..40u32).find_map(|attempt| {
            let r = hash(slot * 31 + attempt * 7919 + 5);
            let x = (r % W as u32) as i32;
            let y = (r / 977 % H as u32) as i32;
            let near = (x0..=x1).contains(&x) && (y0..=y1).contains(&y);
            (free(x, y) && (near || attempt >= 20)).then_some((x, y))
        });
        let Some((x, y)) = spot else { continue };
        put(&mut out, x, y, true);
        if (2..4).contains(&age) {
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                put(&mut out, x + dx, y + dy, true);
            }
        }
    }
    out
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Найбільше кадрів у готовій анімації.
const MAX_OUT: usize = 1200;

/// Крок часу анімації: екран оновлюється раз на 50 мс.
const TICK: u64 = 50;

fn quantize(ms: u16) -> u64 {
    ((ms as u64 + TICK / 2) / TICK).max(1) * TICK
}

/// Анімація для екрана: кадри малюнка змінюються раз на `frame_ms`, ефект робить крок раз на `effect_ms`.
pub fn build(frames: &[Frame], frame_ms: u16, effect: Effect, effect_ms: u16) -> Anim {
    let n = frames.len().max(1) as u64;
    let animated = effect != Effect::None;
    if n == 1 && !animated {
        return Anim {
            frames: vec![frames.first().copied().unwrap_or([0; FB])],
            delays_ms: vec![1000],
        };
    }
    let (fd, ed) = (quantize(frame_ms), quantize(effect_ms));
    let p = effect.period() as u64;
    let a = if n > 1 { n * fd } else { ed * p };
    let b = if animated { ed * p } else { a };
    let lcm = a / gcd(a as usize, b as usize) as u64 * b;
    let mut total = if lcm / TICK <= MAX_OUT as u64 { lcm } else { a.max(b) };
    let mut times: Vec<u64> = (0..total)
        .step_by(TICK as usize)
        .filter(|t| (n > 1 && t % fd == 0) || (animated && t % ed == 0))
        .collect();
    if times.len() > MAX_OUT {
        total = times[MAX_OUT];
        times.truncate(MAX_OUT);
    }
    let mut anim = Anim::default();
    for (i, &t) in times.iter().enumerate() {
        let next = times.get(i + 1).copied().unwrap_or(total);
        let src = &frames[((t / fd) % n) as usize];
        anim.frames.push(effect.apply(src, ((t / ed) % p) as usize));
        anim.delays_ms.push((next - t).min(u16::MAX as u64) as u16);
    }
    anim
}

/* ------------------------------------------------------------ документ */

const UNDO_DEPTH: usize = 200;

/// Малюнок із кадрами, налаштуваннями анімації та історією змін.
#[derive(Clone, Serialize, Deserialize)]
pub struct Doc {
    #[serde(with = "frames_b64")]
    pub frames: Vec<Frame>,
    pub cur: usize,
    pub delay_ms: u16,
    pub effect: Effect,
    #[serde(default = "default_effect_ms")]
    pub effect_ms: u16,
    #[serde(skip)]
    undo: Vec<(Vec<Frame>, usize)>,
    #[serde(skip)]
    redo: Vec<(Vec<Frame>, usize)>,
}

mod frames_b64 {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(frames: &[Frame], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(frames.iter().map(ipc::encode_frame))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Frame>, D::Error> {
        let v: Vec<String> = Vec::deserialize(d)?;
        v.iter()
            .map(|s| ipc::decode_frame(s).map_err(serde::de::Error::custom))
            .collect()
    }
}

fn default_effect_ms() -> u16 {
    100
}

impl Default for Doc {
    fn default() -> Self {
        Self {
            frames: vec![[0; FB]],
            cur: 0,
            delay_ms: 150,
            effect_ms: default_effect_ms(),
            effect: Effect::None,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
}

impl Doc {
    pub fn path() -> PathBuf {
        crate::daemon::config_dir().join("drawing.json")
    }

    pub fn load(path: &Path) -> Option<Doc> {
        let mut d: Doc = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        if d.frames.is_empty() {
            return None;
        }
        d.cur = d.cur.min(d.frames.len() - 1);
        Some(d)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_vec(self).map_err(std::io::Error::other)?)
    }

    pub fn frame(&self) -> &Frame {
        &self.frames[self.cur]
    }

    pub fn frame_mut(&mut self) -> &mut Frame {
        &mut self.frames[self.cur]
    }

    /// Запам'ятати стан перед зміною, щоб її можна було відмінити.
    pub fn checkpoint(&mut self) {
        self.undo.push((self.frames.clone(), self.cur));
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Прибрати останню точку відкату, якщо зміни так і не сталося.
    pub fn drop_checkpoint_if_same(&mut self) {
        if self.undo.last().is_some_and(|(f, _)| *f == self.frames) {
            self.undo.pop();
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some((f, c)) = self.undo.pop() {
            self.redo.push((std::mem::replace(&mut self.frames, f), self.cur));
            self.cur = c.min(self.frames.len() - 1);
        }
    }

    pub fn redo(&mut self) {
        if let Some((f, c)) = self.redo.pop() {
            self.undo.push((std::mem::replace(&mut self.frames, f), self.cur));
            self.cur = c.min(self.frames.len() - 1);
        }
    }

    /// Новий кадр після поточного: порожній або копія.
    pub fn add_frame(&mut self, copy: bool) {
        self.checkpoint();
        let f = if copy { *self.frame() } else { [0; FB] };
        self.frames.insert(self.cur + 1, f);
        self.cur += 1;
    }

    pub fn delete_frame(&mut self) {
        self.checkpoint();
        if self.frames.len() == 1 {
            self.frames[0] = [0; FB];
        } else {
            self.frames.remove(self.cur);
            self.cur = self.cur.min(self.frames.len() - 1);
        }
    }

    pub fn move_frame(&mut self, right: bool) {
        let to = if right { self.cur + 1 } else { self.cur.wrapping_sub(1) };
        if to < self.frames.len() {
            self.checkpoint();
            self.frames.swap(self.cur, to);
            self.cur = to;
        }
    }

    /// Змінити поточний кадр однією дією з можливістю відкату.
    pub fn edit(&mut self, f: impl FnOnce(&Frame) -> Frame) {
        self.checkpoint();
        let new = f(self.frame());
        *self.frame_mut() = new;
        self.drop_checkpoint_if_same();
    }

    pub fn anim(&self) -> Anim {
        build(&self.frames, self.delay_ms, self.effect, self.effect_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(f: &Frame) -> u32 {
        f.iter().map(|b| b.count_ones()).sum()
    }

    #[test]
    fn shapes() {
        let mut f = [0; FB];
        rect(&mut f, (0, 0), (9, 9), true, 1, true);
        assert_eq!(count(&f), 100);
        let mut f = [0; FB];
        rect(&mut f, (9, 9), (0, 0), false, 1, true);
        assert_eq!(count(&f), 36);
        let mut f = [0; FB];
        line(&mut f, (0, 0), (127, 31), 1, true);
        assert!(lit(&f, 0, 0) && lit(&f, 127, 31));
        let mut f = [0; FB];
        ellipse(&mut f, (10, 5), (30, 25), true, 1, true);
        assert!(lit(&f, 20, 15) && !lit(&f, 10, 5));
    }

    #[test]
    fn flood_fills_enclosed_area_only() {
        let mut f = [0; FB];
        rect(&mut f, (10, 10), (20, 20), false, 1, true);
        flood(&mut f, 15, 15, true);
        assert_eq!(count(&f), 121);
        flood(&mut f, 0, 0, true);
        assert_eq!(count(&f), (W * H) as u32);
    }

    #[test]
    fn effects_keep_size_and_loop() {
        let mut f = [0; FB];
        rect(&mut f, (40, 8), (80, 24), true, 1, true);
        for e in Effect::ALL {
            let p = e.period();
            assert_eq!(e.apply(&f, 0), e.apply(&f, p), "{e:?}");
            let a = build(&[f, f], 100, e, 50);
            assert!(a.delays_ms.iter().all(|&d| d >= 50));
            assert_eq!(a.frames.len(), a.delays_ms.len());
            assert!(a.frames.len() <= MAX_OUT);
        }
        assert_eq!(Effect::ScrollLeft.apply(&f, 5), shifted(&f, -10, 0));
    }

    #[test]
    fn two_speeds() {
        let f = [0; FB];
        // 2 кадри по 200 мс (400 мс) і «блимання» (6 кроків по 100 мс): спільний цикл 1200 мс, зміна кожні 100 мс.
        let a = build(&[f, f], 200, Effect::Blink, 100);
        assert_eq!(a.frames.len(), 12);
        assert!(a.delays_ms.iter().all(|&d| d == 100));
        let a = build(&[f], 150, Effect::None, 100);
        assert_eq!(a.frames.len(), 1);
    }

    #[test]
    fn undo_redo() {
        let mut d = Doc::default();
        d.edit(inverted);
        assert_eq!(count(d.frame()), (W * H) as u32);
        d.add_frame(true);
        assert_eq!(d.frames.len(), 2);
        d.undo();
        d.undo();
        assert_eq!(d.frames.len(), 1);
        assert_eq!(count(d.frame()), 0);
        d.redo();
        assert_eq!(count(d.frame()), (W * H) as u32);
        let json = serde_json::to_vec(&d).unwrap();
        let back: Doc = serde_json::from_slice(&json).unwrap();
        assert_eq!(back.frames, d.frames);
    }
}
