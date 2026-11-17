//! Звук, що грає на комп'ютері: спектр і хвиля для заготовок.
//!
//! Захоплення — з монітора виходу за замовчуванням через `parec` (PipeWire/PulseAudio)
//! або `pw-record`; у Windows — WASAPI loopback; у macOS — віртуальний пристрій BlackHole
//! (без нього macOS не дає записати системний звук). Працює, лише поки існує хоч одна заготовка, якій потрібен звук.

use std::f32::consts::PI;
#[cfg(not(any(windows, target_os = "macos")))]
use std::io::Read;
use std::process::Child;
#[cfg(not(any(windows, target_os = "macos")))]
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

pub const BANDS: usize = 32;
pub const WAVE: usize = 128;
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
const RATE: usize = 48_000;
const N: usize = 2048;

#[derive(Clone)]
pub struct Snapshot {
    /// Рівні смуг 0..1, від низьких частот до високих.
    pub bands: [f32; BANDS],
    /// Останні відліки, проріджені до ширини екрана, -1..1.
    pub wave: [f32; WAVE],
    /// Чи вдалося запустити захоплення.
    pub ok: bool,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            bands: [0.0; BANDS],
            wave: [0.0; WAVE],
            ok: true,
        }
    }
}

pub struct Analyzer {
    data: Arc<Mutex<Snapshot>>,
    stop: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

impl Analyzer {
    /// Спільний аналізатор: запускається з першою заготовкою і зупиняється з останньою.
    pub fn shared() -> Arc<Analyzer> {
        static CURRENT: Mutex<Weak<Analyzer>> = Mutex::new(Weak::new());
        let mut cur = CURRENT.lock().unwrap();
        if let Some(a) = cur.upgrade() {
            return a;
        }
        let a = Arc::new(Analyzer::start());
        *cur = Arc::downgrade(&a);
        a
    }

    fn start() -> Analyzer {
        let data = Arc::new(Mutex::new(Snapshot::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let child = Arc::new(Mutex::new(None));
        let (d, s, c) = (data.clone(), stop.clone(), child.clone());
        std::thread::spawn(move || capture(d, s, c));
        Analyzer { data, stop, child }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.data.lock().unwrap().clone()
    }
}

impl Drop for Analyzer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(mut c) = self.child.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn spawn_recorder() -> Option<Child> {
    let rate = RATE.to_string();
    let attempts: [(&str, Vec<&str>); 2] = [
        (
            "parec",
            vec![
                "-d",
                "@DEFAULT_MONITOR@",
                "--format=s16le",
                "--rate",
                &rate,
                "--channels=1",
                "--latency-msec=20",
            ],
        ),
        (
            "pw-record",
            vec![
                "-P",
                "{ stream.capture.sink=true }",
                "--format",
                "s16",
                "--rate",
                &rate,
                "--channels",
                "1",
                "-",
            ],
        ),
    ];
    let child = attempts.into_iter().find_map(|(cmd, args)| {
        crate::platform::host_command(cmd)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()
    });
    child
}

/// Спектр і хвиля з потоку відліків (моно, -1..1).
struct Analysis {
    rate: usize,
    ring: Vec<f32>,
    pos: usize,
    fresh: usize,
    window: Vec<f32>,
    edges: Vec<usize>,
    gain: f32,
}

impl Analysis {
    fn new(rate: usize) -> Self {
        Self {
            rate,
            ring: vec![0f32; N],
            pos: 0,
            fresh: 0,
            window: (0..N)
                .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / N as f32).cos())
                .collect(),
            edges: band_edges(rate),
            gain: 1e-3,
        }
    }

    fn push(&mut self, samples: impl IntoIterator<Item = f32>, data: &Mutex<Snapshot>) {
        for v in samples {
            self.ring[self.pos] = v;
            self.pos = (self.pos + 1) % N;
            self.fresh += 1;
            // ~40 разів на секунду
            if self.fresh >= self.rate / 40 {
                self.fresh = 0;
                self.update(data);
            }
        }
    }

    fn update(&mut self, data: &Mutex<Snapshot>) {
        let (ring, pos) = (&self.ring, self.pos);
        let mut re: Vec<f32> = (0..N).map(|i| ring[(pos + i) % N] * self.window[i]).collect();
        let mut im = vec![0f32; N];
        fft(&mut re, &mut im);
        let mut bands = [0f32; BANDS];
        for (b, w) in self.edges.windows(2).enumerate() {
            let (lo, hi) = (w[0], w[1].max(w[0] + 1));
            let peak = (lo..hi)
                .map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt())
                .fold(0.0, f32::max);
            bands[b] = peak;
        }
        // Автопідсилення: найгучніша смуга повільно тягне шкалу за собою.
        let loud = bands.iter().copied().fold(0.0, f32::max);
        let gain = self.gain;
        self.gain = if loud > gain { loud } else { gain * 0.995 + loud * 0.005 }.max(2.0);
        let mut snap = data.lock().unwrap();
        for (dst, v) in snap.bands.iter_mut().zip(bands) {
            // логарифмічна шкала: 36 дБ від максимуму до нуля
            let db = 20.0 * (v / self.gain).max(1e-6).log10();
            *dst = ((db + 36.0) / 36.0).clamp(0.0, 1.0);
        }
        let step = 4; // 512 останніх відліків → 128 точок
        for (i, w) in snap.wave.iter_mut().enumerate() {
            let k = (pos + N - WAVE * step + i * step) % N;
            *w = ring[k];
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn capture(data: Arc<Mutex<Snapshot>>, stop: Arc<AtomicBool>, slot: Arc<Mutex<Option<Child>>>) {
    let mut analysis = Analysis::new(RATE);
    while !stop.load(Ordering::Relaxed) {
        let Some(mut child) = spawn_recorder() else {
            data.lock().unwrap().ok = false;
            std::thread::sleep(std::time::Duration::from_secs(10));
            continue;
        };
        let mut out = child.stdout.take().expect("stdout");
        *slot.lock().unwrap() = Some(child);
        data.lock().unwrap().ok = true;
        let mut buf = [0u8; 1024];
        while !stop.load(Ordering::Relaxed) {
            // Повний блок: відліки по 2 байти не розірвуться між читаннями.
            if out.read_exact(&mut buf).is_err() {
                break;
            }
            let samples = buf
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| i16::from_le_bytes(*p) as f32 / 32768.0);
            analysis.push(samples, &data);
        }
        if let Some(mut c) = slot.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        if !stop.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    }
}

/// Звідки брати системний звук. Windows: вихід за замовчуванням, cpal вмикає для нього loopback.
#[cfg(windows)]
fn loopback(host: &cpal::Host) -> Option<(cpal::Device, cpal::SupportedStreamConfig)> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let dev = host.default_output_device()?;
    let cfg = dev.default_output_config().ok()?;
    Some((dev, cfg))
}

/// macOS: вхід віртуального пристрою, куди продубльовано вихід (BlackHole, Loopback, Soundflower).
#[cfg(target_os = "macos")]
fn loopback(host: &cpal::Host) -> Option<(cpal::Device, cpal::SupportedStreamConfig)> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let dev = host.input_devices().ok()?.find(|d| {
        d.description().is_ok_and(|desc| {
            ["BlackHole", "Loopback", "Soundflower"]
                .iter()
                .any(|k| desc.name().contains(k))
        })
    })?;
    let cfg = dev.default_input_config().ok()?;
    Some((dev, cfg))
}

#[cfg(any(windows, target_os = "macos"))]
fn capture(data: Arc<Mutex<Snapshot>>, stop: Arc<AtomicBool>, _slot: Arc<Mutex<Option<Child>>>) {
    use std::time::{Duration, Instant};

    use cpal::traits::{DeviceTrait, StreamTrait};

    while !stop.load(Ordering::Relaxed) {
        let host = cpal::default_host();
        let started = loopback(&host).and_then(|(dev, cfg)| {
            let rate = cfg.sample_rate() as usize;
            let channels = cfg.channels().max(1) as usize;
            let state = Arc::new(Mutex::new((Analysis::new(rate), Instant::now())));
            let failed = Arc::new(AtomicBool::new(false));
            let on_err = {
                let failed = failed.clone();
                move |_| failed.store(true, Ordering::Relaxed)
            };
            let feed = {
                let (state, data) = (state.clone(), data.clone());
                move |mono: &mut dyn Iterator<Item = f32>| {
                    let mut st = state.lock().unwrap();
                    st.1 = Instant::now();
                    st.0.push(mono, &data);
                }
            };
            let stream = match cfg.sample_format() {
                cpal::SampleFormat::F32 => dev.build_input_stream(
                    cfg.config(),
                    move |d: &[f32], _: &_| {
                        feed(&mut d.chunks(channels).map(|c| c.iter().sum::<f32>() / channels as f32))
                    },
                    on_err,
                    None,
                ),
                cpal::SampleFormat::I16 => dev.build_input_stream(
                    cfg.config(),
                    move |d: &[i16], _: &_| {
                        feed(
                            &mut d
                                .chunks(channels)
                                .map(|c| c.iter().map(|&v| v as f32 / 32768.0).sum::<f32>() / channels as f32),
                        )
                    },
                    on_err,
                    None,
                ),
                _ => return None,
            }
            .ok()?;
            stream.play().ok()?;
            Some((stream, state, failed, rate))
        });
        let Some((stream, state, failed, rate)) = started else {
            data.lock().unwrap().ok = false;
            std::thread::sleep(Duration::from_secs(10));
            continue;
        };
        data.lock().unwrap().ok = true;
        let tick = Duration::from_millis(25);
        while !stop.load(Ordering::Relaxed) && !failed.load(Ordering::Relaxed) {
            std::thread::sleep(tick);
            // У тиші loopback не надсилає нічого: тоді доливаємо нулі, щоб смуги згасали.
            let mut st = state.lock().unwrap();
            if st.1.elapsed() > tick * 4 {
                st.0.push(std::iter::repeat_n(0.0, rate / 40), &data);
            }
        }
        drop(stream);
        if !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(2));
        }
    }
}

/// Межі смуг у номерах відліків спектра: логарифмічно від 40 Гц до 16 кГц.
fn band_edges(rate: usize) -> Vec<usize> {
    let (lo, hi) = (40f32, 16_000f32);
    (0..=BANDS)
        .map(|i| {
            let f = lo * (hi / lo).powf(i as f32 / BANDS as f32);
            ((f * N as f32 / rate as f32).round() as usize).clamp(1, N / 2)
        })
        .collect()
}

/// Швидке перетворення Фур'є на місці (radix-2, довжина — степінь двійки).
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                (cr, ci) = (cr * wr - ci * wi, cr * wi + ci * wr);
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_finds_tone() {
        let k = 100;
        let mut re: Vec<f32> = (0..N)
            .map(|i| (2.0 * PI * k as f32 * i as f32 / N as f32).sin())
            .collect();
        let mut im = vec![0.0; N];
        fft(&mut re, &mut im);
        let mag = |i: usize| (re[i] * re[i] + im[i] * im[i]).sqrt();
        let peak = (1..N / 2).max_by(|&a, &b| mag(a).total_cmp(&mag(b))).unwrap();
        assert_eq!(peak, k);
    }

    #[test]
    fn bands_increase() {
        let e = band_edges(RATE);
        assert_eq!(e.len(), BANDS + 1);
        assert!(e.windows(2).all(|w| w[0] <= w[1]));
    }
}

#[cfg(test)]
mod live_tests {
    /// Справжнє захоплення звуку: `cargo test capture_runs -- --ignored`.
    #[test]
    #[ignore]
    fn capture_runs() {
        let a = super::Analyzer::shared();
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let s = a.snapshot();
        assert!(s.ok, "захоплення не запустилось");
        println!("рівні: {:?}", &s.bands[..8]);
        drop(a);
        std::thread::sleep(std::time::Duration::from_millis(300));
        // дочірні процеси саме цього тесту
        let me = std::process::id().to_string();
        let left = std::process::Command::new("pgrep")
            .args(["-x", "-P", &me, "parec"])
            .output()
            .unwrap();
        assert!(left.stdout.is_empty(), "parec не зупинився");
    }
}
