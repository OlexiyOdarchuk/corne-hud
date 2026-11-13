//! Заготовки з живими даними комп'ютера: звук, натискання, датчики.

use std::sync::Arc;

use crate::audio::{Analyzer, BANDS, WAVE};
use crate::draw;
use crate::keys::{self, Access};
use crate::oled::{set_px, Frame, FB, H, W};
use crate::pixfont;
use crate::presets::Gen;
use crate::sysinfo::{self, Power, Reading, Sensors};

fn put(f: &mut Frame, x: i32, y: i32, on: bool) {
    if x >= 0 && y >= 0 && (x as usize) < W && (y as usize) < H {
        set_px(f, x as usize, y as usize, on);
    }
}

fn centered(f: &mut Frame, text: &str, y: i32, scale: i32) {
    pixfont::draw(f, (W as i32 - pixfont::width(text, scale)) / 2, y, text, scale, true);
}

/* ------------------------------------------------------------ спектр */

pub struct Spectrum {
    audio: Arc<Analyzer>,
    level: [f32; BANDS],
    peak: [f32; BANDS],
    hold: [f32; BANDS],
    last: f32,
}

impl Spectrum {
    pub fn new() -> Self {
        Self {
            audio: Analyzer::shared(),
            level: [0.0; BANDS],
            peak: [0.0; BANDS],
            hold: [0.0; BANDS],
            last: 0.0,
        }
    }
}

impl Spectrum {
    /// Малює стовпчики для рівнів смуг (0..1); `t` — час кадру в секундах.
    pub fn draw_levels(&mut self, bands: &[f32; BANDS], t: f32, f: &mut Frame) {
        *f = [0; FB];
        let dt = (t - self.last).clamp(0.0, 0.2);
        self.last = t;
        let bar_w = W as i32 / BANDS as i32; // 4: стовпчик 3 px і проміжок
        for (b, &target) in bands.iter().enumerate() {
            self.level[b] = if target > self.level[b] {
                target
            } else {
                (self.level[b] - dt * 1.6).max(target)
            };
            if self.level[b] >= self.peak[b] {
                self.peak[b] = self.level[b];
                self.hold[b] = 0.35;
            } else if self.hold[b] > 0.0 {
                self.hold[b] -= dt;
            } else {
                self.peak[b] = (self.peak[b] - dt * 0.7).max(self.level[b]);
            }
            let x0 = b as i32 * bar_w;
            let h = (self.level[b] * 29.0).round() as i32 + 1;
            for y in H as i32 - h..H as i32 {
                for x in x0..x0 + bar_w - 1 {
                    put(f, x, y, true);
                }
            }
            let py = H as i32 - 3 - (self.peak[b] * 29.0).round() as i32;
            if self.peak[b] > 0.04 {
                for x in x0..x0 + bar_w - 1 {
                    put(f, x, py, true);
                }
            }
        }
    }
}

impl Gen for Spectrum {
    fn frame(&mut self, t: f32, f: &mut Frame) {
        let snap = self.audio.snapshot();
        if !snap.ok {
            *f = [0; FB];
            centered(f, "NO AUDIO", 13, 1);
            return;
        }
        self.draw_levels(&snap.bands, t, f);
    }
    fn fps(&self) -> u32 {
        20
    }
}

/* ------------------------------------------------------------ хвиля */

pub struct Scope {
    audio: Arc<Analyzer>,
    gain: f32,
}

impl Scope {
    pub fn new() -> Self {
        Self {
            audio: Analyzer::shared(),
            gain: 1.0,
        }
    }
}

impl Gen for Scope {
    fn frame(&mut self, _t: f32, f: &mut Frame) {
        *f = [0; FB];
        let snap = self.audio.snapshot();
        if !snap.ok {
            centered(f, "NO AUDIO", 13, 1);
            return;
        }
        let loud = snap.wave.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let want = if loud > 0.002 {
            (0.9 / loud).min(60.0)
        } else {
            self.gain
        };
        self.gain += (want - self.gain) * 0.2;
        let mid = H as i32 / 2;
        let y = |i: usize| {
            (mid as f32 - snap.wave[i] * self.gain * 15.0)
                .round()
                .clamp(0.0, H as f32 - 1.0) as i32
        };
        for x in (0..W as i32).step_by(4) {
            put(f, x, mid, true); // пунктир нуля
        }
        for i in 1..WAVE {
            draw::line(f, (i as i32 - 1, y(i - 1)), (i as i32, y(i)), 1, true);
        }
    }
    fn fps(&self) -> u32 {
        20
    }
}

/* ------------------------------------------------------------ котик */

pub struct Bongo {
    seen: u64,
    right: bool,
    hit_at: f32,
    last: f32,
}

impl Bongo {
    pub fn new() -> Self {
        Self {
            seen: keys::presses(),
            right: false,
            hit_at: -10.0,
            last: 0.0,
        }
    }
}

/// Лапка: піднята біля голови або опущена на клавіатуру.
fn paw(f: &mut Frame, right: bool, down: bool) {
    let m = |x: i32| if right { 127 - x } else { x };
    let span = |x0: i32, x1: i32| (m(x0).min(m(x1)), m(x0).max(m(x1)));
    let (shoulder, hand, (x0, x1), (y0, y1)) = if down {
        ((m(50), 18), (m(48), 23), span(43, 51), (22, 27))
    } else {
        ((m(50), 18), (m(43), 11), span(37, 45), (6, 12))
    };
    draw::line(f, shoulder, hand, 2, true);
    draw::ellipse(f, (x0, y0), (x1, y1), true, 1, true);
    if down {
        // «бум» від удару
        let x = if right { x1 + 3 } else { x0 - 3 };
        for dy in [-2i32, 0, 2] {
            put(
                f,
                x + if right { dy.abs() / 2 } else { -dy.abs() / 2 },
                (y0 + y1) / 2 + dy,
                true,
            );
        }
    } else {
        // пальчики: темні щілинки на верху лапки
        for dx in [-2, 0, 2] {
            put(f, (x0 + x1) / 2 + dx, y0 + 1, false);
        }
    }
}

impl Bongo {
    /// Удар лапкою без справжньої клавіатури (для демо).
    pub fn demo_hit(&mut self, t: f32) {
        self.right = !self.right;
        self.hit_at = t;
    }
}

impl Gen for Bongo {
    fn frame(&mut self, t: f32, f: &mut Frame) {
        *f = [0; FB];
        self.last = t;
        let now = keys::presses();
        if now != self.seen {
            if (now - self.seen) % 2 == 1 {
                self.right = !self.right;
            }
            self.seen = now;
            self.hit_at = t;
        }
        let since = t - self.hit_at;
        let asleep = since > 8.0;
        let left_down = since < 0.12 && !self.right;
        let right_down = since < 0.12 && self.right;

        // голова з вухами, нижню частину ховає стіл
        draw::ellipse(f, (47, 5), (80, 34), false, 1, true);
        draw::line(f, (50, 11), (52, 2), 1, true);
        draw::line(f, (52, 2), (58, 6), 1, true);
        draw::line(f, (77, 11), (75, 2), 1, true);
        draw::line(f, (75, 2), (69, 6), 1, true);
        if asleep {
            draw::line(f, (56, 15), (59, 15), 1, true);
            draw::line(f, (68, 15), (71, 15), 1, true);
        } else if since < 1.0 {
            for (x, y) in [(56, 15), (57, 14), (58, 15), (69, 15), (70, 14), (71, 15)] {
                put(f, x, y, true); // ^ ^ — захоплено друкує
            }
        } else {
            draw::dab(f, 57, 14, 2, true);
            draw::dab(f, 70, 14, 2, true);
        }
        for (x, y) in [(61, 19), (62, 20), (63, 19), (64, 20), (65, 19)] {
            put(f, x, y, true); // ротик w
        }

        // стіл і клавіатура на ньому
        for x in 0..W as i32 {
            for y in 28..H as i32 {
                put(f, x, y, false);
            }
            put(f, x, 28, true);
        }
        draw::rect(f, (45, 25), (82, 27), false, 1, true);
        for x in (47..81).step_by(3) {
            put(f, x, 26, true);
        }

        paw(f, false, left_down);
        paw(f, true, right_down);

        // швидкість друку зліва, сон чи нотки справа
        let wpm = keys::per_minute() / 5;
        match keys::access() {
            Access::Denied => pixfont::draw(f, 92, 10, "NO", 1, true),
            Access::NoKeyboard => pixfont::draw(f, 92, 10, "NO KB", 1, true),
            _ => {}
        }
        if keys::access() == Access::Denied {
            pixfont::draw(f, 92, 17, "ACCESS", 1, true);
        }
        pixfont::draw(f, 2, 3, &wpm.to_string(), 2, true);
        pixfont::draw(f, 2, 15, "WPM", 1, true);
        if asleep {
            let k = ((t * 1.5) as i32) % 3;
            for i in 0..=k {
                pixfont::draw(f, 100 + i * 7, 18 - i * 6, "Z", 1 + (i == 2) as i32, true);
            }
        } else if since < 1.5 && keys::access() == Access::Ok {
            // нотка, що підстрибує
            let y = 6 + ((t * 8.0).sin() * 2.0) as i32;
            draw::ellipse(f, (100, y + 6), (103, y + 8), true, 1, true);
            draw::line(f, (103, y), (103, y + 7), 1, true);
            draw::line(f, (103, y), (106, y + 2), 1, true);
        }
    }
    fn fps(&self) -> u32 {
        20
    }
}

/* ------------------------------------------------------------ датчики */

pub struct System {
    sensors: Sensors,
    cpu: sysinfo::Cpu,
    reading: Reading,
    load: u8,
    ram: u8,
    next: f32,
}

impl System {
    pub fn new() -> Self {
        let mut cpu = sysinfo::Cpu::default();
        cpu.sample();
        Self {
            sensors: Sensors::find(),
            cpu,
            reading: Reading::default(),
            load: 0,
            ram: 0,
            next: 0.0,
        }
    }
}

/// Плитка: підпис, велике значення, смужка заповнення.
fn tile(f: &mut Frame, i: i32, label: &str, value: &str, fill: Option<f32>) {
    let x0 = i * 32;
    pixfont::draw(f, x0 + 2, 1, label, 1, true);
    let scale = if pixfont::width(value, 2) <= 30 { 2 } else { 1 };
    pixfont::draw(f, x0 + 2, if scale == 2 { 9 } else { 12 }, value, scale, true);
    if let Some(v) = fill {
        draw::rect(f, (x0 + 2, 23), (x0 + 29, 27), false, 1, true);
        let w = (v.clamp(0.0, 1.0) * 26.0).round() as i32;
        if w > 0 {
            draw::rect(f, (x0 + 3, 24), (x0 + 2 + w, 26), true, 1, true);
        }
    }
    if i > 0 {
        for y in (0..H as i32).step_by(2) {
            put(f, x0 - 1, y, true);
        }
    }
}

impl Gen for System {
    fn frame(&mut self, t: f32, f: &mut Frame) {
        if t >= self.next {
            self.next = t + 1.0;
            self.reading = self.sensors.read();
            self.load = self.cpu.sample();
            self.ram = sysinfo::ram_percent();
        }
        *f = [0; FB];
        let r = &self.reading;
        match r.cpu_temp {
            Some(c) => {
                let label = format!("CPU{:>4}", format!("{}%", self.load));
                tile(f, 0, &label, &format!("{c:.0}°"), Some(self.load as f32 / 100.0))
            }
            None => tile(f, 0, "CPU", &format!("{}%", self.load), Some(self.load as f32 / 100.0)),
        }
        match r.extra_temp {
            Some((name, c)) => tile(f, 1, name, &format!("{c:.0}°"), Some(c / 100.0)),
            None => tile(f, 1, "GPU", "--", None),
        }
        tile(f, 2, "RAM", &format!("{}%", self.ram), Some(self.ram as f32 / 100.0));
        match r.battery {
            Some((cap, power)) => {
                let label = match power {
                    Power::Charging => "CHG",
                    Power::Full | Power::Idle => "AC",
                    Power::Discharging => "BAT",
                };
                tile(f, 3, label, &format!("{cap}%"), Some(cap as f32 / 100.0));
            }
            None => tile(f, 3, "PWR", "AC", None),
        }
    }
    fn fps(&self) -> u32 {
        10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Кадри котика в різних станах: `CORNE_PREVIEW=dir cargo test bongo_states -- --ignored`.
    #[test]
    #[ignore]
    fn bongo_states() {
        let dir = std::env::var("CORNE_PREVIEW").unwrap();
        let mut g = Bongo::new();
        let mut frames = Vec::new();
        for (t, hit, right) in [
            (20.0, 0.0, false),
            (5.0, 4.95, false),
            (5.0, 4.95, true),
            (5.0, 3.0, false),
        ] {
            g.hit_at = hit;
            g.right = right;
            g.seen = keys::presses();
            let mut f = [0u8; FB];
            g.frame(t, &mut f);
            frames.push(f);
        }
        let anim = crate::oled::Anim {
            delays_ms: vec![500; frames.len()],
            frames,
        };
        for (i, f) in anim.frames.iter().enumerate() {
            crate::oled::to_rgba(f, 4).save(format!("{dir}/bongo{i}.png")).unwrap();
        }
    }
}
