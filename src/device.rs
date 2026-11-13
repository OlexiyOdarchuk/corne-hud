//! Зв'язок із клавіатурою через /dev/hidraw (Raw HID інтерфейс Vial, usage page 0xFF60).
//! Протокол — PROTOCOL.md.

use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use crate::oled::{Frame, FB};

const VID: u32 = 0x4653;
const PID: u32 = 0x0001;

pub const CMD: u8 = 0x70;
pub const PROTO: u8 = 1;
pub const IMAGE_CHUNK: usize = 28;
pub const IMAGE_CHUNKS: usize = FB.div_ceil(IMAGE_CHUNK);
pub const TEXT_LEN: usize = 48;

pub const HELLO: u8 = 0x01;
pub const INFO: u8 = 0x02;
pub const TEXT: u8 = 0x03;
pub const IMAGE: u8 = 0x04;
pub const COMMIT: u8 = 0x05;
pub const SET_SCREEN: u8 = 0x06;
pub const CLEAR: u8 = 0x07;
pub const SET_OFF: u8 = 0x08;

pub type Packet = [u8; 32];

pub fn packet(sub: u8, body: &[u8]) -> Packet {
    let mut p = [0u8; 32];
    p[0] = CMD;
    p[1] = sub;
    p[2..2 + body.len()].copy_from_slice(body);
    p
}

/// Шукає hidraw-вузол клавіатури саме з інтерфейсом Raw HID.
pub fn find() -> Option<PathBuf> {
    let entries = std::fs::read_dir("/sys/class/hidraw").ok()?;
    for e in entries.flatten() {
        let dev = e.path().join("device");
        let Ok(uevent) = std::fs::read_to_string(dev.join("uevent")) else {
            continue;
        };
        let id = format!("HID_ID=0003:{VID:08X}:{PID:08X}");
        if !uevent.lines().any(|l| l.eq_ignore_ascii_case(&id)) {
            continue;
        }
        let Ok(desc) = std::fs::read(dev.join("report_descriptor")) else {
            continue;
        };
        if desc.windows(3).any(|w| w == [0x06, 0x60, 0xFF]) {
            return Some(PathBuf::from("/dev").join(e.file_name()));
        }
    }
    None
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Hello {
    pub proto: u8,
    pub main_screen: u8,
    pub side_screen: u8,
    pub screens_off: bool,
}

pub struct Device {
    file: File,
    path: PathBuf,
    hello_rx: Receiver<Hello>,
    /// inotify на вузлі пристрою: дізнатися, що його відкрила інша програма (Vial).
    watch: Option<OwnedFd>,
}

impl Device {
    pub fn open() -> Result<Self, String> {
        let path = find().ok_or("клавіатуру не знайдено")?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let mut reader = file.try_clone().map_err(|e| e.to_string())?;
        let (tx, hello_rx) = mpsc::channel();
        // Клавіатура відповідає на кожен пакет; читаємо все, щоб буфер не переповнювався,
        // і віддаємо лише відповіді на HELLO. Потік завершується, коли пристрій зникає.
        std::thread::spawn(move || {
            let mut buf = [0u8; 64];
            while let Ok(n) = reader.read(&mut buf) {
                if n >= 7 && buf[0] == CMD && buf[1] == HELLO {
                    let h = Hello {
                        proto: buf[2],
                        main_screen: buf[3],
                        side_screen: buf[4],
                        screens_off: buf[6] != 0,
                    };
                    if tx.send(h).is_err() {
                        break;
                    }
                }
            }
        });
        let watch = watch_opens(&path);
        Ok(Self {
            file,
            path,
            hello_rx,
            watch,
        })
    }

    /// Чи відкривав хтось пристрій з минулого виклику. Власні відкриття сюди не потрапляють:
    /// спостереження ставиться вже після того, як служба відкрила пристрій.
    pub fn opened_by_other(&mut self) -> bool {
        let Some(fd) = &self.watch else { return false };
        let mut buf = [0u8; 4096];
        let mut any = false;
        loop {
            let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                return any;
            }
            any = true;
        }
    }

    /// Чи тримає пристрій відкритим інший процес (перевірка через /proc/*/fd).
    pub fn held_by_other(&self) -> bool {
        let me = std::process::id().to_string();
        let Ok(procs) = std::fs::read_dir("/proc") else {
            return false;
        };
        for p in procs.flatten() {
            let name = p.file_name();
            let name = name.to_string_lossy();
            if name == me || !name.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let Ok(fds) = std::fs::read_dir(p.path().join("fd")) else {
                continue;
            };
            if fds
                .flatten()
                .any(|fd| std::fs::read_link(fd.path()).is_ok_and(|l| l == self.path))
            {
                return true;
            }
        }
        false
    }

    pub fn send(&mut self, p: &Packet) -> std::io::Result<()> {
        let mut report = [0u8; 33]; // перший байт — номер звіту (0)
        report[1..].copy_from_slice(p);
        self.file.write_all(&report)
    }

    pub fn hello(&mut self) -> Result<Hello, String> {
        while self.hello_rx.try_recv().is_ok() {}
        self.send(&packet(HELLO, &[])).map_err(|e| e.to_string())?;
        let h = self
            .hello_rx
            .recv_timeout(Duration::from_millis(500))
            .map_err(|_| "клавіатура не відповідає")?;
        if h.proto != PROTO {
            return Err(format!(
                "версія протоколу прошивки {} (потрібна {PROTO}) — перепрошийте клавіатуру",
                h.proto
            ));
        }
        Ok(h)
    }

    /// Останні відповіді HELLO, що прийшли без очікування.
    pub fn poll_hello(&self) -> Option<Hello> {
        self.hello_rx.try_iter().last()
    }

    /// Надсилає кадр: лише шматки, що відрізняються від `prev`, потім команду показати.
    pub fn send_frame(&mut self, mask: u8, frame: &Frame, prev: Option<&Frame>) -> std::io::Result<()> {
        for c in 0..IMAGE_CHUNKS {
            let off = c * IMAGE_CHUNK;
            let end = (off + IMAGE_CHUNK).min(FB);
            if prev.is_some_and(|p| p[off..end] == frame[off..end]) {
                continue;
            }
            let mut body = vec![mask, c as u8];
            body.extend_from_slice(&frame[off..end]);
            self.send(&packet(IMAGE, &body))?;
        }
        self.send(&packet(COMMIT, &[mask]))
    }
}

fn watch_opens(path: &std::path::Path) -> Option<OwnedFd> {
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    unsafe {
        let fd = libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC);
        if fd < 0 {
            return None;
        }
        let fd = OwnedFd::from_raw_fd(fd);
        (libc::inotify_add_watch(fd.as_raw_fd(), c.as_ptr(), libc::IN_OPEN) >= 0).then_some(fd)
    }
}
