//! Вікно керування. Усе робить служба; вікно лише надсилає їй команди.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Vec2};

use crate::ipc::{self, Client, Request, Response, Screen, Settings, Status, Target};
use crate::oled::{self, Anim, ConvertOpts, Fit, Source};
use crate::paint::{self, frame_image, Painter};
use crate::presets::{self, Gen, PRESETS};
use crate::theme::{self, icon};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Screens,
    Draw,
    Picture,
}

/// `draw` — відкрити одразу вкладку малювання.
pub fn run(draw: bool) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(t!("app.title"))
            .with_app_id("io.github.OlexiyOdarchuk.CorneHud")
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../packaging/icons/corne-hud-256.png"))
                    .unwrap_or_default(),
            )
            .with_inner_size(shot_size().unwrap_or([840.0, 900.0]))
            .with_min_inner_size([620.0, 560.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "corne-hud",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(App::new(draw)))
        }),
    )
    .map_err(|e| e.to_string())
}

enum Action {
    SetScreen(Target, Screen),
    SetOff(bool),
    Pick,
    Send(Target),
    Clear(Target),
    Preset(Target, &'static str),
    Settings(Settings),
}

/// Живий перегляд однієї заготовки у вікні.
struct PresetView {
    gen: Box<dyn Gen>,
    tex: Option<egui::TextureHandle>,
}

struct App {
    client: Option<Client>,
    status: Option<Status>,
    last_poll: Option<Instant>,
    message: String,
    path: Option<PathBuf>,
    source: Option<Source>,
    opts: ConvertOpts,
    anim: Option<Anim>,
    preview: Option<egui::TextureHandle>,
    shown: Option<usize>,
    frame_idx: usize,
    frame_at: Instant,
    preset_text: String,
    preset_views: Vec<PresetView>,
    presets_start: Instant,
    presets_drawn: Option<Instant>,
    tab: Tab,
    painter: Painter,
    /// `CORNE_HUD_SCREENSHOT=file.png`: зняти вікно для документації й закритися.
    shot: Option<(PathBuf, u32)>,
}

/// Розмір вікна для знімка: `CORNE_HUD_SCREENSHOT_SIZE=840x1000`.
fn shot_size() -> Option<[f32; 2]> {
    let v = std::env::var("CORNE_HUD_SCREENSHOT_SIZE").ok()?;
    let (w, h) = v.split_once('x')?;
    Some([w.parse().ok()?, h.parse().ok()?])
}

fn make_views(text: &str) -> Vec<PresetView> {
    PRESETS
        .iter()
        .map(|p| PresetView {
            gen: presets::make(p.id, text).expect("known preset"),
            tex: None,
        })
        .collect()
}

impl App {
    fn new(draw: bool) -> Self {
        let (client, message) = match crate::ensure_daemon() {
            Ok(c) => (Some(c), String::new()),
            Err(e) => (None, e),
        };
        let text = presets::default_text();
        let shot = std::env::var_os("CORNE_HUD_SCREENSHOT").map(|p| (PathBuf::from(p), 0));
        Self {
            client,
            status: None,
            last_poll: None,
            message,
            path: None,
            source: None,
            opts: ConvertOpts::default(),
            anim: None,
            preview: None,
            shown: None,
            frame_idx: 0,
            frame_at: Instant::now(),
            preset_views: make_views(&text),
            preset_text: text,
            presets_start: Instant::now(),
            presets_drawn: None,
            tab: if draw { Tab::Draw } else { Tab::Screens },
            painter: if shot.is_some() {
                Painter::demo()
            } else {
                Painter::new()
            },
            shot,
        }
    }

    /// Оновлює живий перегляд заготовок ~12 разів на секунду.
    fn update_preset_views(&mut self, ctx: &egui::Context) -> Duration {
        let period = Duration::from_millis(80);
        if self.presets_drawn.is_some_and(|t| t.elapsed() < period) {
            return period.saturating_sub(self.presets_drawn.unwrap().elapsed());
        }
        self.presets_drawn = Some(Instant::now());
        let t = self.presets_start.elapsed().as_secs_f32();
        for (i, v) in self.preset_views.iter_mut().enumerate() {
            let mut f = [0u8; oled::FB];
            v.gen.frame(t, &mut f);
            let img = frame_image(&f);
            match &mut v.tex {
                Some(tex) => tex.set(img, egui::TextureOptions::NEAREST),
                None => v.tex = Some(ctx.load_texture(format!("preset{i}"), img, egui::TextureOptions::NEAREST)),
            }
        }
        period
    }

    fn call(&mut self, req: &Request) -> Option<Response> {
        if self.client.is_none() {
            match crate::ensure_daemon() {
                Ok(c) => self.client = Some(c),
                Err(e) => {
                    self.message = e;
                    return None;
                }
            }
        }
        match self.client.as_mut()?.call(req) {
            Ok(Response::Error { message }) => {
                self.message = message;
                None
            }
            Ok(r) => Some(r),
            Err(e) => {
                self.client = None;
                self.status = None;
                self.message = e;
                None
            }
        }
    }

    fn poll(&mut self) {
        if let Some(Response::Status(s)) = self.call(&Request::Status) {
            self.status = Some(s);
        }
    }

    fn load(&mut self, path: PathBuf) {
        match Source::open(&path) {
            Ok(src) => {
                self.source = Some(src);
                self.path = Some(path);
                self.message.clear();
                self.reconvert();
            }
            Err(e) => self.message = t!("msg.open_failed", err = e).into(),
        }
    }

    fn reconvert(&mut self) {
        self.anim = self.source.as_ref().map(|s| s.convert(&self.opts));
        self.frame_idx = 0;
        self.frame_at = Instant::now();
        self.shown = None;
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::SetScreen(t, s) => {
                self.call(&Request::SetScreen {
                    target: t,
                    screen: s,
                    save: true,
                });
            }
            Action::SetOff(off) => {
                self.call(&Request::SetOff { off });
            }
            Action::Pick => {
                let file = rfd::FileDialog::new()
                    .add_filter(t!("picture.images"), &["png", "jpg", "jpeg", "gif", "bmp", "webp"])
                    .pick_file();
                if let Some(p) = file {
                    self.load(p);
                }
            }
            Action::Send(t) => {
                if let Some(anim) = self.anim.clone() {
                    if self.call(&ipc::show_request(t, &anim, true)).is_some() {
                        self.message = t!("msg.sent").into();
                    }
                }
            }
            Action::Preset(t, id) => {
                let text = if presets::find(id).is_some_and(|p| p.uses_text) {
                    self.preset_text.clone()
                } else {
                    String::new()
                };
                if self
                    .call(&Request::Preset {
                        target: t,
                        id: id.to_string(),
                        text,
                    })
                    .is_some()
                {
                    let name = presets::find(id).map_or(id.to_string(), |p| p.label());
                    self.message = t!("msg.enabled", name = name).into();
                }
            }
            Action::Clear(t) => {
                if self.call(&Request::Clear { target: t }).is_some() {
                    self.message = t!("msg.cleared").into();
                }
            }
            Action::Settings(settings) => {
                if self.status.as_ref().is_none_or(|s| s.settings.lang != settings.lang) {
                    crate::i18n::apply(&settings.lang);
                    self.retranslate();
                }
                self.call(&Request::Settings { settings });
            }
        }
        self.poll();
    }

    /// Перемикає кадр попереднього перегляду, коли настав час. Повертає, коли наступний.
    fn update_preview(&mut self, ctx: &egui::Context) -> Option<Duration> {
        let anim = self.anim.as_ref()?;
        let mut wait = None;
        if anim.frames.len() > 1 {
            let delay = Duration::from_millis(anim.delays_ms[self.frame_idx].max(oled::MIN_DELAY_MS) as u64);
            if self.frame_at.elapsed() >= delay {
                self.frame_idx = (self.frame_idx + 1) % anim.frames.len();
                self.frame_at = Instant::now();
            }
            wait = Some(delay.saturating_sub(self.frame_at.elapsed()));
        }
        if self.shown != Some(self.frame_idx) || self.preview.is_none() {
            let img = frame_image(&anim.frames[self.frame_idx]);
            match &mut self.preview {
                Some(tex) => tex.set(img, egui::TextureOptions::NEAREST),
                None => self.preview = Some(ctx.load_texture("preview", img, egui::TextureOptions::NEAREST)),
            }
            self.shown = Some(self.frame_idx);
        }
        wait
    }
}

fn screen_icon(s: Screen) -> &'static str {
    match s {
        Screen::Status => icon::KEYBOARD,
        Screen::Ship => icon::ROCKET_LAUNCH,
        Screen::Pet => icon::CAT,
        Screen::Eq => icon::EQUALIZER,
        Screen::Pc => icon::CLOCK,
        Screen::Image => icon::IMAGE,
    }
}

fn targets() -> [(Target, String); 3] {
    [
        (Target::Main, t!("target.main").into()),
        (Target::Side, t!("target.side").into()),
        (Target::Both, t!("target.both").into()),
    ]
}

impl App {
    /// After a language switch: the default preset text follows the language if untouched.
    fn retranslate(&mut self) {
        let was_default = crate::i18n::LANGS
            .iter()
            .any(|(code, _)| self.preset_text == t!("presets.default_text", locale = code));
        if was_default {
            self.preset_text = presets::default_text();
            self.preset_views = make_views(&self.preset_text);
        }
    }

    fn lang_menu(&mut self, ui: &mut egui::Ui) -> Option<Settings> {
        let status = self.status.as_ref()?;
        let current = status.settings.lang.clone();
        let name = |code: &str| {
            crate::i18n::LANGS
                .iter()
                .find(|(c, _)| *c == code)
                .map_or_else(|| t!("lang.system").to_string(), |(_, n)| n.to_string())
        };
        let mut chosen = current.clone();
        egui::ComboBox::from_id_salt("lang")
            .width(120.0)
            .selected_text(theme::with_icon(icon::GLOBE, &name(&current)))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut chosen, String::new(), t!("lang.system"));
                for (code, n) in crate::i18n::LANGS {
                    ui.selectable_value(&mut chosen, code.to_string(), *n);
                }
            })
            .response
            .on_hover_text(t!("lang.label"));
        (chosen != current).then(|| Settings {
            lang: chosen,
            ..status.settings.clone()
        })
    }

    fn header(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon::KEYBOARD).size(24.0).color(theme::ACCENT));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("Corne").font(theme::bold(19.0)).color(theme::TEXT));
                ui.label(RichText::new(t!("app.subtitle")).size(12.5).color(theme::WEAK));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (color, text, hint) = match &self.status {
                    Some(s) if s.keyboard && s.paused => (
                        theme::WARN,
                        t!("status.paused").to_string(),
                        t!("status.paused_hint").to_string(),
                    ),
                    Some(s) if s.keyboard => (theme::GOOD, t!("status.connected").into(), String::new()),
                    Some(s) => (theme::WARN, t!("status.not_found").into(), s.error.clone()),
                    None => (
                        theme::BAD,
                        t!("status.no_daemon").into(),
                        t!("status.no_daemon_hint").into(),
                    ),
                };
                if let Some(set) = self.lang_menu(ui) {
                    actions.push(Action::Settings(set));
                }
                let r = theme::status_pill(ui, color, &text);
                if !hint.is_empty() {
                    r.on_hover_text(hint);
                }
            });
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            for (tab, glyph, label) in [
                (Tab::Screens, icon::MONITOR, t!("tab.screens")),
                (Tab::Draw, icon::PAINT_BRUSH, t!("tab.draw")),
                (Tab::Picture, icon::IMAGE, t!("tab.picture")),
            ] {
                if theme::toggle(ui, self.tab == tab, theme::with_icon(glyph, &label)).clicked() {
                    self.tab = tab;
                }
            }
        });
    }

    fn screens_tab(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let status = self.status.clone().unwrap_or_default();
        theme::card(ui, |ui| {
            theme::title(ui, icon::MONITOR, &t!("screens.title"), Some(&t!("screens.hint")));
            ui.columns(2, |cols| {
                let sides = [
                    (
                        Target::Main,
                        t!("target.main"),
                        status.main_screen,
                        &status.main_content,
                    ),
                    (
                        Target::Side,
                        t!("target.side"),
                        status.side_screen,
                        &status.side_content,
                    ),
                ];
                for (ui, (target, title, current, content)) in cols.iter_mut().zip(sides) {
                    theme::caption(ui, &title);
                    ui.spacing_mut().item_spacing.y = 4.0;
                    for s in Screen::ALL {
                        if theme::row(ui, current == Some(s), screen_icon(s), &s.label()).clicked() {
                            actions.push(Action::SetScreen(target, s));
                        }
                    }
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(t!("screens.content", content = content))
                            .size(12.5)
                            .color(theme::WEAK),
                    );
                }
            });
            ui.add_space(6.0);
            let off = status.screens_off;
            let label = if off { t!("screens.off_on") } else { t!("screens.off") };
            if theme::toggle(ui, off, theme::with_icon(icon::POWER, &label)).clicked() {
                actions.push(Action::SetOff(!off));
            }
        });

        ui.add_space(12.0);
        theme::card(ui, |ui| {
            theme::title(ui, icon::TRANSLATE, &t!("layout.title"), Some(&t!("layout.hint")));
            let mut set = status.settings.clone();
            ui.horizontal_wrapped(|ui| {
                let label = if set.layout_flash {
                    t!("layout.show")
                } else {
                    t!("layout.hide")
                };
                if theme::toggle(ui, set.layout_flash, theme::with_icon(icon::KEYBOARD, &label)).clicked() {
                    set.layout_flash = !set.layout_flash;
                }
                ui.add_space(8.0);
                ui.label(RichText::new(t!("target.on")).color(theme::WEAK));
                for (t, label) in targets() {
                    ui.add_enabled_ui(set.layout_flash, |ui| ui.radio_value(&mut set.layout_target, t, label));
                }
            });
            let now = if status.layout.is_empty() {
                t!("layout.unknown").to_string()
            } else {
                t!("layout.now", layout = status.layout).to_string()
            };
            ui.label(RichText::new(now).size(12.5).color(theme::WEAK));
            if status.keyboard && status.fw_proto == 1 {
                ui.label(
                    RichText::new(theme::with_icon(icon::WARNING_CIRCLE, &t!("layout.old_fw")))
                        .size(12.5)
                        .color(theme::WARN),
                );
            }
            if set != status.settings {
                actions.push(Action::Settings(set));
            }
        });

        ui.add_space(12.0);
        theme::card(ui, |ui| {
            theme::title(ui, icon::SPARKLE, &t!("presets.title"), Some(&t!("presets.hint")));
            ui.horizontal(|ui| {
                ui.label(t!("presets.text_label"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.preset_text)
                        .hint_text(t!("presets.text_hint"))
                        .desired_width(320.0),
                );
            });
            ui.add_space(6.0);
            // Плитки тягнуться на всю ширину: стільки стовпців, скільки влазить плиток від 230 px.
            let gap = 10.0;
            let avail = ui.available_width();
            let cols = ((avail + gap) / (230.0 + gap)).floor().max(1.0) as usize;
            let img_w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32 - 16.0).floor();
            let img = Vec2::new(img_w, (img_w / 4.0).round());
            egui::Grid::new("presets")
                .num_columns(cols)
                .spacing([gap, gap])
                .show(ui, |ui| {
                    for (i, (p, v)) in PRESETS.iter().zip(&self.preset_views).enumerate() {
                        egui::Frame::new()
                            .fill(theme::SURFACE2)
                            .corner_radius(10)
                            .inner_margin(8)
                            .show(ui, |ui| {
                                ui.set_width(img_w);
                                ui.vertical(|ui| {
                                    if let Some(tex) = &v.tex {
                                        ui.image((tex.id(), img));
                                    }
                                    ui.label(RichText::new(p.label()).color(theme::TEXT));
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;
                                        for (t, label, hint) in [
                                            (Target::Main, t!("target.main_short"), t!("target.main_hint")),
                                            (Target::Side, t!("target.side_short"), t!("target.side_hint")),
                                            (Target::Both, t!("target.both_short"), t!("target.both_hint")),
                                        ] {
                                            if ui.small_button(label).on_hover_text(hint).clicked() {
                                                actions.push(Action::Preset(t, p.id));
                                            }
                                        }
                                    });
                                });
                            });
                        if i % cols == cols - 1 {
                            ui.end_row();
                        }
                    }
                });
        });
    }

    fn picture_tab(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>, opts: &mut ConvertOpts) {
        theme::card(ui, |ui| {
            theme::title(ui, icon::IMAGE, &t!("picture.title"), Some(&t!("picture.hint")));
            let size = Vec2::new(oled::W as f32 * 4.0, oled::H as f32 * 4.0);
            let clicked = theme::bezel(ui, |ui| match &self.preview {
                Some(tex) if self.anim.is_some() => ui.image((tex.id(), size)).interact(egui::Sense::click()).clicked(),
                _ => {
                    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
                    let c = if resp.hovered() { theme::ACCENT } else { theme::WEAK };
                    ui.painter().text(
                        rect.center() - Vec2::new(0.0, 12.0),
                        egui::Align2::CENTER_CENTER,
                        icon::UPLOAD_SIMPLE,
                        egui::FontId::proportional(26.0),
                        c,
                    );
                    ui.painter().text(
                        rect.center() + Vec2::new(0.0, 18.0),
                        egui::Align2::CENTER_CENTER,
                        t!("picture.drop"),
                        egui::FontId::proportional(14.0),
                        c,
                    );
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    resp.clicked()
                }
            });
            if clicked {
                actions.push(Action::Pick);
            }
            ui.horizontal(|ui| {
                if ui
                    .button(theme::with_icon(icon::FOLDER_OPEN, &t!("picture.pick")))
                    .clicked()
                {
                    actions.push(Action::Pick);
                }
                if let Some(p) = &self.path {
                    let name = p
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let frames = self.anim.as_ref().map_or(0, |a| a.frames.len());
                    let extra = if frames > 1 {
                        t!("picture.frames", n = frames).to_string()
                    } else {
                        String::new()
                    };
                    ui.label(RichText::new(format!("{name}{extra}")).color(theme::WEAK));
                }
            });

            ui.add_space(8.0);
            theme::caption(ui, &t!("picture.fit"));
            ui.horizontal(|ui| {
                for (fit, label) in [
                    (Fit::Contain, t!("picture.contain")),
                    (Fit::Cover, t!("picture.cover")),
                    (Fit::Stretch, t!("picture.stretch")),
                ] {
                    if theme::toggle(ui, opts.fit == fit, label).clicked() {
                        opts.fit = fit;
                    }
                }
            });
            ui.add_space(4.0);
            theme::caption(ui, &t!("picture.processing"));
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut opts.dither, t!("picture.dither"));
                ui.add_enabled(
                    !opts.dither,
                    egui::Slider::new(&mut opts.threshold, 0..=255).text(t!("picture.threshold")),
                );
                ui.checkbox(&mut opts.invert, t!("picture.invert"));
            });

            ui.add_space(8.0);
            theme::caption(ui, &t!("picture.show_on"));
            ui.horizontal_wrapped(|ui| {
                let ready = self.anim.is_some();
                for (t, label) in targets() {
                    if theme::primary(ui, ready, label).clicked() {
                        actions.push(Action::Send(t));
                    }
                }
                ui.separator();
                if ui
                    .button(theme::with_icon(icon::TRASH, &t!("picture.clear")))
                    .on_hover_text(t!("picture.clear_hint"))
                    .clicked()
                {
                    actions.push(Action::Clear(Target::Both));
                }
            });
        });

        ui.add_space(12.0);
        theme::card(ui, |ui| {
            theme::title(ui, icon::TERMINAL, &t!("stream.title"), Some(&t!("stream.hint")));
            ui.add(
                egui::TextEdit::multiline(
                    &mut "ffmpeg -re -i video.mp4 -vf \"scale=128:32:force_original_aspect_ratio=decrease,\\\n  pad=128:32:(ow-iw)/2:(oh-ih)/2\" -f rawvideo -pix_fmt gray - \\\n  | corne-hud stream --gray --screen side",
                )
                .code_editor()
                .desired_width(f32::INFINITY),
            );
        });
    }
}

impl App {
    /// Режим знімка для документації: за ~2 с знімає власне вікно, зберігає PNG і закривається.
    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some((path, frames)) = &mut self.shot else { return };
        *frames += 1;
        if *frames == 40 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = image {
            let [w, h] = img.size;
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                if let Err(e) = buf.save(&*path) {
                    eprintln!("corne-hud: {}: {e}", path.display());
                }
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.screenshot(ctx);
        if self.last_poll.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
            self.last_poll = Some(Instant::now());
            self.poll();
        }
        let dropped: Option<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone()));
        if let Some(p) = dropped {
            self.load(p);
            self.tab = Tab::Picture;
        }
        let next_preview = if self.tab == Tab::Picture {
            self.update_preview(ctx)
        } else {
            None
        };
        let next_presets = if self.tab == Tab::Screens {
            Some(self.update_preset_views(ctx))
        } else {
            None
        };
        let text_before = self.preset_text.clone();

        let mut actions = Vec::new();
        let mut paint_out: paint::Out = Vec::new();
        let mut next_paint = None;
        let mut opts = self.opts;

        egui::TopBottomPanel::top("top")
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin {
                left: 16,
                right: 16,
                top: 12,
                bottom: 10,
            }))
            .show_separator_line(false)
            .show(ctx, |ui| self.header(ui, &mut actions));

        egui::TopBottomPanel::bottom("message")
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE2)
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show_separator_line(false)
            .show_animated(ctx, !self.message.is_empty(), |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon::INFO).color(theme::ACCENT).size(16.0));
                    ui.label(RichText::new(&self.message).color(theme::TEXT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(icon::X).on_hover_text(t!("msg.hide")).clicked() {
                            self.message.clear();
                        }
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin {
                left: 16,
                right: 10,
                top: 4,
                bottom: 0,
            }))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(6.0);
                    match self.tab {
                        Tab::Screens => self.screens_tab(ui, &mut actions),
                        Tab::Draw => next_paint = self.painter.ui(ui, &mut paint_out, &mut self.message),
                        Tab::Picture => self.picture_tab(ui, &mut actions, &mut opts),
                    }
                    ui.add_space(16.0);
                });
            });

        if self.preset_text != text_before {
            for (p, v) in PRESETS.iter().zip(self.preset_views.iter_mut()) {
                if p.uses_text {
                    v.gen = presets::make(p.id, &self.preset_text).expect("known preset");
                }
            }
        }
        if opts != self.opts {
            self.opts = opts;
            self.reconvert();
        }
        for a in actions {
            self.apply(a);
        }
        for (req, ok) in paint_out {
            if self.call(&req).is_some() {
                if let Some(m) = ok {
                    self.message = m;
                }
            }
        }

        let poll_in = Duration::from_secs(1).saturating_sub(self.last_poll.map_or(Duration::ZERO, |t| t.elapsed()));
        let wait = [next_preview, next_presets, next_paint]
            .into_iter()
            .flatten()
            .fold(poll_in, Duration::min);
        ctx.request_repaint_after(wait);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.shot.is_none() {
            self.painter.save();
        }
    }
}
