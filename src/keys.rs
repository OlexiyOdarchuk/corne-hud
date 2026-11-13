//! Натискання клавіш для анімацій: лише факт і час натискання, без кодів клавіш.
//!
//! Читає `/dev/input/*-event-kbd` (потрібна група `input` або правило udev з packaging/).

use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// Ще шукаємо клавіатури.
    Searching,
    Ok,
    /// Клавіатури є, але читати їх не дозволено.
    Denied,
    NoKeyboard,
}

struct Watcher {
    presses: AtomicU64,
    access: AtomicU8,
    /// Час останніх натискань — для швидкості друку.
    recent: Mutex<Vec<Instant>>,
    open: Mutex<Vec<PathBuf>>,
}

fn watcher() -> &'static Watcher {
    static W: OnceLock<Watcher> = OnceLock::new();
    W.get_or_init(|| {
        std::thread::spawn(scan_loop);
        Watcher {
            presses: AtomicU64::new(0),
            access: AtomicU8::new(0),
            recent: Mutex::new(Vec::new()),
            open: Mutex::new(Vec::new()),
        }
    })
}

/// Скільки всього натискань від запуску.
pub fn presses() -> u64 {
    watcher().presses.load(Ordering::Relaxed)
}

pub fn access() -> Access {
    match watcher().access.load(Ordering::Relaxed) {
        1 => Access::Ok,
        2 => Access::Denied,
        3 => Access::NoKeyboard,
        _ => Access::Searching,
    }
}

/// Натискань за останні 10 секунд, перераховані на хвилину.
pub fn per_minute() -> u32 {
    let mut r = watcher().recent.lock().unwrap();
    let edge = Instant::now() - Duration::from_secs(10);
    r.retain(|t| *t >= edge);
    r.len() as u32 * 6
}

fn keyboards() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for dir in ["/dev/input/by-id", "/dev/input/by-path"] {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with("-event-kbd") {
                if let Ok(real) = std::fs::canonicalize(e.path()) {
                    if !out.contains(&real) {
                        out.push(real);
                    }
                }
            }
        }
    }
    out
}

fn scan_loop() {
    loop {
        let w = watcher();
        let found = keyboards();
        let mut denied = false;
        for path in &found {
            if w.open.lock().unwrap().contains(path) {
                continue;
            }
            match File::open(path) {
                Ok(file) => {
                    w.open.lock().unwrap().push(path.clone());
                    let p = path.clone();
                    std::thread::spawn(move || read_events(file, p));
                }
                Err(_) => denied = true,
            }
        }
        let any_open = !w.open.lock().unwrap().is_empty();
        let state = if any_open {
            1
        } else if denied {
            2
        } else {
            3
        };
        w.access.store(state, Ordering::Relaxed);
        std::thread::sleep(Duration::from_secs(3));
    }
}

fn read_events(mut file: File, path: PathBuf) {
    // struct input_event на 64-бітних системах: timeval (16) + type (2) + code (2) + value (4).
    const SIZE: usize = 24;
    let w = watcher();
    let mut buf = [0u8; SIZE * 32];
    while let Ok(n) = file.read(&mut buf) {
        if n == 0 {
            break;
        }
        for ev in buf[..n].as_chunks::<SIZE>().0 {
            let kind = u16::from_ne_bytes([ev[16], ev[17]]);
            let value = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
            if kind == 1 && value == 1 {
                w.presses.fetch_add(1, Ordering::Relaxed);
                let mut r = w.recent.lock().unwrap();
                r.push(Instant::now());
                if r.len() > 512 {
                    let edge = Instant::now() - Duration::from_secs(10);
                    r.retain(|t| *t >= edge);
                }
            }
        }
    }
    // Клавіатуру від'єднали — дозволити відкрити її знову.
    w.open.lock().unwrap().retain(|p| *p != path);
}
