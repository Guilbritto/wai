//! `wai-extension.toml`: the model and its validation. Pure data, no I/O.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use wai_settings::Template;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExtId(String);

impl ExtId {
    /// `[a-z0-9][a-z0-9-]{0,62}`.
    pub fn new(s: &str) -> Result<Self, ManifestError> {
        let ok = (1..=63).contains(&s.len())
            && !s.starts_with('-')
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if ok {
            Ok(Self(s.to_string()))
        } else {
            Err(ManifestError::BadId(s.to_string()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub id: ExtId,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub contributes: Contributes,
    pub process: Option<ProcessSpec>,
    pub permissions: Vec<Permission>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Contributes {
    pub templates: Vec<Template>,
    pub commands: Vec<CommandSpec>,
    pub agents: Vec<AgentContribution>,
    pub mcp_servers: Vec<McpServerSpec>,
    pub claude_plugins: Vec<String>,
    pub panels: Vec<PanelSpec>,
    pub themes: Vec<String>,
}

/// Without a `prompt`, the command is served by the extension process.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandSpec {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub prompt: Option<String>,
}

/// `command`: a bare program name (resolved on PATH) or a path relative to the extension dir.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentContribution {
    pub name: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct McpServerSpec {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelSpec {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessSpec {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Permission {
    Process,
    SessionsRead,
    SessionsControl,
    Notify,
    Panels,
    Network,
    Fs(PathBuf),
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Process => f.write_str("process"),
            Self::SessionsRead => f.write_str("sessions:read"),
            Self::SessionsControl => f.write_str("sessions:control"),
            Self::Notify => f.write_str("notify"),
            Self::Panels => f.write_str("panels"),
            Self::Network => f.write_str("network"),
            Self::Fs(p) => write!(f, "fs:{}", p.display()),
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ManifestError {
    #[error("invalid manifest: {0}")]
    Parse(String),
    #[error("invalid extension id {0:?} (expected [a-z0-9][a-z0-9-]{{0,62}})")]
    BadId(String),
    #[error("path {0:?} must be a bare program name or a relative path inside the extension dir")]
    EscapingPath(String),
    #[error("missing permission `{0}`")]
    MissingPermission(Permission),
    #[error("{0}")]
    Unknown(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    id: String,
    name: String,
    version: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    process: Option<ProcessSpec>,
    #[serde(default)]
    contributes: RawContributes,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawContributes {
    #[serde(default)]
    templates: Vec<Template>,
    #[serde(default)]
    commands: Vec<CommandSpec>,
    #[serde(default)]
    agents: Vec<AgentContribution>,
    #[serde(default)]
    mcp_servers: Vec<RawMcp>,
    #[serde(default)]
    claude_plugins: Vec<String>,
    #[serde(default)]
    panels: Vec<PanelSpec>,
    #[serde(default)]
    themes: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMcp {
    name: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

fn parse_permission(s: &str) -> Result<Permission, ManifestError> {
    Ok(match s {
        "process" => Permission::Process,
        "sessions:read" => Permission::SessionsRead,
        "sessions:control" => Permission::SessionsControl,
        "notify" => Permission::Notify,
        "panels" => Permission::Panels,
        "network" => Permission::Network,
        _ => match s.strip_prefix("fs:") {
            Some(p) => {
                check_relative(p)?;
                Permission::Fs(PathBuf::from(p))
            }
            None => return Err(ManifestError::Unknown(format!("unknown permission {s:?}"))),
        },
    })
}

/// Lexical check: no root/prefix, and `..` never climbs above the starting dir.
/// Symlink escapes are the installer's job (the dir does not exist yet at parse time).
fn check_relative(p: &str) -> Result<(), ManifestError> {
    let bad = || ManifestError::EscapingPath(p.to_string());
    let mut depth = 0usize;
    for c in Path::new(p).components() {
        match c {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir => depth = depth.checked_sub(1).ok_or_else(bad)?,
            Component::RootDir | Component::Prefix(_) => return Err(bad()),
        }
    }
    if p.is_empty() || p.contains('\\') && !cfg!(windows) {
        return Err(bad());
    }
    Ok(())
}

/// A bare program name (no separators) or a relative path that stays inside the extension dir.
fn check_command(cmd: &str) -> Result<(), ManifestError> {
    check_relative(cmd)?;
    if Path::new(cmd)
        .components()
        .all(|c| matches!(c, Component::CurDir))
    {
        return Err(ManifestError::EscapingPath(cmd.to_string()));
    }
    Ok(())
}

/// `ext_dir` is the directory the manifest will live in; paths are checked lexically against
/// it, so it is not touched on disk.
pub fn parse_manifest(text: &str, _ext_dir: &Path) -> Result<Manifest, ManifestError> {
    let raw: RawManifest = toml::from_str(text).map_err(|e| ManifestError::Parse(e.to_string()))?;
    let id = ExtId::new(&raw.id)?;
    let permissions = raw
        .permissions
        .iter()
        .map(|p| parse_permission(p))
        .collect::<Result<Vec<_>, _>>()?;
    let need = |p: Permission| {
        if permissions.contains(&p) {
            Ok(())
        } else {
            Err(ManifestError::MissingPermission(p))
        }
    };

    if let Some(p) = &raw.process {
        need(Permission::Process)?;
        check_command(&p.command)?;
    }
    let c = raw.contributes;
    if !c.panels.is_empty() {
        need(Permission::Panels)?;
    }
    for cmd in &c.commands {
        if cmd.prompt.is_none() && raw.process.is_none() {
            return Err(ManifestError::Unknown(format!(
                "command {:?} has neither a prompt nor an extension process",
                cmd.id
            )));
        }
    }
    for a in &c.agents {
        check_command(&a.command)?;
    }
    for m in &c.mcp_servers {
        check_command(&m.command)?;
    }

    Ok(Manifest {
        id,
        name: raw.name,
        version: raw.version,
        description: raw.description,
        process: raw.process,
        permissions,
        contributes: Contributes {
            templates: c.templates,
            commands: c.commands,
            agents: c.agents,
            mcp_servers: c
                .mcp_servers
                .into_iter()
                .map(|m| McpServerSpec {
                    name: m.name,
                    command: m.command,
                    args: m.args,
                    env: m.env.into_iter().collect(),
                })
                .collect(),
            claude_plugins: c.claude_plugins,
            panels: c.panels,
            themes: c.themes,
        },
    })
}
