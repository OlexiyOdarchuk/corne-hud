//! Platform differences: config location, fonts, external tools, starting the service.

use std::path::PathBuf;
use std::process::Command;

/// Settings and saved pictures.
/// Linux: `$XDG_CONFIG_HOME/corne-hud`; macOS: `~/Library/Application Support/corne-hud`;
/// Windows: `%APPDATA%\corne-hud`.
pub fn config_dir() -> PathBuf {
    let home = || PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(home)
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".config"))
    };
    base.join("corne-hud")
}

#[cfg(not(windows))]
/// Inside Flatpak the host tools (playerctl, busctl, parec…) are reached through `flatpak-spawn --host`.
fn in_flatpak() -> bool {
    cfg!(target_os = "linux") && std::path::Path::new("/.flatpak-info").exists()
}

#[cfg(not(windows))]
/// A command for a program installed on the system (not shipped with corne-hud).
pub fn host_command(program: &str) -> Command {
    if in_flatpak() {
        let mut c = Command::new("flatpak-spawn");
        c.args(["--host", program]);
        c
    } else {
        Command::new(program)
    }
}

/// The program to start as the background service. Inside an AppImage `current_exe()` points into
/// a temporary mount that disappears with the window, so the AppImage file itself is used.
pub fn service_exe() -> std::io::Result<PathBuf> {
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return Ok(PathBuf::from(appimage));
    }
    std::env::current_exe()
}

/// Detaches the service from the terminal or window that started it.
pub fn detach(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
}

/// Font files with Cyrillic, best first. `bold` picks a heavier weight where there is a choice.
/// On Linux fontconfig is asked first.
pub fn font_files(fc_pattern: &str, bold: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if cfg!(target_os = "linux") {
        if let Ok(o) = Command::new("fc-match").args(["-f", "%{file}", fc_pattern]).output() {
            let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !p.is_empty() {
                out.push(PathBuf::from(p));
            }
        }
        out.extend(
            if bold {
                [
                    "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
                    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
                ]
            } else {
                [
                    "/usr/share/fonts/TTF/DejaVuSans.ttf",
                    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                ]
            }
            .map(PathBuf::from),
        );
    } else if cfg!(windows) {
        let dir = PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
        let names: &[&str] = if bold {
            &["segoeuib.ttf", "arialbd.ttf", "tahomabd.ttf"]
        } else {
            &["segoeui.ttf", "arial.ttf", "tahoma.ttf"]
        };
        out.extend(names.iter().map(|n| dir.join(n)));
    } else if cfg!(target_os = "macos") {
        let names: &[&str] = if bold {
            &[
                "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
                "/Library/Fonts/Arial Bold.ttf",
                "/System/Library/Fonts/Supplemental/Verdana Bold.ttf",
            ]
        } else {
            &[
                "/System/Library/Fonts/Supplemental/Arial.ttf",
                "/Library/Fonts/Arial.ttf",
                "/System/Library/Fonts/Supplemental/Verdana.ttf",
            ]
        };
        out.extend(names.iter().map(PathBuf::from));
    }
    out
}
