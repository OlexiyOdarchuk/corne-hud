mod audio;
mod daemon;
mod device;
mod draw;
mod gui;
mod ipc;
mod keys;
mod live;
mod media;
mod oled;
mod paint;
mod pixfont;
mod presets;
mod sysinfo;
mod theme;

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};

use ipc::{Client, Request, Response, Screen, Target};
use oled::{ConvertOpts, Fit};

/// Екрани Corne: режими, картинки, GIF, потік кадрів, годинник/CPU/RAM/музика.
/// Без команди відкриває вікно.
#[derive(Parser)]
#[command(name = "corne-hud", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Вікно керування
    Gui,
    /// Вікно, одразу на вкладці малювання
    Draw,
    /// Фонова служба (запускається сама, коли потрібна)
    Daemon,
    /// Стан клавіатури й екранів
    Status,
    /// Вибрати режим екрана
    Screen {
        #[arg(value_enum)]
        target: Target,
        #[arg(value_enum)]
        screen: Screen,
        /// Не запам'ятовувати після вимкнення клавіатури
        #[arg(long)]
        no_save: bool,
    },
    /// Показати картинку або GIF
    Image {
        file: PathBuf,
        /// На який екран
        #[arg(short, long, value_enum, default_value = "side")]
        screen: Target,
        #[arg(long, value_enum, default_value = "contain")]
        fit: Fit,
        /// Без розсіювання: чіткий поріг яскравості
        #[arg(long)]
        no_dither: bool,
        /// Поріг яскравості 0..255 (разом із --no-dither)
        #[arg(long, default_value_t = 128)]
        threshold: u8,
        #[arg(long)]
        invert: bool,
        /// Не відновлювати після перезапуску служби
        #[arg(long)]
        no_save: bool,
    },
    /// Потік кадрів зі stdin: 512 байт на кадр у форматі екрана,
    /// або з --gray 4096 байт (128x32, 1 байт на піксель, як ffmpeg -pix_fmt gray)
    Stream {
        #[arg(short, long, value_enum, default_value = "side")]
        screen: Target,
        #[arg(long)]
        gray: bool,
        /// Поріг для --gray
        #[arg(long, default_value_t = 128)]
        threshold: u8,
        /// Обмежити частоту кадрів (екран показує до 20 кадрів/с)
        #[arg(long)]
        fps: Option<f32>,
    },
    /// Увімкнути заготовку (список — `corne-hud presets`)
    Preset {
        id: String,
        #[arg(short, long, value_enum, default_value = "side")]
        screen: Target,
        /// Текст для заготовок «text» і «bounce»
        #[arg(short, long, default_value = "")]
        text: String,
    },
    /// Список заготовок
    Presets,
    /// Показати свій текст (будь-якою мовою); довгий біжить рядком
    Text {
        text: String,
        #[arg(short, long, value_enum, default_value = "side")]
        screen: Target,
    },
    /// Демо-GIF заготовок і ефектів малювання (для документації)
    #[command(hide = true)]
    DocsMedia { dir: PathBuf },
    /// Прибрати картинку
    Clear {
        #[arg(value_enum, default_value = "both")]
        screen: Target,
    },
    /// Вимкнути екрани
    Off,
    /// Увімкнути екрани
    On,
}

/// Підключається до служби; якщо її немає — запускає у фоні.
pub fn ensure_daemon() -> Result<Client, String> {
    if let Ok(c) = Client::connect() {
        return Ok(c);
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Command::new(exe)
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0) // не завершуватись разом із терміналом
        .spawn()
        .map_err(|e| format!("не вдалося запустити службу: {e}"))?;
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(100));
        if let Ok(c) = Client::connect() {
            return Ok(c);
        }
    }
    Err("служба не запустилась".into())
}

fn print_status(s: &ipc::Status) {
    if s.keyboard && s.paused {
        println!("Клавіатура: підключена, пауза — її відкрила інша програма (Vial)");
    } else if s.keyboard {
        println!("Клавіатура: підключена");
    } else {
        println!("Клавіатура: не підключена ({})", s.error);
    }
    let name = |x: Option<Screen>| x.map_or("?".into(), |s| s.label().to_string());
    println!("Половина з USB: {} | картинка: {}", name(s.main_screen), s.main_content);
    println!("Друга половина: {} | картинка: {}", name(s.side_screen), s.side_content);
    if s.screens_off {
        println!("Екрани вимкнені");
    }
    if !s.layout.is_empty() {
        let shown = if s.settings.layout_flash {
            "показується при перемиканні"
        } else {
            "не показується"
        };
        println!("Розкладка: {} ({shown})", s.layout);
    }
    if s.keyboard && s.fw_proto == 1 {
        println!("Прошивка протоколу 1: розкладка видна лише на екранах із картинкою чи заготовкою");
    }
}

fn run(cmd: Cmd) -> Result<(), String> {
    match cmd {
        Cmd::Gui => gui::run(false),
        Cmd::Draw => gui::run(true),
        Cmd::Daemon => daemon::run(),
        Cmd::Status => match ensure_daemon()?.call_ok(&Request::Status)? {
            Response::Status(s) => {
                print_status(&s);
                Ok(())
            }
            _ => Ok(()),
        },
        Cmd::Screen {
            target,
            screen,
            no_save,
        } => ensure_daemon()?
            .call_ok(&Request::SetScreen {
                target,
                screen,
                save: !no_save,
            })
            .map(|_| ()),
        Cmd::Image {
            file,
            screen,
            fit,
            no_dither,
            threshold,
            invert,
            no_save,
        } => {
            let opts = ConvertOpts {
                fit,
                dither: !no_dither,
                threshold,
                invert,
            };
            let anim = oled::Source::open(&file)?.convert(&opts);
            let n = anim.frames.len();
            ensure_daemon()?.call_ok(&ipc::show_request(screen, &anim, !no_save))?;
            println!(
                "Надіслано: {}",
                if n == 1 {
                    "картинка".to_string()
                } else {
                    format!("анімація, кадрів: {n}")
                }
            );
            Ok(())
        }
        Cmd::Stream {
            screen,
            gray,
            threshold,
            fps,
        } => {
            let mut client = ensure_daemon()?;
            let size = if gray { oled::W * oled::H } else { oled::FB };
            let period = fps.filter(|f| *f > 0.0).map(|f| Duration::from_secs_f32(1.0 / f));
            let mut buf = vec![0u8; size];
            let mut stdin = std::io::stdin().lock();
            let mut next = Instant::now();
            while stdin.read_exact(&mut buf).is_ok() {
                let frame = if gray {
                    oled::from_gray(&buf, threshold)
                } else {
                    buf.as_slice().try_into().unwrap()
                };
                client.call_ok(&Request::Frame {
                    target: screen,
                    frame: ipc::encode_frame(&frame),
                })?;
                if let Some(p) = period {
                    next += p;
                    if let Some(wait) = next.checked_duration_since(Instant::now()) {
                        std::thread::sleep(wait);
                    }
                }
            }
            Ok(())
        }
        Cmd::Preset { id, screen, text } => ensure_daemon()?
            .call_ok(&Request::Preset {
                target: screen,
                id,
                text,
            })
            .map(|_| ()),
        Cmd::Presets => {
            for p in presets::PRESETS {
                println!("{:<8} {}", p.id, p.label);
            }
            Ok(())
        }
        Cmd::Text { text, screen } => ensure_daemon()?
            .call_ok(&Request::Preset {
                target: screen,
                id: "text".into(),
                text,
            })
            .map(|_| ()),
        Cmd::DocsMedia { dir } => media::render(&dir),
        Cmd::Clear { screen } => ensure_daemon()?.call_ok(&Request::Clear { target: screen }).map(|_| ()),
        Cmd::Off => ensure_daemon()?.call_ok(&Request::SetOff { off: true }).map(|_| ()),
        Cmd::On => ensure_daemon()?.call_ok(&Request::SetOff { off: false }).map(|_| ()),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.cmd.unwrap_or(Cmd::Gui)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("corne-hud: {e}");
            ExitCode::FAILURE
        }
    }
}
