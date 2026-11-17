//! Зв'язок із клавіатурою через Raw HID інтерфейс Vial (usage page 0xFF60).
//! Linux: напряму через /dev/hidraw; Windows і macOS: через hidapi. Протокол — PROTOCOL.md.

use crate::oled::{Frame, FB};

#[cfg(not(target_os = "linux"))]
pub use hid::Device;
#[cfg(target_os = "linux")]
pub use linux::Device;

const VID: u16 = 0x4653;
const PID: u16 = 0x0001;
const USAGE_PAGE: u16 = 0xFF60;

pub const CMD: u8 = 0x70;
/// Найновіша відома версія. 2 додала тихе перемикання режиму (без підказки з назвою).
pub const PROTO: u8 = 2;
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

#[derive(Clone, Copy, Debug, Default)]
pub struct Hello {
    pub proto: u8,
    pub main_screen: u8,
    pub side_screen: u8,
    pub screens_off: bool,
}

fn parse_hello(buf: &[u8]) -> Option<Hello> {
    (buf.len() >= 7 && buf[0] == CMD && buf[1] == HELLO).then(|| Hello {
        proto: buf[2],
        main_screen: buf[3],
        side_screen: buf[4],
        screens_off: buf[6] != 0,
    })
}

fn check_proto(h: Hello) -> Result<Hello, String> {
    if h.proto == 0 || h.proto > PROTO {
        return Err(t!("err.fw_proto", v = h.proto).into());
    }
    Ok(h)
}

impl Device {
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

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::CString;
    use std::fs::{File, OpenOptions};
    use std::io::{Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::sync::mpsc::{self, Receiver};
    use std::time::Duration;

    use super::*;

    /// Шукає hidraw-вузол клавіатури саме з інтерфейсом Raw HID.
    pub fn find() -> Option<PathBuf> {
        let entries = std::fs::read_dir("/sys/class/hidraw").ok()?;
        for e in entries.flatten() {
            let dev = e.path().join("device");
            let Ok(uevent) = std::fs::read_to_string(dev.join("uevent")) else {
                continue;
            };
            let id = format!("HID_ID=0003:{:08X}:{:08X}", VID as u32, PID as u32);
            if !uevent.lines().any(|l| l.eq_ignore_ascii_case(&id)) {
                continue;
            }
            let Ok(desc) = std::fs::read(dev.join("report_descriptor")) else {
                continue;
            };
            if desc
                .windows(3)
                .any(|w| w == [0x06, USAGE_PAGE as u8, (USAGE_PAGE >> 8) as u8])
            {
                return Some(PathBuf::from("/dev").join(e.file_name()));
            }
        }
        None
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
            let path = find().ok_or_else(|| t!("err.kb_not_found").to_string())?;
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
                    if let Some(h) = parse_hello(&buf[..n]) {
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
                .map_err(|_| t!("err.kb_no_answer").to_string())?;
            check_proto(h)
        }

        /// Останні відповіді HELLO, що прийшли без очікування.
        pub fn poll_hello(&self) -> Option<Hello> {
            self.hello_rx.try_iter().last()
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
}

#[cfg(not(target_os = "linux"))]
mod hid {
    use super::*;

    pub struct Device {
        dev: hidapi::HidDevice,
    }

    impl Device {
        pub fn open() -> Result<Self, String> {
            let api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
            let info = api
                .device_list()
                .find(|d| d.vendor_id() == VID && d.product_id() == PID && d.usage_page() == USAGE_PAGE)
                .ok_or_else(|| t!("err.kb_not_found").to_string())?;
            let dev = info.open_device(&api).map_err(|e| e.to_string())?;
            Ok(Self { dev })
        }

        /// hidapi не повідомляє, хто ще відкрив пристрій.
        pub fn opened_by_other(&mut self) -> bool {
            false
        }

        pub fn held_by_other(&self) -> bool {
            false
        }

        pub fn send(&mut self, p: &Packet) -> std::io::Result<()> {
            let mut report = [0u8; 33]; // перший байт — номер звіту (0)
            report[1..].copy_from_slice(p);
            self.dev.write(&report).map(|_| ()).map_err(std::io::Error::other)
        }

        /// Читає відповіді, що вже прийшли; повертає останню відповідь HELLO.
        fn drain(&self, wait_ms: i32) -> Option<Hello> {
            let mut buf = [0u8; 64];
            let mut last = None;
            let mut wait = wait_ms;
            while let Ok(n) = self.dev.read_timeout(&mut buf, wait) {
                if n == 0 {
                    break;
                }
                if let Some(h) = parse_hello(&buf[..n]) {
                    last = Some(h);
                }
                wait = 0;
            }
            last
        }

        pub fn hello(&mut self) -> Result<Hello, String> {
            self.drain(0);
            self.send(&packet(HELLO, &[])).map_err(|e| e.to_string())?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
            loop {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    return Err(t!("err.kb_no_answer").to_string());
                }
                if let Some(h) = self.drain(left.as_millis() as i32) {
                    return check_proto(h);
                }
            }
        }

        pub fn poll_hello(&self) -> Option<Hello> {
            self.drain(0)
        }
    }
}
