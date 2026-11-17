//! Дані про комп'ютер: CPU, RAM, температури, батарея, музика, розкладка клавіатури.

#[cfg(not(any(windows, target_os = "macos")))]
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
#[cfg(not(any(windows, target_os = "macos")))]
use std::process::Stdio;

#[cfg(not(any(windows, target_os = "macos")))]
use crate::platform::host_command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(target_os = "linux")]
#[derive(Default)]
pub struct Cpu {
    prev: Option<(u64, u64)>, // (зайнятий, усього)
}

#[cfg(not(target_os = "linux"))]
pub struct Cpu {
    sys: psinfo::System,
}

#[cfg(not(target_os = "linux"))]
impl Default for Cpu {
    fn default() -> Self {
        Self {
            sys: psinfo::System::new(),
        }
    }
}

#[cfg(not(target_os = "linux"))]
impl Cpu {
    pub fn sample(&mut self) -> u8 {
        self.sys.refresh_cpu_usage();
        self.sys.global_cpu_usage().round().clamp(0.0, 100.0) as u8
    }
}

#[cfg(not(target_os = "linux"))]
pub fn ram_percent() -> u8 {
    let mut sys = psinfo::System::new();
    sys.refresh_memory();
    match sys.total_memory() {
        0 => 0,
        t => (sys.used_memory().min(t) * 100 / t) as u8,
    }
}

#[cfg(target_os = "linux")]
impl Cpu {
    /// Завантаження CPU у відсотках з минулого виклику.
    pub fn sample(&mut self) -> u8 {
        let Ok(stat) = std::fs::read_to_string("/proc/stat") else {
            return 0;
        };
        let Some(line) = stat.lines().next() else {
            return 0;
        };
        let v: Vec<u64> = line.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
        if v.len() < 5 {
            return 0;
        }
        let total: u64 = v.iter().sum();
        let idle = v[3] + v[4]; // idle + iowait
        let busy = total - idle;
        let pct = match self.prev {
            Some((pb, pt)) if total > pt => ((busy - pb.min(busy)) * 100 / (total - pt)) as u8,
            _ => 0,
        };
        self.prev = Some((busy, total));
        pct.min(100)
    }
}

#[cfg(target_os = "linux")]
pub fn ram_percent() -> u8 {
    let Ok(info) = std::fs::read_to_string("/proc/meminfo") else {
        return 0;
    };
    let field = |name: &str| -> Option<u64> {
        info.lines()
            .find(|l| l.starts_with(name))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    };
    match (field("MemTotal:"), field("MemAvailable:")) {
        (Some(t), Some(a)) if t > 0 => ((t - a.min(t)) * 100 / t) as u8,
        _ => 0,
    }
}

/* --------------------------------------------------------- температури, батарея */

fn read_trim(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

/// Датчик температури в hwmon: файл `tempN_input` з потрібною міткою або перший.
fn hwmon_temp(names: &[&str], labels: &[&str]) -> Option<PathBuf> {
    let dirs: Vec<PathBuf> = std::fs::read_dir("/sys/class/hwmon")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    for want in names {
        for dir in &dirs {
            if read_trim(&dir.join("name")).as_deref() != Some(want) {
                continue;
            }
            let mut inputs: Vec<PathBuf> = std::fs::read_dir(dir)
                .ok()?
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("temp") && n.ends_with("_input"))
                })
                .collect();
            inputs.sort();
            let labelled = inputs.iter().find(|p| {
                let label = p.to_string_lossy().replace("_input", "_label");
                read_trim(Path::new(&label)).is_some_and(|l| labels.iter().any(|w| l.eq_ignore_ascii_case(w)))
            });
            if let Some(p) = labelled.or(inputs.first()) {
                return Some(p.clone());
            }
        }
    }
    None
}

fn read_temp(p: &Option<PathBuf>) -> Option<f32> {
    let v: f32 = read_trim(p.as_ref()?)?.parse().ok()?;
    Some(v / 1000.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Power {
    Charging,
    Discharging,
    Full,
    /// Від мережі, але не заряджається (обмеження заряду тощо).
    Idle,
}

#[derive(Clone, Debug, Default)]
pub struct Reading {
    pub cpu_temp: Option<f32>,
    /// Друга температура: відеокарта, а якщо її датчика немає — SSD. Назва: GPU або SSD.
    pub extra_temp: Option<(&'static str, f32)>,
    pub battery: Option<(u8, Power)>,
}

/// Знайдені один раз шляхи до датчиків.
pub struct Sensors {
    cpu: Option<PathBuf>,
    gpu: Option<PathBuf>,
    ssd: Option<PathBuf>,
    #[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
    battery: Option<PathBuf>,
}

impl Sensors {
    pub fn find() -> Self {
        let battery = std::fs::read_dir("/sys/class/power_supply").ok().and_then(|d| {
            d.flatten().map(|e| e.path()).find(|p| {
                read_trim(&p.join("type")).as_deref() == Some("Battery")
                    && read_trim(&p.join("scope")).as_deref() != Some("Device") // не батарея миші
                    && p.join("capacity").exists()
            })
        });
        Self {
            cpu: hwmon_temp(
                &["coretemp", "k10temp", "zenpower", "cpu_thermal", "acpitz"],
                &["Package id 0", "Tctl", "Tdie"],
            ),
            gpu: hwmon_temp(&["amdgpu", "nouveau", "radeon", "xe", "i915"], &["edge", "pkg"]),
            ssd: hwmon_temp(&["nvme"], &["Composite"]),
            battery,
        }
    }

    pub fn read(&self) -> Reading {
        let extra = match (read_temp(&self.gpu), read_temp(&self.ssd)) {
            (Some(t), _) => Some(("GPU", t)),
            (None, Some(t)) => Some(("SSD", t)),
            _ => None,
        };
        #[cfg(windows)]
        let battery = windows_battery();
        #[cfg(target_os = "macos")]
        let battery = mac_battery();
        #[cfg(not(any(windows, target_os = "macos")))]
        let battery = self.battery.as_ref().and_then(|b| {
            let cap: u8 = read_trim(&b.join("capacity"))?.parse().ok()?;
            let power = match read_trim(&b.join("status")).as_deref() {
                Some("Charging") => Power::Charging,
                Some("Full") => Power::Full,
                Some("Not charging") => Power::Idle,
                _ => Power::Discharging,
            };
            Some((cap.min(100), power))
        });
        Reading {
            cpu_temp: read_temp(&self.cpu),
            extra_temp: extra,
            battery,
        }
    }
}

#[cfg(windows)]
fn windows_battery() -> Option<(u8, Power)> {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    if unsafe { GetSystemPowerStatus(&mut s) } == 0 {
        return None;
    }
    // 128 — батареї немає, 255 — невідомо.
    if s.BatteryFlag & 128 != 0 || s.BatteryFlag == 255 || s.BatteryLifePercent > 100 {
        return None;
    }
    let cap = s.BatteryLifePercent;
    let power = match (s.ACLineStatus == 1, s.BatteryFlag & 8 != 0) {
        (_, true) => Power::Charging,
        (true, false) if cap >= 100 => Power::Full,
        (true, false) => Power::Idle,
        (false, false) => Power::Discharging,
    };
    Some((cap, power))
}

/* --------------------------------------------------------- розкладка */

#[derive(Clone, Debug, Default)]
pub struct Layout {
    /// Коротка назва для екрана: EN, UA, …
    pub name: String,
    /// Зростає з кожною зміною розкладки.
    pub seq: u64,
}

/// Рядки в лапках із виводу busctl (з восьмеричними екрануваннями UTF-8).
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
fn busctl_strings(out: &str) -> Vec<String> {
    let mut res = Vec::new();
    let mut chars = out.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut bytes = Vec::new();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => {
                    let oct: String = (0..3).filter_map(|_| chars.next_if(|d| d.is_digit(8))).collect();
                    match u8::from_str_radix(&oct, 8) {
                        Ok(b) if !oct.is_empty() => bytes.push(b),
                        _ => {
                            if let Some(n) = chars.next() {
                                bytes.extend(n.to_string().bytes());
                            }
                        }
                    }
                }
                c => bytes.extend(c.to_string().bytes()),
            }
        }
        res.push(String::from_utf8_lossy(&bytes).into_owned());
    }
    res
}

#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
fn short_layout(code: &str) -> String {
    match code {
        "us" | "gb" => "EN".into(),
        c => c.chars().take(3).collect::<String>().to_uppercase(),
    }
}

/// Поточна розкладка KDE (через D-Bus: org.kde.keyboard).
#[cfg(not(any(windows, target_os = "macos")))]
fn query_layout() -> Option<String> {
    let call = |method: &str| {
        let out = host_command("busctl")
            .args([
                "--user",
                "call",
                "org.kde.keyboard",
                "/Layouts",
                "org.kde.KeyboardLayouts",
                method,
            ])
            .stderr(Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let idx: usize = call("getLayout")?.split_whitespace().nth(1)?.parse().ok()?;
    let list = busctl_strings(&call("getLayoutsList")?);
    Some(short_layout(list.get(idx * 3)?))
}

/// Мова розкладки активного вікна Windows: EN, UA, …
#[cfg(windows)]
fn query_layout() -> Option<String> {
    use windows_sys::Win32::Globalization::LCIDToLocaleName;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let mut buf = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
    let n = unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
        let langid = (GetKeyboardLayout(thread) as usize & 0xFFFF) as u32;
        LCIDToLocaleName(langid, buf.as_mut_ptr(), buf.len() as i32, 0)
    };
    if n <= 1 {
        return None;
    }
    let name = String::from_utf16_lossy(&buf[..n as usize - 1]); // "uk-UA"
    Some(match name.split('-').next()? {
        "uk" => "UA".into(),
        lang => lang.to_uppercase(),
    })
}

/// macOS: розкладка з налаштувань HIToolbox (`com.apple.keylayout.Ukrainian-PC` → UA).
/// Через `defaults`, бо API Text Input Sources можна кликати лише з головного потоку.
#[cfg(target_os = "macos")]
fn query_layout() -> Option<String> {
    let out = std::process::Command::new("defaults")
        .args(["read", "com.apple.HIToolbox", "AppleCurrentKeyboardLayoutInputSourceID"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    mac_layout_name(String::from_utf8_lossy(&out.stdout).trim())
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn mac_layout_name(id: &str) -> Option<String> {
    let name = id.rsplit('.').next().filter(|n| !n.is_empty())?;
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| name.starts_with(p));
    Some(
        if starts(&[
            "US",
            "ABC",
            "British",
            "Australian",
            "Canadian",
            "Irish",
            "Colemak",
            "Dvorak",
        ]) {
            "EN".into()
        } else if starts(&["Ukrainian"]) {
            "UA".into()
        } else if starts(&["Russian"]) {
            "RU".into()
        } else if starts(&["Belarusian"]) {
            "BY".into()
        } else if starts(&["German"]) {
            "DE".into()
        } else if starts(&["French"]) {
            "FR".into()
        } else if starts(&["Polish"]) {
            "PL".into()
        } else if starts(&["Spanish"]) {
            "ES".into()
        } else if starts(&["Italian"]) {
            "IT".into()
        } else if starts(&["Czech"]) {
            "CZ".into()
        } else {
            name.chars()
                .filter(char::is_ascii_alphabetic)
                .take(2)
                .collect::<String>()
                .to_uppercase()
        },
    )
}

/// macOS: заряд із `pmset -g batt`, не частіше ніж раз на 15 с.
#[cfg(target_os = "macos")]
fn mac_battery() -> Option<(u8, Power)> {
    use std::time::Instant;
    type Battery = Option<(u8, Power)>;
    static CACHE: Mutex<Option<(Instant, Battery)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap();
    if let Some((at, v)) = *cache {
        if at.elapsed() < Duration::from_secs(15) {
            return v;
        }
    }
    let v = std::process::Command::new("pmset")
        .args(["-g", "batt"])
        .output()
        .ok()
        .and_then(|o| parse_pmset(&String::from_utf8_lossy(&o.stdout)));
    *cache = Some((Instant::now(), v));
    v
}

/// ` -InternalBattery-0 (id=123)<TAB>85%; charging; 1:23 remaining present: true`
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_pmset(out: &str) -> Option<(u8, Power)> {
    let line = out.lines().find(|l| l.contains("InternalBattery"))?;
    let (before, after) = line.split_once('%')?;
    let cap: u8 = before.rsplit(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?;
    let state = after.split(';').nth(1).unwrap_or("").trim();
    let power = match state {
        s if s.starts_with("charging") || s.starts_with("finishing") => Power::Charging,
        s if s.starts_with("charged") => Power::Full,
        s if s.starts_with("AC attached") => Power::Idle,
        _ => Power::Discharging,
    };
    Some((cap.min(100), power))
}

/// macOS: трек зі Spotify або Music через AppleScript (першого разу macOS попросить дозвіл на керування).
/// Загального API «що зараз грає» macOS стороннім програмам не дає.
#[cfg(target_os = "macos")]
pub fn watch_music() -> Arc<Mutex<Option<Track>>> {
    use std::process::{Command, Stdio};

    let state = Arc::new(Mutex::new(None));
    let st = state.clone();
    std::thread::spawn(move || loop {
        let running = |name: &str| {
            Command::new("pgrep")
                .args(["-x", name])
                .stdout(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        };
        let app = ["Spotify", "Music"].into_iter().find(|a| running(a));
        let track = app.and_then(|app| {
            let script = format!(
                "tell application \"{app}\" to (player state as text) & tab & (artist of current track) & tab & (name of current track)"
            );
            let out = Command::new("osascript")
                .args(["-e", &script])
                .stderr(Stdio::null())
                .output()
                .ok()?;
            let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let mut parts = line.splitn(3, '\t');
            let playing = match parts.next()? {
                "playing" => Some(true),
                "paused" => Some(false),
                _ => None,
            };
            make_track(playing, parts.next().unwrap_or(""), parts.next().unwrap_or(""))
        });
        *st.lock().unwrap() = track;
        std::thread::sleep(Duration::from_secs(2));
    });
    state
}

/// Windows і macOS не надсилають іншим програмам подію про зміну розкладки, тож перевіряємо кілька разів на секунду.
#[cfg(any(windows, target_os = "macos"))]
pub fn watch_layout() -> Arc<Mutex<Layout>> {
    let state = Arc::new(Mutex::new(Layout::default()));
    let st = state.clone();
    std::thread::spawn(move || loop {
        if let Some(name) = query_layout() {
            let mut l = st.lock().unwrap();
            if l.name != name {
                l.name = name;
                l.seq += 1;
            }
        }
        // У macOS це запуск `defaults`, тож рідше.
        std::thread::sleep(Duration::from_millis(if cfg!(windows) { 250 } else { 500 }));
    });
    state
}

/// Стежить за перемиканням розкладки. Працює в KDE Plasma; деінде назва лишається порожньою.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn watch_layout() -> Arc<Mutex<Layout>> {
    let state = Arc::new(Mutex::new(Layout::default()));
    let st = state.clone();
    let update = move |st: &Mutex<Layout>| {
        if let Some(name) = query_layout() {
            let mut l = st.lock().unwrap();
            if l.name != name {
                l.name = name;
                l.seq += 1;
            }
        }
    };
    std::thread::spawn(move || loop {
        update(&st);
        let child = host_command("busctl")
            .args([
                "--user",
                "monitor",
                "--json=short",
                "--match",
                "type='signal',interface='org.kde.KeyboardLayouts'",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else {
            std::thread::sleep(Duration::from_secs(60));
            continue;
        };
        if let Some(out) = child.stdout.take() {
            for _signal in BufReader::new(out).lines().map_while(Result::ok) {
                update(&st);
            }
        }
        let _ = child.wait();
        std::thread::sleep(Duration::from_secs(10));
    });
    state
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub playing: bool,
    pub text: String,
}

/// `playing`: Some(true) — грає, Some(false) — пауза, None — зупинено або невідомо.
fn make_track(playing: Option<bool>, artist: &str, title: &str) -> Option<Track> {
    let (playing, title, artist) = (playing?, title.trim(), artist.trim());
    if title.is_empty() {
        return None;
    }
    let text = if artist.is_empty() {
        title.to_string()
    } else {
        format!("{artist} - {title}")
    };
    Some(Track {
        playing,
        text: translit(&text),
    })
}

/// Windows: поточний трек із системного медіа (той самий, що в регуляторі гучності):
/// браузер, Spotify, медіаплеєр…
#[cfg(windows)]
pub fn watch_music() -> Arc<Mutex<Option<Track>>> {
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };

    let state = Arc::new(Mutex::new(None));
    let st = state.clone();
    std::thread::spawn(move || {
        let _ = unsafe {
            windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED)
        };
        let mut manager = None;
        loop {
            if manager.is_none() {
                manager = Manager::RequestAsync().and_then(|op| op.join()).ok();
            }
            let track = manager.as_ref().and_then(|m| {
                let session = m.GetCurrentSession().ok()?;
                let props = session.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
                let playing = match session.GetPlaybackInfo().ok()?.PlaybackStatus().ok()? {
                    Status::Playing => Some(true),
                    Status::Paused => Some(false),
                    _ => None,
                };
                let artist = props.Artist().map(|s| s.to_string()).unwrap_or_default();
                let title = props.Title().map(|s| s.to_string()).unwrap_or_default();
                make_track(playing, &artist, &title)
            });
            *st.lock().unwrap() = track;
            std::thread::sleep(Duration::from_secs(1));
        }
    });
    state
}

/// Стежить за плеєрами через `playerctl --follow` (MPRIS: браузер, Spotify, mpv…).
/// Якщо playerctl немає — просто нічого не показує.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn watch_music() -> Arc<Mutex<Option<Track>>> {
    let state = Arc::new(Mutex::new(None));
    let st = state.clone();
    std::thread::spawn(move || loop {
        let child = host_command("playerctl")
            .args(["--follow", "metadata", "--format", "{{status}}\t{{artist}}\t{{title}}"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else {
            std::thread::sleep(Duration::from_secs(60));
            continue;
        };
        if let Some(out) = child.stdout.take() {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let mut parts = line.splitn(3, '\t');
                let (status, artist, title) = (
                    parts.next().unwrap_or(""),
                    parts.next().unwrap_or(""),
                    parts.next().unwrap_or(""),
                );
                let playing = match status {
                    "Playing" => Some(true),
                    "Paused" => Some(false),
                    _ => None,
                };
                *st.lock().unwrap() = make_track(playing, artist, title);
            }
        }
        let _ = child.wait();
        *st.lock().unwrap() = None;
        std::thread::sleep(Duration::from_secs(5));
    });
    state
}

/// Кирилиця латиницею (українська транслітерація), бо шрифт на екрані лише ASCII.
pub fn translit(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let lower: String = ch.to_lowercase().collect();
        let t = match lower.as_str() {
            "а" => "a",
            "б" => "b",
            "в" => "v",
            "г" => "h",
            "ґ" => "g",
            "д" => "d",
            "е" => "e",
            "є" => "ie",
            "ж" => "zh",
            "з" => "z",
            "и" => "y",
            "і" => "i",
            "ї" => "i",
            "й" => "i",
            "к" => "k",
            "л" => "l",
            "м" => "m",
            "н" => "n",
            "о" => "o",
            "п" => "p",
            "р" => "r",
            "с" => "s",
            "т" => "t",
            "у" => "u",
            "ф" => "f",
            "х" => "kh",
            "ц" => "ts",
            "ч" => "ch",
            "ш" => "sh",
            "щ" => "shch",
            "ь" | "ъ" => "",
            "ю" => "iu",
            "я" => "ia",
            "ё" => "io",
            "ы" => "y",
            "э" => "e",
            "’" | "ʼ" | "‘" | "`" => "'",
            "«" | "»" | "“" | "”" | "„" => "\"",
            "—" | "–" | "−" => "-",
            "…" => "...",
            _ => {
                if ch.is_ascii() && !ch.is_ascii_control() {
                    out.push(ch);
                } else if ch.is_whitespace() {
                    out.push(' ');
                }
                continue;
            }
        };
        if ch.is_uppercase() {
            let mut cs = t.chars();
            if let Some(first) = cs.next() {
                out.push(first.to_ascii_uppercase());
                out.extend(cs);
            }
        } else {
            out.push_str(t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{mac_layout_name, parse_pmset, translit, Power};

    #[test]
    fn busctl_output() {
        let out = r#"a(sss) 2 "us" "" "English" "ua" "" "\320\243\320\272\321\200""#;
        let v = super::busctl_strings(out);
        assert_eq!(v, ["us", "", "English", "ua", "", "Укр"]);
        assert_eq!(super::short_layout("us"), "EN");
        assert_eq!(super::short_layout("ua"), "UA");
    }

    #[test]
    fn ukrainian() {
        assert_eq!(translit("Океан Ельзи — Обійми"), "Okean Elzy - Obiimy");
        assert_eq!(translit("Щедрик"), "Shchedryk");
        assert_eq!(translit("abc 123"), "abc 123");
    }
    #[test]
    fn mac_layouts() {
        assert_eq!(
            mac_layout_name("com.apple.keylayout.Ukrainian-PC").as_deref(),
            Some("UA")
        );
        assert_eq!(mac_layout_name("com.apple.keylayout.ABC").as_deref(), Some("EN"));
        assert_eq!(
            mac_layout_name("com.apple.keylayout.USInternational-PC").as_deref(),
            Some("EN")
        );
        assert_eq!(mac_layout_name("com.apple.keylayout.Norwegian").as_deref(), Some("NO"));
        assert_eq!(mac_layout_name(""), None);
    }

    #[test]
    fn pmset() {
        let out = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=4653)\t85%; discharging; 4:20 remaining present: true\n";
        assert_eq!(parse_pmset(out), Some((85, Power::Discharging)));
        let out = " -InternalBattery-0 (id=1)\t100%; charged; 0:00 remaining present: true";
        assert_eq!(parse_pmset(out), Some((100, Power::Full)));
        let out = " -InternalBattery-0 (id=1)\t80%; AC attached; not charging present: true";
        assert_eq!(parse_pmset(out), Some((80, Power::Idle)));
        assert_eq!(parse_pmset("Now drawing from 'AC Power'\n"), None);
    }
}
