//! Interface language. Order: `CORNE_HUD_LANG`, the choice saved in settings, the system locale.
//! Translations live in `locales/*.yml`.

/// Available languages: code and self-name.
pub const LANGS: &[(&str, &str)] = &[("en", "English"), ("uk", "Українська")];

fn known(code: &str) -> Option<&'static str> {
    let base = code.split(['-', '_', '.', '@']).next()?.to_ascii_lowercase();
    LANGS.iter().map(|(c, _)| *c).find(|c| *c == base)
}

/// System language, if there is a translation for it; English otherwise.
pub fn system() -> &'static str {
    sys_locale::get_locales().find_map(|l| known(&l)).unwrap_or("en")
}

/// Sets the language for the given saved choice (empty means the system one).
pub fn apply(pref: &str) {
    let env = std::env::var("CORNE_HUD_LANG").ok();
    let lang = env
        .as_deref()
        .and_then(known)
        .or_else(|| known(pref))
        .unwrap_or_else(system);
    rust_i18n::set_locale(lang);
}

pub fn init() {
    apply(&crate::daemon::load_settings().lang);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_codes() {
        assert_eq!(known("uk_UA.UTF-8"), Some("uk"));
        assert_eq!(known("en-US"), Some("en"));
        assert_eq!(known("C"), None);
    }

    #[test]
    fn same_keys_in_every_language() {
        let keys = |src: &str| -> Vec<String> {
            let mut path: Vec<(usize, String)> = Vec::new();
            let mut out = Vec::new();
            for line in src.lines() {
                let t = line.trim_start();
                if t.is_empty() || t.starts_with('#') || t.starts_with("_version") {
                    continue;
                }
                let indent = line.len() - t.len();
                let Some((k, v)) = t.split_once(':') else { continue };
                path.retain(|(i, _)| *i < indent);
                path.push((indent, k.trim_matches('"').to_string()));
                if !v.trim().is_empty() {
                    out.push(path.iter().map(|(_, k)| k.as_str()).collect::<Vec<_>>().join("."));
                }
            }
            out.sort();
            out
        };
        let en = keys(include_str!("../locales/en.yml"));
        let uk = keys(include_str!("../locales/uk.yml"));
        assert_eq!(en, uk);
    }
}
