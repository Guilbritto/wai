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

fn read_layer(path: &Path) -> Result<Option<toml::Value>, SettingsError> {
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
    toml::from_str::<toml::Table>(&text)
        .map(|t| Some(toml::Value::Table(t)))
        .map_err(|e| SettingsError::Parse {
            path: path.into(),
            message: e.to_string(),
        })
}

impl Settings {
    /// Merges defaults, then `<user_dir>/settings.toml`, then `<project_dir>/.wai/settings.toml`.
    pub fn load(user_dir: &Path, project_dir: Option<&Path>) -> Result<Settings, SettingsError> {
        let mut merged = toml::Value::try_from(Settings::default()).expect("defaults serialize");
        let mut paths = vec![user_dir.join("settings.toml")];
        paths.extend(project_dir.map(|p| p.join(".wai").join("settings.toml")));
        for path in paths {
            if let Some(layer) = read_layer(&path)? {
                merge(&mut merged, layer);
            }
        }
        // The last file read is the one that can be at fault only if types are wrong;
        // report the user file path as the best available location.
        merged
            .try_into()
            .map_err(|e: toml::de::Error| SettingsError::Parse {
                path: user_dir.join("settings.toml"),
                message: e.to_string(),
            })
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

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
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
        let s = Settings::load(d.path(), None).unwrap();
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
        let s = Settings::load(d.path(), None).unwrap();
        assert_eq!(s.theme, ThemeChoice::Light);
        assert!(s.agents.contains_key("claude"));
    }

    #[test]
    fn project_overrides_user_per_key() {
        let u = tempfile::tempdir().unwrap();
        let p = tempfile::tempdir().unwrap();
        write(
            u.path(),
            "settings.toml",
            "[agents.claude]\ncommand = \"a\"\n[terminal]\nshell = \"/bin/zsh\"\n",
        );
        write(
            p.path(),
            ".wai/settings.toml",
            "[agents.claude]\ncommand = \"b\"\n",
        );
        let s = Settings::load(u.path(), Some(p.path())).unwrap();
        assert_eq!(s.agents["claude"].command, "b");
        assert_eq!(s.terminal.shell.as_deref(), Some("/bin/zsh"));
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
        let path = d.path().join("state.json");
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
