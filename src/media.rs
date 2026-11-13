//! Демо-анімації для документації: сітки заготовок і ефектів у GIF.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Rgba, RgbaImage};

use crate::draw::{self, Effect};
use crate::oled::{get_px, Frame, FB, H, W};
use crate::presets::{self, PRESETS};

const SCALE: u32 = 2;
const PAD: u32 = 10;
const LABEL: u32 = 18;
const BG: Rgba<u8> = Rgba([28, 30, 36, 255]);

/// Малюнок-приклад: усмішка й напис.
pub fn sample() -> Frame {
    let mut f = [0u8; FB];
    draw::ellipse(&mut f, (3, 1), (31, 30), false, 2, true);
    draw::dab(&mut f, 12, 11, 3, true);
    draw::dab(&mut f, 22, 11, 3, true);
    draw::line(&mut f, (10, 19), (13, 23), 2, true);
    draw::line(&mut f, (13, 23), (21, 23), 2, true);
    draw::line(&mut f, (21, 23), (24, 19), 2, true);
    presets::draw_text(&mut f, "Corne", 42, 8, 22.0);
    f
}

struct Grid {
    cols: u32,
    rows: u32,
}

impl Grid {
    fn cell_w() -> u32 {
        W as u32 * SCALE + PAD
    }
    fn cell_h() -> u32 {
        H as u32 * SCALE + PAD + LABEL
    }
    fn size(&self) -> (u32, u32) {
        (self.cols * Self::cell_w() + PAD, self.rows * Self::cell_h() + PAD)
    }
    fn origin(&self, i: u32) -> (u32, u32) {
        (
            PAD + i % self.cols * Self::cell_w(),
            PAD + i / self.cols * Self::cell_h(),
        )
    }
}

fn blit(img: &mut RgbaImage, f: &Frame, (ox, oy): (u32, u32)) {
    for y in 0..H as u32 * SCALE {
        for x in 0..W as u32 * SCALE {
            let on = get_px(f, (x / SCALE) as usize, (y / SCALE) as usize);
            img.put_pixel(
                ox + x,
                oy + y,
                if on {
                    Rgba([225, 240, 255, 255])
                } else {
                    Rgba([8, 9, 12, 255])
                },
            );
        }
    }
}

/// Підпис під клітинкою, сірим текстом. Рядок ширший за кадр, тому малюється двома половинами.
fn label(img: &mut RgbaImage, text: &str, (ox, oy): (u32, u32)) {
    for half in 0..2u32 {
        let mut t = [0u8; FB];
        presets::draw_text(&mut t, text, -(half as i32) * W as i32, 2, 12.0);
        for y in 0..16usize {
            for x in 0..W {
                if get_px(&t, x, y) {
                    let px = ox + half * W as u32 + x as u32;
                    let py = oy + H as u32 * SCALE + 3 + y as u32;
                    if px < ox + W as u32 * SCALE && py < img.height() {
                        img.put_pixel(px, py, Rgba([170, 175, 190, 255]));
                    }
                }
            }
        }
    }
}

fn save_gif(path: &Path, frames: Vec<RgbaImage>, delay_ms: u32) -> Result<(), String> {
    let file = File::create(path).map_err(|e| e.to_string())?;
    let mut enc = GifEncoder::new_with_speed(BufWriter::new(file), 10);
    enc.set_repeat(Repeat::Infinite).map_err(|e| e.to_string())?;
    for img in frames {
        let frame = image::Frame::from_parts(img, 0, 0, Delay::from_numer_denom_ms(delay_ms, 1));
        enc.encode_frame(frame).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn render(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;

    // Заготовки: 3 стовпці, 4 секунди.
    let grid = Grid {
        cols: 3,
        rows: PRESETS.len().div_ceil(3) as u32,
    };
    let mut gens: Vec<_> = PRESETS
        .iter()
        .map(|p| presets::make(p.id, presets::DEFAULT_TEXT).unwrap())
        .collect();
    let (gw, gh) = grid.size();
    let mut frames = Vec::new();
    for k in 0..80 {
        let mut img = RgbaImage::from_pixel(gw, gh, BG);
        for (i, (p, g)) in PRESETS.iter().zip(&mut gens).enumerate() {
            let mut f = [0u8; FB];
            g.frame(k as f32 / 20.0, &mut f);
            let o = grid.origin(i as u32);
            blit(&mut img, &f, o);
            label(&mut img, p.label, o);
        }
        frames.push(img);
    }
    save_gif(&dir.join("presets.gif"), frames, 50)?;

    // Ефекти на малюнку-прикладі.
    let effects: Vec<Effect> = Effect::ALL.into_iter().filter(|e| *e != Effect::None).collect();
    let grid = Grid {
        cols: 3,
        rows: effects.len().div_ceil(3) as u32,
    };
    let (gw, gh) = grid.size();
    let src = sample();
    let mut frames = Vec::new();
    for k in 0..96 {
        let mut img = RgbaImage::from_pixel(gw, gh, BG);
        for (i, e) in effects.iter().enumerate() {
            let o = grid.origin(i as u32);
            blit(&mut img, &e.apply(&src, k), o);
            label(&mut img, e.label(), o);
        }
        frames.push(img);
    }
    save_gif(&dir.join("effects.gif"), frames, 70)?;

    Ok(())
}
