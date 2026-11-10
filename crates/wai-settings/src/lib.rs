use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

mod approve;
mod edit;
pub use approve::{Approval, is_approved, seed_approved, unapproved};
pub use edit::{Change, write_change};

/// An agent wai knows how to start, with the hints Settings shows when it is not ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinAgent {
    pub id: &'static str,
    pub name: &'static str,
    pub command: &'static str,
    pub install: &'static str,
    pub login: &'static str,
}

pub const BUILTIN_AGENTS: [BuiltinAgent; 4] = [
    BuiltinAgent {
        id: "claude",
        name: "Claude",
        command: "npx -y @agentclientprotocol/claude-agent-acp@0.101.0",
        install: "Install Node.js (it provides npx)",
        login: "claude /login",
    },
    BuiltinAgent {
        id: "codex",
        name: "Codex",
        command: "npx -y @agentclientprotocol/codex-acp@2.6.0",
        install: "Install Node.js (it provides npx)",
        login: "npx @openai/codex login",
    },
    BuiltinAgent {
        id: "antigravity",
        name: "Antigravity",
        command: "agy acp",
        install: "https://antigravity.google/docs/cli",
        login: "agy login",
    },
    BuiltinAgent {
        id: "copilot",
        name: "Copilot",
        command: "copilot --acp --stdio",
        install: "npm install -g @github/copilot",
        login: "copilot, then /login",
    },
];

pub fn builtin(id: &str) -> Option<&'static BuiltinAgent> {
    BUILTIN_AGENTS.iter().find(|a| a.id == id)
}

/// The built-in's name, else the id as given (an extension agent's id is its own label).
pub fn agent_label(id: &str) -> String {
    builtin(id).map_or(id, |a| a.name).to_string()
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    /// `None` until the user chooses one: wai never picks an AI for them.
    pub default_agent: Option<String>,
    pub agents: BTreeMap<String, AgentConfig>,
    /// Agent id -> the id of the model (config value) it starts with; user settings only.
    pub models: BTreeMap<String, String>,
    pub terminal: TerminalSettings,
    pub theme: ThemeChoice,
    pub updates: UpdateSettings,
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

/// The opt-in update check, from the user's settings only (see `Settings::load`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// Off by default: wai makes no request unless the user turns this on.
    pub check: bool,
    /// `owner/repo` on GitHub whose latest release is checked. Unset, nothing is checked; this
    /// default is the placeholder until wai has a public repository.
    pub repo: Option<String>,
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
            default_agent: None,
            agents: BUILTIN_AGENTS
                .iter()
                .map(|a| {
                    let command = a.command.into();
                    (a.id.to_string(), AgentConfig { command })
                })
                .collect(),
            models: BTreeMap::new(),
            terminal: TerminalSettings::default(),
            theme: ThemeChoice::default(),
            updates: UpdateSettings::default(),
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

/// Config files are small; one past this is not read.
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// A config file's text; `None` when it does not exist. R41: a repository can ship a huge file,
/// a FIFO or a link to /dev/zero as `.wai/…`: only a regular file is opened, and at most 1 MB
/// of it is read.
pub fn read_config(path: &Path) -> std::io::Result<Option<String>> {
    use std::io::{Error, ErrorKind, Read as _};
    match std::fs::metadata(path) {
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
        Ok(meta) if !meta.is_file() => return Err(Error::other("not a regular file")),
        Ok(_) => {}
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(Error::other("larger than 1 MB"));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|e| Error::new(ErrorKind::InvalidData, e))
}

fn read_layer(path: &Path) -> Result<Option<toml::Table>, SettingsError> {
    let text = match read_config(path) {
        Ok(Some(t)) => t,
        Ok(None) => return Ok(None),
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
    /// `terminal` (R15: opening a cloned repository must not choose what wai runs), nor `updates`
    /// or `models`; its `default_agent` counts only when the user already chose one, and names it,
    /// an agent in the user's own `[agents]`, or an extension agent (an id with a `/`). Those
    /// tables, or a project file that does not load, are left out; the returned notices say so.
    pub fn load(
        user_dir: &Path,
        project_dir: Option<&Path>,
    ) -> Result<(Settings, Vec<String>), SettingsError> {
        let user_path = user_dir.join("settings.toml");
        let mut merged = toml::Value::try_from(Settings::default()).expect("defaults serialize");
        let mut user_agents = Vec::new();
        if let Some(layer) = read_layer(&user_path)? {
            if let Some(toml::Value::Table(agents)) = layer.get("agents") {
                user_agents = agents.keys().cloned().collect();
            }
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
            // Nor may it make wai send a request, or name the repository whose page wai opens.
            if layer.remove("updates").is_some() {
                notices.push(format!(
                    "Ignored updates settings from {}: only user settings can turn on the update check.",
                    path.display()
                ));
            }
            if layer.remove("models").is_some() {
                notices.push(format!(
                    "Ignored models settings from {}: only user settings can choose default models.",
                    path.display()
                ));
            }
            if let Some(name) = layer.get("default_agent").and_then(toml::Value::as_str) {
                let kept = user.default_agent.as_deref().is_some_and(|chosen| {
                    name == chosen || user_agents.iter().any(|a| a == name) || name.contains('/')
                });
                if !kept {
                    notices.push(format!(
                        "Ignored default_agent \"{name}\" from {}: a project can only pick an AI you already chose or set up.",
                        path.display()
                    ));
                    layer.remove("default_agent");
                }
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

/// The `default_agent` of `<project_dir>/.wai/settings.toml` as written, whether or not `load`
/// honours it. `None` when there is none or the file does not load.
pub fn project_default_agent(project_dir: &Path) -> Option<String> {
    let layer = read_layer(&project_dir.join(".wai").join("settings.toml")).ok()??;
    layer.get("default_agent")?.as_str().map(str::to_string)
}

/// A named starting point for a conversation: which agent, and a first prompt that is
/// pre-filled but not sent.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Template {
    pub name: String,
    /// `None`: the default agent. It can only name an agent the user or the defaults define.
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub prompt: String,
}

#[derive(Deserialize)]
struct TemplateFile {
    #[serde(default)]
    template: Vec<Template>,
}

fn read_templates(path: &Path, notices: &mut Vec<String>) -> Vec<Template> {
    let text = match read_config(path) {
        Ok(Some(t)) => t,
        Ok(None) => return Vec::new(),
        Err(e) => {
            notices.push(format!(
                "Ignored templates: cannot read {}: {e}",
                path.display()
            ));
            return Vec::new();
        }
    };
    match toml::from_str::<TemplateFile>(&text) {
        Ok(file) => file.template,
        Err(e) => {
            notices.push(format!("Ignored templates in {}: {e}", path.display()));
            Vec::new()
        }
    }
}

/// `<user_dir>/templates.toml` then `<project_dir>/.wai/templates.toml`, each a list of
/// `[[template]]` tables (`name`, optional `agent`, `prompt`). Project entries come after the
/// user's; a project template replaces a user one of the same name. A file that does not load
/// is skipped with a notice (templates never block a launch or hold code, so the project may
/// define them, R15).
pub fn load_templates(user_dir: &Path, project_dir: Option<&Path>) -> (Vec<Template>, Vec<String>) {
    let mut notices = Vec::new();
    let mut all = read_templates(&user_dir.join("templates.toml"), &mut notices);
    if let Some(dir) = project_dir {
        let project = read_templates(&dir.join(".wai").join("templates.toml"), &mut notices);
        all.retain(|t| !project.iter().any(|p| p.name == t.name));
        all.extend(project);
    }
    (all, notices)
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

/// R3: `/tmp` and `/private/tmp` are one folder; a path that does not exist stays as given. On
/// Windows without the `\\?\` prefix (dunce), which keeps it only where dropping it would name
/// another file: what wai compares, shows, sends to agents and uses as a terminal cwd.
pub fn canonical(path: &Path) -> PathBuf {
    try_canonical(path).unwrap_or_else(|_| path.to_path_buf())
}

/// `canonical`, or why the path does not resolve.
pub fn try_canonical(path: &Path) -> std::io::Result<PathBuf> {
    dunce::canonicalize(path)
}

/// A path as wai shows it: never with Windows' verbatim prefix (`\\?\C:\x` shows as `C:\x`,
/// `\\?\UNC\host\share` as `\\host\share`), even where `canonical` has to keep it.
pub fn shown(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.to_string(),
        _ => text.into_owned(),
    }
}

/// The program to start for `command`. A bare name is looked up on PATH here, never left to the
/// OS: Windows searches a process's current dir first, and a unix PATH may name it (an empty
/// entry, `.`); agents and MCP servers run in the project. On Windows with PATHEXT (`npx` is
/// `npx.cmd`). `None` when PATH has no such program. A path stays as given.
pub fn find_program(command: &Path) -> Option<PathBuf> {
    if command.file_name() != Some(command.as_os_str()) {
        return Some(command.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    let pathext = match cfg!(windows) {
        true => std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into()),
        false => String::new(),
    };
    resolve_on_path(command.as_os_str(), &path, &pathext)
}

/// The first program `name` names in `path`'s absolute dirs (a relative entry would be the
/// current dir's). With a `pathext`, the Windows way: a file with each extension, and as given
/// only when it has one. Without (unix): as given, an executable file.
pub fn resolve_on_path(
    name: &std::ffi::OsStr,
    path: &std::ffi::OsStr,
    pathext: &str,
) -> Option<PathBuf> {
    let exts: Vec<_> = pathext.split(';').filter(|e| !e.is_empty()).collect();
    let with_ext = exts.iter().map(|ext| {
        let mut name = name.to_os_string();
        name.push(ext);
        name
    });
    let as_given = exts.is_empty() || Path::new(name).extension().is_some();
    let as_given = as_given.then(|| name.to_os_string());
    let names: Vec<_> = as_given.into_iter().chain(with_ext).collect();
    let program = |candidate: &PathBuf| match exts.is_empty() {
        true => executable(candidate),
        false => candidate.is_file(),
    };
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(program)
}

/// A file the user may run (on Windows: any file).
pub fn executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        (path.metadata()).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct AppState {
    pub project: Option<PathBuf>,
    pub open_sessions: Vec<OpenSession>,
    pub terminal_visible: bool,
    pub sidebar_width: f32,
    /// Agent commands and shells the user approved; `None` for a state from before approvals.
    pub approved: Option<BTreeSet<String>>,
    /// Agent id -> the models its last session offered.
    pub models_seen: BTreeMap<String, Vec<SeenModel>>,
    /// The Open list's groups, in their order.
    pub groups: Vec<SessionGroup>,
}

/// A named group of open sessions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionGroup {
    pub name: String,
    pub collapsed: bool,
    /// Its sessions' ids.
    // ponytail: by session id alone; add the agent if two agents ever share an id.
    pub sessions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeenModel {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct OpenSession {
    pub agent: String,
    pub session_id: String,
    pub cwd: PathBuf,
    pub pane: usize,
    /// The user's name for the session; `None` shows the agent's title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            project: None,
            open_sessions: Vec::new(),
            terminal_visible: false,
            sidebar_width: 260.0,
            approved: None,
            models_seen: BTreeMap::new(),
            groups: Vec::new(),
        }
    }
}

impl AppState {
    /// `None` when the file is missing or does not parse.
    pub fn read(path: &Path) -> Option<AppState> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Missing or corrupt files yield the default state.
    pub fn load(path: &Path) -> AppState {
        Self::read(path).unwrap_or_default()
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
    fn defaults_have_four_agents_and_no_default() {
        let d = tempfile::tempdir().unwrap();
        let (s, _) = Settings::load(d.path(), None).unwrap();
        assert_eq!(s.default_agent, None);
        let ids: Vec<_> = s.agents.keys().map(String::as_str).collect();
        assert_eq!(ids, ["antigravity", "claude", "codex", "copilot"]);
        assert_eq!(
            s.agents["codex"].command,
            "npx -y @agentclientprotocol/codex-acp@2.6.0"
        );
        assert_eq!(s.agents["copilot"].command, "copilot --acp --stdio");
        assert_eq!(agent_label("copilot"), "Copilot");
        assert_eq!(agent_label("my/agent"), "my/agent");
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

    /// A cloned repository cannot turn the update check on or point it at another repository
    /// (whose release page the notice would open).
    #[test]
    fn project_cannot_set_updates() {
        let u = tempfile::tempdir().unwrap();
        let p = tempfile::tempdir().unwrap();
        assert_eq!(Settings::default().updates, UpdateSettings::default());
        assert!(!UpdateSettings::default().check && UpdateSettings::default().repo.is_none());
        write(
            p.path(),
            ".wai/settings.toml",
            "theme = \"light\"\n[updates]\ncheck = true\nrepo = \"evil/wai\"\n",
        );
        let (s, notices) = Settings::load(u.path(), Some(p.path())).unwrap();
        assert_eq!(s.theme, ThemeChoice::Light);
        assert_eq!(s.updates, UpdateSettings::default());
        let path = p.path().join(".wai").join("settings.toml");
        assert_eq!(
            notices,
            [format!(
                "Ignored updates settings from {}: only user settings can turn on the update check.",
                path.display()
            )]
        );

        write(
            u.path(),
            "settings.toml",
            "[updates]\ncheck = true\nrepo = \"owner/wai\"\n",
        );
        let (s, _) = Settings::load(u.path(), None).unwrap();
        assert!(s.updates.check);
        assert_eq!(s.updates.repo.as_deref(), Some("owner/wai"));
    }

    #[test]
    fn project_default_needs_a_user_choice() {
        let (u, p) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        write(
            p.path(),
            ".wai/settings.toml",
            "default_agent = \"copilot\"\n",
        );
        let (s, notices) = Settings::load(u.path(), Some(p.path())).unwrap();
        assert_eq!(s.default_agent, None);
        let path = p.path().join(".wai").join("settings.toml");
        assert_eq!(
            notices,
            [format!(
                "Ignored default_agent \"copilot\" from {}: a project can only pick an AI you already chose or set up.",
                path.display()
            )]
        );
    }

    #[test]
    fn project_default_may_name_the_users_agents_or_an_extension_agent() {
        let (u, p) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        write(
            u.path(),
            "settings.toml",
            "default_agent = \"claude\"\n[agents.other]\ncommand = \"other-acp\"\n",
        );
        for (name, honoured) in [
            ("claude", true),
            ("other", true),
            ("ext/agent", true),
            ("copilot", false),
        ] {
            write(
                p.path(),
                ".wai/settings.toml",
                &format!("default_agent = \"{name}\"\n"),
            );
            let (s, _) = Settings::load(u.path(), Some(p.path())).unwrap();
            let want = if honoured { name } else { "claude" };
            assert_eq!(s.default_agent.as_deref(), Some(want), "{name}");
            assert_eq!(project_default_agent(p.path()).as_deref(), Some(name));
        }
    }

    #[test]
    fn project_cannot_set_models() {
        let (u, p) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        write(u.path(), "settings.toml", "[models]\nclaude = \"opus\"\n");
        write(
            p.path(),
            ".wai/settings.toml",
            "[models]\nclaude = \"haiku\"\n",
        );
        let (s, notices) = Settings::load(u.path(), Some(p.path())).unwrap();
        assert_eq!(s.models["claude"], "opus");
        let path = p.path().join(".wai").join("settings.toml");
        assert_eq!(
            notices,
            [format!(
                "Ignored models settings from {}: only user settings can choose default models.",
                path.display()
            )]
        );
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
                name: Some("Mine".into()),
            }],
            terminal_visible: true,
            sidebar_width: 300.0,
            approved: Some(["x".into()].into()),
            models_seen: [(
                "codex".to_string(),
                vec![SeenModel {
                    id: "gpt-5".into(),
                    name: "GPT-5".into(),
                }],
            )]
            .into(),
            groups: vec![SessionGroup {
                name: "Review".into(),
                collapsed: true,
                sessions: vec!["s1".into()],
            }],
        };
        state.save(&path).unwrap();
        assert_eq!(AppState::load(&path), state);
        assert_eq!(AppState::read(&d.path().join("missing.json")), None);
        std::fs::write(&path, "{").unwrap();
        assert_eq!(AppState::read(&path), None);
        assert_eq!(AppState::load(&path), AppState::default());
        std::fs::write(&path, r#"{"terminal_visible":true}"#).unwrap();
        assert_eq!(AppState::load(&path).approved, None);
        // A state from before names and groups.
        let old = r#"{"open_sessions":[{"agent":"a","session_id":"s","cwd":"/","pane":0}]}"#;
        std::fs::write(&path, old).unwrap();
        let state = AppState::load(&path);
        assert_eq!(state.open_sessions[0].name, None);
        assert_eq!(state.groups, []);
    }

    #[test]
    fn templates_merge_user_and_project() {
        let u = tempfile::tempdir().unwrap();
        let p = tempfile::tempdir().unwrap();
        write(
            u.path(),
            "templates.toml",
            "[[template]]\nname = \"review\"\nprompt = \"user review\"\n\
             [[template]]\nname = \"docs\"\nagent = \"other\"\nprompt = \"write docs\"\n",
        );
        write(
            p.path(),
            ".wai/templates.toml",
            "[[template]]\nname = \"review\"\nprompt = \"project review\"\n\
             [[template]]\nname = \"bug\"\nprompt = \"\"\"\nline 1\nline 2\n\"\"\"\n",
        );
        let (all, notices) = load_templates(u.path(), Some(p.path()));
        assert_eq!(notices, Vec::<String>::new());
        let names: Vec<_> = all.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["docs", "review", "bug"]);
        assert_eq!(all[0].agent.as_deref(), Some("other"));
        assert_eq!(all[1].prompt, "project review");
        assert_eq!(all[2].prompt, "line 1\nline 2\n");
    }

    /// R41: a repository can ship a huge `.wai/` file: past 1 MB it is skipped with a notice.
    #[test]
    fn oversized_project_files_are_skipped_with_a_notice() {
        let u = tempfile::tempdir().unwrap();
        let p = tempfile::tempdir().unwrap();
        write(u.path(), "settings.toml", "theme = \"light\"\n");
        let padding = format!("# {}\n", "x".repeat(2 * 1024 * 1024));
        write(
            p.path(),
            ".wai/settings.toml",
            &format!("theme = \"dark\"\n{padding}"),
        );
        write(
            p.path(),
            ".wai/templates.toml",
            &format!("[[template]]\nname = \"big\"\n{padding}"),
        );

        let (s, notices) = Settings::load(u.path(), Some(p.path())).unwrap();
        assert_eq!(s.theme, ThemeChoice::Light);
        let path = p.path().join(".wai").join("settings.toml");
        let want = format!(
            "Ignored project settings: cannot read {}: larger than 1 MB",
            path.display()
        );
        assert_eq!(notices, [want]);

        let (all, notices) = load_templates(u.path(), Some(p.path()));
        assert_eq!(all, []);
        let path = p.path().join(".wai").join("templates.toml");
        let want = format!(
            "Ignored templates: cannot read {}: larger than 1 MB",
            path.display()
        );
        assert_eq!(notices, [want]);
    }

    /// R41: nor is a `.wai/` file that is no regular file opened (/dev/zero never ends).
    #[cfg(unix)]
    #[test]
    fn project_files_that_are_not_regular_are_skipped() {
        let u = tempfile::tempdir().unwrap();
        let p = tempfile::tempdir().unwrap();
        std::fs::create_dir(p.path().join(".wai")).unwrap();
        for name in ["settings.toml", "templates.toml"] {
            std::os::unix::fs::symlink("/dev/zero", p.path().join(".wai").join(name)).unwrap();
        }

        let (_, notices) = Settings::load(u.path(), Some(p.path())).unwrap();
        assert!(notices[0].ends_with(": not a regular file"), "{notices:?}");
        let (_, notices) = load_templates(u.path(), Some(p.path()));
        assert!(notices[0].ends_with(": not a regular file"), "{notices:?}");
    }

    /// What `std::fs::canonicalize` returns on Windows never shows; wai's own canonical form
    /// never carries the prefix (checked for real on Windows CI).
    #[test]
    fn simplified_paths_never_show_verbatim_prefix() {
        for (raw, shows) in [
            (r"\\?\C:\Users\me\proj", r"C:\Users\me\proj"),
            (r"\\?\C:\", r"C:\"),
            (r"\\?\UNC\server\share\proj", r"\\server\share\proj"),
            (r"C:\Users\me\proj", r"C:\Users\me\proj"),
            (r"\\server\share", r"\\server\share"),
            ("/Users/me/proj", "/Users/me/proj"),
        ] {
            assert_eq!(shown(Path::new(raw)), shows);
        }
        let tmp = canonical(&std::env::temp_dir());
        assert!(!tmp.to_string_lossy().starts_with(r"\\?\"), "{tmp:?}");
        let missing = Path::new("no-such-wai-dir");
        assert_eq!(
            canonical(missing),
            missing,
            "a path that does not resolve stays"
        );
    }

    /// Windows finds a bare name in the current dir first, and agents and MCP servers run in the
    /// project: wai resolves the name on PATH itself, PATHEXT included (`npx` is `npx.cmd`).
    #[test]
    fn bare_commands_resolve_on_path_never_relative() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        for (dir, name) in [(&a, "npx"), (&b, "npx.cmd"), (&b, "node.exe")] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join(name), "").unwrap();
        }
        let path = std::env::join_paths([&a, &b]).unwrap();
        let find = |name: &str| resolve_on_path(name.as_ref(), &path, ".com;.exe;.bat;.cmd");
        // npm's extensionless `npx` is a sh script, not a Windows program.
        assert_eq!(find("npx"), Some(b.join("npx.cmd")));
        assert_eq!(find("node"), Some(b.join("node.exe")));
        assert_eq!(find("node.exe"), Some(b.join("node.exe")));
        assert_eq!(find("jira"), None);
        // A relative PATH entry is never searched: `.` (the crate dir under `cargo test`) holds
        // Cargo.toml.
        let crate_dir = env!("CARGO_MANIFEST_DIR");
        assert!(resolve_on_path("Cargo".as_ref(), crate_dir.as_ref(), ".toml").is_some());
        assert_eq!(
            resolve_on_path("Cargo".as_ref(), ".".as_ref(), ".toml"),
            None
        );
        // Paths stay as given.
        assert_eq!(find_program(Path::new("bin/run")), Some("bin/run".into()));
    }

    /// R56: on unix too wai looks a bare name up itself, in PATH's absolute entries only: an
    /// empty entry, `.` or any relative one is the current dir (the project, for agents and MCP
    /// servers). Only an executable file counts.
    #[cfg(unix)]
    #[test]
    fn bare_commands_resolve_on_absolute_path_entries_on_unix() {
        use std::os::unix::fs::PermissionsExt as _;

        let tmp = tempfile::tempdir().unwrap();
        let dir = |name: &str| tmp.path().join(name);
        let tool = |dir: &Path, mode: u32| {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join("tool"), "").unwrap();
            let mode = std::fs::Permissions::from_mode(mode);
            std::fs::set_permissions(dir.join("tool"), mode).unwrap();
        };
        tool(&dir("relative"), 0o755);
        tool(&dir("plain"), 0o644);
        tool(&dir("exe"), 0o755);
        std::fs::create_dir_all(dir("folder").join("tool")).unwrap();
        // `relative` as a relative entry: up from the current dir to `/`, then down to it.
        let up = "../".repeat(std::env::current_dir().unwrap().components().count());
        let relative = Path::new(&up).join(dir("relative").strip_prefix("/").unwrap());
        assert!(
            relative.join("tool").is_file(),
            "a relative entry with the program"
        );
        let entries = [
            Path::new(""),
            Path::new("."),
            &relative,
            &dir("plain"),
            &dir("folder"),
            &dir("exe"),
        ];
        let path = std::env::join_paths(entries).unwrap();
        assert!(path.to_string_lossy().starts_with(":.:"), "{path:?}");

        let find = |name: &str| resolve_on_path(name.as_ref(), &path, "");
        assert_eq!(find("tool"), Some(dir("exe").join("tool")));
        assert_eq!(find("missing"), None);
        // `find_program` takes that way off Windows: `sh` is found on the test's PATH.
        let sh = find_program(Path::new("sh")).expect("sh on PATH");
        assert!(sh.is_absolute() && sh.ends_with("sh"), "{sh:?}");
    }

    #[test]
    fn malformed_templates_are_skipped_with_a_notice() {
        let u = tempfile::tempdir().unwrap();
        let p = tempfile::tempdir().unwrap();
        write(u.path(), "templates.toml", "[[template]]\nname = \"ok\"\n");
        write(
            p.path(),
            ".wai/templates.toml",
            "[[template]]\nprompt = 3\n",
        );
        let (all, notices) = load_templates(u.path(), Some(p.path()));
        assert_eq!(all.len(), 1);
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].starts_with("Ignored templates in "),
            "{notices:?}"
        );
    }
}
