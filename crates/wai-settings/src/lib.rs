use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const DEFAULT_CLAUDE_COMMAND: &str = "npx -y @agentclientprotocol/claude-agent-acp@0.101.0";

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub default_agent: String,
    pub agents: BTreeMap<String, AgentConfig>,
    pub terminal: TerminalSettings,
    pub theme: ThemeChoice,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct AgentConfig {
    pub command: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct TerminalSettings {
    pub shell: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    #[default]
    Dark,
    Light,
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("invalid settings in {}: {message}", path.display())]
    Parse { path: PathBuf, message: String },
    #[error("cannot read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_agent: "claude".into(),
            agents: BTreeMap::from([(
                "claude".to_string(),
                AgentConfig {
                    command: DEFAULT_CLAUDE_COMMAND.into(),
                },
            )]),
            terminal: TerminalSettings::default(),
            theme: ThemeChoice::default(),
        }
    }
}

fn merge(base: &mut toml::Value, over: toml::Value) {
    match (base, over) {
        (toml::Value::Table(b), toml::Value::Table(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(slot) => merge(slot, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (slot, v) => *slot = v,
    }
}

fn read_layer(path: &Path) -> Result<Option<toml::Table>, SettingsError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(SettingsError::Io {
                path: path.into(),
                source,
            });
        }
    };
    toml::from_str(&text)
        .map(Some)
        .map_err(|e: toml::de::Error| SettingsError::Parse {
            path: path.into(),
            message: e.to_string(),
        })
}

/// `path` is the file whose layer was merged last.
fn parse(merged: toml::Value, path: &Path) -> Result<Settings, SettingsError> {
    merged
        .try_into()
        .map_err(|e: toml::de::Error| SettingsError::Parse {
            path: path.into(),
            message: e.to_string(),
        })
}

impl Settings {
    /// Merges defaults, then `<user_dir>/settings.toml`, then `<project_dir>/.wai/settings.toml`.
    /// An error in the user's file fails the load. The project file may not set `agents` or
    /// `terminal` (R15: opening a cloned repository must not choose what wai runs), so its
    /// `default_agent` can only name an agent the defaults or the user define. Those tables, or a
    /// project file that does not load, are left out; the returned notices say so.
    pub fn load(
        user_dir: &Path,
        project_dir: Option<&Path>,
    ) -> Result<(Settings, Vec<String>), SettingsError> {
        let user_path = user_dir.join("settings.toml");
        let mut merged = toml::Value::try_from(Settings::default()).expect("defaults serialize");
        if let Some(layer) = read_layer(&user_path)? {
            merge(&mut merged, toml::Value::Table(layer));
        }
        let user = parse(merged.clone(), &user_path)?;
        let Some(path) = project_dir.map(|p| p.join(".wai").join("settings.toml")) else {
            return Ok((user, Vec::new()));
        };
        let mut notices = Vec::new();
        let project = read_layer(&path).and_then(|layer| {
            let Some(mut layer) = layer else {
                return Ok(None);
            };
            let stripped = ["agents", "terminal"].map(|key| layer.remove(key));
            if stripped.iter().any(Option::is_some) {
                notices.push(format!(
                    "Ignored agents/terminal settings from {}: project settings cannot change commands.",
                    path.display()
                ));
            }
            merge(&mut merged, toml::Value::Table(layer));
            parse(merged, &path).map(Some)
        });
        match project {
            Ok(settings) => Ok((settings.unwrap_or(user), notices)),
            Err(e) => {
                notices.push(format!("Ignored project settings: {e}"));
                Ok((user, notices))
            }
        }
    }
}

pub fn config_dir() -> PathBuf {
    #[cfg(windows)]
    {
        dirs::config_dir().unwrap_or_default().join("wai")
    }
    #[cfg(not(windows))]
    {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".config")
            .join("wai")
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct AppState {
    pub project: Option<PathBuf>,
    pub open_sessions: Vec<OpenSession>,
    pub terminal_visible: bool,
    pub sidebar_width: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct OpenSession {
    pub agent: String,
    pub session_id: String,
    pub cwd: PathBuf,
    pub pane: usize,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            project: None,
            open_sessions: Vec::new(),
            terminal_visible: false,
            sidebar_width: 260.0,
        }
    }
}

impl AppState {
    /// Missing or corrupt files yield the default state.
    pub fn load(path: &Path) -> AppState {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Creates the folder on a first run.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_file_name("state.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, body: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn defaults_when_no_files() {
        let d = tempfile::tempdir().unwrap();
        let (s, _) = Settings::load(d.path(), None).unwrap();
        assert_eq!(s.default_agent, "claude");
        assert_eq!(
            s.agents["claude"].command,
            "npx -y @agentclientprotocol/claude-agent-acp@0.101.0"
        );
    }

    #[test]
    fn user_file_overrides_default() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "settings.toml", "theme = \"light\"\n");
        let (s, _) = Settings::load(d.path(), None).unwrap();
        assert_eq!(s.theme, ThemeChoice::Light);
        assert!(s.agents.contains_key("claude"));
    }

    /// R15: opening a cloned repository must not run what its settings say.
    #[test]
    fn project_cannot_set_agent_commands_or_shell() {
        let u = tempfile::tempdir().unwrap();
        let p = tempfile::tempdir().unwrap();
        write(
            u.path(),
            "settings.toml",
            "[terminal]\nshell = \"/bin/zsh\"\n",
        );
        write(
            p.path(),
            ".wai/settings.toml",
            "[agents.claude]\ncommand = \"evil\"\n[terminal]\nshell = \"/bin/evil\"\n",
        );
        let (s, notices) = Settings::load(u.path(), Some(p.path())).unwrap();
        assert_eq!(s.agents, Settings::default().agents);
        assert_eq!(s.terminal.shell.as_deref(), Some("/bin/zsh"));
        let path = p.path().join(".wai").join("settings.toml");
        assert_eq!(
            notices,
            [format!(
                "Ignored agents/terminal settings from {}: project settings cannot change commands.",
                path.display()
            )]
        );
    }

    #[test]
    fn project_overrides_theme_and_default_agent() {
        let u = tempfile::tempdir().unwrap();
        let p = tempfile::tempdir().unwrap();
        write(
            u.path(),
            "settings.toml",
            "theme = \"dark\"\n[agents.other]\ncommand = \"other-acp\"\n",
        );
        write(
            p.path(),
            ".wai/settings.toml",
            "theme = \"light\"\ndefault_agent = \"other\"\n",
        );
        let (s, notices) = Settings::load(u.path(), Some(p.path())).unwrap();
        assert_eq!(s.theme, ThemeChoice::Light);
        assert_eq!(s.default_agent, "other");
        assert_eq!(notices, Vec::<String>::new());
    }

    /// A broken project file costs the project its settings, not the launch.
    #[test]
    fn invalid_project_settings_fall_back_to_user() {
        let u = tempfile::tempdir().unwrap();
        write(u.path(), "settings.toml", "theme = \"light\"\n");
        for body in ["theme = [", "theme = 3\n"] {
            let p = tempfile::tempdir().unwrap();
            write(p.path(), ".wai/settings.toml", body);
            let (s, notices) = Settings::load(u.path(), Some(p.path())).unwrap();
            assert_eq!(s.theme, ThemeChoice::Light, "{body}");
            let path = p.path().join(".wai").join("settings.toml");
            let want = format!(
                "Ignored project settings: invalid settings in {}: ",
                path.display()
            );
            assert_eq!(notices.len(), 1, "{body}: {notices:?}");
            assert!(notices[0].starts_with(&want), "{body}: {notices:?}");
        }
    }

    #[test]
    fn invalid_toml_reports_path() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "settings.toml", "theme = [");
        match Settings::load(d.path(), None) {
            Err(SettingsError::Parse { path, .. }) => {
                assert_eq!(path, d.path().join("settings.toml"))
            }
            other => panic!("expected Parse error, got {other:?}"),
        }
    }

    #[test]
    fn state_roundtrip_and_corrupt_falls_back() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("wai").join("state.json");
        let state = AppState {
            project: Some("/tmp/proj".into()),
            open_sessions: vec![OpenSession {
                agent: "claude".into(),
                session_id: "s1".into(),
                cwd: "/tmp/proj".into(),
                pane: 1,
            }],
            terminal_visible: true,
            sidebar_width: 300.0,
        };
        state.save(&path).unwrap();
        assert_eq!(AppState::load(&path), state);
        std::fs::write(&path, "{").unwrap();
        assert_eq!(AppState::load(&path), AppState::default());
    }
}
