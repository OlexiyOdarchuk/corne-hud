//! Фонова служба: тримає зв'язок із клавіатурою, шле годинник/CPU/RAM/музику,
//! програє картинки й анімації, приймає кадри потоку і команди через сокет.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Timelike;

use crate::device::{self, Device, Packet};
use crate::ipc::{self, Request, Response, Screen, Settings, Status, Target};
use crate::oled::{self, Anim, Frame};
use crate::pixfont;
use crate::presets::{self, Gen};
use crate::sysinfo;

#[allow(clippy::large_enum_variant)] // два слоти, розмір не важливий
enum Content {
    None,
    Anim {
        anim: Anim,
        idx: usize,
        next_at: Instant,
    },
    Stream {
        frame: Frame,
        fresh: bool,
    },
    Live {
        gen: Box<dyn Gen>,
        id: String,
        start: Instant,
        next_at: Instant,
    },
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedPreset {
    id: String,
    text: String,
}

struct Slot {
    content: Content,
    last_sent: Option<Frame>,
}

impl Slot {
    fn describe(&self) -> String {
        match &self.content {
            Content::None => "немає".into(),
            Content::Anim { anim, .. } if anim.frames.len() == 1 => "картинка".into(),
            Content::Anim { anim, .. } => format!("анімація, кадрів: {}", anim.frames.len()),
            Content::Stream { .. } => "потік кадрів".into(),
            Content::Live { id, .. } => format!("заготовка «{}»", presets::find(id).map_or(id.as_str(), |p| p.label)),
        }
    }
}

struct State {
    slots: [Slot; 2],
    pending: VecDeque<Packet>,
    status: Status,
    flash: Option<Flash>,
    /// Коли закінчився останній показ: відповіді клавіатури, надіслані під час показу, застарілі.
    flash_ended: Option<Instant>,
    layout: Arc<Mutex<sysinfo::Layout>>,
}

/// Короткий показ розкладки поверх екранів.
struct Flash {
    frame: Frame,
    until: Instant,
    /// Половини, на яких іде показ, і режим, до якого повернутись (None — уже була «картинка»).
    slots: Vec<(usize, Option<Screen>)>,
}

const FLASH_TIME: Duration = Duration::from_millis(800);

fn settings_file() -> PathBuf {
    config_dir().join("settings.json")
}

fn load_settings() -> Settings {
    std::fs::read_to_string(settings_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Кадр із назвою розкладки великими літерами в рамці.
fn layout_frame(name: &str) -> Frame {
    let mut f = [0u8; oled::FB];
    let scale = if pixfont::width(name, 4) <= 120 { 4 } else { 2 };
    let x = (oled::W as i32 - pixfont::width(name, scale)) / 2;
    let y = (oled::H as i32 - 5 * scale) / 2;
    pixfont::draw(&mut f, x, y, name, scale, true);
    crate::draw::rect(&mut f, (0, 0), (oled::W as i32 - 1, oled::H as i32 - 1), false, 1, true);
    f
}

pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("corne-hud")
}

fn slot_file(i: usize) -> PathBuf {
    config_dir().join(["main.bin", "side.bin"][i])
}

fn preset_file(i: usize) -> PathBuf {
    config_dir().join(["main.preset.json", "side.preset.json"][i])
}

fn live(id: &str, text: &str) -> Option<Content> {
    let now = Instant::now();
    Some(Content::Live {
        gen: presets::make(id, text)?,
        id: id.to_string(),
        start: now,
        next_at: now,
    })
}

/// Друга половина отримує кадри через вузький канал між половинами — їй менше кадрів.
const MAX_FPS: [u32; 2] = [20, 10];

const MASKS: [u8; 2] = [1, 2];

pub fn run() -> Result<(), String> {
    let path = ipc::socket_path();
    if UnixStream::connect(&path).is_ok() {
        // Не помилка: інша копія вже обслуговує клавіатуру (наприклад, її запустило вікно).
        eprintln!("corne-hud: служба вже запущена");
        return Ok(());
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).map_err(|e| format!("{}: {e}", path.display()))?;

    let restore = |i: usize| {
        let saved: Option<SavedPreset> = std::fs::read_to_string(preset_file(i))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok());
        if let Some(c) = saved.and_then(|p| live(&p.id, &p.text)) {
            return c;
        }
        match oled::load_anim(&slot_file(i)) {
            Some(anim) => Content::Anim {
                anim,
                idx: 0,
                next_at: Instant::now(),
            },
            None => Content::None,
        }
    };
    let layout = sysinfo::watch_layout();
    let state = Arc::new(Mutex::new(State {
        slots: [
            Slot {
                content: restore(0),
                last_sent: None,
            },
            Slot {
                content: restore(1),
                last_sent: None,
            },
        ],
        pending: VecDeque::new(),
        status: Status {
            settings: load_settings(),
            ..Status::default()
        },
        flash: None,
        flash_ended: None,
        layout: layout.clone(),
    }));

    {
        let state = state.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                let state = state.clone();
                std::thread::spawn(move || serve(conn, state));
            }
        });
    }

    eprintln!("corne-hud: служба запущена, сокет {}", path.display());
    let music = sysinfo::watch_music();
    let mut layout_seen: Option<u64> = None;
    let mut cpu = sysinfo::Cpu::default();
    let mut dev: Option<Device> = None;
    let mut next_try = Instant::now();
    let mut next_info = Instant::now();
    let mut next_hello = Instant::now();
    let mut text_sent: Option<(String, Instant)> = None;
    let mut paused = false;
    let mut next_check = Instant::now();

    loop {
        let now = Instant::now();
        if dev.is_none() && now >= next_try {
            next_try = now + Duration::from_secs(2);
            // Якщо Vial уже відкрив клавіатуру, навіть HELLO не шлемо — одразу пауза.
            let opened = Device::open().and_then(|mut d| {
                if d.held_by_other() {
                    return Ok((d, None));
                }
                d.hello().map(|h| (d, Some(h)))
            });
            match opened {
                Ok((d, h)) => {
                    eprintln!("corne-hud: клавіатуру підключено");
                    let mut st = state.lock().unwrap();
                    paused = h.is_none();
                    st.status.paused = paused;
                    if let Some(h) = h {
                        apply_hello(&mut st.status, h);
                    }
                    st.status.keyboard = true;
                    st.status.error.clear();
                    for s in &mut st.slots {
                        s.last_sent = None;
                        match &mut s.content {
                            Content::Anim { next_at, .. } => *next_at = now,
                            Content::Stream { fresh, .. } => *fresh = true,
                            Content::Live { next_at, .. } => *next_at = now,
                            Content::None => {}
                        }
                    }
                    text_sent = None;
                    next_info = now;
                    dev = Some(d);
                }
                Err(e) => {
                    let mut st = state.lock().unwrap();
                    st.status.keyboard = false;
                    st.status.error = e;
                }
            }
        }

        // Поки клавіатуру тримає інша програма, мовчимо: клавіатура відповідає на кожен
        // пакет усім, хто її відкрив, і Vial прийняв би чужу відповідь за свою.
        if let Some(d) = dev.as_mut() {
            if d.opened_by_other() && !paused {
                paused = true;
                next_check = now + Duration::from_secs(1);
                state.lock().unwrap().status.paused = true;
                eprintln!("corne-hud: клавіатуру відкрила інша програма, пауза");
            }
            if paused && now >= next_check {
                next_check = now + Duration::from_secs(1);
                if !d.held_by_other() {
                    paused = false;
                    next_info = now;
                    next_hello = now;
                    text_sent = None;
                    state.lock().unwrap().status.paused = false;
                    eprintln!("corne-hud: інша програма закрила клавіатуру, продовжуємо");
                }
            }
        }

        if let Some(d) = dev.as_mut().filter(|_| !paused) {
            let res = tick(
                d,
                &state,
                now,
                &mut next_info,
                &mut next_hello,
                &mut text_sent,
                &mut cpu,
                &music,
                &layout,
                &mut layout_seen,
            );
            if let Err(e) = res {
                eprintln!("corne-hud: зв'язок втрачено: {e}");
                let mut st = state.lock().unwrap();
                st.status.keyboard = false;
                st.status.error = e.to_string();
                dev = None;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn apply_hello(st: &mut Status, h: device::Hello) {
    st.main_screen = Screen::from_index(h.main_screen);
    st.side_screen = Screen::from_index(h.side_screen);
    st.screens_off = h.screens_off;
    st.fw_proto = h.proto;
}

#[allow(clippy::too_many_arguments)]
fn tick(
    d: &mut Device,
    state: &Mutex<State>,
    now: Instant,
    next_info: &mut Instant,
    next_hello: &mut Instant,
    text_sent: &mut Option<(String, Instant)>,
    cpu: &mut sysinfo::Cpu,
    music: &Mutex<Option<sysinfo::Track>>,
    layout: &Mutex<sysinfo::Layout>,
    layout_seen: &mut Option<u64>,
) -> std::io::Result<()> {
    let mut st = state.lock().unwrap();
    let st = &mut *st;

    while let Some(p) = st.pending.pop_front() {
        d.send(&p)?;
    }

    // Режими можна міняти й клавішами — раз на 2 с питаємо клавіатуру.
    if let Some(h) = d.poll_hello() {
        let stale = st.flash.is_some() || st.flash_ended.is_some_and(|t| now - t < Duration::from_millis(500));
        if stale {
            st.status.fw_proto = h.proto;
        } else {
            apply_hello(&mut st.status, h);
        }
    }
    if now >= *next_hello {
        *next_hello = now + Duration::from_secs(2);
        d.send(&device::packet(device::HELLO, &[]))?;
    }

    if now >= *next_info {
        *next_info = now + Duration::from_secs(1);
        let t = chrono::Local::now();
        let track = music.lock().unwrap().clone();
        let icon = match &track {
            Some(tr) if tr.playing => 1,
            Some(_) => 2,
            None => 0,
        };
        let info = [
            t.hour() as u8,
            t.minute() as u8,
            cpu.sample(),
            sysinfo::ram_percent(),
            icon,
        ];
        d.send(&device::packet(device::INFO, &info))?;

        let text = match track {
            Some(tr) => tr.text,
            None => t.format("%a %d.%m.%Y").to_string(),
        };
        let stale = text_sent
            .as_ref()
            .is_none_or(|(s, at)| *s != text || at.elapsed() > Duration::from_secs(15));
        if stale {
            let mut buf = [0u8; device::TEXT_LEN];
            let bytes = text.as_bytes();
            let n = bytes.len().min(device::TEXT_LEN - 1);
            buf[..n].copy_from_slice(&bytes[..n]);
            for off in (0..device::TEXT_LEN).step_by(24) {
                let mut body = vec![off as u8, 24];
                body.extend_from_slice(&buf[off..off + 24]);
                d.send(&device::packet(device::TEXT, &body))?;
            }
            *text_sent = Some((text, now));
        }
    }

    // Розкладка змінилась — коротко показати її.
    let (name, seq) = {
        let l = layout.lock().unwrap();
        (l.name.clone(), l.seq)
    };
    st.status.layout = name.clone();
    if layout_seen.replace(seq).is_some_and(|old| old != seq) && st.status.settings.layout_flash && !name.is_empty() {
        start_flash(d, st, &name, now)?;
    }
    if st.flash.as_ref().is_some_and(|fl| now >= fl.until) {
        end_flash(d, st, now)?;
    }
    let flashing: Vec<usize> = st.flash.iter().flat_map(|fl| fl.slots.iter().map(|s| s.0)).collect();

    for (i, slot) in st.slots.iter_mut().enumerate() {
        if flashing.contains(&i) {
            continue;
        }
        let frame = match &mut slot.content {
            Content::Anim { anim, idx, next_at } if now >= *next_at => {
                let f = anim.frames[*idx];
                let delay = if anim.frames.len() == 1 {
                    3_600_000
                } else {
                    anim.delays_ms[*idx].max(oled::MIN_DELAY_MS) as u64
                };
                *idx = (*idx + 1) % anim.frames.len();
                *next_at = now + Duration::from_millis(delay);
                Some(f)
            }
            Content::Stream { frame, fresh } if *fresh => {
                *fresh = false;
                Some(*frame)
            }
            Content::Live {
                gen, start, next_at, ..
            } if now >= *next_at => {
                let fps = gen.fps().clamp(1, MAX_FPS[i]);
                *next_at += Duration::from_secs_f32(1.0 / fps as f32);
                if *next_at < now {
                    *next_at = now; // не наздоганяти пропущені кадри
                }
                let mut f = [0u8; oled::FB];
                gen.frame((now - *start).as_secs_f32(), &mut f);
                Some(f)
            }
            _ => None,
        };
        if let Some(f) = frame {
            d.send_frame(MASKS[i], &f, slot.last_sent.as_ref())?;
            slot.last_sent = Some(f);
        }
    }
    Ok(())
}

fn quiet_screen_packet(i: usize, screen: Screen) -> Packet {
    device::packet(device::SET_SCREEN, &[MASKS[i], screen.index(), 0, 1])
}

fn start_flash(d: &mut Device, st: &mut State, name: &str, now: Instant) -> std::io::Result<()> {
    let frame = layout_frame(name);
    // Уже показуємо — лише оновити напис і продовжити.
    if let Some(fl) = st.flash.as_mut() {
        fl.frame = frame;
        fl.until = now + FLASH_TIME;
        for &(i, _) in &fl.slots {
            d.send_frame(MASKS[i], &frame, st.slots[i].last_sent.as_ref())?;
            st.slots[i].last_sent = Some(frame);
        }
        return Ok(());
    }
    let quiet = st.status.fw_proto >= 2;
    let mut slots = Vec::new();
    for &i in st.status.settings.layout_target.slots() {
        let screen = if i == 0 {
            st.status.main_screen
        } else {
            st.status.side_screen
        };
        let Some(screen) = screen else { continue };
        if screen == Screen::Image {
            slots.push((i, None));
        } else if quiet {
            // Стара прошивка показала б поверх назву режиму, тому лише з новою.
            slots.push((i, Some(screen)));
        }
    }
    for &(i, prev) in &slots {
        d.send_frame(MASKS[i], &frame, st.slots[i].last_sent.as_ref())?;
        st.slots[i].last_sent = Some(frame);
        if prev.is_some() {
            d.send(&quiet_screen_packet(i, Screen::Image))?;
        }
    }
    if !slots.is_empty() {
        st.flash = Some(Flash {
            frame,
            until: now + FLASH_TIME,
            slots,
        });
    }
    Ok(())
}

fn end_flash(d: &mut Device, st: &mut State, now: Instant) -> std::io::Result<()> {
    let Some(fl) = st.flash.take() else { return Ok(()) };
    st.flash_ended = Some(now);
    for (i, prev) in fl.slots {
        if let Some(screen) = prev {
            d.send(&quiet_screen_packet(i, screen))?;
            note_screen(&mut st.status, if i == 0 { Target::Main } else { Target::Side }, screen);
        }
        let slot = &mut st.slots[i];
        match &mut slot.content {
            Content::Anim { next_at, .. } | Content::Live { next_at, .. } => *next_at = now,
            Content::Stream { fresh, .. } => *fresh = true,
            Content::None => {
                // прибрати напис із пам'яті «картинки»
                d.send(&device::packet(device::CLEAR, &[MASKS[i]]))?;
                slot.last_sent = None;
            }
        }
    }
    Ok(())
}

fn serve(conn: UnixStream, state: Arc<Mutex<State>>) {
    let Ok(mut writer) = conn.try_clone() else {
        return;
    };
    for line in BufReader::new(conn).lines() {
        let Ok(line) = line else { break };
        let resp = match serde_json::from_str::<Request>(&line) {
            Ok(req) => handle(req, &state),
            Err(e) => Response::Error {
                message: format!("незрозумілий запит: {e}"),
            },
        };
        let mut out = serde_json::to_string(&resp).unwrap_or_default();
        out.push('\n');
        if writer.write_all(out.as_bytes()).is_err() {
            break;
        }
    }
}

fn set_screen_packet(target: Target, screen: Screen, save: bool) -> Packet {
    device::packet(device::SET_SCREEN, &[target.mask(), screen.index(), save as u8])
}

fn note_screen(st: &mut Status, target: Target, screen: Screen) {
    for &i in target.slots() {
        if i == 0 {
            st.main_screen = Some(screen);
        } else {
            st.side_screen = Some(screen);
        }
    }
}

fn handle(req: Request, state: &Mutex<State>) -> Response {
    let mut st = state.lock().unwrap();
    match req {
        Request::Settings { settings } => {
            let _ = std::fs::create_dir_all(config_dir());
            if let Err(e) = std::fs::write(settings_file(), serde_json::to_string(&settings).unwrap_or_default()) {
                eprintln!("corne-hud: не вдалося зберегти налаштування: {e}");
            }
            st.status.settings = settings;
        }
        Request::Status => {
            let mut s = st.status.clone();
            s.layout = st.layout.lock().unwrap().name.clone();
            s.main_content = st.slots[0].describe();
            s.side_content = st.slots[1].describe();
            return Response::Status(s);
        }
        Request::SetScreen { target, screen, save } => {
            st.pending.push_back(set_screen_packet(target, screen, save));
            note_screen(&mut st.status, target, screen);
        }
        Request::SetOff { off } => {
            st.pending.push_back(device::packet(device::SET_OFF, &[off as u8, 1]));
            st.status.screens_off = off;
        }
        Request::Show {
            target,
            frames,
            delays_ms,
            persist,
        } => {
            let frames: Result<Vec<Frame>, String> = frames.iter().map(|f| ipc::decode_frame(f)).collect();
            let frames = match frames {
                Ok(f) if !f.is_empty() => f,
                Ok(_) => {
                    return Response::Error {
                        message: "немає кадрів".into(),
                    }
                }
                Err(e) => return Response::Error { message: e },
            };
            let mut delays = delays_ms;
            delays.resize(frames.len(), 1000);
            let anim = Anim {
                frames,
                delays_ms: delays,
            };
            for &i in target.slots() {
                let _ = std::fs::remove_file(preset_file(i));
                if persist {
                    let _ = std::fs::create_dir_all(config_dir());
                    if let Err(e) = oled::save_anim(&slot_file(i), &anim) {
                        eprintln!("corne-hud: не вдалося зберегти: {e}");
                    }
                }
                st.slots[i].content = Content::Anim {
                    anim: anim.clone(),
                    idx: 0,
                    next_at: Instant::now(),
                };
            }
            st.pending.push_back(set_screen_packet(target, Screen::Image, true));
            note_screen(&mut st.status, target, Screen::Image);
        }
        Request::Frame { target, frame } => {
            let frame = match ipc::decode_frame(&frame) {
                Ok(f) => f,
                Err(e) => return Response::Error { message: e },
            };
            let mut switch = false;
            for &i in target.slots() {
                switch |= !matches!(st.slots[i].content, Content::Stream { .. });
                st.slots[i].content = Content::Stream { frame, fresh: true };
            }
            if switch {
                st.pending.push_back(set_screen_packet(target, Screen::Image, false));
                note_screen(&mut st.status, target, Screen::Image);
            }
        }
        Request::Clear { target } => {
            for &i in target.slots() {
                st.slots[i].content = Content::None;
                st.slots[i].last_sent = None;
                let _ = std::fs::remove_file(slot_file(i));
                let _ = std::fs::remove_file(preset_file(i));
            }
            st.pending.push_back(device::packet(device::CLEAR, &[target.mask()]));
        }
        Request::Preset { target, id, text } => {
            if presets::find(&id).is_none() {
                return Response::Error {
                    message: format!("немає заготовки «{id}»"),
                };
            }
            let _ = std::fs::create_dir_all(config_dir());
            let saved = serde_json::to_string(&SavedPreset {
                id: id.clone(),
                text: text.clone(),
            })
            .unwrap_or_default();
            for &i in target.slots() {
                st.slots[i].content = live(&id, &text).expect("заготовку перевірено вище");
                let _ = std::fs::write(preset_file(i), &saved);
                let _ = std::fs::remove_file(slot_file(i));
            }
            st.pending.push_back(set_screen_packet(target, Screen::Image, true));
            note_screen(&mut st.status, target, Screen::Image);
        }
    }
    Response::Ok
}
