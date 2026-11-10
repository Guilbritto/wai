use std::path::Path;

use toml_edit::{DocumentMut, Item, Table, value};

use crate::{SettingsError, ThemeChoice, read_config};

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    DefaultAgent(String),
    Theme(ThemeChoice),
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

fn set(t: &mut Table, key: &str, v: Option<toml_edit::Item>) {
    match v {
        Some(mut v) => {
            if let (Some(old), Some(new)) = (t.get(key).and_then(Item::as_value), v.as_value_mut())
            {
                *new.decor_mut() = old.decor().clone();
            }
            t.insert(key, v);
        }
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
        Change::Theme(theme) => {
            let name = match theme {
                ThemeChoice::Dark => "dark",
                ThemeChoice::Light => "light",
            };
            set(root, "theme", Some(value(name)));
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
    }
    std::fs::create_dir_all(user_dir).map_err(io)?;
    let tmp = user_dir.join("settings.toml.tmp");
    std::fs::write(&tmp, doc.to_string()).map_err(io)?;
    std::fs::rename(&tmp, &path).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Settings;

    #[test]
    fn write_keeps_comments_and_creates_the_file() {
        let d = tempfile::tempdir().unwrap();
        write_change(d.path(), &Change::Theme(ThemeChoice::Light)).unwrap();
        let p = d.path().join("settings.toml");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "theme = \"light\"\n");
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
    fn write_keeps_comments_on_changed_keys_and_inline_tables() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.toml");
        std::fs::write(
            &p,
            "theme = \"light\" # keep\nmodels = { codex = \"a\", other = \"y\" }\nterminal = { shell = \"a\", x = 1 }\n",
        )
        .unwrap();
        write_change(d.path(), &Change::Theme(ThemeChoice::Dark)).unwrap();
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
        let t: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(t["models"]["codex"].as_str(), Some("b"));
        assert_eq!(t["models"]["other"].as_str(), Some("y"));
        assert_eq!(t["terminal"]["shell"].as_str(), Some("z"));
        assert_eq!(t["terminal"]["x"].as_integer(), Some(1));
    }

    #[test]
    fn write_refuses_a_broken_file() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("settings.toml"), "theme = [").unwrap();
        assert!(matches!(
            write_change(d.path(), &Change::Theme(ThemeChoice::Dark)),
            Err(SettingsError::Parse { .. })
        ));
        assert_eq!(
            std::fs::read_to_string(d.path().join("settings.toml")).unwrap(),
            "theme = ["
        );
    }
}
