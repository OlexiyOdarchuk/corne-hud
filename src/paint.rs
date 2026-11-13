//! Вкладка «Малювання»: полотно 128×32, інструменти, кадри, ефекти й показ наживо.

use std::time::{Duration, Instant};

use eframe::egui::{
    self, Color32, Key, Modifiers, Pos2, Rect, RichText, Sense, Stroke as Line, TextureHandle, TextureOptions, Vec2,
};

use crate::draw::{self, Doc, Effect};
use crate::ipc::{self, Request, Target};
use crate::oled::{self, get_px, set_px, Anim, ConvertOpts, Frame, Source, FB, H, W};
use crate::presets;
use crate::theme::{self, icon};

const LIT: [u8; 3] = [225, 240, 255];
const DARK: [u8; 3] = [12, 14, 18];
const ONION: [u8; 3] = [45, 75, 120];
const GHOST: [u8; 3] = [110, 150, 200];
const MAX_IMPORT: usize = 300;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Pencil,
    Eraser,
    Line,
    Rect,
    Ellipse,
    Bucket,
    Text,
}

impl Tool {
    const ALL: [Tool; 7] = [
        Tool::Pencil,
        Tool::Eraser,
        Tool::Line,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Bucket,
        Tool::Text,
    ];

    fn label(self) -> &'static str {
        match self {
            Tool::Pencil => "Олівець",
            Tool::Eraser => "Гумка",
            Tool::Line => "Лінія",
            Tool::Rect => "Прямокутник",
            Tool::Ellipse => "Овал",
            Tool::Bucket => "Заливка",
            Tool::Text => "Текст",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Tool::Pencil => icon::PENCIL_SIMPLE,
            Tool::Eraser => icon::ERASER,
            Tool::Line => icon::LINE_SEGMENT,
            Tool::Rect => icon::RECTANGLE,
            Tool::Ellipse => icon::CIRCLE,
            Tool::Bucket => icon::PAINT_BUCKET,
            Tool::Text => icon::TEXT_T,
        }
    }

    fn key(self) -> Key {
        match self {
            Tool::Pencil => Key::B,
            Tool::Eraser => Key::E,
            Tool::Line => Key::L,
            Tool::Rect => Key::R,
            Tool::Ellipse => Key::O,
            Tool::Bucket => Key::F,
            Tool::Text => Key::T,
        }
    }

    fn hint(self) -> String {
        let what = match self {
            Tool::Pencil => "Малює від руки. Права кнопка миші стирає",
            Tool::Eraser => "Стирає. Права кнопка миші малює",
            Tool::Line => "Тягніть від початку до кінця лінії",
            Tool::Rect => "Тягніть від кута до кута",
            Tool::Ellipse => "Тягніть від кута до кута",
            Tool::Bucket => "Заливає замкнену область. Права кнопка заливає чорним",
            Tool::Text => "Клацніть, де має бути лівий верхній кут напису. Права кнопка вирізає напис",
        };
        format!("{what} (клавіша {:?})", self.key())
    }
}

/// Перетворення кадру для кнопок правки.
type FrameFn = fn(&Frame) -> Frame;

/// Зміна поточного кадру однією дією.
type FrameEdit = Box<dyn FnOnce(&Frame) -> Frame>;

/// Натиснута кнопка миші на полотні.
struct Stroke {
    on: bool,
    start: (i32, i32),
    last: (i32, i32),
    base: Frame,
}

pub struct Painter {
    doc: Doc,
    tool: Tool,
    size: i32,
    fill: bool,
    text: String,
    text_size: f32,
    onion: bool,
    grid: bool,
    live: bool,
    live_target: Target,
    live_sent: Option<Frame>,
    live_at: Instant,
    stroke: Option<Stroke>,
    canvas: Option<TextureHandle>,
    thumbs: Vec<(Frame, TextureHandle)>,
    anim: Anim,
    anim_key: Option<(Vec<Frame>, u16, Effect, u16)>,
    anim_tex: Option<TextureHandle>,
    anim_idx: usize,
    anim_at: Instant,
    dirty: bool,
    saved_at: Instant,
}

/// Що вкладка просить зробити службу: запит і повідомлення про успіх.
pub type Out = Vec<(Request, Option<String>)>;

impl Painter {
    pub fn new() -> Self {
        Self {
            doc: Doc::load(&Doc::path()).unwrap_or_default(),
            tool: Tool::Pencil,
            size: 1,
            fill: false,
            text: "Corne".into(),
            text_size: 16.0,
            onion: true,
            grid: true,
            live: false,
            live_target: Target::Side,
            live_sent: None,
            live_at: Instant::now(),
            stroke: None,
            canvas: None,
            thumbs: Vec::new(),
            anim: Anim::default(),
            anim_key: None,
            anim_tex: None,
            anim_idx: 0,
            anim_at: Instant::now(),
            dirty: false,
            saved_at: Instant::now(),
        }
    }

    pub fn save(&mut self) {
        if self.dirty && self.doc.save(&Doc::path()).is_ok() {
            self.dirty = false;
        }
        self.saved_at = Instant::now();
    }

    fn text_stamp(&self, (x, y): (i32, i32)) -> Frame {
        let mut t = [0u8; FB];
        presets::draw_text(&mut t, &self.text, x, y, self.text_size);
        t
    }

    fn begin(&mut self, p: (i32, i32), primary: bool) {
        let on = primary != (self.tool == Tool::Eraser);
        self.doc.checkpoint();
        self.stroke = Some(Stroke {
            on,
            start: p,
            last: p,
            base: *self.doc.frame(),
        });
        match self.tool {
            Tool::Bucket => draw::flood(self.doc.frame_mut(), p.0, p.1, on),
            Tool::Text => {
                let stamp = self.text_stamp(p);
                let f = self.doc.frame_mut();
                for y in 0..H {
                    for x in 0..W {
                        if get_px(&stamp, x, y) {
                            set_px(f, x, y, on);
                        }
                    }
                }
            }
            _ => self.drag(p),
        }
    }

    fn drag(&mut self, p: (i32, i32)) {
        let Some(s) = &mut self.stroke else { return };
        let (on, start, size, fill) = (s.on, s.start, self.size, self.fill);
        let f = &mut self.doc.frames[self.doc.cur];
        match self.tool {
            Tool::Pencil | Tool::Eraser => {
                draw::line(f, s.last, p, size, on);
                s.last = p;
            }
            Tool::Line => {
                *f = s.base;
                draw::line(f, start, p, size, on);
            }
            Tool::Rect => {
                *f = s.base;
                draw::rect(f, start, p, fill, size, on);
            }
            Tool::Ellipse => {
                *f = s.base;
                draw::ellipse(f, start, p, fill, size, on);
            }
            Tool::Bucket | Tool::Text => {}
        }
    }

    fn end(&mut self) {
        self.stroke = None;
        self.doc.drop_checkpoint_if_same();
        self.dirty = true;
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        ctx.input_mut(|i| {
            if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || i.consume_key(Modifiers::COMMAND, Key::Y)
            {
                self.doc.redo();
                self.dirty = true;
            } else if i.consume_key(Modifiers::COMMAND, Key::Z) {
                self.doc.undo();
                self.dirty = true;
            }
            for t in Tool::ALL {
                if i.consume_key(Modifiers::NONE, t.key()) {
                    self.tool = t;
                }
            }
        });
    }

    fn import(&mut self, msg: &mut String) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Зображення", &["png", "jpg", "jpeg", "gif", "bmp", "webp"])
            .pick_file()
        else {
            return;
        };
        match Source::open(&path) {
            Ok(src) => {
                let anim = src.convert(&ConvertOpts::default());
                self.doc.checkpoint();
                self.doc.frames = anim.frames.into_iter().take(MAX_IMPORT).collect();
                self.doc.cur = 0;
                if self.doc.frames.len() > 1 {
                    self.doc.delay_ms = anim.delays_ms[0].clamp(oled::MIN_DELAY_MS, 2000);
                }
                self.dirty = true;
                *msg = format!("Відкрито: {}", path.file_name().unwrap_or_default().to_string_lossy());
            }
            Err(e) => *msg = format!("Не вдалося відкрити файл: {e}"),
        }
    }

    fn export(&mut self, msg: &mut String) {
        let anim = self.doc.anim();
        let gif = anim.frames.len() > 1;
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(if gif { "corne-drawing.gif" } else { "corne-drawing.png" })
            .add_filter(if gif { "GIF" } else { "PNG" }, &[if gif { "gif" } else { "png" }])
            .save_file()
        else {
            return;
        };
        *msg = match oled::save_image(&path, &anim, 4) {
            Ok(()) => format!("Збережено: {}", path.display()),
            Err(e) => format!("Не вдалося зберегти: {e}"),
        };
    }

    fn canvas_image(&self, ghost: Option<&Frame>) -> egui::ColorImage {
        let prev = (self.onion && self.doc.cur > 0).then(|| &self.doc.frames[self.doc.cur - 1]);
        let f = self.doc.frame();
        let mut rgba = Vec::with_capacity(W * H * 4);
        for y in 0..H {
            for x in 0..W {
                let c = if get_px(f, x, y) {
                    LIT
                } else if ghost.is_some_and(|g| get_px(g, x, y)) {
                    GHOST
                } else if prev.is_some_and(|p| get_px(p, x, y)) {
                    ONION
                } else {
                    DARK
                };
                rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        egui::ColorImage::from_rgba_unmultiplied([W, H], &rgba)
    }

    /// Полотно з малюванням мишею.
    fn canvas_ui(&mut self, ui: &mut egui::Ui) {
        let scale = (ui.available_width() / W as f32).floor().clamp(3.0, 7.0);
        let (rect, resp) =
            ui.allocate_exact_size(Vec2::new(W as f32 * scale, H as f32 * scale), Sense::click_and_drag());
        let cell = |p: Pos2| {
            (
                (((p.x - rect.min.x) / scale).floor() as i32).clamp(-64, W as i32 + 64),
                (((p.y - rect.min.y) / scale).floor() as i32).clamp(-64, H as i32 + 64),
            )
        };
        let (pos, primary, secondary) = ui.input(|i| {
            (
                i.pointer.interact_pos().or(i.pointer.hover_pos()),
                i.pointer.primary_down(),
                i.pointer.secondary_down(),
            )
        });

        if self.stroke.is_none() && resp.is_pointer_button_down_on() && (primary || secondary) {
            if let Some(p) = pos {
                self.begin(cell(p), primary);
            }
        } else if self.stroke.is_some() {
            if primary || secondary {
                if let Some(p) = pos {
                    self.drag(cell(p));
                }
            } else {
                self.end();
            }
        }

        let hover = resp.hover_pos().map(cell);
        let ghost = match (self.tool, hover, &self.stroke) {
            (Tool::Text, Some(p), None) => Some(self.text_stamp(p)),
            _ => None,
        };
        let img = self.canvas_image(ghost.as_ref());
        let tex = match &mut self.canvas {
            Some(t) => {
                t.set(img, TextureOptions::NEAREST);
                t
            }
            None => self
                .canvas
                .insert(ui.ctx().load_texture("canvas", img, TextureOptions::NEAREST)),
        };
        let painter = ui.painter_at(rect);
        painter.image(
            tex.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        if self.grid && scale >= 5.0 {
            for x in 1..W {
                let a = if x % 8 == 0 { 26 } else { 9 };
                let px = rect.min.x + x as f32 * scale;
                painter.line_segment(
                    [Pos2::new(px, rect.min.y), Pos2::new(px, rect.max.y)],
                    Line::new(1.0_f32, Color32::from_white_alpha(a)),
                );
            }
            for y in 1..H {
                let a = if y % 8 == 0 { 26 } else { 9 };
                let py = rect.min.y + y as f32 * scale;
                painter.line_segment(
                    [Pos2::new(rect.min.x, py), Pos2::new(rect.max.x, py)],
                    Line::new(1.0_f32, Color32::from_white_alpha(a)),
                );
            }
        }
        if let (Some((x, y)), true) = (
            hover,
            matches!(
                self.tool,
                Tool::Pencil | Tool::Eraser | Tool::Line | Tool::Rect | Tool::Ellipse
            ),
        ) {
            let a = (self.size - 1) / 2;
            let min = Pos2::new(rect.min.x + (x - a) as f32 * scale, rect.min.y + (y - a) as f32 * scale);
            let r = Rect::from_min_size(min, Vec2::splat(self.size as f32 * scale));
            painter.rect_stroke(
                r,
                0.0,
                Line::new(1.0_f32, Color32::from_rgb(240, 170, 60)),
                egui::StrokeKind::Inside,
            );
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
    }

    fn frames_ui(&mut self, ui: &mut egui::Ui) {
        self.thumbs.truncate(self.doc.frames.len());
        for (i, f) in self.doc.frames.iter().enumerate() {
            let img = || frame_image(f);
            match self.thumbs.get_mut(i) {
                Some((old, tex)) if old != f => {
                    tex.set(img(), TextureOptions::NEAREST);
                    *old = *f;
                }
                Some(_) => {}
                None => self.thumbs.push((
                    *f,
                    ui.ctx()
                        .load_texture(format!("thumb{i}"), img(), TextureOptions::NEAREST),
                )),
            }
        }
        egui::ScrollArea::horizontal().id_salt("frames").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                for (i, (_, tex)) in self.thumbs.iter().enumerate() {
                    let current = i == self.doc.cur;
                    let resp = ui
                        .vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 3.0;
                            let img = ui.add(
                                egui::Image::new((tex.id(), Vec2::new(W as f32, H as f32)))
                                    .corner_radius(4)
                                    .sense(Sense::click()),
                            );
                            let color = if current { theme::ACCENT } else { theme::BORDER };
                            ui.painter().rect_stroke(
                                img.rect.expand(3.0),
                                6.0,
                                Line::new(if current { 2.0_f32 } else { 1.0 }, color),
                                egui::StrokeKind::Middle,
                            );
                            let n = RichText::new(format!("{}", i + 1)).size(12.0);
                            ui.label(if current {
                                n.color(theme::ACCENT)
                            } else {
                                n.color(theme::WEAK)
                            });
                            img
                        })
                        .inner;
                    if resp.clicked() {
                        self.doc.cur = i;
                    }
                }
            });
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button(theme::with_icon(icon::PLUS, "Новий кадр")).clicked() {
                self.doc.add_frame(false);
                self.dirty = true;
            }
            if ui
                .button(theme::with_icon(icon::COPY, "Копія кадру"))
                .on_hover_text("Копія поточного кадру: змініть у ній щось, і вийде рух")
                .clicked()
            {
                self.doc.add_frame(true);
                self.dirty = true;
            }
            if ui.button(theme::with_icon(icon::TRASH, "Видалити")).clicked() {
                self.doc.delete_frame();
                self.dirty = true;
            }
            let n = self.doc.frames.len();
            if ui
                .add_enabled(self.doc.cur > 0, egui::Button::new(icon::CARET_LEFT))
                .on_hover_text("Посунути кадр раніше")
                .clicked()
            {
                self.doc.move_frame(false);
                self.dirty = true;
            }
            if ui
                .add_enabled(self.doc.cur + 1 < n, egui::Button::new(icon::CARET_RIGHT))
                .on_hover_text("Посунути кадр пізніше")
                .clicked()
            {
                self.doc.move_frame(true);
                self.dirty = true;
            }
            ui.separator();
            ui.checkbox(&mut self.onion, "Тінь попереднього кадру")
                .on_hover_text("Видно, де що було кадром раніше: зручно малювати рух");
        });
    }

    /// Готова анімація (з ефектом): оновлює кадр перегляду. Повертає, коли наступний кадр.
    fn update_anim(&mut self, ctx: &egui::Context) -> Option<Duration> {
        let key = (
            self.doc.frames.clone(),
            self.doc.delay_ms,
            self.doc.effect,
            self.doc.effect_ms,
        );
        if self.stroke.is_none() && self.anim_key.as_ref() != Some(&key) {
            self.anim = self.doc.anim();
            self.anim_key = Some(key);
            self.anim_idx = 0;
            self.anim_at = Instant::now();
        }
        let n = self.anim.frames.len();
        if n == 0 {
            return None;
        }
        self.anim_idx %= n;
        let delay = Duration::from_millis(self.anim.delays_ms[self.anim_idx] as u64);
        if self.anim_at.elapsed() >= delay {
            self.anim_idx = (self.anim_idx + 1) % n;
            self.anim_at = Instant::now();
        }
        let img = frame_image(&self.anim.frames[self.anim_idx]);
        match &mut self.anim_tex {
            Some(t) => t.set(img, TextureOptions::NEAREST),
            None => self.anim_tex = Some(ctx.load_texture("anim", img, TextureOptions::NEAREST)),
        }
        (n > 1).then(|| delay.saturating_sub(self.anim_at.elapsed()))
    }

    fn tools_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for t in Tool::ALL {
                if theme::toggle(ui, self.tool == t, theme::with_icon(t.icon(), t.label()))
                    .on_hover_text(t.hint())
                    .clicked()
                {
                    self.tool = t;
                }
            }
        });
        ui.horizontal_wrapped(|ui| match self.tool {
            Tool::Text => {
                ui.label("Напис:");
                ui.add(egui::TextEdit::singleline(&mut self.text).desired_width(220.0));
                ui.add(
                    egui::Slider::new(&mut self.text_size, 8.0..=32.0)
                        .step_by(1.0)
                        .text("розмір"),
                );
            }
            Tool::Bucket => {
                ui.label(RichText::new("Клацніть усередині замкненої області").color(theme::WEAK));
            }
            _ => {
                ui.add(egui::Slider::new(&mut self.size, 1..=5).text("товщина"));
                if matches!(self.tool, Tool::Rect | Tool::Ellipse) {
                    ui.checkbox(&mut self.fill, "Залита фігура");
                }
            }
        });
    }

    fn edit_bar(&mut self, ui: &mut egui::Ui, msg: &mut String) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if ui
                .add_enabled(self.doc.can_undo(), egui::Button::new(icon::ARROW_COUNTER_CLOCKWISE))
                .on_hover_text("Відмінити (Ctrl+Z)")
                .clicked()
            {
                self.doc.undo();
                self.dirty = true;
            }
            if ui
                .add_enabled(self.doc.can_redo(), egui::Button::new(icon::ARROW_CLOCKWISE))
                .on_hover_text("Повторити (Ctrl+Shift+Z)")
                .clicked()
            {
                self.doc.redo();
                self.dirty = true;
            }
            ui.separator();
            let mut edit: Option<FrameEdit> = None;
            let buttons: [(&str, &str, FrameFn); 8] = [
                (icon::BROOM, "Очистити кадр", |_| [0; FB]),
                (icon::CIRCLE_HALF, "Інвертувати", draw::inverted),
                (
                    icon::FLIP_HORIZONTAL,
                    "Віддзеркалити: ліве стає правим",
                    |f| draw::flipped(f, true),
                ),
                (
                    icon::FLIP_VERTICAL,
                    "Перевернути: верх стає низом",
                    |f| draw::flipped(f, false),
                ),
                (
                    icon::ARROW_LEFT,
                    "Зсунути на піксель ліворуч",
                    |f| draw::shifted(f, -1, 0),
                ),
                (
                    icon::ARROW_RIGHT,
                    "Зсунути на піксель праворуч",
                    |f| draw::shifted(f, 1, 0),
                ),
                (icon::ARROW_UP, "Зсунути на піксель угору", |f| {
                    draw::shifted(f, 0, -1)
                }),
                (icon::ARROW_DOWN, "Зсунути на піксель униз", |f| {
                    draw::shifted(f, 0, 1)
                }),
            ];
            for (i, (glyph, hint, f)) in buttons.into_iter().enumerate() {
                if i == 4 {
                    ui.separator();
                }
                if ui.button(glyph).on_hover_text(hint).clicked() {
                    edit = Some(Box::new(f));
                }
            }
            if let Some(e) = edit {
                self.doc.edit(e);
                self.dirty = true;
            }
            ui.separator();
            if theme::toggle(ui, self.grid, icon::GRID_FOUR)
                .on_hover_text("Сітка")
                .clicked()
            {
                self.grid = !self.grid;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(icon::FILE_PLUS)
                    .on_hover_text("Новий малюнок (можна відмінити)")
                    .clicked()
                {
                    self.doc.checkpoint();
                    self.doc.frames = vec![[0; FB]];
                    self.doc.cur = 0;
                    self.dirty = true;
                }
                if ui
                    .button(icon::FLOPPY_DISK)
                    .on_hover_text("Зберегти як PNG або GIF")
                    .clicked()
                {
                    self.export(msg);
                }
                if ui
                    .button(icon::FOLDER_OPEN)
                    .on_hover_text("Відкрити картинку чи GIF як основу (можна відмінити)")
                    .clicked()
                {
                    self.import(msg);
                }
            });
        });
    }

    fn animation_ui(&mut self, ui: &mut egui::Ui, out: &mut Out) {
        let n = self.anim.frames.len();
        ui.horizontal(|ui| {
            theme::bezel(ui, |ui| {
                if let Some(tex) = &self.anim_tex {
                    ui.image((tex.id(), Vec2::new(W as f32 * 2.0, H as f32 * 2.0)));
                }
            });
            ui.add_space(6.0);
            ui.vertical(|ui| {
                let before = (self.doc.effect, self.doc.delay_ms, self.doc.effect_ms);
                ui.horizontal(|ui| {
                    ui.label("Ефект");
                    egui::ComboBox::from_id_salt("effect")
                        .selected_text(self.doc.effect.label())
                        .show_ui(ui, |ui| {
                            for e in Effect::ALL {
                                ui.selectable_value(&mut self.doc.effect, e, e.label());
                            }
                        });
                });
                if self.doc.effect != Effect::None {
                    let mut ms = self.doc.effect_ms as u32;
                    ui.add(
                        egui::Slider::new(&mut ms, 50..=500)
                            .step_by(50.0)
                            .text("мс на крок ефекту"),
                    );
                    self.doc.effect_ms = ms as u16;
                }
                if self.doc.frames.len() > 1 {
                    let mut ms = self.doc.delay_ms as u32;
                    ui.add(
                        egui::Slider::new(&mut ms, 50..=2000)
                            .step_by(50.0)
                            .text("мс на кадр малюнка"),
                    );
                    self.doc.delay_ms = ms as u16;
                }
                if before != (self.doc.effect, self.doc.delay_ms, self.doc.effect_ms) {
                    self.dirty = true;
                }
                let info = if n > 1 {
                    let total: u32 = self.anim.delays_ms.iter().map(|&d| d as u32).sum();
                    format!("Кадрів на екрані: {n} · цикл {:.1} с", total as f32 / 1000.0)
                } else {
                    "Нерухома картинка".to_string()
                };
                ui.label(RichText::new(info).size(12.5).color(theme::WEAK));
            });
        });

        ui.add_space(8.0);
        theme::caption(ui, "Показати на екрані");
        ui.horizontal_wrapped(|ui| {
            for (t, label) in [
                (Target::Main, "Половина з USB"),
                (Target::Side, "Друга половина"),
                (Target::Both, "Обидві"),
            ] {
                if theme::primary(ui, true, label).clicked() {
                    out.push((
                        ipc::show_request(t, &self.doc.anim(), true),
                        Some("Малюнок на екрані".into()),
                    ));
                    self.live = false;
                }
            }
        });

        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let live = theme::with_icon(icon::BROADCAST, "Малювати наживо");
            if theme::toggle(ui, self.live, live)
                .on_hover_text("Кожен штрих одразу з'являється на клавіатурі: поточний кадр, без ефекту")
                .clicked()
            {
                self.live = !self.live;
                self.live_sent = None;
            }
            ui.label(RichText::new("на").color(theme::WEAK));
            for (t, label) in [(Target::Main, "USB"), (Target::Side, "другу"), (Target::Both, "обидві")] {
                if ui.radio_value(&mut self.live_target, t, label).changed() {
                    self.live_sent = None;
                }
            }
        });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, out: &mut Out, msg: &mut String) -> Option<Duration> {
        self.shortcuts(ui.ctx());
        let next = self.update_anim(ui.ctx());

        theme::card(ui, |ui| {
            theme::title(
                ui,
                icon::PAINT_BRUSH,
                "Малювання",
                Some("Ліва кнопка миші малює, права стирає. Ctrl+Z відміняє, клавіші інструментів — у підказках."),
            );
            self.tools_ui(ui);
            ui.add_space(4.0);
            theme::bezel(ui, |ui| self.canvas_ui(ui));
            ui.add_space(4.0);
            self.edit_bar(ui, msg);
        });

        ui.add_space(12.0);
        theme::card(ui, |ui| {
            theme::title(
                ui,
                icon::FILM_STRIP,
                &format!("Кадри · {} з {}", self.doc.cur + 1, self.doc.frames.len()),
                Some("Щоб намалювати рух, зробіть копію кадру і змініть щось у ній."),
            );
            self.frames_ui(ui);
        });

        ui.add_space(12.0);
        theme::card(ui, |ui| {
            theme::title(
                ui,
                icon::MAGIC_WAND,
                "Анімація й показ",
                Some("Ефект оживляє навіть один кадр. Так малюнок виглядатиме на клавіатурі:"),
            );
            self.animation_ui(ui, out);
        });

        let mut wait = next;
        if self.live && self.live_sent.as_ref() != Some(self.doc.frame()) {
            let gap = Duration::from_millis(oled::MIN_DELAY_MS as u64);
            if self.live_at.elapsed() >= gap {
                let frame = *self.doc.frame();
                out.push((
                    Request::Frame {
                        target: self.live_target,
                        frame: ipc::encode_frame(&frame),
                    },
                    None,
                ));
                self.live_sent = Some(frame);
                self.live_at = Instant::now();
            } else {
                let left = gap.saturating_sub(self.live_at.elapsed());
                wait = Some(wait.map_or(left, |w| w.min(left)));
            }
        }

        if self.dirty && self.stroke.is_none() && self.saved_at.elapsed() >= Duration::from_secs(2) {
            self.save();
        }
        if self.dirty {
            wait = Some(wait.map_or(Duration::from_secs(2), |w| w.min(Duration::from_secs(2))));
        }
        wait
    }
}

impl Drop for Painter {
    fn drop(&mut self) {
        self.save();
    }
}

/// Кадр як на справжньому екрані.
pub fn frame_image(f: &Frame) -> egui::ColorImage {
    let mut rgba = Vec::with_capacity(W * H * 4);
    for y in 0..H {
        for x in 0..W {
            let c = if get_px(f, x, y) { LIT } else { DARK };
            rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([W, H], &rgba)
}
