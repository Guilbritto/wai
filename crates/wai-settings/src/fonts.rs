//! Which font each area draws in, from the `[interface]`, `[editor]`, `[terminal]` and
//! `[conversation]` font keys. Pure: the caller says which families are installed.

use crate::Settings;

pub const UI_SIZE: f32 = 16.0;
pub const EDITOR_SIZE: f32 = 13.0;
pub const CHAT_SIZE: f32 = 13.0;
const MIN_SIZE: f32 = 6.0;
const MAX_SIZE: f32 = 100.0;

/// The families wai draws in when nothing is set.
#[derive(Debug, Clone, PartialEq)]
pub struct FontDefaults {
    pub ui_family: String,
    pub mono_family: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub family: String,
    pub size: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedFonts {
    pub ui: Face,
    pub editor: Face,
    pub terminal: Face,
    pub chat: Face,
    pub chat_code: Face,
}

/// The first installed family of a comma list (`'Fira Code', Menlo, monospace`); the generic
/// names count as installed. `None` when the list names no installed family.
fn first_installed(
    value: &str,
    defaults: &FontDefaults,
    installed: &dyn Fn(&str) -> bool,
) -> Option<String> {
    value
        .split(',')
        .map(|name| name.trim_matches(|c: char| c.is_whitespace() || c == '\'' || c == '"'))
        .filter(|name| !name.is_empty())
        .find_map(|name| match name {
            "monospace" => Some(defaults.mono_family.clone()),
            "sans-serif" | "system-ui" => Some(defaults.ui_family.clone()),
            _ => installed(name).then(|| name.to_string()),
        })
}

fn clamp(size: f32) -> f32 {
    size.clamp(MIN_SIZE, MAX_SIZE)
}

/// Resolves every area's face; the notices say which family lists named nothing installed.
pub fn resolve(
    settings: &Settings,
    defaults: &FontDefaults,
    installed: &dyn Fn(&str) -> bool,
) -> (ResolvedFonts, Vec<String>) {
    let mut notices = Vec::new();
    let mut face = |family: &Option<String>, size: Option<f32>, fallback: Face| {
        let family = match family.as_deref().filter(|f| !f.trim().is_empty()) {
            Some(value) => first_installed(value, defaults, installed).unwrap_or_else(|| {
                notices.push(format!(
                    "None of the fonts \"{value}\" is installed; using {}.",
                    fallback.family
                ));
                fallback.family.clone()
            }),
            None => fallback.family,
        };
        let size = size.filter(|s| s.is_finite()).map_or(fallback.size, clamp);
        Face { family, size }
    };
    let (ui, ed, t, c) = (
        &settings.interface,
        &settings.editor,
        &settings.terminal,
        &settings.conversation,
    );
    let ui = face(
        &ui.font_family,
        ui.font_size,
        Face {
            family: defaults.ui_family.clone(),
            size: UI_SIZE,
        },
    );
    let editor = face(
        &ed.font_family,
        ed.font_size,
        Face {
            family: defaults.mono_family.clone(),
            size: EDITOR_SIZE,
        },
    );
    let terminal = face(&t.font_family, t.font_size, editor.clone());
    let chat = face(
        &c.font_family,
        c.font_size,
        Face {
            family: ui.family.clone(),
            size: CHAT_SIZE,
        },
    );
    let chat_code = face(&c.code_font_family, c.code_font_size, editor.clone());
    (
        ResolvedFonts {
            ui,
            editor,
            terminal,
            chat,
            chat_code,
        },
        notices,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> FontDefaults {
        FontDefaults {
            ui_family: "Inter".into(),
            mono_family: "Menlo".into(),
        }
    }

    fn all(_: &str) -> bool {
        true
    }

    #[test]
    fn first_installed_family_wins() {
        let mut s = Settings::default();
        s.editor.font_family = Some("'Fira Code', Menlo, monospace".into());
        let only_menlo = |f: &str| f == "Menlo";
        let (r, notices) = resolve(&s, &defaults(), &only_menlo);
        assert_eq!(r.editor.family, "Menlo");
        assert!(notices.is_empty(), "{notices:?}");
        s.editor.font_family = Some("\"Fira Code\" , Menlo".into());
        let both = |f: &str| f == "Menlo" || f == "Fira Code";
        assert_eq!(resolve(&s, &defaults(), &both).0.editor.family, "Fira Code");
        s.editor.font_family = Some("Nope, monospace".into());
        let none = |_: &str| false;
        let (r, notices) = resolve(&s, &defaults(), &none);
        assert_eq!(r.editor.family, "Menlo");
        assert!(notices.is_empty());
        s.interface.font_family = Some("Nope, system-ui".into());
        assert_eq!(resolve(&s, &defaults(), &none).0.ui.family, "Inter");
    }

    #[test]
    fn no_installed_family_uses_default_with_notice() {
        let mut s = Settings::default();
        s.interface.font_family = Some("Nope, Missing".into());
        let none = |_: &str| false;
        let (r, notices) = resolve(&s, &defaults(), &none);
        assert_eq!(r.ui.family, "Inter");
        assert_eq!(
            notices,
            ["None of the fonts \"Nope, Missing\" is installed; using Inter."]
        );
        assert_eq!(r.editor.family, "Menlo");
    }

    #[test]
    fn sizes_clamp_to_6_and_100() {
        let mut s = Settings::default();
        s.interface.font_size = Some(2.0);
        s.editor.font_size = Some(400.0);
        s.conversation.font_size = Some(20.0);
        let (r, _) = resolve(&s, &defaults(), &all);
        assert_eq!((r.ui.size, r.editor.size, r.chat.size), (6.0, 100.0, 20.0));
        let d = resolve(&Settings::default(), &defaults(), &all).0;
        assert_eq!((d.ui.size, d.editor.size, d.chat.size), (16.0, 13.0, 13.0));
    }

    #[test]
    fn terminal_falls_back_to_editor() {
        let mut s = Settings::default();
        s.editor.font_family = Some("Fira".into());
        s.editor.font_size = Some(15.0);
        let (r, _) = resolve(&s, &defaults(), &all);
        assert_eq!(
            r.terminal,
            Face {
                family: "Fira".into(),
                size: 15.0
            }
        );
        s.terminal.font_family = Some("Term".into());
        s.terminal.font_size = Some(11.0);
        let (r, _) = resolve(&s, &defaults(), &all);
        assert_eq!(
            r.terminal,
            Face {
                family: "Term".into(),
                size: 11.0
            }
        );
    }

    #[test]
    fn chat_family_falls_back_to_interface() {
        let mut s = Settings::default();
        s.interface.font_family = Some("Sans".into());
        let (r, _) = resolve(&s, &defaults(), &all);
        assert_eq!(
            r.chat,
            Face {
                family: "Sans".into(),
                size: 13.0
            }
        );
        s.conversation.font_family = Some("Serif".into());
        s.conversation.font_size = Some(16.0);
        let (r, _) = resolve(&s, &defaults(), &all);
        assert_eq!(
            r.chat,
            Face {
                family: "Serif".into(),
                size: 16.0
            }
        );
    }

    #[test]
    fn chat_code_falls_back_to_editor() {
        let mut s = Settings::default();
        s.editor.font_family = Some("Fira".into());
        s.editor.font_size = Some(15.0);
        let (r, _) = resolve(&s, &defaults(), &all);
        assert_eq!(
            r.chat_code,
            Face {
                family: "Fira".into(),
                size: 15.0
            }
        );
        s.conversation.code_font_family = Some("Code".into());
        s.conversation.code_font_size = Some(12.0);
        let (r, _) = resolve(&s, &defaults(), &all);
        assert_eq!(
            r.chat_code,
            Face {
                family: "Code".into(),
                size: 12.0
            }
        );
    }
}
