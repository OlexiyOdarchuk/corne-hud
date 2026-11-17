//! Зв'язок вікна й команд із фоновою службою: рядки JSON через Unix-сокет
//! (на Windows — через TCP на 127.0.0.1).

use std::io::{BufRead, BufReader, Write};
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::oled::{Anim, Frame, FB};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    #[value(help = t!("cli.t_main").to_string())]
    Main,
    #[value(help = t!("cli.t_side").to_string())]
    Side,
    #[value(help = t!("cli.t_both").to_string())]
    Both,
}

impl Target {
    pub fn mask(self) -> u8 {
        match self {
            Target::Main => 1,
            Target::Side => 2,
            Target::Both => 3,
        }
    }
    pub fn slots(self) -> &'static [usize] {
        match self {
            Target::Main => &[0],
            Target::Side => &[1],
            Target::Both => &[0, 1],
        }
    }
}

/// Режими екрана, порядок як у прошивці (oled_ui.h).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Screen {
    Status,
    Ship,
    Pet,
    Eq,
    Pc,
    Image,
}

impl Screen {
    pub const ALL: [Screen; 6] = [
        Screen::Status,
        Screen::Ship,
        Screen::Pet,
        Screen::Eq,
        Screen::Pc,
        Screen::Image,
    ];

    pub fn from_index(i: u8) -> Option<Screen> {
        Self::ALL.get(i as usize).copied()
    }
    pub fn index(self) -> u8 {
        self as u8
    }
    pub fn label(self) -> String {
        match self {
            Screen::Status => t!("screen.status"),
            Screen::Ship => t!("screen.ship"),
            Screen::Pet => t!("screen.pet"),
            Screen::Eq => t!("screen.eq"),
            Screen::Pc => t!("screen.pc"),
            Screen::Image => t!("screen.image"),
        }
        .into()
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Status,
    SetScreen {
        target: Target,
        screen: Screen,
        save: bool,
    },
    SetOff {
        off: bool,
    },
    /// Картинка чи анімація (кадри в base64). Режим цільового екрана перемикається на «картинку».
    Show {
        target: Target,
        frames: Vec<String>,
        delays_ms: Vec<u16>,
        persist: bool,
    },
    /// Один кадр потоку, без збереження.
    Frame {
        target: Target,
        frame: String,
    },
    Clear {
        target: Target,
    },
    /// Жива заготовка (див. presets.rs).
    Preset {
        target: Target,
        id: String,
        text: String,
    },
    /// Налаштування служби.
    Settings {
        settings: Settings,
    },
}

/// Налаштування, які служба пам'ятає між запусками.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Показувати розкладку клавіатури при перемиканні.
    pub layout_flash: bool,
    pub layout_target: Target,
    /// Interface language: `en`, `uk`…; empty means the system language.
    #[serde(default)]
    pub lang: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            layout_flash: true,
            layout_target: Target::Both,
            lang: String::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Status {
    pub keyboard: bool,
    pub error: String,
    pub main_screen: Option<Screen>,
    pub side_screen: Option<Screen>,
    pub screens_off: bool,
    /// Клавіатуру відкрила інша програма (Vial): служба мовчить, щоб не заважати.
    #[serde(default)]
    pub paused: bool,
    /// Що зараз на «картинці» кожної половини: опис для людини.
    pub main_content: String,
    pub side_content: String,
    /// Поточна розкладка (EN, UA…), порожньо — невідомо.
    #[serde(default)]
    pub layout: String,
    /// Версія протоколу прошивки (0 — ще невідома).
    #[serde(default)]
    pub fw_proto: u8,
    #[serde(default)]
    pub settings: Settings,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Status(Status),
    Error { message: String },
}

#[cfg(unix)]
pub type Stream = std::os::unix::net::UnixStream;
#[cfg(unix)]
pub type Listener = std::os::unix::net::UnixListener;
#[cfg(windows)]
pub type Stream = std::net::TcpStream;
#[cfg(windows)]
pub type Listener = std::net::TcpListener;

#[cfg(unix)]
pub fn socket_path() -> std::path::PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => std::path::PathBuf::from(dir).join("corne-hud.sock"),
        None => {
            let uid = unsafe { libc::getuid() };
            std::env::temp_dir().join(format!("corne-hud-{uid}.sock"))
        }
    }
}

#[cfg(windows)]
const ADDR: &str = "127.0.0.1:47653";

/// Де служба чекає з'єднань (для повідомлень).
pub fn endpoint() -> String {
    #[cfg(unix)]
    return socket_path().display().to_string();
    #[cfg(windows)]
    return ADDR.to_string();
}

pub fn connect() -> std::io::Result<Stream> {
    #[cfg(unix)]
    return Stream::connect(socket_path());
    #[cfg(windows)]
    return Stream::connect_timeout(&ADDR.parse().unwrap(), Duration::from_secs(1));
}

pub fn bind() -> Result<Listener, String> {
    #[cfg(unix)]
    {
        let path = socket_path();
        let _ = std::fs::remove_file(&path);
        Listener::bind(&path).map_err(|e| format!("{}: {e}", path.display()))
    }
    #[cfg(windows)]
    Listener::bind(ADDR).map_err(|e| format!("{ADDR}: {e}"))
}

pub fn encode_frame(f: &Frame) -> String {
    base64::engine::general_purpose::STANDARD.encode(f)
}

pub fn decode_frame(s: &str) -> Result<Frame, String> {
    let v = base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| e.to_string())?;
    v.try_into()
        .map_err(|v: Vec<u8>| t!("err.frame_size", need = FB, got = v.len()).into())
}

pub fn show_request(target: Target, anim: &Anim, persist: bool) -> Request {
    Request::Show {
        target,
        frames: anim.frames.iter().map(encode_frame).collect(),
        delays_ms: anim.delays_ms.clone(),
        persist,
    }
}

/// Постійне з'єднання зі службою.
pub struct Client {
    reader: BufReader<Stream>,
    writer: Stream,
}

impl Client {
    pub fn connect() -> Result<Self, String> {
        let s = connect().map_err(|_| t!("err.no_daemon").to_string())?;
        s.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let writer = s.try_clone().map_err(|e| e.to_string())?;
        Ok(Self {
            reader: BufReader::new(s),
            writer,
        })
    }

    pub fn call(&mut self, req: &Request) -> Result<Response, String> {
        let mut line = serde_json::to_string(req).map_err(|e| e.to_string())?;
        line.push('\n');
        self.writer.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
        let mut resp = String::new();
        self.reader.read_line(&mut resp).map_err(|e| e.to_string())?;
        if resp.is_empty() {
            return Err(t!("err.conn_closed").into());
        }
        serde_json::from_str(&resp).map_err(|e| e.to_string())
    }

    /// Те саме, але помилку з відповіді теж перетворює на Err.
    pub fn call_ok(&mut self, req: &Request) -> Result<Response, String> {
        match self.call(req)? {
            Response::Error { message } => Err(message),
            r => Ok(r),
        }
    }
}
