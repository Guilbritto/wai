//! Read-only view of what Claude Code keeps on disk: plugins and MCP servers. Only names,
//! versions, counts and transports are kept; env, headers, tokens, commands, urls and account
//! data are never copied out of the files.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::{canonical, read_config, shown};

const MAX_CLAUDE_JSON_BYTES: u64 = 32 * 1024 * 1024;

pub fn claude_home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

pub fn short(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", shown(rest).replace('\\', "/")),
        Err(_) => shown(path),
    }
}

pub fn project_key_for(path: &Path) -> String {
    let text = shown(path);
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text
    }
}

/// A hash of a file's bytes; `None` when the file was missing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stamp(Option<u64>);

impl Stamp {
    pub fn of(bytes: Option<&[u8]>) -> Stamp {
        Stamp(bytes.map(|b| {
            let mut h = DefaultHasher::new();
            b.hash(&mut h);
            h.finish()
        }))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClaudeData {
    pub plugins: Vec<Plugin>,
    pub mcp: Vec<McpServer>,
    pub skills: Vec<Entry>,
    pub agents: Vec<Entry>,
    pub instructions: Vec<Entry>,
    pub hooks: Vec<Entry>,
    pub problems: Vec<String>,
    pub settings_stamp: Stamp,
    pub project_key: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub marketplace: String,
    pub version: String,
    pub root: PathBuf,
    pub enabled: bool,
    pub locked: Option<String>,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum McpSource {
    Plugin(String),
    User,
    Local,
    Project,
}

#[derive(Clone, Debug, PartialEq)]
pub struct McpServer {
    pub name: String,
    pub label: String,
    pub source: McpSource,
    pub transport: &'static str,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub group: String,
    pub name: String,
    pub path: PathBuf,
    pub summary: String,
    pub read_only: bool,
}

struct Loaded {
    value: Option<Value>,
    stamp: Stamp,
    broken: bool,
}

/// A JSON file: missing is fine; one that does not read or parse adds a problem.
fn load(path: &Path, home: &Path, big: bool, problems: &mut Vec<String>) -> Loaded {
    let text = if big {
        read_capped(path, MAX_CLAUDE_JSON_BYTES)
    } else {
        read_config(path)
    };
    let text = match text {
        Ok(Some(t)) => t,
        Ok(None) => {
            return Loaded {
                value: None,
                stamp: Stamp::default(),
                broken: false,
            };
        }
        Err(e) => {
            problems.push(format!("{} does not parse: {e}", short(path, home)));
            return Loaded {
                value: None,
                stamp: Stamp::default(),
                broken: true,
            };
        }
    };
    let stamp = Stamp::of(Some(text.as_bytes()));
    match serde_json::from_str::<Value>(&text) {
        Ok(value) => Loaded {
            value: Some(value),
            stamp,
            broken: false,
        },
        Err(e) => {
            problems.push(format!("{} does not parse: {e}", short(path, home)));
            Loaded {
                value: None,
                stamp,
                broken: true,
            }
        }
    }
}

fn read_capped(path: &Path, cap: u64) -> std::io::Result<Option<String>> {
    use std::io::{Error, ErrorKind, Read as _};
    match std::fs::metadata(path) {
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
        Ok(meta) if !meta.is_file() => return Err(Error::other("not a regular file")),
        Ok(_) => {}
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(cap + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > cap {
        return Err(Error::other("larger than 32 MB"));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|e| Error::new(ErrorKind::InvalidData, e))
}

fn git_root(project: &Path) -> Option<PathBuf> {
    project
        .ancestors()
        .find(|p| p.join(".git").exists())
        .map(Path::to_path_buf)
}

fn transport(server: &Value) -> &'static str {
    match server["type"].as_str() {
        Some("http") => "HTTP",
        Some("sse") => "SSE",
        Some("ws") => "WebSocket",
        _ => "stdio",
    }
}

/// A manifest path stays inside the plugin: no `..`, nothing absolute.
fn inside(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    rel.components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
        .then(|| root.join(rel))
}

fn json_file(root: &Path, rel: &str) -> Option<Value> {
    let text = read_config(&inside(root, rel)?).ok()??;
    serde_json::from_str(&text).ok()
}

fn paths(v: &Value, root: &Path) -> Vec<PathBuf> {
    match v {
        Value::String(s) => inside(root, s).into_iter().collect(),
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str().and_then(|s| inside(root, s)))
            .collect(),
        _ => Vec::new(),
    }
}

/// Servers named by `{key: {...}}` or a bare `{name: {...}}` map, a path to one, or an array.
fn servers(v: &Value, key: &str, root: &Path, depth: u8, out: &mut Vec<(String, &'static str)>) {
    if depth > 2 {
        return;
    }
    match v {
        Value::String(s) if !s.ends_with(".mcpb") => {
            if let Some(x) = json_file(root, s) {
                servers(&x, key, root, depth + 1, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| servers(x, key, root, depth + 1, out)),
        Value::Object(o) => {
            let map = v[key].as_object().unwrap_or(o);
            for (name, server) in map {
                if server.is_object() && !out.iter().any(|(n, _)| n == name) {
                    out.push((name.clone(), transport(server)));
                }
            }
        }
        _ => {}
    }
}

fn add_events(map: &serde_json::Map<String, Value>, out: &mut Vec<(String, usize)>) {
    for (event, list) in map {
        let n: usize = list
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m["hooks"].as_array())
            .map(Vec::len)
            .sum();
        if n == 0 {
            continue;
        }
        match out.iter_mut().find(|(e, _)| e == event) {
            Some(x) => x.1 += n,
            None => out.push((event.clone(), n)),
        }
    }
}

/// Events with their handler counts, from inline hooks, a path to a hooks file, or an array.
fn hook_events(v: &Value, root: &Path, depth: u8, out: &mut Vec<(String, usize)>) {
    if depth > 2 {
        return;
    }
    match v {
        Value::String(s) => {
            if let Some(x) = json_file(root, s) {
                hook_events(&x, root, depth + 1, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| hook_events(x, root, depth + 1, out)),
        Value::Object(o) => add_events(o.get("hooks").and_then(Value::as_object).unwrap_or(o), out),
        _ => {}
    }
}

fn plugin_hook_events(root: &Path, m: &Value) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    if let Some(v) = json_file(root, "hooks/hooks.json") {
        hook_events(&v, root, 0, &mut out);
    }
    // A manifest entry naming the default file would count its hooks twice.
    let default = canonical(&root.join("hooks/hooks.json"));
    let same = |x: &Value| {
        x.as_str()
            .and_then(|s| inside(root, s))
            .is_some_and(|p| canonical(&p) == default)
    };
    match &m["hooks"] {
        x if same(x) => {}
        Value::Array(a) => a
            .iter()
            .filter(|x| !same(x))
            .for_each(|x| hook_events(x, root, 1, &mut out)),
        x => hook_events(x, root, 0, &mut out),
    }
    out
}

fn md_paths(dir: &Path, recursive: bool, left: u8, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if recursive && left > 0 {
                md_paths(&p, true, left - 1, out);
            }
        } else if p.extension().is_some_and(|x| x == "md") {
            out.push(p);
        }
    }
}

/// A manifest `agents`/`commands` entry: a `.md` file, or a folder of them.
fn md_list(list: &[PathBuf], recursive: bool, left: u8) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in list {
        if p.is_dir() {
            md_paths(p, recursive, left, &mut out);
        } else if p.extension().is_some_and(|x| x == "md") && p.is_file() {
            out.push(p.clone());
        }
    }
    out
}

/// `dir/SKILL.md`, or `dir/*/SKILL.md`.
fn skill_files(dir: &Path) -> Vec<PathBuf> {
    if dir.join("SKILL.md").is_file() {
        return vec![dir.join("SKILL.md")];
    }
    std::fs::read_dir(dir).map_or_else(
        |_| Vec::new(),
        |rd| {
            rd.flatten()
                .map(|e| e.path().join("SKILL.md"))
                .filter(|p| p.is_file())
                .collect()
        },
    )
}

fn plugin_skill_files(root: &Path, m: &Value) -> Vec<PathBuf> {
    let dir = root.join("skills");
    let mut out = if dir.is_dir() {
        skill_files(&dir)
    } else {
        Vec::new()
    };
    for p in paths(&m["skills"], root) {
        out.extend(skill_files(&p));
    }
    if !dir.is_dir() && m["skills"].is_null() && root.join("SKILL.md").is_file() {
        out.push(root.join("SKILL.md"));
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| seen.insert(canonical(p)));
    out
}

fn plugin_agent_files(root: &Path, m: &Value) -> Vec<PathBuf> {
    if m["agents"].is_null() {
        md_list(&[root.join("agents")], true, 8)
    } else {
        md_list(&paths(&m["agents"], root), true, 8)
    }
}

struct Scan {
    summary: String,
    default_enabled: bool,
    servers: Vec<(String, &'static str)>,
}

fn marketplace_entry(home: &Path, marketplace: &str, name: &str) -> Option<Value> {
    let known = json_file(&home.join(".claude/plugins"), "known_marketplaces.json")?;
    let at = known[marketplace]["installLocation"].as_str()?;
    let list = json_file(Path::new(at), ".claude-plugin/marketplace.json")?;
    list["plugins"]
        .as_array()?
        .iter()
        .find(|p| p["name"] == name)
        .cloned()
}

fn scan(root: &Path, home: &Path, marketplace: &str, name: &str) -> Scan {
    let manifest = json_file(root, ".claude-plugin/plugin.json");
    let m = manifest.as_ref().unwrap_or(&Value::Null);
    let entry = if manifest.is_none() {
        marketplace_entry(home, marketplace, name)
    } else {
        None
    };
    let skills = plugin_skill_files(root, m).len();
    let agents = plugin_agent_files(root, m).len();

    let commands = match &m["commands"] {
        Value::Object(o) => o.len(),
        Value::Null => md_list(&[root.join("commands")], false, 0).len(),
        v => md_list(&paths(v, root), false, 0).len(),
    };

    let hooks: usize = plugin_hook_events(root, m).iter().map(|e| e.1).sum();

    let mut mcp = Vec::new();
    if let Some(v) = json_file(root, ".mcp.json") {
        servers(&v, "mcpServers", root, 0, &mut mcp);
    }
    servers(&m["mcpServers"], "mcpServers", root, 0, &mut mcp);

    let mut lsp = Vec::new();
    if let Some(v) = json_file(root, ".lsp.json") {
        servers(&v, "lspServers", root, 0, &mut lsp);
    }
    servers(&m["lspServers"], "lspServers", root, 0, &mut lsp);
    if let Some(e) = &entry {
        servers(&e["lspServers"], "lspServers", root, 0, &mut lsp);
    }

    let counts = [
        (skills, "skill", "skills"),
        (agents, "agent", "agents"),
        (commands, "command", "commands"),
        (hooks, "hook", "hooks"),
        (mcp.len(), "MCP server", "MCP servers"),
        (lsp.len(), "language server", "language servers"),
    ];
    let mut summary = counts
        .iter()
        .filter(|(n, ..)| *n > 0)
        .map(|(n, one, many)| format!("{n} {}", if *n == 1 { one } else { many }))
        .collect::<Vec<_>>()
        .join(" · ");
    if summary.is_empty() {
        let description = m["description"]
            .as_str()
            .or_else(|| entry.as_ref().and_then(|e| e["description"].as_str()));
        summary = description
            .and_then(|d| d.lines().next())
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .unwrap_or("No components")
            .to_string();
    }

    Scan {
        summary,
        default_enabled: m["defaultEnabled"].as_bool().unwrap_or(true),
        servers: mcp,
    }
}

/// At most the first `cap` bytes of a regular file, lossy; empty when unreadable.
fn head(path: &Path, cap: u64) -> String {
    use std::io::Read as _;
    if !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
        return String::new();
    }
    let mut bytes = Vec::new();
    let _ = std::fs::File::open(path).and_then(|f| f.take(cap).read_to_end(&mut bytes));
    String::from_utf8_lossy(&bytes).into_owned()
}

/// `name` and `description` from the `---` block at the top of a file.
fn frontmatter(text: &str) -> (Option<String>, Option<String>) {
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return (None, None);
    }
    let (mut name, mut description) = (None, None);
    let mut key: Option<(&str, String)> = None;
    let mut done = |k: Option<(&str, String)>| {
        if let Some((k, v)) = k {
            let v = v.trim().trim_matches(['"', '\'']).trim().to_string();
            match k {
                "name" if !v.is_empty() => name = Some(v),
                "description" if !v.is_empty() => description = Some(v),
                _ => {}
            }
        }
    };
    for line in lines {
        if line.trim_end() == "---" {
            break;
        }
        if line.starts_with([' ', '\t']) {
            if let Some((_, v)) = key.as_mut() {
                v.push(' ');
                v.push_str(line.trim());
            }
        } else {
            done(key.take());
            if let Some((k, v)) = line.split_once(':') {
                let v = v.trim();
                let v = if v.starts_with(['>', '|']) { "" } else { v };
                key = Some((k.trim(), v.to_string()));
            }
        }
    }
    done(key);
    (name, description)
}

/// The `@path` tokens of a markdown file, outside fenced blocks and inline code.
fn imports(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let plain = line.split('`').step_by(2).collect::<Vec<_>>().join(" ");
        out.extend(
            plain
                .replace("\\ ", "\0")
                .split_whitespace()
                .filter(|t| t.len() > 1)
                .filter_map(|t| t.strip_prefix('@'))
                .map(|t| t.replace('\0', " ")),
        );
    }
    out
}

fn md_entry(group: &str, file: &Path, fallback: &str, read_only: bool) -> Entry {
    let (name, description) = frontmatter(&head(file, 8 * 1024));
    Entry {
        group: group.to_string(),
        name: name.unwrap_or_else(|| fallback.to_string()),
        path: file.to_path_buf(),
        summary: description
            .and_then(|d| d.lines().next().map(|l| l.trim().to_string()))
            .unwrap_or_default(),
        read_only,
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn skill_group(group: &str, files: Vec<PathBuf>, read_only: bool) -> Vec<Entry> {
    let mut v: Vec<_> = files
        .iter()
        .map(|f| {
            md_entry(
                group,
                f,
                &f.parent().map(file_name).unwrap_or_default(),
                read_only,
            )
        })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

fn agent_group(group: &str, files: Vec<PathBuf>, read_only: bool) -> Vec<Entry> {
    let mut v: Vec<_> = files
        .iter()
        .map(|f| {
            let stem = f.file_stem().map(|n| n.to_string_lossy().into_owned());
            md_entry(group, f, &stem.unwrap_or_default(), read_only)
        })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

const MAX_INSTRUCTION_BYTES: u64 = 256 * 1024;
const MAX_IMPORT_HOPS: u8 = 4;

fn walk_imports(
    file: &Path,
    group: &str,
    home: &Path,
    hop: u8,
    seen: &mut HashSet<PathBuf>,
    out: &mut Vec<Entry>,
) {
    if hop >= MAX_IMPORT_HOPS {
        return;
    }
    let by = file_name(file);
    let dir = file.parent().unwrap_or(Path::new(""));
    for token in imports(&head(file, MAX_INSTRUCTION_BYTES)) {
        let target = match token.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => dir.join(&token),
        };
        if !target.is_file() {
            continue;
        }
        let target = canonical(&target);
        if !seen.insert(target.clone()) {
            continue;
        }
        out.push(Entry {
            group: group.to_string(),
            name: file_name(&target),
            path: target.clone(),
            summary: format!("Included by {by}"),
            read_only: false,
        });
        walk_imports(&target, group, home, hop + 1, seen, out);
    }
}

fn instruction(
    file: &Path,
    group: &str,
    summary: Option<&str>,
    home: &Path,
    seen: &mut HashSet<PathBuf>,
    out: &mut Vec<Entry>,
) {
    if !file.is_file() {
        return;
    }
    let file = canonical(file);
    if !seen.insert(file.clone()) {
        return;
    }
    let summary = summary.map(String::from).unwrap_or_else(|| {
        head(&file, MAX_INSTRUCTION_BYTES)
            .lines()
            .find_map(|l| l.strip_prefix("# "))
            .map_or_else(|| "Every project".to_string(), |h| h.trim().to_string())
    });
    out.push(Entry {
        group: group.to_string(),
        name: file_name(&file),
        path: file.clone(),
        summary,
        read_only: false,
    });
    walk_imports(&file, group, home, 0, seen, out);
}

fn hook_group(group: &str, settings: Option<&Value>, path: PathBuf, out: &mut Vec<Entry>) {
    let Some(map) = settings.and_then(|v| v["hooks"].as_object()) else {
        return;
    };
    let mut events = Vec::new();
    add_events(map, &mut events);
    for (name, n) in events {
        out.push(Entry {
            group: group.to_string(),
            name,
            path: path.clone(),
            summary: if n == 1 {
                "1 hook".to_string()
            } else {
                format!("{n} hooks")
            },
            read_only: false,
        });
    }
}

/// Skills, agents, instructions and hooks; the plugin ones only from enabled plugins.
fn fill_entries(
    data: &mut ClaudeData,
    home: &Path,
    project: &Path,
    settings: [Option<&Value>; 3],
    at_home: bool,
) {
    let [user, shared, local] = settings;
    let (user_dir, project_dir) = (home.join(".claude"), project.join(".claude"));

    data.skills = skill_group("User", skill_files(&user_dir.join("skills")), false);
    if !at_home {
        data.skills.extend(skill_group(
            "Project",
            skill_files(&project_dir.join("skills")),
            false,
        ));
    }
    let mut user_agents = Vec::new();
    md_paths(&user_dir.join("agents"), true, 3, &mut user_agents);
    data.agents = agent_group("User", user_agents, false);
    let mut project_agents = Vec::new();
    if !at_home {
        md_paths(&project_dir.join("agents"), true, 3, &mut project_agents);
    }
    data.agents
        .extend(agent_group("Project", project_agents, false));

    let mut seen = HashSet::new();
    instruction(
        &user_dir.join("CLAUDE.md"),
        "User",
        None,
        home,
        &mut seen,
        &mut data.instructions,
    );
    let ancestors: Vec<_> = project.ancestors().collect();
    for dir in ancestors.into_iter().rev() {
        let here = dir == project;
        for (file, mine) in [
            (dir.join("CLAUDE.md"), "This project"),
            (dir.join(".claude/CLAUDE.md"), "This project"),
            (dir.join("CLAUDE.local.md"), "Yours, not shared"),
        ] {
            let summary = if here { mine } else { "Parent folder" };
            instruction(
                &file,
                "Project",
                Some(summary),
                home,
                &mut seen,
                &mut data.instructions,
            );
        }
    }

    hook_group(
        "User",
        user,
        user_dir.join("settings.json"),
        &mut data.hooks,
    );
    // Opened at home, the project's files are the user's own.
    let (shared, local) = if at_home {
        (None, None)
    } else {
        (shared, local)
    };
    hook_group(
        "Project",
        shared,
        project_dir.join("settings.json"),
        &mut data.hooks,
    );
    hook_group(
        "Project",
        local,
        project_dir.join("settings.local.json"),
        &mut data.hooks,
    );

    let mut plugin_hooks = Vec::new();
    for plugin in data.plugins.iter().filter(|p| p.enabled) {
        let root = &plugin.root;
        if root.as_os_str().is_empty() {
            continue;
        }
        let manifest = json_file(root, ".claude-plugin/plugin.json").unwrap_or(Value::Null);
        let m = &manifest;
        data.skills
            .extend(skill_group(&plugin.name, plugin_skill_files(root, m), true));
        data.agents
            .extend(agent_group(&plugin.name, plugin_agent_files(root, m), true));
        let events = plugin_hook_events(root, m);
        if events.is_empty() {
            continue;
        }
        let default = root.join("hooks/hooks.json");
        let path = if default.is_file() {
            default
        } else {
            paths(&m["hooks"], root)
                .into_iter()
                .next()
                .unwrap_or_else(|| root.join(".claude-plugin/plugin.json"))
        };
        plugin_hooks.push(Entry {
            group: "From plugins".to_string(),
            name: plugin.name.clone(),
            path,
            summary: events
                .iter()
                .map(|e| e.0.as_str())
                .collect::<Vec<_>>()
                .join(" · "),
            read_only: true,
        });
    }
    data.hooks.extend(plugin_hooks);
}

/// What `claude mcp list` says about one server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum McpStatus {
    Connected,
    NeedsAuth,
    Failed(String),
    Pending,
    Disabled,
    Other(String),
}

/// Reads `claude mcp list`: the status of each server by name. The command or URL part of a
/// line is never kept.
pub fn parse_mcp_list(text: &str) -> BTreeMap<String, McpStatus> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        if line.starts_with("claude.ai ") {
            continue;
        }
        let Some((name, rest)) = line.split_once(": ") else {
            continue;
        };
        let marked = |at: &usize| rest[at + 3..].starts_with(['✔', '!', '✘', '⏸', '⊘']);
        let Some(at) = rest.rmatch_indices(" - ").map(|(at, _)| at).find(marked) else {
            continue;
        };
        let status = rest[at + 3..].trim();
        let status = if status.starts_with('✔') {
            McpStatus::Connected
        } else if status.starts_with("! Needs authentication") {
            McpStatus::NeedsAuth
        } else if let Some(reason) = status.strip_prefix("✘ Failed to connect") {
            let reason = reason.trim_start().trim_start_matches('—');
            McpStatus::Failed(reason.trim().to_string())
        } else if status.starts_with('⏸') {
            McpStatus::Pending
        } else if status.starts_with('⊘') {
            McpStatus::Disabled
        } else {
            McpStatus::Other(status.to_string())
        };
        map.insert(name.to_string(), status);
    }
    map
}

pub fn read_claude(home: &Path, project: &Path) -> ClaudeData {
    let mut data = ClaudeData::default();
    let problems = &mut data.problems;
    let root = git_root(project);

    let user_settings = load(&home.join(".claude/settings.json"), home, false, problems);
    data.settings_stamp = user_settings.stamp;
    let project_dir = project.join(".claude");
    // Opened at home (Finder, Dock), the project's .claude is the user's own.
    let at_home = canonical(project) == canonical(home);
    let mut project_file = |name: &str| match at_home {
        true => Loaded {
            value: None,
            stamp: Stamp::default(),
            broken: false,
        },
        false => load(&project_dir.join(name), home, false, problems),
    };
    let project_local = project_file("settings.local.json");
    let project_shared = project_file("settings.json");
    let root_local = match root.as_deref() {
        Some(r) if r != project => {
            load(
                &r.join(".claude/settings.local.json"),
                home,
                false,
                problems,
            )
            .value
        }
        _ => None,
    };
    // Lookup order; the two project files also lock the toggle.
    let layers = [
        (
            &root_local,
            Some("Set in the repository's .claude/settings.local.json"),
        ),
        (
            &project_local.value,
            Some("Set in this project's .claude/settings.local.json"),
        ),
        (
            &project_shared.value,
            Some("Set in this project's .claude/settings.json"),
        ),
        (&user_settings.value, None),
    ];

    let config = load(&home.join(".claude.json"), home, true, problems);
    let projects = &config.value.as_ref().unwrap_or(&Value::Null)["projects"];
    data.project_key = [Some(project.to_path_buf()), root.clone()]
        .into_iter()
        .flatten()
        .map(|p| project_key_for(&p))
        .find(|k| projects.get(k).is_some());
    let entry = data.project_key.as_ref().map(|k| &projects[k]);

    let installed = load(
        &home.join(".claude/plugins/installed_plugins.json"),
        home,
        false,
        &mut data.problems,
    )
    .value
    .unwrap_or(Value::Null);
    let mut plugin_servers = Vec::new();
    for (id, installs) in installed["plugins"].as_object().into_iter().flatten() {
        let installs = installs.as_array().map(Vec::as_slice).unwrap_or_default();
        let here = installs.iter().find(|i| {
            matches!(i["scope"].as_str(), Some("project" | "local"))
                && i["projectPath"]
                    .as_str()
                    .is_some_and(|p| canonical(Path::new(p)) == project)
        });
        let outer = installs
            .iter()
            .find(|i| matches!(i["scope"].as_str(), Some("user" | "managed")));
        let Some(install) = here.or(outer) else {
            continue;
        };
        let (name, marketplace) = id.split_once('@').unwrap_or((id, ""));
        let plugin_root = PathBuf::from(install["installPath"].as_str().unwrap_or_default());
        let scanned = if plugin_root.as_os_str().is_empty() {
            Scan {
                summary: "No components".to_string(),
                default_enabled: true,
                servers: Vec::new(),
            }
        } else {
            scan(&plugin_root, home, marketplace, name)
        };

        let found = layers
            .iter()
            .find_map(|(v, _)| v.as_ref().and_then(|v| v["enabledPlugins"].get(id)));
        let enabled = match found {
            Some(v) => *v != Value::Bool(false),
            None => scanned.default_enabled,
        };
        let locked = layers
            .iter()
            .find_map(|(v, lock)| {
                lock.filter(|_| {
                    v.as_ref()
                        .is_some_and(|v| v["enabledPlugins"].get(id).is_some())
                })
            })
            .or_else(|| (install["scope"] == "managed").then_some("Managed by your organization"))
            .or_else(|| {
                user_settings
                    .broken
                    .then_some("Fix ~/.claude/settings.json first")
            });
        if enabled {
            plugin_servers.push((name.to_string(), scanned.servers));
        }
        data.plugins.push(Plugin {
            id: id.clone(),
            name: name.to_string(),
            marketplace: marketplace.to_string(),
            version: install["version"].as_str().unwrap_or_default().to_string(),
            root: plugin_root,
            enabled,
            locked: locked.map(String::from),
            summary: scanned.summary,
        });
    }

    let disabled: Vec<&str> = entry
        .and_then(|e| e["disabledMcpServers"].as_array())
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut add = |name: String, label: String, source: McpSource, transport| {
        let enabled = !disabled.contains(&name.as_str());
        data.mcp.push(McpServer {
            name,
            label,
            source,
            transport,
            enabled,
        });
    };
    for (plugin, list) in plugin_servers {
        for (server, t) in list {
            add(
                format!("plugin:{plugin}:{server}"),
                server,
                McpSource::Plugin(plugin.clone()),
                t,
            );
        }
    }
    let config = config.value.as_ref().unwrap_or(&Value::Null);
    let mut from_map = |map: &Value, source: McpSource| {
        for (name, server) in map.as_object().into_iter().flatten() {
            add(
                name.clone(),
                name.clone(),
                source.clone(),
                transport(server),
            );
        }
    };
    from_map(&config["mcpServers"], McpSource::User);
    if let Some(e) = entry {
        from_map(&e["mcpServers"], McpSource::Local);
    }
    let mcp_json = load(&project.join(".mcp.json"), home, false, &mut data.problems);
    if let Some(v) = mcp_json.value {
        let mut found = Vec::new();
        servers(&v, "mcpServers", project, 0, &mut found);
        for (name, t) in found {
            add(name.clone(), name, McpSource::Project, t);
        }
    }
    fill_entries(
        &mut data,
        home,
        project,
        [
            user_settings.value.as_ref(),
            project_shared.value.as_ref(),
            project_local.value.as_ref(),
        ],
        at_home,
    );
    data
}

#[derive(Debug)]
pub enum WriteError {
    Changed,
    Busy,
    Invalid(String),
    Io(std::io::Error),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Changed => f.write_str("it changed since it was shown"),
            WriteError::Busy => f.write_str("Claude is writing it; try again"),
            WriteError::Invalid(s) => f.write_str(s),
            WriteError::Io(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<std::io::Error> for WriteError {
    fn from(e: std::io::Error) -> Self {
        WriteError::Io(e)
    }
}

struct Lock(PathBuf);

impl Lock {
    fn take(target: &Path) -> Result<Lock, WriteError> {
        let mut name = target.as_os_str().to_owned();
        name.push(".lock");
        let dir = PathBuf::from(name);
        let fresh = |dir: &Path| {
            std::fs::metadata(dir)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_none_or(|age| age < std::time::Duration::from_secs(10))
        };
        let mut tries = 0;
        let mut cleared = false;
        loop {
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(Lock(dir)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if !fresh(&dir) {
                        if cleared {
                            return Err(WriteError::Busy);
                        }
                        cleared = true;
                        let _ = std::fs::remove_dir(&dir);
                    } else if tries >= 10 {
                        return Err(WriteError::Busy);
                    } else {
                        tries += 1;
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}

fn indent_of(text: &str) -> String {
    text.lines()
        .find(|l| l.starts_with([' ', '\t']) && !l.trim().is_empty())
        .map(|l| l[..l.len() - l.trim_start().len()].to_string())
        .unwrap_or_else(|| "  ".to_string())
}

fn replace_json(
    path: &Path,
    seen: Option<Stamp>,
    edit: impl FnOnce(&mut Value) -> Result<bool, String>,
) -> Result<Stamp, WriteError> {
    let target = if path.exists() {
        crate::try_canonical(path)?
    } else {
        let mut t = path.to_path_buf();
        for _ in 0..8 {
            match std::fs::read_link(&t) {
                Ok(l) => t = t.parent().unwrap_or(Path::new("")).join(l),
                Err(_) => break,
            }
        }
        if let Some(dir) = t.parent() {
            std::fs::create_dir_all(dir)?;
        }
        t
    };
    let _lock = Lock::take(&target)?;
    let read = |p: &Path| match std::fs::read(p) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(WriteError::Io(e)),
    };
    let bytes = read(&target)?;
    let base = Stamp::of(bytes.as_deref());
    if seen.is_some_and(|s| s != base) {
        return Err(WriteError::Changed);
    }
    let (mut value, text) = match &bytes {
        Some(b) => {
            let text = String::from_utf8(b.clone())
                .map_err(|_| WriteError::Invalid("the file is not valid UTF-8".into()))?;
            let v = serde_json::from_str(&text)
                .map_err(|e| WriteError::Invalid(format!("the file is not valid JSON: {e}")))?;
            (v, text)
        }
        None => (Value::Object(Default::default()), String::new()),
    };
    if !edit(&mut value).map_err(WriteError::Invalid)? {
        return Ok(base);
    }
    let indent = indent_of(&text);
    let mut out = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
    serde::Serialize::serialize(&value, &mut ser)
        .map_err(|e| WriteError::Invalid(e.to_string()))?;
    if text.ends_with('\n') {
        out.push(b'\n');
    }
    let mut name = target.as_os_str().to_owned();
    name.push(".wai.tmp");
    let tmp = PathBuf::from(name);
    match std::fs::remove_file(&tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut f| std::io::Write::write_all(&mut f, &out));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    let finish = || -> Result<(), WriteError> {
        if bytes.is_some() {
            std::fs::set_permissions(&tmp, std::fs::metadata(&target)?.permissions())?;
        }
        if Stamp::of(read(&target)?.as_deref()) != base {
            return Err(WriteError::Changed);
        }
        std::fs::rename(&tmp, &target)?;
        Ok(())
    };
    if let Err(e) = finish() {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(Stamp::of(Some(&out)))
}

pub fn set_plugin_enabled(
    home: &Path,
    id: &str,
    on: bool,
    seen: Stamp,
) -> Result<Stamp, WriteError> {
    replace_json(&home.join(".claude/settings.json"), Some(seen), |root| {
        let obj = root
            .as_object_mut()
            .ok_or("settings.json is not a JSON object")?;
        let plugins = obj
            .entry("enabledPlugins")
            .or_insert_with(|| Value::Object(Default::default()))
            .as_object_mut()
            .ok_or("enabledPlugins is not an object")?;
        plugins.insert(id.to_string(), Value::Bool(on));
        Ok(true)
    })
}

/// Turns one MCP server on or off for a project by editing `disabledMcpServers` in
/// `~/.claude.json`, nothing else.
pub fn set_mcp_enabled(
    home: &Path,
    project_key: &str,
    server: &str,
    on: bool,
) -> Result<(), WriteError> {
    replace_json(&home.join(".claude.json"), None, |root| {
        let obj = root
            .as_object_mut()
            .ok_or("~/.claude.json is not a JSON object")?;
        if !on && !obj.contains_key("projects") {
            obj.insert("projects".into(), Value::Object(Default::default()));
        }
        let Some(projects) = obj.get_mut("projects") else {
            return Ok(false);
        };
        let projects = projects
            .as_object_mut()
            .ok_or("projects is not an object")?;
        if !on && !projects.contains_key(project_key) {
            projects.insert(project_key.into(), Value::Object(Default::default()));
        }
        let Some(entry) = projects.get_mut(project_key) else {
            return Ok(false);
        };
        let entry = entry
            .as_object_mut()
            .ok_or("the project entry is not an object")?;
        if !on && !entry.contains_key("disabledMcpServers") {
            entry.insert("disabledMcpServers".into(), Value::Array(Vec::new()));
        }
        let Some(list) = entry.get_mut("disabledMcpServers") else {
            return Ok(false);
        };
        let list = list
            .as_array_mut()
            .ok_or("disabledMcpServers is not a list")?;
        let at = list.iter().position(|v| v.as_str() == Some(server));
        match (on, at) {
            (false, None) => list.push(Value::String(server.into())),
            (true, Some(i)) => {
                list.remove(i);
            }
            _ => return Ok(false),
        }
        Ok(true)
    })
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn parse_mcp_list_reads_claude_output() {
        let text = "Checking MCP server health…\n\
\n\
claude.ai Figma: https://mcp.figma.com/mcp - ✔ Connected\n\
plugin:context7:context7: https://mcp.context7.com/mcp?client=claude-code-plugin (HTTP) - ! Needs authentication\n\
plugin:github:github: https://api.githubcopilot.com/mcp/ (HTTP) - ✘ Failed to connect — HTTP 400: Error POSTing to endpoint: bad request: Authorization header is badly formatted\n\
plugin:playwright:playwright: npx @playwright/mcp@latest - ✔ Connected\n\
u: u-mcp - ⏸ Pending approval (run `claude` to approve)\n\
l: https://l.test (SSE) - ⊘ Disabled for this project (re-enable via /mcp)\n";
        let map = parse_mcp_list(text);
        let want = BTreeMap::from([
            ("plugin:context7:context7".to_string(), McpStatus::NeedsAuth),
            (
                "plugin:github:github".to_string(),
                McpStatus::Failed(
                    "HTTP 400: Error POSTing to endpoint: bad request: Authorization header is badly formatted"
                        .into(),
                ),
            ),
            ("plugin:playwright:playwright".to_string(), McpStatus::Connected),
            ("u".to_string(), McpStatus::Pending),
            ("l".to_string(), McpStatus::Disabled),
        ]);
        assert_eq!(map, want);
    }

    fn put(path: impl AsRef<Path>, text: &str) {
        let path = path.as_ref();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn put_json(path: impl AsRef<Path>, v: Value) {
        put(path, &v.to_string());
    }

    fn fixture(home: &Path, project: &Path) {
        let h = shown(home);
        let p = shown(project);
        let key = project_key_for(project);
        let cache = home.join(".claude/plugins/cache");
        fs::create_dir_all(project).unwrap();
        put_json(
            home.join(".claude/plugins/installed_plugins.json"),
            json!({"version":2,"plugins":{
                "alpha@m":[{"scope":"user","installPath":format!("{h}/.claude/plugins/cache/m/alpha/1.2.0"),"version":"1.2.0"}],
                "beta@m":[{"scope":"project","projectPath":p,"installPath":format!("{h}/.claude/plugins/cache/m/beta/abc1234"),"version":"abc1234"}],
                "gamma@n":[{"scope":"project","projectPath":"/elsewhere","installPath":format!("{h}/x/gamma"),"version":"1.0.0"}],
                "delta@n":[{"scope":"user","installPath":format!("{h}/.claude/plugins/cache/n/delta/2.0.0"),"version":"2.0.0"}]}}),
        );
        let a = cache.join("m/alpha/1.2.0");
        put_json(
            a.join(".claude-plugin/plugin.json"),
            json!({"name":"alpha","description":"Alpha tools"}),
        );
        for f in [
            "skills/one/SKILL.md",
            "skills/two/SKILL.md",
            "agents/rev.md",
            "agents/sub/deep.md",
            "commands/go.md",
        ] {
            put(a.join(f), "x");
        }
        put_json(
            a.join("hooks/hooks.json"),
            json!({"hooks":{
                "SessionStart":[{"hooks":[{"type":"command","command":"echo s3cr3t"}]}],
                "Stop":[{"hooks":[{"type":"command","command":"a"},{"type":"command","command":"b"}]}]}}),
        );
        put_json(
            a.join(".mcp.json"),
            json!({"mcpServers":{"db":{"command":"db-mcp","args":["--token","s3cr3t"],"env":{"KEY":"s3cr3t"}}}}),
        );
        let b = cache.join("m/beta/abc1234");
        put_json(b.join(".claude-plugin/plugin.json"), json!({"name":"beta"}));
        put_json(
            b.join(".mcp.json"),
            json!({"web":{"type":"http","url":"https://x.test/?k=s3cr3t","headers":{"Authorization":"Bearer s3cr3t"}}}),
        );
        fs::create_dir_all(cache.join("n/delta/2.0.0")).unwrap();
        let market = home.join(".claude/plugins/marketplaces/n");
        put_json(
            home.join(".claude/plugins/known_marketplaces.json"),
            json!({"n":{"installLocation":shown(&market)}}),
        );
        put_json(
            market.join(".claude-plugin/marketplace.json"),
            json!({"plugins":[{"name":"delta","description":"Go language server","lspServers":{"gopls":{"command":"gopls"}}}]}),
        );
        put_json(
            home.join(".claude/settings.json"),
            json!({"model":"opus","enabledPlugins":{"delta@n":false},"hooks":{
                "SessionStart":[{"hooks":[{"type":"command","command":"echo s3cr3t"}]}],
                "PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"x"},{"type":"command","command":"y"}]}]}}),
        );
        put(
            home.join(".claude/skills/mine/SKILL.md"),
            "---\nname: mine\ndescription: \"My skill\"\n---\nbody",
        );
        put(home.join(".claude/skills/synced/abc/SKILL.md"), "x");
        put(
            project.join(".claude/skills/local/SKILL.md"),
            "no frontmatter",
        );
        put(
            a.join("skills/one/SKILL.md"),
            "---\nname: one\ndescription: >\n  Folded\n  text\n---\n",
        );
        put(
            home.join(".claude/agents/helper.md"),
            "---\nname: helper\ndescription: Helps\n---\n",
        );
        put(project.join(".claude/agents/proj.md"), "x");
        put(
            home.join(".claude/CLAUDE.md"),
            "# Global rules\n- @instructions/a.md here\n`@instructions/no.md`\n```\n@instructions/no.md\n```\n@~/extra.md\n@missing.md\n",
        );
        put(home.join(".claude/instructions/a.md"), "@b.md");
        put(home.join(".claude/instructions/b.md"), "@a.md");
        put(home.join(".claude/instructions/no.md"), "x");
        put(home.join("extra.md"), "x");
        put(project.join("CLAUDE.md"), "x");
        put_json(
            home.join(".claude.json"),
            json!({"oauthAccount":{"emailAddress":"s3cr3t@x.test"},"mcpServers":{"u":{"command":"u-mcp"}},
                "projects":{key:{"mcpServers":{"l":{"type":"sse","url":"https://l.test"}},"disabledMcpServers":["plugin:alpha:db"]}}}),
        );
        put_json(
            project.join(".mcp.json"),
            json!({"mcpServers":{"p":{"type":"ws","url":"wss://p.test"}}}),
        );
    }

    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let base = canonical(tmp.path());
        let (home, project) = (base.join("home"), base.join("proj"));
        fixture(&home, &project);
        (tmp, home, project)
    }

    fn alpha_manifest(home: &Path) -> PathBuf {
        home.join(".claude/plugins/cache/m/alpha/1.2.0/.claude-plugin/plugin.json")
    }

    #[test]
    fn plugins_follow_installs_and_enabled_plugins() {
        let (_t, home, project) = setup();
        let d = read_claude(&home, &project);
        let ids: Vec<_> = d.plugins.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["alpha@m", "beta@m", "delta@n"]);
        let on: Vec<_> = d.plugins.iter().map(|p| p.enabled).collect();
        assert_eq!(on, [true, true, false]);
        assert_eq!(d.plugins[0].version, "1.2.0");
        assert!(d.plugins.iter().all(|p| p.locked.is_none()));
    }

    #[test]
    fn plugin_summary_counts_components() {
        let (_t, home, project) = setup();
        let d = read_claude(&home, &project);
        assert_eq!(
            d.plugins[0].summary,
            "2 skills · 2 agents · 1 command · 3 hooks · 1 MCP server"
        );
        assert_eq!(d.plugins[1].summary, "1 MCP server");
        assert_eq!(d.plugins[2].summary, "1 language server");
    }

    #[test]
    fn default_enabled_false_starts_off() {
        let (_t, home, project) = setup();
        put_json(
            alpha_manifest(&home),
            json!({"name":"alpha","description":"Alpha tools","defaultEnabled":false}),
        );
        assert!(!read_claude(&home, &project).plugins[0].enabled);
    }

    #[test]
    fn project_and_local_settings_lock_the_toggle() {
        let (_t, home, project) = setup();
        put_json(
            project.join(".claude/settings.json"),
            json!({"enabledPlugins":{"alpha@m":false}}),
        );
        let alpha = read_claude(&home, &project).plugins.remove(0);
        assert!(!alpha.enabled);
        assert_eq!(
            alpha.locked.as_deref(),
            Some("Set in this project's .claude/settings.json")
        );
        put_json(
            project.join(".claude/settings.local.json"),
            json!({"enabledPlugins":{"alpha@m":true}}),
        );
        let alpha = read_claude(&home, &project).plugins.remove(0);
        assert!(alpha.enabled);
        assert_eq!(
            alpha.locked.as_deref(),
            Some("Set in this project's .claude/settings.local.json")
        );
    }

    #[test]
    fn project_at_home_is_not_a_second_user() {
        let (_t, home, project) = setup();
        put_json(
            home.join(".claude/settings.json"),
            json!({"enabledPlugins":{"alpha@m":true},"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"x"}]}]}}),
        );
        put(
            home.join(".claude/skills/mine/SKILL.md"),
            "---\nname: mine\n---\n",
        );
        let _ = project;
        let d = read_claude(&home, &home);
        assert_eq!(
            d.plugins.iter().find(|p| p.id == "alpha@m").unwrap().locked,
            None
        );
        let mine = |g: &str| d.skills.iter().filter(|e| e.group == g).count();
        assert_eq!((mine("User"), mine("Project")), (1, 0));
        let hooks = d.hooks.iter().filter(|e| e.name == "SessionStart").count();
        assert_eq!(hooks, 1);
    }

    #[test]
    fn empty_install_path_is_skipped() {
        let (_t, home, project) = setup();
        let cwd = home.join("elsewhere");
        put(cwd.join("skills/x/SKILL.md"), "x");
        put_json(
            home.join(".claude/plugins/installed_plugins.json"),
            json!({"plugins":{
                "a@m":[{"scope":"user","installPath":"","version":"1"}],
                "b@m":[{"scope":"user","version":"1"}]}}),
        );
        let old = std::env::current_dir().unwrap();
        std::env::set_current_dir(&cwd).unwrap();
        let d = read_claude(&home, &project);
        std::env::set_current_dir(old).unwrap();
        assert_eq!(d.plugins.len(), 2);
        assert!(d.plugins.iter().all(|p| p.summary == "No components"));
        assert!(d.skills.iter().all(|e| e.group != "a" && e.group != "b"));
        assert!(d.skills.iter().all(|e| !e.path.starts_with(&cwd)));
    }

    #[test]
    fn git_root_local_settings_lock_the_toggle() {
        let (_t, home, _) = setup();
        let root = home.parent().unwrap().join("repo");
        let project = root.join("sub");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        put_json(
            root.join(".claude/settings.local.json"),
            json!({"enabledPlugins":{"alpha@m":false}}),
        );
        let alpha = read_claude(&home, &project).plugins.remove(0);
        assert!(!alpha.enabled);
        assert_eq!(
            alpha.locked.as_deref(),
            Some("Set in the repository's .claude/settings.local.json")
        );
        fs::create_dir_all(home.join(".git")).unwrap();
        put_json(
            home.join(".claude/settings.local.json"),
            json!({"enabledPlugins":{"alpha@m":false}}),
        );
        let alpha = read_claude(&home, &home).plugins.remove(0);
        assert_eq!(alpha.locked, None);
    }

    #[test]
    fn default_manifest_paths_are_not_counted_twice() {
        let (_t, home, project) = setup();
        let before = read_claude(&home, &project);
        put_json(
            alpha_manifest(&home),
            json!({"name":"alpha","skills":"./skills/","hooks":"./hooks/hooks.json"}),
        );
        let d = read_claude(&home, &project);
        let n = d.skills.iter().filter(|e| e.group == "alpha").count();
        assert_eq!(n, 2);
        assert!(d.plugins[0].summary.starts_with("2 skills · "));
        assert!(d.plugins[0].summary.contains("3 hooks"));
        assert_eq!(
            d.hooks.iter().filter(|e| e.name == "alpha").count(),
            before.hooks.iter().filter(|e| e.name == "alpha").count()
        );
    }

    #[test]
    fn manifest_paths_add_or_replace() {
        let (_t, home, project) = setup();
        let a = home.join(".claude/plugins/cache/m/alpha/1.2.0");
        put_json(
            alpha_manifest(&home),
            json!({"name":"alpha","skills":"./extra/","agents":["./more/x.md"],"mcpServers":"./m.json"}),
        );
        put(a.join("extra/three/SKILL.md"), "x");
        put(a.join("more/x.md"), "x");
        put_json(
            a.join("m.json"),
            json!({"mcpServers":{"db2":{"command":"y"}}}),
        );
        assert_eq!(
            read_claude(&home, &project).plugins[0].summary,
            "3 skills · 1 agent · 1 command · 3 hooks · 2 MCP servers"
        );
    }

    #[test]
    fn mcp_servers_from_plugins_user_local_and_project() {
        let (_t, home, project) = setup();
        let d = read_claude(&home, &project);
        let names: Vec<_> = d.mcp.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["plugin:alpha:db", "plugin:beta:web", "u", "l", "p"]);
        let labels: Vec<_> = d.mcp.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["db", "web", "u", "l", "p"]);
        let t: Vec<_> = d.mcp.iter().map(|s| s.transport).collect();
        assert_eq!(t, ["stdio", "HTTP", "stdio", "SSE", "WebSocket"]);
        let sources: Vec<_> = d.mcp.iter().map(|s| s.source.clone()).collect();
        assert_eq!(
            sources,
            [
                McpSource::Plugin("alpha".into()),
                McpSource::Plugin("beta".into()),
                McpSource::User,
                McpSource::Local,
                McpSource::Project
            ]
        );
        let on: Vec<_> = d.mcp.iter().map(|s| s.enabled).collect();
        assert_eq!(on, [false, true, true, true, true]);
        assert_eq!(d.project_key, Some(project_key_for(&project)));
    }

    #[test]
    fn disabled_plugins_add_no_servers() {
        let (_t, home, project) = setup();
        put_json(
            home.join(".claude/plugins/cache/n/delta/2.0.0/.mcp.json"),
            json!({"mcpServers":{"z":{"command":"z"}}}),
        );
        let d = read_claude(&home, &project);
        assert!(d.mcp.iter().all(|s| !s.name.starts_with("plugin:delta:")));
        assert_eq!(d.plugins[2].summary, "1 MCP server · 1 language server");
    }

    #[test]
    fn project_key_prefers_an_existing_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let base = canonical(tmp.path());
        let home = base.join("home");
        let repo = base.join("repo");
        let project = repo.join("sub");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(&project).unwrap();
        put_json(
            home.join(".claude.json"),
            json!({"projects":{project_key_for(&repo):{}}}),
        );
        assert_eq!(
            read_claude(&home, &project).project_key,
            Some(project_key_for(&repo))
        );
        put_json(home.join(".claude.json"), json!({"projects":{}}));
        assert_eq!(read_claude(&home, &project).project_key, None);
    }

    #[test]
    fn nothing_secret_is_kept() {
        let (_t, home, project) = setup();
        assert!(!format!("{:?}", read_claude(&home, &project)).contains("s3cr3t"));
    }

    #[test]
    fn broken_files_are_reported_and_lock_their_toggles() {
        let (_t, home, project) = setup();
        let settings = home.join(".claude/settings.json");
        let good = fs::read_to_string(&settings).unwrap();
        put(&settings, "{oops");
        let d = read_claude(&home, &project);
        assert!(d.problems[0].starts_with("~/.claude/settings.json does not parse"));
        assert_eq!(d.plugins.len(), 3);
        assert!(
            d.plugins
                .iter()
                .all(|p| p.locked.as_deref() == Some("Fix ~/.claude/settings.json first"))
        );
        put(&settings, &good);
        put(home.join(".claude.json"), "{");
        let d = read_claude(&home, &project);
        assert!(d.problems[0].starts_with("~/.claude.json does not parse"));
        let names: Vec<_> = d.mcp.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["plugin:alpha:db", "plugin:beta:web", "p"]);
    }

    #[test]
    fn short_paths_use_a_tilde() {
        let home = Path::new("/home/me");
        assert_eq!(short(&home.join(".claude/x"), home), "~/.claude/x");
        assert_eq!(short(Path::new("/opt/x"), home), "/opt/x");
    }

    #[test]
    fn stamps_follow_the_bytes() {
        assert_eq!(Stamp::of(None), Stamp::default());
        assert_ne!(Stamp::of(Some(b"a")), Stamp::of(Some(b"b")));
        let (_t, home, project) = setup();
        let bytes = fs::read(home.join(".claude/settings.json")).unwrap();
        assert_eq!(
            read_claude(&home, &project).settings_stamp,
            Stamp::of(Some(&bytes))
        );
    }

    #[test]
    fn frontmatter_reads_name_and_description() {
        assert_eq!(
            frontmatter("---\nname: a\ndescription: \"Quoted\"\n---\n"),
            (Some("a".into()), Some("Quoted".into()))
        );
        assert_eq!(
            frontmatter("---\ndescription: >\n  Folded\n  text\n---\n")
                .1
                .as_deref(),
            Some("Folded text")
        );
        assert_eq!(frontmatter("no frontmatter"), (None, None));
    }

    #[test]
    fn imports_skip_code() {
        assert_eq!(
            imports("a @x.md `@y.md`\n```\n@z.md\n```\n@my\\ file.md"),
            ["x.md", "my file.md"]
        );
    }

    fn group<'a>(list: &'a [Entry], g: &str) -> Vec<&'a Entry> {
        list.iter().filter(|e| e.group == g).collect()
    }

    #[test]
    fn skills_from_user_project_and_enabled_plugins() {
        let (_t, home, project) = setup();
        let d = read_claude(&home, &project);
        let pairs: Vec<_> = d
            .skills
            .iter()
            .map(|e| (e.group.as_str(), e.name.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("User", "mine"),
                ("Project", "local"),
                ("alpha", "one"),
                ("alpha", "two")
            ]
        );
        assert_eq!(d.skills[0].summary, "My skill");
        assert_eq!(d.skills[2].summary, "Folded text");
        let ro: Vec<_> = d.skills.iter().map(|e| e.read_only).collect();
        assert_eq!(ro, [false, false, true, true]);
        assert!(d.skills.iter().all(|e| e.name != "synced"));
    }

    #[test]
    fn agents_from_user_project_and_plugins() {
        let (_t, home, project) = setup();
        let d = read_claude(&home, &project);
        let pairs: Vec<_> = d
            .agents
            .iter()
            .map(|e| (e.group.as_str(), e.name.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("User", "helper"),
                ("Project", "proj"),
                ("alpha", "deep"),
                ("alpha", "rev")
            ]
        );
        assert_eq!(d.agents[0].summary, "Helps");
    }

    #[test]
    fn instructions_follow_imports() {
        let (_t, home, project) = setup();
        let d = read_claude(&home, &project);
        let user = group(&d.instructions, "User");
        let names: Vec<_> = user.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["CLAUDE.md", "a.md", "b.md", "extra.md"]);
        let sums: Vec<_> = user.iter().map(|e| e.summary.as_str()).collect();
        assert_eq!(
            sums,
            [
                "Global rules",
                "Included by CLAUDE.md",
                "Included by a.md",
                "Included by CLAUDE.md"
            ]
        );
    }

    #[test]
    fn imports_stop_after_four_hops() {
        let (_t, home, project) = setup();
        let dir = home.join(".claude");
        put(dir.join("CLAUDE.md"), "@d1.md");
        for i in 1..=5 {
            put(dir.join(format!("d{i}.md")), &format!("@d{}.md", i + 1));
        }
        let d = read_claude(&home, &project);
        let names: Vec<_> = group(&d.instructions, "User")
            .iter()
            .map(|e| e.name.clone())
            .collect();
        assert_eq!(names, ["CLAUDE.md", "d1.md", "d2.md", "d3.md", "d4.md"]);
    }

    #[test]
    fn project_instructions_walk_up() {
        let (t, home, _) = setup();
        let project = canonical(t.path()).join("root/proj");
        put(project.parent().unwrap().join("CLAUDE.md"), "x");
        put(project.join("CLAUDE.md"), "x");
        put(project.join(".claude/CLAUDE.md"), "x");
        put(project.join("CLAUDE.local.md"), "x");
        let d = read_claude(&home, &project);
        let sums: Vec<_> = group(&d.instructions, "Project")
            .iter()
            .map(|e| e.summary.clone())
            .collect();
        assert_eq!(
            sums,
            [
                "Parent folder",
                "This project",
                "This project",
                "Yours, not shared"
            ]
        );
    }

    #[test]
    fn hooks_show_events_not_commands() {
        let (_t, home, project) = setup();
        let d = read_claude(&home, &project);
        let user: Vec<_> = group(&d.hooks, "User")
            .iter()
            .map(|e| (e.name.as_str(), e.summary.as_str()))
            .collect();
        assert_eq!(
            user,
            [("SessionStart", "1 hook"), ("PreToolUse", "2 hooks")]
        );
        let plugins = group(&d.hooks, "From plugins");
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "alpha");
        assert_eq!(plugins[0].summary, "SessionStart · Stop");
        assert!(plugins[0].path.ends_with("alpha/1.2.0/hooks/hooks.json"));
        assert!(plugins[0].read_only);
        assert!(!format!("{d:?}").contains("s3cr3t"));
    }

    #[test]
    fn disabled_plugins_contribute_nothing() {
        let (_t, home, project) = setup();
        put_json(
            home.join(".claude/settings.json"),
            json!({"enabledPlugins":{"alpha@m":false}}),
        );
        let d = read_claude(&home, &project);
        for list in [&d.skills, &d.agents, &d.hooks] {
            assert!(list.iter().all(|e| e.group != "alpha" && e.name != "alpha"));
        }
    }

    #[test]
    fn odd_files_do_not_break_the_read() {
        let (_t, home, project) = setup();
        let cache = home.join(".claude/plugins/cache/m");
        put(cache.join("beta/abc1234/.claude-plugin/plugin.json"), "[]");
        put(
            cache.join("beta/abc1234/hooks/hooks.json"),
            r#"{"hooks":[]}"#,
        );
        let odd = home.join(".claude/skills/odd/SKILL.md");
        fs::create_dir_all(odd.parent().unwrap()).unwrap();
        fs::write(&odd, [0xff, 0xfe, b'x']).unwrap();
        let path = home.join(".claude/plugins/installed_plugins.json");
        let mut installed: Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        installed["plugins"]["eps@m"] =
            json!([{"scope":"user","installPath":shown(&cache.join("gone")),"version":"1"}]);
        put_json(&path, installed);
        let d = read_claude(&home, &project);
        assert!(d.plugins.iter().any(|p| p.name == "beta"));
        let eps = d.plugins.iter().find(|p| p.name == "eps").unwrap();
        assert_eq!(eps.summary, "No components");
        let odd = d.skills.iter().find(|e| e.name == "odd").unwrap();
        assert_eq!(odd.summary, "");
    }

    fn settings_path(home: &Path) -> PathBuf {
        home.join(".claude/settings.json")
    }

    fn stamp_of(path: &Path) -> Stamp {
        Stamp::of(fs::read(path).ok().as_deref())
    }

    #[test]
    fn plugin_toggle_changes_only_its_key() {
        let t = tempfile::tempdir().unwrap();
        let p = settings_path(t.path());
        put(
            &p,
            "{\n    \"model\": \"opus\",\n    \"enabledPlugins\": {\"a@m\": true},\n    \"hooks\": {\"Stop\": []},\n    \"zeta\": 1\n}\n",
        );
        let before = fs::read(&p).unwrap();
        let st = set_plugin_enabled(t.path(), "a@m", false, Stamp::of(Some(&before))).unwrap();
        let after = fs::read(&p).unwrap();
        let text = String::from_utf8(after.clone()).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["model", "enabledPlugins", "hooks", "zeta"]);
        assert_eq!(v["enabledPlugins"]["a@m"], json!(false));
        assert!(text.ends_with('\n'));
        assert!(text.lines().nth(1).unwrap().starts_with("    \""));
        assert_eq!(st, Stamp::of(Some(&after)));
    }

    #[test]
    fn plugin_toggle_creates_a_missing_file() {
        let t = tempfile::tempdir().unwrap();
        set_plugin_enabled(t.path(), "a@m", true, Stamp::default()).unwrap();
        let text = fs::read_to_string(settings_path(t.path())).unwrap();
        assert_eq!(
            text,
            "{\n  \"enabledPlugins\": {\n    \"a@m\": true\n  }\n}"
        );
    }

    #[test]
    fn changed_file_is_left_alone() {
        let t = tempfile::tempdir().unwrap();
        let p = settings_path(t.path());
        put(&p, "{\"model\":\"opus\"}");
        let seen = stamp_of(&p);
        fs::write(&p, "{\"model\":\"other\"}").unwrap();
        let r = set_plugin_enabled(t.path(), "a@m", false, seen);
        assert!(matches!(r, Err(WriteError::Changed)));
        assert_eq!(fs::read_to_string(&p).unwrap(), "{\"model\":\"other\"}");
    }

    #[test]
    fn invalid_files_are_left_alone() {
        let t = tempfile::tempdir().unwrap();
        let p = settings_path(t.path());
        for text in ["{oops", "{\"enabledPlugins\":[]}"] {
            put(&p, text);
            let r = set_plugin_enabled(t.path(), "a@m", false, stamp_of(&p));
            assert!(matches!(r, Err(WriteError::Invalid(_))), "{text}");
            assert_eq!(fs::read_to_string(&p).unwrap(), text);
        }
    }

    #[test]
    fn toggling_off_and_on_restores_the_bytes() {
        let t = tempfile::tempdir().unwrap();
        let p = settings_path(t.path());
        put(
            &p,
            "{\n  \"numStartups\": 3,\n  \"lastCost\": 0.123456,\n  \"enabledPlugins\": {\n    \"a@m\": true,\n    \"b@m\": false\n  },\n  \"hooks\": {\n    \"Stop\": []\n  }\n}\n",
        );
        let original = fs::read(&p).unwrap();
        let orig_v: Value = serde_json::from_slice(&original).unwrap();
        let st = set_plugin_enabled(t.path(), "a@m", false, stamp_of(&p)).unwrap();
        let mut v: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        assert_eq!(v["enabledPlugins"]["a@m"], json!(false));
        v["enabledPlugins"]["a@m"] = json!(true);
        assert_eq!(v, orig_v);
        set_plugin_enabled(t.path(), "a@m", true, st).unwrap();
        assert_eq!(fs::read(&p).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_target_is_written() {
        let t = tempfile::tempdir().unwrap();
        let real = t.path().join("dotfiles/settings.json");
        put(&real, "{\"model\":\"opus\"}");
        let link = settings_path(t.path());
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        set_plugin_enabled(t.path(), "a@m", true, stamp_of(&link)).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(fs::read_to_string(&real).unwrap().contains("a@m"));
    }

    #[cfg(unix)]
    #[test]
    fn mode_is_kept() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let p = settings_path(t.path());
        put(&p, "{\"model\":\"opus\"}");
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        set_plugin_enabled(t.path(), "a@m", true, stamp_of(&p)).unwrap();
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn busy_lock_gives_up() {
        let t = tempfile::tempdir().unwrap();
        let p = settings_path(t.path());
        put(&p, "{}");
        let lock = t.path().join(".claude/settings.json.lock");
        fs::create_dir(&lock).unwrap();
        let start = std::time::Instant::now();
        let r = set_plugin_enabled(t.path(), "a@m", true, stamp_of(&p));
        assert!(matches!(r, Err(WriteError::Busy)));
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(fs::read_to_string(&p).unwrap(), "{}");
        assert!(lock.exists());
    }

    #[cfg(unix)]
    #[test]
    fn stale_lock_is_replaced() {
        let t = tempfile::tempdir().unwrap();
        let p = settings_path(t.path());
        put(&p, "{}");
        let lock = t.path().join(".claude/settings.json.lock");
        fs::create_dir(&lock).unwrap();
        fs::File::open(&lock)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(60))
            .unwrap();
        set_plugin_enabled(t.path(), "a@m", true, stamp_of(&p)).unwrap();
        assert!(!lock.exists());
    }

    #[test]
    fn lock_is_released() {
        let t = tempfile::tempdir().unwrap();
        let p = settings_path(t.path());
        set_plugin_enabled(t.path(), "a@m", true, Stamp::default()).unwrap();
        assert!(!p.with_extension("json.lock").exists());
    }

    fn claude_json(home: &Path) -> PathBuf {
        home.join(".claude.json")
    }

    const MCP_FIXTURE: &str = "{\n    \"zeta\": 1,\n    \"oauthAccount\": {\"name\": \"Zoë\", \"id\": 7},\n    \"nums\": [0.1, 1e21, 12345678901234567890, [1, [2, {\"b\": 1, \"a\": 2}]]],\n    \"projects\": {\n        \"/z/proj\": {\"allowedTools\": [], \"disabledMcpServers\": [\"old\"], \"b\": 1, \"a\": 2},\n        \"/a/proj\": {\"x\": null}\n    }\n}\n";

    #[test]
    fn mcp_write_keeps_everything_else() {
        let t = tempfile::tempdir().unwrap();
        let p = claude_json(t.path());
        put(&p, MCP_FIXTURE);
        let orig: Value = serde_json::from_str(MCP_FIXTURE).unwrap();
        set_mcp_enabled(t.path(), "/z/proj", "db", false).unwrap();
        let text = fs::read_to_string(&p).unwrap();
        let mut v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            v["projects"]["/z/proj"]["disabledMcpServers"],
            json!(["old", "db"])
        );
        v["projects"]["/z/proj"]["disabledMcpServers"] = json!(["old"]);
        assert_eq!(v, orig);
        assert_eq!(v.to_string(), orig.to_string());
        assert!(text.ends_with("}\n"));
        assert!(text.lines().nth(1).unwrap().starts_with("    \""));
        set_mcp_enabled(t.path(), "/z/proj", "db", true).unwrap();
        set_mcp_enabled(t.path(), "/z/proj", "old", true).unwrap();
        let v: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        assert_eq!(v["projects"]["/z/proj"]["disabledMcpServers"], json!([]));
    }

    #[cfg(unix)]
    #[test]
    fn mcp_write_keeps_mode_and_symlink() {
        use std::os::unix::fs::PermissionsExt;
        for mode in [0o600, 0o640] {
            let t = tempfile::tempdir().unwrap();
            let p = claude_json(t.path());
            put(&p, MCP_FIXTURE);
            fs::set_permissions(&p, fs::Permissions::from_mode(mode)).unwrap();
            set_mcp_enabled(t.path(), "/a/proj", "db", false).unwrap();
            assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, mode);
        }
        let t = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let real = other.path().join("real.json");
        put(&real, MCP_FIXTURE);
        fs::set_permissions(&real, fs::Permissions::from_mode(0o640)).unwrap();
        let link = claude_json(t.path());
        std::os::unix::fs::symlink(&real, &link).unwrap();
        set_mcp_enabled(t.path(), "/a/proj", "db", false).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(fs::read_to_string(&real).unwrap().contains("\"db\""));
        assert_eq!(
            fs::metadata(&real).unwrap().permissions().mode() & 0o777,
            0o640
        );
        for dir in [t.path(), other.path()] {
            for e in fs::read_dir(dir).unwrap() {
                let n = e.unwrap().file_name().to_string_lossy().into_owned();
                assert!(!n.ends_with(".wai.tmp") && !n.ends_with(".lock"), "{n}");
            }
        }
    }

    #[test]
    fn mcp_write_creates_missing_entry_only_when_turning_off() {
        let t = tempfile::tempdir().unwrap();
        let p = claude_json(t.path());
        put(&p, "{\"a\":1}");
        set_mcp_enabled(t.path(), "/p", "db", false).unwrap();
        let v: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        assert_eq!(v["projects"]["/p"]["disabledMcpServers"], json!(["db"]));
        assert_eq!(v["a"], json!(1));
        set_mcp_enabled(t.path(), "/p", "db", false).unwrap();
        let v: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        assert_eq!(v["projects"]["/p"]["disabledMcpServers"], json!(["db"]));
    }

    #[test]
    fn mcp_on_without_entry_writes_nothing() {
        let t = tempfile::tempdir().unwrap();
        let p = claude_json(t.path());
        put(&p, "{\"projects\":{}}");
        fs::File::open(&p)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(600))
            .unwrap();
        let m = fs::metadata(&p).unwrap().modified().unwrap();
        for text in ["{\"projects\":{}}", "{}", "{\"projects\":{\"/p\":{}}}"] {
            fs::write(&p, text).unwrap();
            fs::File::open(&p).unwrap().set_modified(m).unwrap();
            set_mcp_enabled(t.path(), "/p", "db", true).unwrap();
            assert_eq!(fs::read_to_string(&p).unwrap(), text);
            assert_eq!(fs::metadata(&p).unwrap().modified().unwrap(), m);
        }
    }

    #[test]
    fn mcp_write_rejects_wrong_shapes() {
        let t = tempfile::tempdir().unwrap();
        let p = claude_json(t.path());
        for text in [
            "{\"projects\":[]}",
            "{\"projects\":{\"/p\":[]}}",
            "{\"projects\":{\"/p\":{\"disabledMcpServers\":\"x\"}}}",
        ] {
            for on in [false, true] {
                put(&p, text);
                let r = set_mcp_enabled(t.path(), "/p", "db", on);
                assert!(matches!(r, Err(WriteError::Invalid(_))), "{text}");
                assert_eq!(fs::read_to_string(&p).unwrap(), text);
            }
        }
    }

    #[test]
    fn mcp_write_aborts_if_the_file_changes_underneath() {
        let t = tempfile::tempdir().unwrap();
        let p = claude_json(t.path());
        put(&p, "{\"a\":1}");
        let other = {
            replace_json(&p, None, |_| {
                fs::write(&p, "{\"a\":2}").unwrap();
                Ok(true)
            })
        };
        assert!(matches!(other, Err(WriteError::Changed)));
        assert_eq!(fs::read_to_string(&p).unwrap(), "{\"a\":2}");
        assert!(!t.path().join(".claude.json.wai.tmp").exists());
    }

    #[cfg(unix)]
    #[test]
    fn planted_tmp_symlink_is_not_followed() {
        let t = tempfile::tempdir().unwrap();
        let p = claude_json(t.path());
        put(&p, "{}");
        let victim = t.path().join("victim");
        put(&victim, "keep");
        std::os::unix::fs::symlink(&victim, t.path().join(".claude.json.wai.tmp")).unwrap();
        set_mcp_enabled(t.path(), "/p", "db", false).unwrap();
        assert_eq!(fs::read_to_string(&victim).unwrap(), "keep");
        assert!(fs::read_to_string(&p).unwrap().contains("\"db\""));
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_is_kept() {
        let t = tempfile::tempdir().unwrap();
        let link = claude_json(t.path());
        std::os::unix::fs::symlink("dot/real.json", &link).unwrap();
        set_mcp_enabled(t.path(), "/p", "db", false).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_link(&link).unwrap(), Path::new("dot/real.json"));
        assert!(
            fs::read_to_string(t.path().join("dot/real.json"))
                .unwrap()
                .contains("\"db\"")
        );
    }
}
