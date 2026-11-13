//! Дані для екрана «Годинник, CPU/RAM, музика».

use std::io::{BufRead, BufReader};
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
    fn ukrainian() {
        assert_eq!(translit("Океан Ельзи — Обійми"), "Okean Elzy - Obiimy");
        assert_eq!(translit("Щедрик"), "Shchedryk");
        assert_eq!(translit("abc 123"), "abc 123");
    }
}
