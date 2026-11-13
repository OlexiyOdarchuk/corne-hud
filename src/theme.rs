//! Оформлення вікна: палітра, шрифти, картки, кнопки, вкладки.

use std::sync::Arc;

use eframe::egui::{
    self, Button, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Response, RichText,
    Stroke, TextStyle, Vec2,
};

pub use egui_phosphor::regular as icon;

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const BG: Color32 = rgb(0x10, 0x12, 0x17);
pub const SURFACE: Color32 = rgb(0x18, 0x1b, 0x22);
pub const SURFACE2: Color32 = rgb(0x22, 0x26, 0x2f);
pub const HOVER: Color32 = rgb(0x2c, 0x31, 0x3c);
pub const PRESSED: Color32 = rgb(0x35, 0x3b, 0x48);
pub const BORDER: Color32 = rgb(0x29, 0x2e, 0x38);
pub const TEXT: Color32 = rgb(0xe7, 0xe9, 0xef);
pub const WEAK: Color32 = rgb(0x8d, 0x94, 0xa3);
pub const ACCENT: Color32 = rgb(0xf3, 0xa8, 0x47);
pub const ON_ACCENT: Color32 = rgb(0x1f, 0x15, 0x05);
pub const GOOD: Color32 = rgb(0x5f, 0xd0, 0x8e);
pub const WARN: Color32 = rgb(0xec, 0xb4, 0x4c);
pub const BAD: Color32 = rgb(0xec, 0x6d, 0x72);

const BOLD: &str = "bold";

/// Файл системного шрифту з кирилицею потрібної товщини (через fontconfig).
fn system_font(weight: &str) -> Option<Vec<u8>> {
    let pattern = format!("Inter,Noto Sans,DejaVu Sans,sans-serif:lang=uk:variable=false:weight={weight}");
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", &pattern])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&out.stdout).into_owned();
    if path.ends_with(".ttc") {
        return None;
    }
    std::fs::read(path.trim()).ok()
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    if let Some(data) = system_font("regular") {
        fonts
            .font_data
            .insert("ui".into(), Arc::new(FontData::from_owned(data)));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "ui".into());
    }
    let mut bold = fallback;
    if let Some(data) = system_font("semibold") {
        fonts
            .font_data
            .insert("ui-bold".into(), Arc::new(FontData::from_owned(data)));
        bold.insert(0, "ui-bold".into());
    } else if fonts.font_data.contains_key("ui") {
        bold.insert(0, "ui".into());
    }
    fonts.families.insert(FontFamily::Name(BOLD.into()), bold);
    fonts
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD.into()))
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.text_styles = [
            (TextStyle::Heading, bold(18.0)),
            (TextStyle::Body, FontId::proportional(14.5)),
            (TextStyle::Button, FontId::proportional(14.5)),
            (TextStyle::Small, FontId::proportional(12.0)),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();

        let sp = &mut s.spacing;
        sp.item_spacing = Vec2::new(8.0, 8.0);
        sp.button_padding = Vec2::new(11.0, 5.0);
        sp.interact_size = Vec2::new(40.0, 28.0);
        sp.slider_width = 150.0;
        sp.combo_width = 220.0;
        sp.icon_width = 16.0;
        sp.icon_spacing = 6.0;
        sp.menu_margin = Margin::same(6);

        let v = &mut s.visuals;
        v.panel_fill = BG;
        v.window_fill = SURFACE;
        v.window_stroke = Stroke::new(1.0_f32, BORDER);
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        v.faint_bg_color = SURFACE;
        v.extreme_bg_color = rgb(0x0c, 0x0e, 0x12);
        v.code_bg_color = rgb(0x0c, 0x0e, 0x12);
        v.hyperlink_color = ACCENT;
        v.warn_fg_color = WARN;
        v.error_fg_color = BAD;
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = Stroke::new(1.0_f32, ON_ACCENT);
        v.text_cursor.stroke = Stroke::new(2.0_f32, ACCENT);
        v.slider_trailing_fill = true;

        let r = CornerRadius::same(6);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = SURFACE;
        w.noninteractive.weak_bg_fill = SURFACE;
        w.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
        w.noninteractive.corner_radius = r;
        for (wv, fill, stroke) in [
            (&mut w.inactive, SURFACE2, Stroke::new(1.0_f32, rgb(0x3a, 0x40, 0x4e))),
            (&mut w.hovered, HOVER, Stroke::new(1.0_f32, rgb(0x3b, 0x42, 0x50))),
            (&mut w.active, PRESSED, Stroke::new(1.0_f32, ACCENT)),
            (&mut w.open, HOVER, Stroke::new(1.0_f32, rgb(0x3b, 0x42, 0x50))),
        ] {
            wv.bg_fill = fill;
            wv.weak_bg_fill = fill;
            wv.bg_stroke = stroke;
            wv.fg_stroke = Stroke::new(1.5_f32, TEXT);
            wv.corner_radius = r;
            wv.expansion = 0.0;
        }
    });
}

/// Картка: заокруглена панель із рамкою, на всю ширину.
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Заголовок картки зі значком і підказкою під ним.
pub fn title(ui: &mut egui::Ui, glyph: &str, text: &str, hint: Option<&str>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(glyph).size(19.0).color(ACCENT));
        ui.label(RichText::new(text).font(bold(16.0)).color(TEXT));
    });
    if let Some(h) = hint {
        ui.label(RichText::new(h).color(WEAK).size(13.0));
    }
    ui.add_space(4.0);
}

/// Підпис невеликої групи всередині картки.
pub fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).font(bold(11.5)).color(WEAK));
}

/// Головна кнопка дії: заповнена акцентним кольором.
pub fn primary(ui: &mut egui::Ui, enabled: bool, text: impl Into<String>) -> Response {
    let text = RichText::new(text.into())
        .color(if enabled { ON_ACCENT } else { WEAK })
        .font(bold(14.0));
    ui.add_enabled(
        enabled,
        Button::new(text)
            .fill(if enabled { ACCENT } else { SURFACE2 })
            .min_size(Vec2::new(0.0, 30.0)),
    )
}

/// Кнопка-перемикач: виділена акцентом, коли вибрана.
pub fn toggle(ui: &mut egui::Ui, selected: bool, text: impl Into<String>) -> Response {
    let text = text.into();
    let rich = if selected {
        RichText::new(text).color(ON_ACCENT)
    } else {
        RichText::new(text).color(TEXT)
    };
    ui.add(Button::new(rich).fill(if selected { ACCENT } else { SURFACE2 }))
}

/// Значок і текст разом.
pub fn with_icon(glyph: &str, text: &str) -> String {
    format!("{glyph}  {text}")
}

/// Плашка стану з кольоровою крапкою. Розмір — за текстом, тож працює в будь-якому розташуванні.
pub fn status_pill(ui: &mut egui::Ui, color: Color32, text: &str) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), FontId::proportional(13.5), TEXT);
    let size = Vec2::new(galley.size().x + 40.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(99), SURFACE2);
    p.circle_filled(egui::pos2(rect.left() + 17.0, rect.center().y), 4.5, color);
    p.galley(
        egui::pos2(rect.left() + 28.0, rect.center().y - galley.size().y / 2.0),
        galley,
        TEXT,
    );
    resp
}

/// Рядок списку на всю ширину: значок і текст зліва, вибраний підсвічено акцентом.
pub fn row(ui: &mut egui::Ui, selected: bool, glyph: &str, text: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), egui::Sense::click());
    let (fill, fg) = if selected {
        (ACCENT, ON_ACCENT)
    } else if resp.hovered() {
        (HOVER, TEXT)
    } else {
        (Color32::TRANSPARENT, TEXT)
    };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(8), fill);
    let y = rect.center().y;
    p.text(
        egui::pos2(rect.left() + 12.0, y),
        egui::Align2::LEFT_CENTER,
        glyph,
        FontId::proportional(16.0),
        if selected { ON_ACCENT } else { ACCENT },
    );
    p.text(
        egui::pos2(rect.left() + 36.0, y),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::proportional(14.5),
        fg,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Рамка «екрана» навколо перегляду кадру.
pub fn bezel<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(rgb(0x05, 0x06, 0x08))
        .stroke(Stroke::new(1.0_f32, rgb(0x33, 0x38, 0x44)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(6))
        .show(ui, add)
        .inner
}
