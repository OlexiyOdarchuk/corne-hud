//! Дані про комп'ютер: CPU, RAM, температури, батарея, музика, розкладка клавіатури.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
pub struct Cpu {
    prev: Option<(u64, u64)>, // (зайнятий, усього)
}

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

/* --------------------------------------------------------- розкладка */

#[derive(Clone, Debug, Default)]
pub struct Layout {
    /// Коротка назва для екрана: EN, UA, …
    pub name: String,
    /// Зростає з кожною зміною розкладки.
    pub seq: u64,
}

/// Рядки в лапках із виводу busctl (з восьмеричними екрануваннями UTF-8).
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

fn short_layout(code: &str) -> String {
    match code {
        "us" | "gb" => "EN".into(),
        c => c.chars().take(3).collect::<String>().to_uppercase(),
    }
}

/// Поточна розкладка KDE (через D-Bus: org.kde.keyboard).
fn query_layout() -> Option<String> {
    let call = |method: &str| {
        let out = Command::new("busctl")
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

/// Стежить за перемиканням розкладки. Працює в KDE Plasma; деінде назва лишається порожньою.
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
        let child = Command::new("busctl")
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

/// Стежить за плеєрами через `playerctl --follow` (MPRIS: браузер, Spotify, mpv…).
/// Якщо playerctl немає — просто нічого не показує.
pub fn watch_music() -> Arc<Mutex<Option<Track>>> {
    let state = Arc::new(Mutex::new(None));
    let st = state.clone();
    std::thread::spawn(move || loop {
        let child = Command::new("playerctl")
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
                let track = match (status, title.trim()) {
                    ("Playing" | "Paused", t) if !t.is_empty() => {
                        let text = if artist.trim().is_empty() {
                            t.to_string()
                        } else {
                            format!("{} - {t}", artist.trim())
                        };
                        Some(Track {
                            playing: status == "Playing",
                            text: translit(&text),
                        })
                    }
                    _ => None,
                };
                *st.lock().unwrap() = track;
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
    use super::translit;

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
}
