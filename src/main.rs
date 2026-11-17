// Windows: без чорного вікна консолі для GUI; команди підключаються до консолі, з якої їх запустили.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod audio;
mod daemon;
mod device;
mod draw;
mod gui;
mod i18n;
mod ipc;
mod keys;
mod live;
mod media;
mod oled;
mod paint;
mod pixfont;
mod platform;
mod presets;
mod sysinfo;
mod theme;

use std::io::Read;

use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};

use ipc::{Client, Request, Response, Screen, Target};
use oled::{ConvertOpts, Fit};

#[macro_use]
extern crate rust_i18n;

rust_i18n::i18n!("locales", fallback = "en");

#[derive(Parser)]
#[command(name = "corne-hud", version, about = t!("cli.about").to_string())]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    #[command(about = t!("cli.gui").to_string())]
    Gui,
    #[command(about = t!("cli.draw").to_string())]
    Draw,
    #[command(about = t!("cli.daemon").to_string())]
    Daemon,
    #[command(about = t!("cli.status").to_string())]
    Status,
    #[command(about = t!("cli.screen").to_string())]
    Screen {
        #[arg(value_enum)]
        target: Target,
        #[arg(value_enum)]
        screen: Screen,
        #[arg(long, help = t!("cli.no_save").to_string())]
        no_save: bool,
    },
    #[command(about = t!("cli.image").to_string())]
    Image {
        file: PathBuf,
        #[arg(short, long, value_enum, default_value = "side", help = t!("cli.image_screen").to_string())]
        screen: Target,
        #[arg(long, value_enum, default_value = "contain", help = t!("cli.fit").to_string())]
        fit: Fit,
        #[arg(long, help = t!("cli.no_dither").to_string())]
        no_dither: bool,
        #[arg(long, default_value_t = 128, help = t!("cli.threshold").to_string())]
        threshold: u8,
        #[arg(long, help = t!("cli.invert").to_string())]
        invert: bool,
        #[arg(long, help = t!("cli.image_no_save").to_string())]
        no_save: bool,
    },
    #[command(about = t!("cli.stream").to_string())]
    Stream {
        #[arg(short, long, value_enum, default_value = "side", help = t!("cli.image_screen").to_string())]
        screen: Target,
        #[arg(long, help = t!("cli.gray").to_string())]
        gray: bool,
        #[arg(long, default_value_t = 128, help = t!("cli.stream_threshold").to_string())]
        threshold: u8,
        #[arg(long, help = t!("cli.fps").to_string())]
        fps: Option<f32>,
    },
    #[command(about = t!("cli.preset").to_string())]
    Preset {
        id: String,
        #[arg(short, long, value_enum, default_value = "side", help = t!("cli.image_screen").to_string())]
        screen: Target,
        #[arg(short, long, default_value = "", help = t!("cli.preset_text").to_string())]
        text: String,
    },
    #[command(about = t!("cli.presets").to_string())]
    Presets,
    #[command(about = t!("cli.text").to_string())]
    Text {
        text: String,
        #[arg(short, long, value_enum, default_value = "side", help = t!("cli.image_screen").to_string())]
        screen: Target,
    },
    /// Demo GIFs of presets and drawing effects (for the docs).
    #[command(hide = true)]
    DocsMedia { dir: PathBuf },
    #[command(about = t!("cli.clear").to_string())]
    Clear {
        #[arg(value_enum, default_value = "both")]
        screen: Target,
    },
    #[command(about = t!("cli.off").to_string())]
    Off,
    #[command(about = t!("cli.on").to_string())]
    On,
}

/// Підключається до служби; якщо її немає — запускає у фоні.
pub fn ensure_daemon() -> Result<Client, String> {
    if let Ok(c) = Client::connect() {
        return Ok(c);
    }
    let exe = platform::service_exe().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(exe);
    cmd.arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    platform::detach(&mut cmd); // не завершуватись разом із терміналом
    cmd.spawn().map_err(|e| t!("err.daemon_start", err = e).to_string())?;
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(100));
        if let Ok(c) = Client::connect() {
            return Ok(c);
        }
    }
    Err(t!("err.daemon_not_started").into())
}

fn print_status(s: &ipc::Status) {
    if s.keyboard && s.paused {
        println!("{}", t!("cli.st_paused"));
    } else if s.keyboard {
        println!("{}", t!("cli.st_connected"));
    } else {
        println!("{}", t!("cli.st_not_found", err = s.error));
    }
    let name = |x: Option<Screen>| x.map_or("?".into(), |s| s.label());
    println!(
        "{}",
        t!("cli.st_main", mode = name(s.main_screen), content = s.main_content)
    );
    println!(
        "{}",
        t!("cli.st_side", mode = name(s.side_screen), content = s.side_content)
    );
    if s.screens_off {
        println!("{}", t!("cli.st_off"));
    }
    if !s.layout.is_empty() {
        let shown = if s.settings.layout_flash {
            t!("cli.st_layout_shown")
        } else {
            t!("cli.st_layout_hidden")
        };
        println!("{}", t!("cli.st_layout", layout = s.layout, shown = shown));
    }
    if s.keyboard && s.fw_proto == 1 {
        println!("{}", t!("cli.st_old_fw"));
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
            if n == 1 {
                println!("{}", t!("cli.sent_image"));
            } else {
                println!("{}", t!("cli.sent_anim", n = n));
            }
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
                println!("{:<8} {}", p.id, p.label());
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
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
    i18n::init();
    let cli = Cli::parse();
    match run(cli.cmd.unwrap_or(Cmd::Gui)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("corne-hud: {e}");
            ExitCode::FAILURE
        }
    }
}
