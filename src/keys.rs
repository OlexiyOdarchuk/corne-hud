//! Натискання клавіш для анімацій: лише факт і час натискання, без кодів клавіш.
//!
//! Linux: читає `/dev/input/*-event-kbd` (потрібна група `input` або правило udev з packaging/).
//! Windows: низькорівневий хук клавіатури, без прав адміністратора.
//! macOS: Quartz event tap лише для читання; потрібен дозвіл «Моніторинг вводу».

#[cfg(not(any(windows, target_os = "macos")))]
use std::fs::File;
#[cfg(not(any(windows, target_os = "macos")))]
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
    #[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
    open: Mutex<Vec<PathBuf>>,
}

fn watcher() -> &'static Watcher {
    static W: OnceLock<Watcher> = OnceLock::new();
    W.get_or_init(|| {
        #[cfg(not(any(windows, target_os = "macos")))]
        std::thread::spawn(scan_loop);
        #[cfg(windows)]
        std::thread::spawn(hook_loop);
        #[cfg(target_os = "macos")]
        std::thread::spawn(tap_loop);
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

fn record_press(w: &Watcher) {
    w.presses.fetch_add(1, Ordering::Relaxed);
    let mut r = w.recent.lock().unwrap();
    r.push(Instant::now());
    if r.len() > 512 {
        let edge = Instant::now() - Duration::from_secs(10);
        r.retain(|t| *t >= edge);
    }
}

#[cfg(windows)]
fn hook_loop() {
    use std::sync::atomic::AtomicBool;

    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage, KBDLLHOOKSTRUCT, MSG,
        WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    /// Затиснуті клавіші: автоповтор не рахується як нове натискання.
    static DOWN: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];

    unsafe extern "system" fn proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let vk = unsafe { (*(lparam as *const KBDLLHOOKSTRUCT)).vkCode } as usize & 0xFF;
            match wparam as u32 {
                WM_KEYDOWN | WM_SYSKEYDOWN => {
                    if !DOWN[vk].swap(true, Ordering::Relaxed) {
                        record_press(watcher());
                    }
                }
                WM_KEYUP | WM_SYSKEYUP => DOWN[vk].store(false, Ordering::Relaxed),
                _ => {}
            }
        }
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(proc), GetModuleHandleW(std::ptr::null()), 0) };
    if hook.is_null() {
        watcher().access.store(2, Ordering::Relaxed);
        return;
    }
    watcher().access.store(1, Ordering::Relaxed);
    // Хук працює, поки цей потік обробляє повідомлення.
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(target_os = "macos")]
fn tap_loop() {
    use std::ffi::c_void;
    use std::sync::atomic::AtomicPtr;

    type Callback = extern "C" fn(*mut c_void, u32, *mut c_void, *mut c_void) -> *mut c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events: u64,
            callback: Callback,
            user_info: *mut c_void,
        ) -> *mut c_void;
        fn CGEventTapEnable(tap: *mut c_void, enable: bool);
        fn CGEventGetIntegerValueField(event: *mut c_void, field: u32) -> i64;
        fn CGPreflightListenEventAccess() -> bool;
        fn CGRequestListenEventAccess() -> bool;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFMachPortCreateRunLoopSource(alloc: *const c_void, port: *mut c_void, order: isize) -> *mut c_void;
        fn CFRunLoopGetCurrent() -> *mut c_void;
        fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
        fn CFRunLoopRun();
        static kCFRunLoopCommonModes: *const c_void;
    }

    const SESSION_TAP: u32 = 1; // kCGSessionEventTap
    const HEAD_INSERT: u32 = 0; // kCGHeadInsertEventTap
    const LISTEN_ONLY: u32 = 1; // kCGEventTapOptionListenOnly
    const KEY_DOWN: u32 = 10; // kCGEventKeyDown
    const AUTOREPEAT: u32 = 8; // kCGKeyboardEventAutorepeat
    const DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const DISABLED_BY_USER: u32 = 0xFFFF_FFFF;

    static TAP: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

    extern "C" fn on_event(_proxy: *mut c_void, kind: u32, event: *mut c_void, _info: *mut c_void) -> *mut c_void {
        match kind {
            KEY_DOWN => {
                if unsafe { CGEventGetIntegerValueField(event, AUTOREPEAT) } == 0 {
                    record_press(watcher());
                }
            }
            // Система вимикає повільні перехоплювачі; вмикаємо знову.
            DISABLED_BY_TIMEOUT | DISABLED_BY_USER => unsafe { CGEventTapEnable(TAP.load(Ordering::Relaxed), true) },
            _ => {}
        }
        event
    }

    // Без дозволу «Моніторинг вводу» перехоплювач не створиться: просимо один раз і чекаємо.
    if !unsafe { CGPreflightListenEventAccess() } {
        unsafe { CGRequestListenEventAccess() };
    }
    loop {
        let tap = unsafe {
            CGEventTapCreate(
                SESSION_TAP,
                HEAD_INSERT,
                LISTEN_ONLY,
                1 << KEY_DOWN,
                on_event,
                std::ptr::null_mut(),
            )
        };
        if tap.is_null() {
            watcher().access.store(2, Ordering::Relaxed);
            std::thread::sleep(Duration::from_secs(5));
            continue;
        }
        TAP.store(tap, Ordering::Relaxed);
        watcher().access.store(1, Ordering::Relaxed);
        unsafe {
            let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
            CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
            CFRunLoopRun();
        }
        return;
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
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

#[cfg(not(any(windows, target_os = "macos")))]
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

#[cfg(not(any(windows, target_os = "macos")))]
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
                record_press(w);
            }
        }
    }
    // Клавіатуру від'єднали — дозволити відкрити її знову.
    w.open.lock().unwrap().retain(|p| *p != path);
}
