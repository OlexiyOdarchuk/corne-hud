//! Зв'язок вікна й команд із фоновою службою: рядки JSON через Unix-сокет.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::oled::{Anim, Frame, FB};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// Половина, підключена до USB
    Main,
    /// Друга половина
    Side,
    /// Обидві
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
    pub fn label(self) -> &'static str {
        match self {
            Screen::Status => "Статус: шар, WPM, модифікатори",
            Screen::Ship => "Корабель і астероїди",
            Screen::Pet => "Вихованець",
            Screen::Eq => "Еквалайзер",
            Screen::Pc => "Годинник, CPU/RAM, музика",
            Screen::Image => "Картинка / анімація",
        }
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
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Status(Status),
    Error { message: String },
}

pub fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("corne-hud.sock"),
        None => {
            use std::os::unix::fs::MetadataExt;
            let uid = std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(0);
            std::env::temp_dir().join(format!("corne-hud-{uid}.sock"))
        }
    }
}

pub fn encode_frame(f: &Frame) -> String {
    base64::engine::general_purpose::STANDARD.encode(f)
}

pub fn decode_frame(s: &str) -> Result<Frame, String> {
    let v = base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| e.to_string())?;
    v.try_into()
        .map_err(|v: Vec<u8>| format!("кадр має бути {FB} байт, а не {}", v.len()))
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
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Client {
    pub fn connect() -> Result<Self, String> {
        let s = UnixStream::connect(socket_path()).map_err(|_| "служба corne-hud не запущена".to_string())?;
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
            return Err("служба закрила з'єднання".into());
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
