use std::path::Path;

use toml_edit::{DocumentMut, Item, Table, value};

use crate::{Processing, SettingsError, ThemeChoice, read_config};

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    DefaultAgent(String),
    ThemeMode(ThemeChoice),
    /// The theme picked for the dark or light mode; `None` removes the key.
    ThemeName {
        dark: bool,
        name: Option<String>,
    },
    FontFamily(FontKey, Option<String>),
    FontSize(FontKey, Option<f32>),
    Shell(Option<String>),
    AgentCommand {
        agent: String,
        command: String,
    },
    Model {
        agent: String,
        model: Option<String>,
    },
    UpdatesCheck(bool),
    UpdatesRepo(Option<String>),
    Processing(Processing),
    /// Appends an index URL to `[extensions] indexes`, unless it is there already.
    AddIndex(String),
}

/// The area of the settings a font key belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontKey {
    Interface,
    Editor,
    Terminal,
    Chat,
    ChatCode,
}

impl FontKey {
    /// The table and the `font_family`/`font_size` key names of this area.
    fn keys(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Interface => ("interface", "font_family", "font_size"),
            Self::Editor => ("editor", "font_family", "font_size"),
            Self::Terminal => ("terminal", "font_family", "font_size"),
            Self::Chat => ("conversation", "font_family", "font_size"),
            Self::ChatCode => ("conversation", "code_font_family", "code_font_size"),
        }
    }
}

fn table<'a>(parent: &'a mut Table, name: &str) -> &'a mut Table {
    let item = parent
        .entry(name)
        .or_insert_with(|| Item::Table(Table::new()));
    if item.is_inline_table() {
        *item = Item::Table(std::mem::take(item).into_table().expect("an inline table"));
    } else if !item.is_table() {
        *item = Item::Table(Table::new());
    }
    let t = item.as_table_mut().expect("just made a table");
    t.set_implicit(true);
    t
}

/// `[theme]`; a legacy `theme = "x"` becomes `[theme] mode = "x"` first.
fn theme_table(root: &mut Table) -> &mut Table {
    if let Some(mode) = root.get("theme").and_then(|i| i.as_str()).map(String::from) {
        // The comment lines above the old key stay above the new header.
        let above = root
            .key("theme")
            .and_then(|k| k.leaf_decor().prefix().and_then(|p| p.as_str()))
            .map(String::from);
        root.remove("theme");
        let t = table(root, "theme");
        set(t, "mode", Some(value(mode)));
        if let Some(above) = above {
            t.decor_mut().set_prefix(above);
        }
    }
    table(root, "theme")
}

fn set(t: &mut Table, key: &str, v: Option<toml_edit::Item>) {
    match v {
        Some(mut v) => match t.get_mut(key) {
            Some(slot) => {
                if let (Some(old), Some(new)) = (slot.as_value(), v.as_value_mut()) {
                    *new.decor_mut() = old.decor().clone();
                }
                *slot = v;
            }
            None => {
                t.insert(key, v);
            }
        },
        None => {
            t.remove(key);
        }
    }
}

/// Changes one key of `<user_dir>/settings.toml`, keeping the rest of the file as written.
/// A file that does not parse is left alone.
pub fn write_change(user_dir: &Path, change: &Change) -> Result<(), SettingsError> {
    let path = user_dir.join("settings.toml");
    let io = |source| SettingsError::Io {
        path: path.clone(),
        source,
    };
    let text = read_config(&path).map_err(io)?.unwrap_or_default();
    let parse = |message: String| SettingsError::Parse {
        path: path.clone(),
        message,
    };
    toml::from_str::<toml::Table>(&text).map_err(|e| parse(e.to_string()))?;
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| parse(e.to_string()))?;
    let root = doc.as_table_mut();
    match change {
        Change::DefaultAgent(id) => {
            set(root, "default_agent", Some(value(id.as_str())));
        }
        Change::ThemeMode(mode) => {
            let name = match mode {
                ThemeChoice::Dark => "dark",
                ThemeChoice::Light => "light",
                ThemeChoice::System => "system",
            };
            // A legacy `theme = "x"` is rewritten in place, keeping its comment.
            if root.get("theme").is_some_and(Item::is_str) {
                set(root, "theme", Some(value(name)));
            } else {
                set(theme_table(root), "mode", Some(value(name)));
            }
        }
        Change::ThemeName { dark, name } => {
            let key = if *dark { "dark" } else { "light" };
            set(theme_table(root), key, name.as_deref().map(value));
        }
        Change::FontFamily(font, family) => {
            let (area, key, _) = font.keys();
            set(table(root, area), key, family.as_deref().map(value));
        }
        Change::FontSize(font, size) => {
            let (area, _, key) = font.keys();
            // Through the text, so 13.3 is written as 13.3 and not as its f64 widening.
            let size = size.and_then(|s| s.to_string().parse::<f64>().ok());
            set(table(root, area), key, size.map(value));
        }
        Change::Shell(shell) => set(
            table(root, "terminal"),
            "shell",
            shell.as_deref().map(value),
        ),
        Change::AgentCommand { agent, command } => {
            let agent = table(table(root, "agents"), agent);
            set(agent, "command", Some(value(command.as_str())));
        }
        Change::Model { agent, model } => {
            set(table(root, "models"), agent, model.as_deref().map(value))
        }
        Change::UpdatesCheck(on) => {
            set(table(root, "updates"), "check", Some(value(*on)));
        }
        Change::UpdatesRepo(repo) => {
            set(table(root, "updates"), "repo", repo.as_deref().map(value))
        }
        Change::Processing(mode) => {
            let name = match mode {
                Processing::Steps => "steps",
                Processing::Grouped => "grouped",
                Processing::Hidden => "hidden",
            };
            set(table(root, "conversation"), "processing", Some(value(name)));
        }
        Change::AddIndex(url) => {
            let ext = table(root, "extensions");
            if !ext.get("indexes").is_some_and(|i| i.is_array()) {
                ext.insert("indexes", value(toml_edit::Array::new()));
            }
            let list = ext["indexes"].as_array_mut().expect("just made an array");
            if !list.iter().any(|v| v.as_str() == Some(url)) {
                list.push(url.as_str());
            }
        }
    }
    std::fs::create_dir_all(user_dir).map_err(io)?;
    let tmp = user_dir.join("settings.toml.tmp");
    std::fs::write(&tmp, doc.to_string()).map_err(io)?;
    std::fs::rename(&tmp, &path).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Processing, Settings};

    #[test]
    fn write_keeps_comments_and_creates_the_file() {
        let d = tempfile::tempdir().unwrap();
        write_change(d.path(), &Change::ThemeMode(ThemeChoice::Light)).unwrap();
        let p = d.path().join("settings.toml");
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "[theme]\nmode = \"light\"\n"
        );
        std::fs::remove_file(&p).unwrap();
        std::fs::write(&p, "# mine\ntheme = \"light\" # keep\n").unwrap();
        write_change(d.path(), &Change::DefaultAgent("codex".into())).unwrap();
        write_change(
            d.path(),
            &Change::Model {
                agent: "codex".into(),
                model: Some("gpt-5".into()),
            },
        )
        .unwrap();
        write_change(
            d.path(),
            &Change::AgentCommand {
                agent: "codex".into(),
                command: "codex-acp".into(),
            },
        )
        .unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(
            text.starts_with("# mine\ntheme = \"light\" # keep\n"),
            "{text}"
        );
        let (s, _) = Settings::load(d.path(), None).unwrap();
        assert_eq!(s.default_agent.as_deref(), Some("codex"));
        assert_eq!(s.models["codex"], "gpt-5");
        assert_eq!(s.agents["codex"].command, "codex-acp");
        write_change(
            d.path(),
            &Change::Model {
                agent: "codex".into(),
                model: None,
            },
        )
        .unwrap();
        write_change(d.path(), &Change::Shell(None)).unwrap();
        let (s, _) = Settings::load(d.path(), None).unwrap();
        assert!(!s.models.contains_key("codex") && s.terminal.shell.is_none());
    }

    #[test]
    fn add_index_appends_once() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.toml");
        std::fs::write(&p, "# mine\ntheme = \"light\"\n").unwrap();
        write_change(d.path(), &Change::AddIndex("https://a/i.toml".into())).unwrap();
        write_change(d.path(), &Change::AddIndex("https://b/i.toml".into())).unwrap();
        write_change(d.path(), &Change::AddIndex("https://a/i.toml".into())).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("# mine\ntheme = \"light\"\n"), "{text}");
        let (s, _) = Settings::load(d.path(), None).unwrap();
        assert_eq!(
            s.extensions.indexes,
            ["https://a/i.toml", "https://b/i.toml"]
        );
    }

    #[test]
    fn write_keeps_comments_on_changed_keys_and_inline_tables() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.toml");
        std::fs::write(
            &p,
            "theme = \"light\" # keep\nmodels = { codex = \"a\", other = \"y\" }\nterminal = { shell = \"a\", x = 1 }\n",
        )
        .unwrap();
        write_change(d.path(), &Change::ThemeMode(ThemeChoice::Dark)).unwrap();
        write_change(
            d.path(),
            &Change::Model {
                agent: "codex".into(),
                model: Some("b".into()),
            },
        )
        .unwrap();
        write_change(d.path(), &Change::Shell(Some("z".into()))).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("theme = \"dark\" # keep"), "{text}");
        std::fs::write(
            &p,
            "# mine\ntheme = \"light\" # keep\n[agents.codex]\n# cmd\ncommand = \"a\" # c\n",
        )
        .unwrap();
        write_change(d.path(), &Change::ThemeMode(ThemeChoice::Dark)).unwrap();
        write_change(
            d.path(),
            &Change::AgentCommand {
                agent: "codex".into(),
                command: "b".into(),
            },
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "# mine\ntheme = \"dark\" # keep\n[agents.codex]\n# cmd\ncommand = \"b\" # c\n"
        );
        let t: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(t["models"]["codex"].as_str(), Some("b"));
        assert_eq!(t["models"]["other"].as_str(), Some("y"));
        assert_eq!(t["terminal"]["shell"].as_str(), Some("z"));
        assert_eq!(t["terminal"]["x"].as_integer(), Some(1));
    }

    #[test]
    fn write_sets_the_processing_mode() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.toml");
        std::fs::write(&p, "# mine\ntheme = \"light\"\n").unwrap();
        write_change(d.path(), &Change::Processing(Processing::Grouped)).unwrap();
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "# mine\ntheme = \"light\"\n\n[conversation]\nprocessing = \"grouped\"\n"
        );
        write_change(d.path(), &Change::Processing(Processing::Hidden)).unwrap();
        write_change(d.path(), &Change::Processing(Processing::Steps)).unwrap();
        let (s, _) = Settings::load(d.path(), None).unwrap();
        assert_eq!(s.conversation.processing, Processing::Steps);
        assert_eq!(s.theme.mode, ThemeChoice::Light);
        write_change(d.path(), &Change::Processing(Processing::Hidden)).unwrap();
        let (s, _) = Settings::load(d.path(), None).unwrap();
        assert_eq!(s.conversation.processing, Processing::Hidden);
    }

    #[test]
    fn theme_keys_are_written() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.toml");
        let read = || std::fs::read_to_string(&p).unwrap();
        let name = |dark, n: Option<&str>| Change::ThemeName {
            dark,
            name: n.map(String::from),
        };
        std::fs::write(&p, "# mine\ntheme = \"light\"\n").unwrap();
        write_change(d.path(), &name(true, Some("Night"))).unwrap();
        assert_eq!(
            read(),
            "# mine\n[theme]\nmode = \"light\"\ndark = \"Night\"\n"
        );
        write_change(d.path(), &name(false, Some("Day"))).unwrap();
        write_change(d.path(), &Change::ThemeMode(ThemeChoice::System)).unwrap();
        let (s, _) = Settings::load(d.path(), None).unwrap();
        assert_eq!(s.theme.mode, ThemeChoice::System);
        assert_eq!(s.theme.dark.as_deref(), Some("Night"));
        assert_eq!(s.theme.light.as_deref(), Some("Day"));
        assert!(read().starts_with("# mine\n"));
        write_change(d.path(), &name(true, None)).unwrap();
        let (s, _) = Settings::load(d.path(), None).unwrap();
        assert_eq!(
            (s.theme.dark, s.theme.light.as_deref()),
            (None, Some("Day"))
        );
        // A mode written over a legacy string keeps the string form and its comment.
        std::fs::write(&p, "theme = \"light\" # keep\n").unwrap();
        write_change(d.path(), &Change::ThemeMode(ThemeChoice::System)).unwrap();
        assert_eq!(read(), "theme = \"system\" # keep\n");
    }

    #[test]
    fn font_keys_are_written_and_removed() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.toml");
        std::fs::write(&p, "# mine\n[terminal]\nshell = \"zsh\" # keep\n").unwrap();
        let areas = [
            FontKey::Interface,
            FontKey::Editor,
            FontKey::Terminal,
            FontKey::Chat,
            FontKey::ChatCode,
        ];
        for (i, area) in areas.into_iter().enumerate() {
            let family = Change::FontFamily(area, Some(format!("F{i}")));
            write_change(d.path(), &family).unwrap();
            write_change(
                d.path(),
                &Change::FontSize(area, Some(12.0 + i as f32 + 0.3)),
            )
            .unwrap();
        }
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(
            text.starts_with("# mine\n[terminal]\nshell = \"zsh\" # keep\n"),
            "{text}"
        );
        assert!(text.contains("code_font_size = 16.3"), "{text}");
        let (s, _) = Settings::load(d.path(), None).unwrap();
        assert_eq!(s.terminal.shell.as_deref(), Some("zsh"));
        assert_eq!(s.interface.font_family.as_deref(), Some("F0"));
        assert_eq!(s.interface.font_size, Some(12.3));
        assert_eq!(s.editor.font_family.as_deref(), Some("F1"));
        assert_eq!(s.terminal.font_family.as_deref(), Some("F2"));
        assert_eq!(s.terminal.font_size, Some(14.3));
        assert_eq!(s.conversation.font_family.as_deref(), Some("F3"));
        assert_eq!(s.conversation.font_size, Some(15.3));
        assert_eq!(s.conversation.code_font_family.as_deref(), Some("F4"));
        assert_eq!(s.conversation.code_font_size, Some(16.3));
        for area in areas {
            write_change(d.path(), &Change::FontFamily(area, None)).unwrap();
            write_change(d.path(), &Change::FontSize(area, None)).unwrap();
        }
        let (s, _) = Settings::load(d.path(), None).unwrap();
        assert_eq!(s.terminal.shell.as_deref(), Some("zsh"));
        assert_eq!(s.interface, crate::FontArea::default());
        assert_eq!(s.conversation.font_family, None);
        assert_eq!(s.conversation.code_font_size, None);
        assert!(
            std::fs::read_to_string(&p)
                .unwrap()
                .contains("shell = \"zsh\" # keep")
        );
    }

    #[test]
    fn write_refuses_a_broken_file() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("settings.toml"), "theme = [").unwrap();
        assert!(matches!(
            write_change(d.path(), &Change::ThemeMode(ThemeChoice::Dark)),
            Err(SettingsError::Parse { .. })
        ));
        assert_eq!(
            std::fs::read_to_string(d.path().join("settings.toml")).unwrap(),
            "theme = ["
        );
    }
}
