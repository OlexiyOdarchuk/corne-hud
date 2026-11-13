//! Кадр OLED 128x32 і перетворення картинок у нього.
//!
//! Формат кадру такий самий, як у прошивці: 512 байт, байт — стовпчик із 8 пікселів
//! (біт 0 зверху), індекс `x + (y / 8) * 128`.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use image::imageops::FilterType;
use image::{AnimationDecoder, GrayImage, RgbaImage};
use serde::{Deserialize, Serialize};

pub const W: usize = 128;
pub const H: usize = 32;
pub const FB: usize = W * H / 8;

pub type Frame = [u8; FB];

/// Найкоротша пауза між кадрами: екран оновлюється 20 разів на секунду.
pub const MIN_DELAY_MS: u16 = 50;
const MAX_FRAMES: usize = 2000;

pub fn get_px(f: &Frame, x: usize, y: usize) -> bool {
    f[x + (y / 8) * W] >> (y % 8) & 1 == 1
}

pub fn set_px(f: &mut Frame, x: usize, y: usize, on: bool) {
    let i = x + (y / 8) * W;
    if on {
        f[i] |= 1 << (y % 8);
    } else {
        f[i] &= !(1 << (y % 8));
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
pub enum Fit {
    /// Вписати цілком, по краях — чорне
    #[default]
    Contain,
    /// Заповнити весь екран, зайве обрізати
    Cover,
    /// Розтягнути без збереження пропорцій
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConvertOpts {
    pub fit: Fit,
    /// Розсіювання (Floyd–Steinberg): передає півтони точками.
    pub dither: bool,
    /// Поріг яскравості 0..255 (без розсіювання).
    pub threshold: u8,
    pub invert: bool,
}

impl Default for ConvertOpts {
    fn default() -> Self {
        Self {
            fit: Fit::Contain,
            dither: true,
            threshold: 128,
            invert: false,
        }
    }
}

/// Кадри з паузами після кожного.
#[derive(Clone, Debug, Default)]
pub struct Anim {
    pub frames: Vec<Frame>,
    pub delays_ms: Vec<u16>,
}

/// Вихідні кольорові кадри файлу: щоб змінювати налаштування без повторного читання.
pub struct Source {
    pub frames: Vec<(RgbaImage, u16)>,
}

impl Source {
    pub fn open(path: &Path) -> Result<Self, String> {
        let is_gif = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("gif"));
        let frames = if is_gif {
            let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let dec = image::codecs::gif::GifDecoder::new(BufReader::new(file)).map_err(|e| e.to_string())?;
            let mut out = Vec::new();
            for f in dec.into_frames().take(MAX_FRAMES) {
                let f = f.map_err(|e| e.to_string())?;
                let (n, d) = f.delay().numer_denom_ms();
                let ms = n.checked_div(d).unwrap_or(100);
                out.push((f.into_buffer(), ms.clamp(MIN_DELAY_MS as u32, 60_000) as u16));
            }
            out
        } else {
            let img = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
            vec![(img.to_rgba8(), 1000)]
        };
        if frames.is_empty() {
            return Err("у файлі немає кадрів".into());
        }
        Ok(Self { frames })
    }

    pub fn convert(&self, opts: &ConvertOpts) -> Anim {
        Anim {
            frames: self.frames.iter().map(|(img, _)| convert(img, opts)).collect(),
            delays_ms: self.frames.iter().map(|(_, d)| *d).collect(),
        }
    }
}

/// Колір → яскравість, прозоре — на чорному тлі.
fn to_gray(img: &RgbaImage) -> GrayImage {
    GrayImage::from_fn(img.width(), img.height(), |x, y| {
        let [r, g, b, a] = img.get_pixel(x, y).0;
        let l = (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000;
        image::Luma([(l * a as u32 / 255) as u8])
    })
}

fn fit_to_screen(gray: &GrayImage, fit: Fit) -> GrayImage {
    let (w, h) = (gray.width().max(1) as f32, gray.height().max(1) as f32);
    let (sw, sh) = (W as f32, H as f32);
    let mut canvas = GrayImage::new(W as u32, H as u32);
    match fit {
        Fit::Stretch => return image::imageops::resize(gray, W as u32, H as u32, FilterType::Triangle),
        Fit::Contain | Fit::Cover => {
            let k = if fit == Fit::Contain {
                (sw / w).min(sh / h)
            } else {
                (sw / w).max(sh / h)
            };
            let nw = ((w * k).round() as u32).max(1);
            let nh = ((h * k).round() as u32).max(1);
            let scaled = image::imageops::resize(gray, nw, nh, FilterType::Triangle);
            let dx = (W as i64 - nw as i64) / 2;
            let dy = (H as i64 - nh as i64) / 2;
            image::imageops::replace(&mut canvas, &scaled, dx, dy);
        }
    }
    canvas
}

pub fn convert(img: &RgbaImage, opts: &ConvertOpts) -> Frame {
    let small = fit_to_screen(&to_gray(img), opts.fit);
    let mut lum: Vec<f32> = small.pixels().map(|p| p.0[0] as f32).collect();
    let mut frame = [0u8; FB];
    for y in 0..H {
        for x in 0..W {
            let i = y * W + x;
            let old = lum[i];
            let on = if opts.dither {
                old >= 128.0
            } else {
                old >= opts.threshold as f32
            };
            set_px(&mut frame, x, y, on != opts.invert);
            if opts.dither {
                let err = old - if on { 255.0 } else { 0.0 };
                let mut spread = |dx: i32, dy: i32, k: f32| {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx >= 0 && (nx as usize) < W && (ny as usize) < H {
                        lum[ny as usize * W + nx as usize] += err * k;
                    }
                };
                spread(1, 0, 7.0 / 16.0);
                spread(-1, 1, 3.0 / 16.0);
                spread(0, 1, 5.0 / 16.0);
                spread(1, 1, 1.0 / 16.0);
            }
        }
    }
    frame
}

/// Сірі 8-бітні пікселі 128x32 (наприклад, з ffmpeg `-pix_fmt gray`) → кадр за порогом.
pub fn from_gray(raw: &[u8], threshold: u8) -> Frame {
    let mut f = [0u8; FB];
    for y in 0..H {
        for x in 0..W {
            set_px(&mut f, x, y, raw[y * W + x] >= threshold);
        }
    }
    f
}

/// Збереження анімації на диск: [кількість u32][для кожного кадру: пауза u16, 512 байт].
pub fn save_anim(path: &Path, anim: &Anim) -> std::io::Result<()> {
    let mut out = Vec::with_capacity(4 + anim.frames.len() * (2 + FB));
    out.extend_from_slice(&(anim.frames.len() as u32).to_le_bytes());
    for (f, d) in anim.frames.iter().zip(&anim.delays_ms) {
        out.extend_from_slice(&d.to_le_bytes());
        out.extend_from_slice(f);
    }
    std::fs::write(path, out)
}

pub fn load_anim(path: &Path) -> Option<Anim> {
    let data = std::fs::read(path).ok()?;
    let n = u32::from_le_bytes(data.get(0..4)?.try_into().ok()?) as usize;
    let mut anim = Anim::default();
    let mut pos = 4;
    for _ in 0..n {
        let d = u16::from_le_bytes(data.get(pos..pos + 2)?.try_into().ok()?);
        let f: Frame = data.get(pos + 2..pos + 2 + FB)?.try_into().ok()?;
        anim.delays_ms.push(d);
        anim.frames.push(f);
        pos += 2 + FB;
    }
    (!anim.frames.is_empty()).then_some(anim)
}

/// Кадр як RGBA-картинка: світлі пікселі на чорному, кожен — квадрат `scale`×`scale`.
pub fn to_rgba(f: &Frame, scale: u32) -> RgbaImage {
    RgbaImage::from_fn(W as u32 * scale, H as u32 * scale, |x, y| {
        if get_px(f, (x / scale) as usize, (y / scale) as usize) {
            image::Rgba([225, 240, 255, 255])
        } else {
            image::Rgba([12, 14, 18, 255])
        }
    })
}

/// Зберігає один кадр у PNG або анімацію в GIF (за розширенням файлу).
pub fn save_image(path: &Path, anim: &Anim, scale: u32) -> Result<(), String> {
    let is_gif = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("gif"));
    if !is_gif {
        let f = anim.frames.first().ok_or("немає кадрів")?;
        return to_rgba(f, scale).save(path).map_err(|e| e.to_string());
    }
    use image::codecs::gif::{GifEncoder, Repeat};
    let file = File::create(path).map_err(|e| e.to_string())?;
    let mut enc = GifEncoder::new_with_speed(std::io::BufWriter::new(file), 10);
    enc.set_repeat(Repeat::Infinite).map_err(|e| e.to_string())?;
    for (f, d) in anim.frames.iter().zip(&anim.delays_ms) {
        let delay = image::Delay::from_numer_denom_ms(*d as u32, 1);
        enc.encode_frame(image::Frame::from_parts(to_rgba(f, scale), 0, 0, delay))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
