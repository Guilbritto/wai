# wai settings screen and agent choice: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a Settings tab where the user picks which of four ACP agents wai uses (none pre-chosen), picks models, and changes theme, shell and updates. Every change applies live, and nothing new runs without the user's approval.

**Architecture:**
- **`wai-settings`** gains the data model: an optional `default_agent`, the four built-ins, `[models]`, a tighter R15, approved values in `state.json`, and a `toml_edit` writer.
- **`wai-agent`** gains ACP session config options (model, mode, thought level).
- **The `wai` crate** gains:
  - a `Pane::Settings(Entity<SettingsView>)` that is fed snapshots by the `Workspace`;
  - a watcher on the user's `settings.toml` that runs everything through one `Workspace::apply_settings`;
  - an approval dialog that guards every agent command and shell.

**Tech stack:** Rust 2024; GPUI through `gpui-kit = "=0.9.0"` (gpui-component widgets: `Select`, `Switch`, `Input`, `Button`, dialogs); `agent-client-protocol = "5"` (schema 1.15.1); `toml` 0.9 plus `toml_edit` (0.25, already in `Cargo.lock`); `notify` 8 through `wai_project::ProjectWatcher`.

**Spec:** `docs/superpowers/specs/2026-11-10-wai-settings-and-agents-design.md`. Read it first; this plan argues from it.

## Global constraints

- **Built-in agents, exact commands:**
  - `claude`: `npx -y @agentclientprotocol/claude-agent-acp@0.101.0`
  - `codex`: `npx -y @agentclientprotocol/codex-acp@2.6.0`
  - `antigravity`: `agy acp`
  - `copilot`: `copilot --acp --stdio`
- **R15:** a project's `.wai/settings.toml` never sets `agents`, `terminal`, `updates` or `models`. Its `default_agent` is honoured only under the rule in Task 1.
- **No login screen**, and no credential reading, storing or forwarding.
- **The UI copy is English.** Exact strings are given where tests pin them.
- **The screen writes only the user's file** (`<config_dir>/settings.toml`). It never writes over a file that does not parse.
- **Approval:** wai runs an agent command or a terminal shell only when that exact value is approved. Approved values live in `state.json`, never in `settings.toml`.
- **Keys:** ⌘, (`secondary-,`) opens Settings; ⌘W closes its pane.
- **Platforms:** must build on Windows x86_64 too. Paths go through `wai_settings::shown` when displayed.
- **The CI gate:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build -p wai-agent --bin wai-fake-agent`, `cargo test --workspace`.
- **Commits:** `feat:`, `fix:`, `docs:` or `test:`, no scope, no `Co-Authored-By`.

## Review focus

These are the failure modes the spec implies but no feature test would naturally hit. Each one has a test in the task named after it.

1. **Feedback loop on wai's own writes.** wai's write to `settings.toml` comes back through the watcher. It must not re-ask an approval just given, and must not flicker the screen. (Task 6: `own_write_reloads_once_without_a_second_dialog`)
2. **A deleted `settings.toml` while wai runs.** The defaults come back, so there is no default agent. Open sessions keep working, no agent is killed, and the next ⌘N opens Settings. (Task 6: `deleted_user_file_falls_back_to_first_run`)
3. **Typing in a Settings field while the file reloads.** A reload must not reset an input the user is editing. Fields commit on Enter or blur only. (Task 4: `reload_keeps_a_field_being_edited`)
4. **Commands with quotes or spaces, and `npx.cmd` on Windows.** CLI detection must parse the command the way the spawn does, through `AcpAgent::from_str`. (Task 3: `program_parses_like_the_spawn`)
5. **A project switch while Settings is open.** `set_project` re-reads the settings with the new project's layer, and the screen and the project override note follow. (Task 6: `project_switch_reloads_settings`)

---

### Task 1: Settings model — optional default, built-ins, `[models]`, tighter R15

**Files:**
- Modify: `crates/wai-settings/src/lib.rs`
- Modify (compile fixes only): `crates/wai/src/workspace.rs`, `crates/wai/src/sidebar.rs`, `crates/wai/src/palette.rs`, and any other `default_agent` reader (`grep -rn default_agent crates/wai/src`)

**Interfaces:**
- **Produces:**
  - `Settings.default_agent: Option<String>`
  - `Settings.models: BTreeMap<String, String>` (agent id → config value id)
  - `pub struct BuiltinAgent { pub id: &'static str, pub name: &'static str, pub command: &'static str, pub install: &'static str, pub login: &'static str }`
  - `pub const BUILTIN_AGENTS: [BuiltinAgent; 4]`
  - `pub fn builtin(id: &str) -> Option<&'static BuiltinAgent>`
  - `pub fn agent_label(id: &str) -> String`: the built-in's name, else `id`
  - `pub fn project_default_agent(project_dir: &Path) -> Option<String>`: the `default_agent` of `<project_dir>/.wai/settings.toml` as written, read through `read_layer`. `None` when there is none or the file does not load. Task 8 shows "This project uses …" when this equals the loaded `default_agent`.
  - `Workspace::default_agent(&self) -> Option<String>`

  `BUILTIN_AGENTS` values: commands from Global Constraints. Names, install hints and login hints:

  | id | `name` | `install` | `login` |
  |---|---|---|---|
  | `claude` | `Claude` | `Install Node.js (it provides npx)` | `claude /login` |
  | `codex` | `Codex` | `Install Node.js (it provides npx)` | `npx @openai/codex login` |
  | `antigravity` | `Antigravity` | `https://antigravity.google/docs/cli` | `agy login` |
  | `copilot` | `Copilot` | `npm install -g @github/copilot` | `copilot, then /login` |

  Task 11 verifies these hints against the real CLIs.

- [ ] **Step 1: Write the failing tests** in `lib.rs`'s `mod tests`. Replace `defaults_when_no_files` and `project_overrides_theme_and_default_agent`, and keep the other existing tests passing.

```rust
#[test]
fn defaults_have_four_agents_and_no_default() {
    let d = tempfile::tempdir().unwrap();
    let (s, _) = Settings::load(d.path(), None).unwrap();
    assert_eq!(s.default_agent, None);
    let ids: Vec<_> = s.agents.keys().map(String::as_str).collect();
    assert_eq!(ids, ["antigravity", "claude", "codex", "copilot"]);
    assert_eq!(s.agents["codex"].command, "npx -y @agentclientprotocol/codex-acp@2.6.0");
    assert_eq!(s.agents["copilot"].command, "copilot --acp --stdio");
    assert_eq!(agent_label("copilot"), "Copilot");
    assert_eq!(agent_label("my/agent"), "my/agent");
}

#[test]
fn project_default_needs_a_user_choice() {
    let (u, p) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    write(p.path(), ".wai/settings.toml", "default_agent = \"copilot\"\n");
    let (s, notices) = Settings::load(u.path(), Some(p.path())).unwrap();
    assert_eq!(s.default_agent, None);
    let path = p.path().join(".wai").join("settings.toml");
    assert_eq!(notices, [format!(
        "Ignored default_agent \"copilot\" from {}: a project can only pick an AI you already chose or set up.",
        path.display()
    )]);
}

#[test]
fn project_default_may_name_the_users_agents_or_an_extension_agent() {
    let (u, p) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    write(u.path(), "settings.toml",
        "default_agent = \"claude\"\n[agents.other]\ncommand = \"other-acp\"\n");
    for (name, honoured) in [("claude", true), ("other", true), ("ext/agent", true), ("copilot", false)] {
        write(p.path(), ".wai/settings.toml", &format!("default_agent = \"{name}\"\n"));
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
    write(p.path(), ".wai/settings.toml", "[models]\nclaude = \"haiku\"\n");
    let (s, notices) = Settings::load(u.path(), Some(p.path())).unwrap();
    assert_eq!(s.models["claude"], "opus");
    let path = p.path().join(".wai").join("settings.toml");
    assert_eq!(notices, [format!(
        "Ignored models settings from {}: only user settings can choose default models.",
        path.display()
    )]);
}
```

- [ ] **Step 2: Run them and confirm they fail.**
  - Run: `cargo test -p wai-settings`
  - Expected: compile errors (`agent_label` is missing, and `default_agent` is not an `Option`).

- [ ] **Step 3: Implement the model in `lib.rs`.**
  - `Settings::default()` has `default_agent: None`, `agents` holding all four built-ins, and `models` empty. `DEFAULT_CLAUDE_COMMAND` becomes the `claude` entry of `BUILTIN_AGENTS`.
  - In `Settings::load`, keep the user layer's `agents` keys and its `default_agent` before merging.
  - Strip `models` from the project layer, with the notice above.
  - After merging the project layer, keep its `default_agent` only when:
    - the user layer set a `default_agent`; and
    - the name is the user's default, or a key of the user layer's `[agents]`, or contains `/` (an extension agent id).

    Otherwise restore the user's value and push the notice above.

- [ ] **Step 4: Fix the `wai` crate so it compiles,** with `Workspace::default_agent() -> Option<String>` (a clone of `self.settings.default_agent`).
  - Every caller that starts or lists with the default agent returns early on `None`:
    - `start_agent`, `refresh_history`, `new_session`;
    - `start_template` and `run_ext_command` when they fall back to the default;
    - `ExtEvent::OpenSession` with no agent, which replies `Err("No default AI is chosen")`;
    - `agent_problem`;
    - `lit` and `Dragged::History`;
    - `sidebar.rs` `can_fork`.
  - In `workspace.rs` tests, `fake_settings` sets `default_agent: Some("fake".into())`, and the other test fixtures follow.
  - Task 7 replaces the early returns in `new_session` and the template paths with opening Settings.

- [ ] **Step 5: Run the settings tests and the full suite.**
  - Run: `cargo test -p wai-settings && cargo build -p wai-agent --bin wai-fake-agent && cargo test -p wai`
  - Expected: PASS.

- [ ] **Step 6: Commit.** `git commit -m "feat: built-in agents and no default agent in settings"`

---

### Task 2: The settings writer and approved values

**Files:**
- Create: `crates/wai-settings/src/edit.rs`, `crates/wai-settings/src/approve.rs`
- Modify: `crates/wai-settings/src/lib.rs` (`mod` lines, `AppState` fields), `crates/wai-settings/Cargo.toml` and the root `Cargo.toml` (`toml_edit = "0.25"`, matching the lock's 0.25.x)

**Interfaces:**
- **Produces, in `edit.rs`:**
  - `pub enum Change`, with these variants:
    - `DefaultAgent(String)`
    - `Theme(ThemeChoice)`
    - `Shell(Option<String>)`
    - `AgentCommand { agent: String, command: String }`
    - `Model { agent: String, model: Option<String> }`
    - `UpdatesCheck(bool)`
    - `UpdatesRepo(Option<String>)`
  - `pub fn write_change(user_dir: &Path, change: &Change) -> Result<(), SettingsError>`
- **Produces, in `approve.rs`:**
  - `#[derive(Debug, Clone, PartialEq)] pub enum Approval { Agent { id: String, old: Option<String>, new: String }, Shell { old: Option<String>, new: String } }`
  - `pub fn seed_approved(settings: &Settings) -> BTreeSet<String>`: every agent command plus the shell, if one is set
  - `pub fn is_approved(value: &str, approved: &BTreeSet<String>) -> bool`: a built-in command, or in the set
  - `pub fn unapproved(old: &Settings, new: &Settings, approved: &BTreeSet<String>) -> Vec<Approval>`: in agent-id order, then the shell. `old` is the value in force, `None` for an agent that is new.
- **Produces, in `AppState` (`lib.rs`):**
  - `pub approved: Option<BTreeSet<String>>`. `None` means a state from before this version.
  - `pub models_seen: BTreeMap<String, Vec<SeenModel>>`
  - `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)] pub struct SeenModel { pub id: String, pub name: String }`

- [ ] **Step 1: Write the failing tests.**

```rust
// edit.rs
#[test]
fn write_keeps_comments_and_creates_the_file() {
    let d = tempfile::tempdir().unwrap();
    write_change(d.path(), &Change::Theme(ThemeChoice::Light)).unwrap();
    let p = d.path().join("settings.toml");
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "theme = \"light\"\n");
    std::fs::write(&p, "# mine\ntheme = \"light\" # keep\n").unwrap();
    write_change(d.path(), &Change::DefaultAgent("codex".into())).unwrap();
    write_change(d.path(), &Change::Model { agent: "codex".into(), model: Some("gpt-5".into()) }).unwrap();
    write_change(d.path(), &Change::AgentCommand { agent: "codex".into(), command: "codex-acp".into() }).unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.starts_with("# mine\ntheme = \"light\" # keep\n"), "{text}");
    let (s, _) = Settings::load(d.path(), None).unwrap();
    assert_eq!(s.default_agent.as_deref(), Some("codex"));
    assert_eq!(s.models["codex"], "gpt-5");
    assert_eq!(s.agents["codex"].command, "codex-acp");
    write_change(d.path(), &Change::Model { agent: "codex".into(), model: None }).unwrap();
    write_change(d.path(), &Change::Shell(None)).unwrap();
    let (s, _) = Settings::load(d.path(), None).unwrap();
    assert!(!s.models.contains_key("codex") && s.terminal.shell.is_none());
}

#[test]
fn write_refuses_a_broken_file() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("settings.toml"), "theme = [").unwrap();
    assert!(matches!(
        write_change(d.path(), &Change::Theme(ThemeChoice::Dark)),
        Err(SettingsError::Parse { .. })
    ));
    assert_eq!(std::fs::read_to_string(d.path().join("settings.toml")).unwrap(), "theme = [");
}

// approve.rs
#[test]
fn builtins_are_approved_and_new_values_are_not() {
    let approved = BTreeSet::new();
    let old = Settings::default();
    assert!(unapproved(&old, &old, &approved).is_empty());
    let mut new = old.clone();
    new.agents.get_mut("codex").unwrap().command = "evil".into();
    new.agents.insert("mine".into(), AgentConfig { command: "mine-acp".into() });
    new.terminal.shell = Some("/bin/evil".into());
    assert_eq!(unapproved(&old, &new, &approved), [
        Approval::Agent { id: "codex".into(), old: Some("npx -y @agentclientprotocol/codex-acp@2.6.0".into()), new: "evil".into() },
        Approval::Agent { id: "mine".into(), old: None, new: "mine-acp".into() },
        Approval::Shell { old: None, new: "/bin/evil".into() },
    ]);
    let seeded = seed_approved(&new);
    assert!(unapproved(&old, &new, &seeded).is_empty());
}
```

  Also extend `state_roundtrip_and_corrupt_falls_back` with `approved: Some(["x".into()].into())` and one `models_seen` entry. Add an assertion that a state JSON without those keys loads with `approved == None`.

- [ ] **Step 2: Run them and confirm they fail.**
  - Run: `cargo test -p wai-settings`
  - Expected: compile errors (`write_change`, `unapproved` and the new fields are missing).

- [ ] **Step 3: Implement `write_change`.**
  - Read through `read_config`, then parse with `toml_edit::DocumentMut`.
  - When the text fails `toml::from_str::<toml::Table>`, return `SettingsError::Parse` and do not write.
  - Set or remove the one key:
    - `default_agent`, `theme`;
    - `terminal.shell`;
    - `agents.<id>.command`;
    - `models.<id>`;
    - `updates.check`, `updates.repo`.
  - Write `settings.toml.tmp` and rename it over the file, creating the dir if needed.
  - Agent ids containing `/` are quoted keys, which `toml_edit` does on its own.

- [ ] **Step 4: Implement `approve.rs`** as the interfaces say. A shell set to `None` is never pending.

- [ ] **Step 5: Run the tests.**
  - Run: `cargo test -p wai-settings`
  - Expected: PASS.

- [ ] **Step 6: Commit.** `git commit -m "feat: settings writer and approved commands"`

---

### Task 3: Session config options in `wai-agent`

**Files:**
- Modify:
  - `crates/wai-agent/src/types.rs`
  - `crates/wai-agent/src/convert.rs`
  - `crates/wai-agent/src/actor.rs`
  - `crates/wai-agent/src/lib.rs` (re-exports)
  - `crates/wai-agent/src/bin/wai-fake-agent.rs`
- Test: `crates/wai-agent/tests/fake_agent.rs`

**Interfaces:**
- **Produces:**
  - `#[derive(Debug, Clone, PartialEq)] pub struct ConfigValue { pub id: String, pub name: String }`
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ConfigCategory { Mode, Model, ThoughtLevel, Other }`
  - `#[derive(Debug, Clone, PartialEq)] pub struct ConfigOption { pub id: String, pub name: String, pub category: ConfigCategory, pub current: String, pub values: Vec<ConfigValue> }`
  - `SessionStarted.config: Vec<ConfigOption>` (select options only; grouped values flattened in order; boolean options dropped)
  - `SessionEvent::ConfigChanged(Vec<ConfigOption>)`, from `SessionUpdate::ConfigOptionUpdate`
  - `AgentHandle::set_config(&self, session: SessionId, option: String, value: String) -> Result<Vec<ConfigOption>, AgentError>`: `session/set_config_option`, returning the agent's new options
  - `pub fn program(command: &str) -> Option<PathBuf>`: the program `connect` would spawn, resolved with `wai_settings::find_program`. `None` when it does not parse or is not on `PATH`. `connect` uses it too, so the two never disagree.
- **Fake agent flag `--models`:**
  - `session/new`, `load` and `fork` return two options:
    - `model` (category `model`): `fake-small` "Fake Small" and `fake-large` "Fake Large", current `fake-small`;
    - `effort` (category `thought_level`): `low` "Low" and `high` "High", current `low`.
  - `session/set_config_option` stores the value per session and responds with the options.
  - With `--models`, the turn's reply is `model:<current model id>`.
  - Document the flag in the bin's header list.

- [ ] **Step 1: Write the failing tests** in `fake_agent.rs`, following its helpers for spawning `wai-fake-agent`:
  - `config_options_come_with_the_session`: with `--models`, `new_session(...).config` has two options. The first is `ConfigOption { id: "model", name: _, category: Model, current: "fake-small", values: [fake-small, fake-large] }`.
  - `set_config_switches_the_model`: `set_config(id, "model", "fake-large")` returns options with `current == "fake-large"`, and the next prompt's reply text is `model:fake-large`.
  - `program_parses_like_the_spawn` (Review focus 4):
    - `program("'/bin/sh' -c x") == Some("/bin/sh".into())` on unix;
    - `program("no-such-program-xyz --acp") == None`;
    - `program("") == None`.

- [ ] **Step 2: Run them and confirm they fail.**
  - Run: `cargo build -p wai-agent --bin wai-fake-agent && cargo test -p wai-agent --test fake_agent`
  - Expected: compile errors.

- [ ] **Step 3: Implement.**
  - `convert::session_started(id, modes, config_options)` maps `Option<Vec<acp::SessionConfigOption>>`.
  - `convert::session_event` maps `U::ConfigOptionUpdate`.
  - A `Command::SetConfig` goes in the actor next to `SetMode`, built with `acp::SetSessionConfigOptionRequest`.
  - The fake agent follows its `modes()` helper.

- [ ] **Step 4: Run the tests.**
  - Run: `cargo build -p wai-agent --bin wai-fake-agent && cargo test -p wai-agent`
  - Expected: PASS.

- [ ] **Step 5: Commit.** `git commit -m "feat: session config options from agents"`

---

### Task 4: The Settings pane — Appearance, Terminal, Updates, search, read-only error

**Files:**
- Create: `crates/wai/src/settings_view.rs`
- Modify:
  - `crates/wai/src/main.rs` (`mod settings_view;`)
  - `crates/wai/src/app.rs` (the `OpenSettings` action and its `secondary-,` binding in the `Workspace` context)
  - `crates/wai/src/workspace.rs`
  - `crates/wai/src/topbar.rs` (a ⚙ button)
  - `crates/wai/src/palette.rs` (an `Entry::Settings` row labelled `Settings`)

**Interfaces:**
- **Produces:**
  - `pub enum Section { Ai, Appearance, Terminal, Updates }`, shown in that order and titled `AI`, `Appearance`, `Terminal`, `Updates`.
  - `pub struct SettingsData { pub settings: Settings, pub error: Option<String>, pub notice: Option<SharedString>, pub agents: Vec<AgentRow> }`. `AgentRow` is defined in Task 8; until then `agents` is empty.
  - `pub enum SettingsEvent { Change(Change), OpenFile, Test(String) }`. `SettingsView` implements `EventEmitter<SettingsEvent>`.
  - `SettingsView::new(section: Section, data: SettingsData, window, cx) -> Self`
  - `SettingsView::set(&mut self, data: SettingsData, cx)`
  - `SettingsView::show(&mut self, section: Section, cx)`
  - `Pane::Settings(Entity<SettingsView>)`
  - `Workspace::open_settings(&mut self, section: Section, notice: Option<&str>, window, cx)`
  - `Workspace::change_setting(&mut self, change: Change, window, cx)`. It calls `write_change(&self.user_dir, &change)`, and on `Err(e)` pushes a warning notification `Could not save settings: {e}`. The watcher added in Task 6 then applies the change.
  - `Workspace::sync_settings_view(&self, cx)`
- **Placement:** like `editor_pane`. Settings focuses its existing pane if there is one. Otherwise it takes the focused pane when that pane is empty, else a new pane to the right (`< MAX_PANES`), else the next pane that is not the editor. A session it replaces stays open in the sidebar.
- **Lifetime:** not saved in `state.json`. `close_pane` drops it.
- **Rows** (each has a `debug_selector` for tests):
  - Theme: a `Select` with Dark and Light (`settings-theme`);
  - Shell: an `Input`, where an empty value means `Shell(None)` (`settings-shell`);
  - Check for updates: a `Switch` (`settings-updates-check`);
  - Repository: an `Input` (`settings-updates-repo`).
- **Inputs** emit `Change` only on Enter or blur.
- **Search:** a case-insensitive substring match over the row labels. While the query is not empty, every section's matching rows show under their section titles.
- **Footer:** `Open settings.toml` emits `OpenFile`, which the workspace handles as `open_file(user_dir.join("settings.toml"))`.
- **With `error: Some(e)`:** a banner `settings.toml has an error: {e}` with an **Open** button. Every control is disabled.

- [ ] **Step 1: Write the failing tests** in `settings_view.rs`'s `mod tests`, using `workspace::tests::{open, fake_settings}` with `user_dir` set to a tempdir:
  - `cmd_comma_opens_one_settings_pane`: dispatching `OpenSettings` twice leaves exactly one `Pane::Settings`, and it is focused. `CloseSession` (⌘W) removes it.
  - `theme_change_writes_the_user_file`: `change_setting(Change::Theme(Light))` makes `<user_dir>/settings.toml` contain `theme = "light"`.
  - `search_filters_rows_across_sections`: the query `"shell"` leaves only the Shell row visible (by its debug selector).
  - `broken_file_makes_the_screen_read_only`: with `error = Some("x")`, the banner text is `settings.toml has an error: x`, and `change_setting` from the view is ignored (the file is unchanged).
  - `reload_keeps_a_field_being_edited` (Review focus 3): type into Shell without committing, call `set(data)` with a different shell, and the input still shows the typed text.

- [ ] **Step 2: Run them and confirm they fail.**
  - Run: `cargo test -p wai settings_view`
  - Expected: compile errors.

- [ ] **Step 3: Implement** `settings_view.rs`, `Pane::Settings` rendering in `Workspace::render`, the three entry points (⌘,, the ⚙ button with tooltip `Settings`, and the palette row) and `open_settings`. The view is a card with `aurora` helpers: a 140 px section list on the left and the search on top.

- [ ] **Step 4: Run the tests.**
  - Run: `cargo test -p wai`
  - Expected: PASS.

- [ ] **Step 5: Commit.** `git commit -m "feat: settings pane with appearance, terminal and updates"`

---

### Task 5: A broken file at launch no longer stops wai

**Files:**
- Modify: `crates/wai/src/main.rs`, `crates/wai/src/app.rs` (`run` gains `settings_error: Option<String>`), `crates/wai/src/workspace.rs` (field `pub settings_error: Option<String>`)

**Interfaces:**
- **Consumes:** `open_settings` from Task 4.
- **Produces:**
  - `pub fn load_or_default(user_dir: &Path, project: &Path) -> (Settings, Vec<String>, Option<String>)` in `main.rs`. On `Err(e)` it returns `Settings::default()`, no notices and `Some(e.to_string())`.
  - When `settings_error` is `Some`, `app::run` opens Settings on `Section::Ai` with the banner, and does not call `start_agent`.

- [ ] **Step 1: Write the failing test** in `main.rs`: `broken_user_file_opens_with_defaults`. With `settings.toml` set to `theme = [`, `load_or_default` returns `default_agent == None`, and an error that starts with `invalid settings in`.
- [ ] **Step 2: Run it and confirm it fails.**
  - Run: `cargo test -p wai broken_user_file`
  - Expected: FAIL (the function is missing).
- [ ] **Step 3: Implement it,** and replace the `?` on `Settings::load` in `main`.
- [ ] **Step 4: Run the tests.**
  - Run: `cargo test -p wai`
  - Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "fix: open with defaults when settings.toml is broken"`

---

### Task 6: Live apply — watch, reload, apply

**Files:**
- Modify:
  - `crates/wai/src/workspace.rs`
  - `crates/wai/src/agents.rs` (`pub fn set_configs(&mut self, configs: BTreeMap<String, AgentConfig>)`, which replaces `configs`; a running agent keeps its process, and the next spawn uses the new command)
  - `crates/wai/src/app.rs` (calls `watch_settings`)

**Interfaces:**
- **Consumes:**
  - `write_change` and `Change` (Task 2);
  - `SettingsView::set` and `sync_settings_view` (Task 4);
  - `settings_error` (Task 5).
- **Produces:**
  - `Workspace::watch_settings(&mut self, window, cx)`. A `ProjectWatcher` on `user_dir`, filtered to `user_dir.join("settings.toml")`. `FsChange::Rescan` counts as a change.
  - `Workspace::reload_settings(&mut self, window, cx)`. It calls `Settings::load(&self.user_dir, Some(&self.project))`:
    - `Ok` goes to `apply_settings`;
    - `Err(e)` keeps the settings in force, sets `settings_error`, pushes a warning notification with the error, and syncs the view.
  - `Workspace::apply_settings(&mut self, new: Settings, notices: Vec<String>, window, cx)`:
    - clears `settings_error`;
    - calls `Theme::change` when the theme changes;
    - calls `registry.set_configs`;
    - when the default changes to `Some`, calls `start_agent` and `refresh_history`;
    - replaces `self.settings`, so the shell applies to terminals created afterwards and the updates to the next check;
    - shows the notices;
    - syncs the view.

  The project layer's changes arrive through the existing `watch_project` batch. `disk_changed` calls `reload_settings` when the batch touches `<project>/.wai/settings.toml`. `set_project` calls `reload_settings` after switching.

- [ ] **Step 1: Write the failing tests** in `workspace.rs` tests, with `user_dir` set to a tempdir and `watch_settings` started:
  - `editing_the_file_applies_the_theme`: writing `theme = "light"` to `<user_dir>/settings.toml` → `wait_until` `!cx.theme().is_dark()`.
  - `broken_edit_keeps_the_last_good_settings`: writing `theme = [` keeps `w.settings.theme`, sets `settings_error`, and shows a notification.
  - `own_write_reloads_once_without_a_second_dialog` (Review focus 1): after `change_setting(Change::Theme(Light))`, wait until it applies. `apply_settings` ran once (count it with a `#[cfg(test)] pub(crate) applied: usize` field), and no dialog is open.
  - `deleted_user_file_falls_back_to_first_run` (Review focus 2):
    1. start with `default_agent = "fake"` in the file and one open session;
    2. delete the file;
    3. `default_agent()` is `None`, and the session is still in `w.sessions` with its agent not shut down (`registry.status("fake")` is not `Failed`).
  - `project_switch_reloads_settings` (Review focus 5): with a project whose `.wai/settings.toml` has `theme = "light"`, `set_project` applies the light theme.
  - `changed_command_applies_to_the_next_start`: change `[agents.fake] command` while the agent runs, and `registry` keeps the old handle until it exits.

- [ ] **Step 2: Run them and confirm they fail.**
  - Run: `cargo test -p wai editing_the_file broken_edit own_write deleted_user project_switch changed_command`
  - Expected: FAIL.

- [ ] **Step 3: Implement** the watcher, `reload_settings`, `apply_settings` and `set_configs`.

- [ ] **Step 4: Run the tests.**
  - Run: `cargo test -p wai`
  - Expected: PASS.

- [ ] **Step 5: Commit.** `git commit -m "feat: apply settings live"`

---

### Task 7: First run — with no default agent, everything leads to Settings

**Files:**
- Modify: `crates/wai/src/workspace.rs`, `crates/wai/src/sidebar.rs`, `crates/wai/src/app.rs`

**Interfaces:**
- **Consumes:** `open_settings` (Task 4), `default_agent()` (Task 1).
- **Produces:** the constant `pub(crate) const CHOOSE_AI: &str = "Choose an AI to start a session.";`
- **With `default_agent()` set to `None`:**
  - At launch, `app::run` calls `open_settings(Section::Ai, Some(CHOOSE_AI))` instead of `start_agent`.
  - These all call `open_settings(Section::Ai, Some(CHOOSE_AI))`:
    - `new_session` (⌘N, **+ New session**, and the empty pane's button);
    - `start_template` for a template without an agent;
    - `run_ext_command` for a prompt-only command.
  - The sidebar's History shows `Choose an AI in Settings to see its history.` instead of rows.
  - `Entry::NewSessionWith` (the palette) is unchanged.
- **`change_setting(Change::DefaultAgent(id))`** goes through Task 6's reload. That reload starts the agent and refreshes the history.

- [ ] **Step 1: Write the failing tests:**
  - `no_default_opens_settings_on_new_session`: with `Settings { default_agent: None, ..fake_settings("") }`, `NewSession` gives one `Pane::Settings` on `Section::Ai`, its notice is `CHOOSE_AI`, and the fake agent never started (`registry.status("fake") == None`).
  - `choosing_a_default_starts_it`: `change_setting(Change::DefaultAgent("fake".into()))` → `wait_until` `registry.status("fake")` is `Ready`, and a following `NewSession` opens a session.
  - `existing_default_is_unchanged`: with `fake_settings("")` (default `Some("fake")`), `NewSession` opens a session as before (the existing `new_session_appears_in_open_list` still passes).
  - `history_hint_without_default`: the sidebar renders the hint text.

- [ ] **Step 2: Run them and confirm they fail.**
  - Run: `cargo test -p wai no_default choosing_a_default history_hint`
  - Expected: FAIL.
- [ ] **Step 3: Implement it.**
- [ ] **Step 4: Run the tests.**
  - Run: `cargo test -p wai`
  - Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "feat: first run asks for an AI"`

---

### Task 8: The AI section — list, status, Test, warnings

**Files:**
- Modify: `crates/wai/src/settings_view.rs`, `crates/wai/src/workspace.rs`

**Interfaces:**
- **Consumes:**
  - `wai_agent::program` (Task 3);
  - `BUILTIN_AGENTS`, `agent_label` and `builtin` (Task 1);
  - `AgentRegistry::status` and `names` (existing).
- **Produces:**
  - `#[derive(Clone, Debug, PartialEq)] pub enum RowStatus { CliNotFound, NotChecked, Starting, Ready, SignedOut, Failed(String) }`, with `pub fn text(&self, login: Option<&str>) -> String` returning:
    - `CLI not found`;
    - `Not checked`;
    - `Starting…`;
    - `Ready`;
    - `Not signed in · run {login}`, or `Not signed in` without a hint;
    - `Failed: {last line}`.
  - `#[derive(Clone, Debug)] pub struct AgentRow`, with these fields:
    - `pub id: String`
    - `pub label: String`
    - `pub extension: Option<String>`
    - `pub status: RowStatus`
    - `pub command: String`
    - `pub models: Vec<SeenModel>`
    - `pub default_model: Option<String>`
    - `pub is_default: bool`
  - `SettingsData.project_default: Option<String>`: `project_default_agent(&project)` when it equals `settings.default_agent`, else `None`
  - `Workspace::agent_rows(&self, cx) -> Vec<AgentRow>`:
    - `registry.names()`, then the four built-ins in `BUILTIN_AGENTS` order;
    - `CliNotFound` when `program(command)` is `None`;
    - otherwise the registry's status mapped to a `RowStatus`, with `None` becoming `NotChecked`.
  - `Workspace::test_agent(&mut self, id: &str, window, cx)`:
    1. `registry.handle(id)`;
    2. `new_session(config_dir().join("scratch"), vec![])`;
    3. on success, `mark_ready` and store `started.config`'s model values in `models_seen[id]` (saved in `state.json`);
    4. on `AgentError::AuthRequired`, `set_status(SignedOut)`;
    5. sync the view.

    `ponytail:` the agent stays running after Test. Stop it when idle if the memory matters.
- **Each row shows:**
  - a radio (`settings-agent-{id}`, emitting `Change::DefaultAgent`);
  - the label (`{label} · {extension}` for extension agents);
  - the status text;
  - **Test** (`settings-test-{id}`).
- **`copilot`'s row** adds `Copilot may edit files and run commands without asking.`
- **Without a status:** a built-in row with `CliNotFound` adds its `install` hint.
- **When `project_default` is `Some(p)`:** the section says `This project uses {agent_label(p)}.`

- [ ] **Step 1: Write the failing tests:**
  - `rows_list_builtins_and_user_agents`: with `fake_settings("")`, the rows are `antigravity`, `claude`, `codex`, `copilot` and `fake`, in registry order, all four built-ins present. On a `PATH` without `agy`, `antigravity` is `CliNotFound` (set `PATH` to an empty tempdir with `temp_env`, or check `program` returning `None` for `agy`).
  - `test_learns_ready_and_models`: with `fake_settings("--models")`, `test_agent("fake")` → `wait_until` the row is `Ready` and `models_seen["fake"]` holds `[fake-small, fake-large]`.
  - `test_reports_signed_out`: with `fake_settings("--require-auth")`, the row becomes `SignedOut`, and its text is `Not signed in`.
  - `copilot_row_warns`: the copilot row's rendered text contains the warning.

- [ ] **Step 2: Run them and confirm they fail.**
  - Run: `cargo test -p wai rows_list test_learns test_reports copilot_row`
  - Expected: FAIL.
- [ ] **Step 3: Implement it.**
- [ ] **Step 4: Run the tests.**
  - Run: `cargo test -p wai`
  - Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "feat: AI section with status and test"`

---

### Task 9: Approval of commands and shells

**Files:**
- Modify: `crates/wai/src/workspace.rs`, `crates/wai/src/settings_view.rs` (the **Advanced** fold with a command `Input` that emits `Change::AgentCommand` on Enter), `crates/wai/src/app.rs`

**Interfaces:**
- **Consumes:**
  - `Approval`, `unapproved`, `seed_approved` and `is_approved` (Task 2);
  - `apply_settings` (Task 6).
- **Produces:**
  - The fields `approved: BTreeSet<String>` and `shell: Option<String>` (the approved shell in use) on `Workspace`.
    - Loaded from `AppState.approved` in `Workspace::new` when `state_path` is `Some`.
    - When the state has `None`, `seed_approved(&settings)` seeds them.
    - `save_state` writes them.
  - `Workspace::ask_approval(&mut self, pending: Vec<Approval>, window, cx)`: one dialog per item, in order.
    - Title: `Run a new command?`
    - Body: `{agent_label(id)}: {old} → {new}`, or `{label} (new): {new}`, or `Terminal shell: {old or "system default"} → {new}`.
    - Buttons: **Allow** and **Don't allow**.
    - **Allow:** insert `new` into `approved`, `save_state`, and set the registry config for that agent to `new` (or set `self.shell`).
    - **Don't allow:** keep the value in force. For a new agent, leave it out of the registry. Nothing is written to `settings.toml`.
  - **Where it is enforced:**
    - The registry only ever gets agents whose command `is_approved`. For an agent whose new command is not approved, it keeps the last approved config, or none.
    - `TerminalView::new` takes `self.shell`, not `self.settings.terminal.shell`.
    - At launch, `app::run` calls `ask_approval(unapproved(&Settings::default(), &settings, &approved))` before `start_agent`.
    - `apply_settings` calls `ask_approval(unapproved(&old, &new, &approved))`.
    - `change_setting` for `AgentCommand` or `Shell` writes the file. The reload then asks, so this is one path.
    - `change_setting(DefaultAgent(id))` for an agent whose command is not approved asks through the same reload.

- [ ] **Step 1: Write the failing tests:**
  - `hand_edited_command_asks_before_running`: write `[agents.fake] command = "<fake> --reply other"` to the user file → a dialog opens with title `Run a new command?`. Before an answer, a new session still uses the old command (its reply is the default `Hello from fake agent`).
  - `declined_command_never_runs`: decline, then relaunch a workspace with the same `state_path`. The dialog opens again, and the command is not in `AppState.approved`.
  - `allowed_command_is_remembered`: allow, then relaunch. No dialog, and the new command runs.
  - `existing_setup_is_seeded`: a `state.json` without `approved`, plus settings with a custom `[agents.fake]` → no dialog at launch, and `approved` holds the fake command.
  - `unapproved_shell_is_not_used`: a hand-edited shell is pending, and `toggle_terminal` creates the terminal with the previous shell.

- [ ] **Step 2: Run them and confirm they fail.**
  - Run: `cargo test -p wai hand_edited declined_command allowed_command existing_setup unapproved_shell`
  - Expected: FAIL.
- [ ] **Step 3: Implement it.**
- [ ] **Step 4: Run the tests.**
  - Run: `cargo test -p wai`
  - Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "feat: approve agent commands and shells before running them"`

---

### Task 10: Models — header pickers, default model, Settings select

**Files:**
- Modify:
  - `crates/wai/src/chat/view.rs`
  - `crates/wai/src/chat/render.rs`
  - `crates/wai/src/chat/model.rs`
  - `crates/wai/src/workspace.rs`
  - `crates/wai/src/settings_view.rs`

**Interfaces:**
- **Consumes:**
  - `ConfigOption`, `ConfigCategory`, `SessionEvent::ConfigChanged` and `AgentHandle::set_config` (Task 3);
  - `models_seen` and `AgentRow.models` (Task 8);
  - `Change::Model` (Task 2).
- **Produces:**
  - `ChatSession` holds `config: Vec<ConfigOption>` and one `SelectState` per option whose category is `Model`, `ThoughtLevel` or `Mode`, drawn in the header in that order, each 140 px wide.
    - When any option has category `Mode`, the old `modes` picker is not drawn.
    - `ConfigChanged` rebuilds the pickers.
    - A confirm calls `set_config`. On an error it reverts and adds the notice `Could not switch {option name}: {e}`.
  - `ChatSession::apply_default_model(&mut self, model: &str, cx)`:
    - if the session has a `Model` option offering `model`, `set_config` it;
    - otherwise add the notice `{label} no longer offers the model {model}; using {current name}.` (kind `Info`).
  - **The default model applies only to brand-new sessions.** `start_session`'s `Ok` branch calls `apply_default_model` when `settings.models` has the agent. `load` (resume and restore) and fork never call it.
  - **`models_seen` stays fresh:** every new session updates `models_seen[agent]` from its `Model` option, and saves it.
  - **The Settings AI row** gets a `Default model` `Select` (`settings-model-{id}`) with an `Agent's choice` entry first, which emits `Change::Model { model: None }`, and then `models` in order. With no models it shows `Start a session or press Test to see the models.`

- [ ] **Step 1: Write the failing tests:**
  - `header_shows_model_and_effort_pickers`: with `fake_settings("--models")`, a new session has two config pickers, and `modes` is not drawn.
  - `switching_model_applies_to_this_session`: pick `fake-large`, send a prompt, and the reply is `model:fake-large`. A second new session replies `model:fake-small`.
  - `default_model_applies_to_new_sessions_only`: `settings.models["fake"] = "fake-large"` → a new session replies `model:fake-large`. A session restored from state (`restore`) does not call `set_config`. Check that the fake agent's reply for the replayed session stays `model:fake-small` after a prompt.
  - `missing_default_model_shows_a_notice`: `models["fake"] = "gone"` → the session's items include the notice `fake no longer offers the model gone; using Fake Small.`
  - `settings_lists_seen_models`: after one session, the AI row's select offers `Agent's choice`, `Fake Small` and `Fake Large`.

- [ ] **Step 2: Run them and confirm they fail.**
  - Run: `cargo test -p wai header_shows switching_model default_model missing_default settings_lists_seen`
  - Expected: FAIL.
- [ ] **Step 3: Implement it.**
- [ ] **Step 4: Run the tests.**
  - Run: `cargo test -p wai`
  - Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "feat: model pickers and default models"`

---

### Task 11: Drag the Settings pane, verify the real CLIs, and update the docs

**Files:**
- Modify:
  - `crates/wai/src/workspace.rs` (a `Dragged::Settings` variant)
  - `crates/wai/src/settings_view.rs` (its title bar is the drag source)
  - `docs/user-guide.md`
  - `README.md`
- Create: `crates/wai-agent/tests/real_agents.rs` (ignored tests)

**Interfaces:**
- **Consumes:** `layout::place` (existing).
- **Produces:**
  - Dragging the Settings title onto a pane works like an open session's header. `from` is the `Pane::Settings` index.
  - Extend `drop_session` and `lit` for `Dragged::Settings` without changing the session rules.

- [ ] **Step 1: Write the failing test** `drag_settings_to_right_edge_splits`, mirroring `drag_open_row_to_right_edge_splits`. Settings in pane 0 and a session in pane 1: drop Settings on pane 1's right quarter, and the panes become `[Session, Settings]`.
- [ ] **Step 2: Run it and confirm it fails.**
  - Run: `cargo test -p wai drag_settings`
  - Expected: FAIL.
- [ ] **Step 3: Implement the drag.**
- [ ] **Step 4: Add `real_agents.rs`** with `#[ignore]` tests, one per built-in (in the spirit of `real_claude.rs`). Each runs `initialize` and `session/new` through `AgentHandle` with the `BUILTIN_AGENTS` command.
  - Run each on a machine that has the CLI: `cargo test -p wai-agent --test real_agents -- --ignored`.
  - Fix any wrong command, install hint or login hint in `BUILTIN_AGENTS`.
  - Note in the test file which ones were verified, and when.
- [ ] **Step 5: Update the docs.**
  - **`docs/user-guide.md`:**
    - a "Settings screen" subsection under `## Settings`, covering how to open it, the sections, the first run and approval;
    - the four agents and their hints, under `### Agent commands`;
    - the model pickers, under `### Chatting`;
    - `### What a project's .wai/ may set` gains the `default_agent` rule and `[models]`;
    - drop the sentence that an invalid file stops wai.
  - **`README.md`:**
    - "First run": no default agent, Settings opens;
    - "Known limitations": drop "settings are read at launch", and keep the project-folder line only if it is still true.
- [ ] **Step 6: Run the whole CI gate.**
  - Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo build -p wai-agent --bin wai-fake-agent && cargo test --workspace`
  - Expected: PASS.
- [ ] **Step 7: Commit.** `git commit -m "feat: drag the settings pane, verify agents, document settings"`

---

## Not in this plan, on purpose

- **ACP `unstable_auth_status`:** `session/new`'s auth-required error, which Test triggers, already detects signing out. Add it if an agent reports signed-in without failing `session/new`.
- **Stopping an agent after Test:** it stays running (`ponytail:` comment in Task 8).
- **Projects 2 and 3:** the Conversation section and **Configure with AI** add their own rows to `Section` later.
