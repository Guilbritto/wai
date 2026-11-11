# wai Settings modal (master plan 1.9) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Settings becomes a modal over the window with three nav groups (wai, Claude, Other agents): today's wai sections and Extensions unchanged in behaviour, Claude's plugins, MCP servers, skills, instructions, agents and hooks read from `~/.claude` (plugin and MCP toggles write Claude's files safely, Open opens the file), and Coming soon wherever wai does not act yet.

**Architecture:**
- `wai-settings` gains `claude.rs`: pure readers and writers for Claude Code's files (`read_claude`, `set_plugin_enabled`, `set_mcp_enabled`, `parse_mcp_list`), tested on temp dirs.
- The `wai` crate opens `SettingsView` in a gpui-component dialog instead of `Pane::Settings`.
  - `settings_view/claude.rs` turns a `ClaudeData` snapshot into page groups and rows and renders them.
  - `claude_config.rs` (`impl Workspace`, like `palette.rs`) reads and writes off the UI thread and runs `claude mcp list` on request.

**Tech Stack:** Rust 2024; GPUI via `gpui-kit = "=0.9.0"` (`WindowExt::open_dialog`, `Dialog`, `DialogEntrance`, `Switch`, `Button`, `Input`); `serde_json` with `preserve_order`; `smol::process` for the CLI check.

**Spec:** `docs/superpowers/specs/2026-11-11-wai-settings-modal-design.md`. Read it first. Its "What Claude Code actually stores" section is the file-format reference for Tasks 3–8.

## Global Constraints

- Binding: `.superpowers/sdd/master-onda-1/global-constraints.md`. Every shell starts with `export PATH="$HOME/.cargo/bin:$PATH"`.
- Run `df -h /System/Volumes/Data` before building; under 8 GB free, stop and report. Run `cargo clean` in the worktree when done.
- Worktree `.claude/worktrees/settings-modal`, branch `feat/settings-modal`, from `development` (`git log` first). No push, no merge.
- `crates/wai/src/workspace.rs` is the conflict hot spot. Another session works there (new-session drafts). Before each task, `git log development -3`; rebase if it moved.
- Commits: `feat:` / `fix:` / `test:` / `docs:` / `refactor:`. No scope, no Co-Authored-By, no Jira refs. Imperative, first line < 72 chars.
- Gate, all green before each commit:
  ```bash
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo build -p wai-agent --bin wai-fake-agent && cargo build -p wai-ext --bin wai-fake-ext
  cargo test --workspace
  ```
- Tests never read the developer's `~/.claude`, `~/.claude.json` or config, and never run the real `claude`. `open_at` points `claude_home` and `claude_program` at missing temp paths (Tasks 6, 8). A test that needs a project folder sets `w.project = wai_settings::canonical(tmp)` directly, never `set_project`. Never write into `std::env::temp_dir()` itself.
- Never touch `/Applications/wai.app` or kill `wai`, `claude-agent-acp` or `claude` processes you did not start.
- Idle CPU < 0.5%: no timers, polling or animation while the modal is closed. Reads, writes and the CLI check run on the background executor.
- UI copy is English, exactly as the spec gives it. Aurora tokens are in `docs/superpowers/specs/2026-11-09-wai-visual-aurora-design.md`. Tones map to theme colours: Ok `success`, Warn `warning`, Err `danger`, Muted `muted_foreground`.
- Keep existing ids: `tab-*`, `sidebar-*` (including `sidebar-extensions`), `topbar-settings`, `new-session`, `send`, `stop`, approval ids, `settings-theme`, `settings-processing`, `settings-shell`, `settings-updates-check`, `settings-updates-switch`, `settings-updates-repo`, `settings-agent-*`, `settings-test-*`, `settings-model-*`, `settings-command-*`, `advanced-*`, `settings-open-file`, `settings-error-open`, `ext-*`, `consent-*`.
- No secret is held, shown or logged. Data structs never carry an `env`, `headers`, `args`, `command`, `url` or hook `command` value. MCP rows show name, source and transport only.
- Text from any file in `~/.claude` (names, descriptions, headings) goes through `wai_ext::panel::clamp`: names 80 chars, summaries 160. The extensions view does the same.
- Paths are displayed through `wai_settings::claude::short` (`~/…`), which uses `shown` for anything outside home. The workspace must build on Windows x86_64. Symlink, mode and process tests are `#[cfg(unix)]`.

## Review Focus

1. **Claude rewrites its file between show and toggle.** Claude writes `~/.claude.json` constantly while sessions run. A toggle must not write, must say so and must show the file again. Tests: Task 5 `changed_file_is_left_alone`, Task 6 `changed_settings_are_not_overwritten`.
2. **A double click on a toggle.** The second click must not write with a stale stamp. Toggles are disabled while a write runs. Test: Task 6 `toggles_wait_for_the_write`.
3. **Dotfiles: `~/.claude/settings.json` is a symlink into a git repo** (this machine versions `~/.claude`). The link must survive and the target gets the change. Test: Task 5 `symlink_target_is_written`.
4. **Odd plugin trees.** A missing install dir, a `plugin.json` that is not an object, a `SKILL.md` that is not UTF-8 or has no frontmatter, a hooks file of the wrong shape. The read lists what it can and never panics. Test: Task 4 `odd_files_do_not_break_the_read`.
5. **A nested dialog over the modal.** Editing an agent command on AI asks for approval above the modal. Allow writes and leaves the modal open. Test: Task 1 `advanced_command_is_written_on_allow` (extended).

---

### Task 1: Settings opens in a modal, with today's sections

**Files:**
- Modify: `crates/wai/src/settings_view.rs` (render, `Section`, `NAV`, `modal_size`, `open_modal`, tests)
- Modify: `crates/wai/src/workspace.rs`
  - remove `Pane::Settings`, `Dragged::Settings` and their arms in the drop, `lit` and render code
  - remove `toggle_settings`
  - add the `settings_view` field
  - `open_settings`, `sync_settings_view`, `settings_data`
  - tests at ~2750–2800, ~2995–3030, ~3167–3200
- Modify: `crates/wai/src/topbar.rs` (Settings button)
- Modify: `crates/wai/src/app.rs` (launch order)

**Interfaces:**
- Produces:
  - `pub(crate) fn modal_size(viewport: Size<Pixels>, maximized: bool) -> Size<Pixels>`
  - `pub(crate) fn open_modal(view: Entity<SettingsView>, window: &mut Window, cx: &mut App)`
  - `impl Section { fn title(self) -> &'static str; fn key(self) -> &'static str; fn description(self) -> &'static str }`
  - `pub(crate) const NAV: &[(&str, &[Section])]`. In this task it is `[("wai", &[Ai, Appearance, Conversation, Terminal, Updates])]`; later tasks append.
  - `SettingsData.context: String`
  - `SettingsView::show(&mut self, section: Section, window: &mut Window, cx: &mut Context<Self>)`
  - `SettingsView::maximized(&self) -> bool`
  - `SettingsView::context(&self) -> &str`
  - `SettingsView::nav_count(&self, section: Section, cx: &App) -> Option<usize>`
  - `Workspace.settings_view: Option<WeakEntity<SettingsView>>`
  - `Workspace::settings_view(&self) -> Option<Entity<SettingsView>>` (`pub(crate)`)
  - `Workspace::close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>)` (`pub(crate)`)
  - `Workspace::open_settings(section, notice, window, cx)` keeps its signature.
- Element ids: `settings-modal` (root debug selector), `settings-title`, `settings-maximize`, `settings-close`, `settings-nav-<key>`.

- [ ] **Step 1: Write the failing tests**

In the `settings_view.rs` tests:
- Change the `view()` helper to `workspace.read_with(cx, |w, _| w.settings_view()).expect("the settings modal")`.
- Rename `cmd_comma_opens_one_settings_pane` to `cmd_comma_opens_one_settings_modal`.
- Replace `search_filters_rows_across_sections` with `search_filters_the_page`.
- Add the tests below.

```rust
#[test]
fn modal_size_follows_the_viewport() {
    let vp = gpui_kit::size(gpui_kit::px(1400.), gpui_kit::px(900.));
    assert_eq!(super::modal_size(vp, false), gpui_kit::size(gpui_kit::px(1232.), gpui_kit::px(774.)));
    assert_eq!(super::modal_size(vp, true), gpui_kit::size(gpui_kit::px(1376.), gpui_kit::px(876.)));
}

#[gpui_kit::test]
fn cmd_comma_opens_one_settings_modal(cx: &mut TestAppContext) {
    let (workspace, cx, _dir) = settings_ws(cx);
    cx.simulate_keystrokes("secondary-,");
    cx.simulate_keystrokes("secondary-,");
    workspace.read_with(cx, |w, _| {
        assert!(w.settings_view().is_some());
        assert_eq!(w.panes.len(), 1, "no pane for settings");
    });
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    cx.simulate_keystrokes("escape");
    assert!(workspace.read_with(cx, |w, _| w.settings_view().is_none()));
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
}
```

- `close_button_and_outside_click_close_the_modal`
  - Open with `open_settings(Section::Ai, None, …)`, then `click("settings-close", cx)`. Assert `settings_view().is_none()`.
  - Open again. Run `cx.simulate_click(point(px(8.), viewport.height - px(8.)), Modifiers::none())`, then `cx.run_until_parked()`. Assert `settings_view().is_none()`.
- `maximize_grows_the_modal`
  - Open, then read `cx.debug_bounds("settings-modal").unwrap().size.width` into `before`.
  - `click("settings-maximize", cx)`.
  - Assert `view.read_with(cx, |v, _| v.maximized())` and that the width is now `> before`.
- `title_names_the_agent_and_the_project`
  - With `fake_ws(cx, "")`: `view.context() == format!("(fake · {})", crate::workspace::folder_name(&project))`, where `project` is the workspace's `project`.
  - With `settings_ws`: `format!("({})", folder_name(&project))`.
- `nav_switches_pages_and_clears_the_search`
  - Open on Appearance. Set the search to `"x"`. `click("settings-nav-terminal", cx)`.
  - Assert `v.section() == Section::Terminal`, `cx.debug_bounds("settings-shell").is_some()` and `v.search.read(cx).value() == ""`.
- `search_filters_the_page`
  - Open on Updates. Set the search to `"repo"`.
  - Assert `settings-updates-repo` is shown and `settings-updates-check` is not.
- `advanced_command_is_written_on_allow` (keep it, add one line): after `click("approval-allow", cx)`, assert `workspace.read_with(cx, |w, _| w.settings_view().is_some())`.
- `open_file_creates_a_missing_settings_toml` (keep it, add one line): after the tab appears, assert `settings_view().is_none()`.

In the `workspace.rs` tests:
- `no_default_opens_settings_on_new_session`
  - Read the view from `w.settings_view()`: section `Ai`, notice `CHOOSE_AI`.
  - Add `assert!(matches!(w.panes.as_slice(), [Pane::Empty(_)]))`.
- `choosing_a_default_clears_the_notice`: the view comes from `w.settings_view()`.
- Rename `close_settings_keeps_the_sessions` to `settings_modal_leaves_the_panes_alone`.
  - After ⌘,: `w.panes.len() == 1` and `w.panes[0].session() == Some(&session)`.
  - After `escape`: the modal is gone, and the panes and `w.sessions` are unchanged.
- Rename `settings_button_toggles_the_pane` to `settings_button_opens_the_modal`.
  - `click("topbar-settings")`: the modal is open.
  - `escape`: it is closed.
- Delete `drag_settings_to_right_edge_splits`.

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wai -- settings_view:: workspace::tests::settings`
Expected: compile errors (`settings_view`, `modal_size`, `maximized` not found).

- [ ] **Step 3: Implement**

The non-obvious parts:

```rust
pub(crate) fn open_modal(view: Entity<SettingsView>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let size = modal_size(window.viewport_size(), view.read(cx).maximized());
        dialog.w(size.width).h(size.height).centered().close_button(false)
            .entrance(DialogEntrance::None).p_0().child(view.clone())
    });
}
```

- `modal_size`: 88% × 86% of the viewport; maximized is the viewport − 24 px on each axis.
- The builder owns the only strong handle. `Workspace` stores `view.downgrade()`, so closing the dialog drops the view, and `settings_view()` returns `upgrade()`.
- `open_settings`
  - When open: `show(section)` and `set(data)`.
  - Otherwise: create the view, `subscribe_in` the same events as today, call `open_modal`, store the weak handle.
  - `SettingsEvent::OpenFile` calls `close_settings` first.
  - `close_settings` runs `window.close_dialog(cx)` only when `settings_view()` is some.
- Inside the view, `settings-close` runs `window.close_dialog(cx)`. `settings-maximize` flips `maximized` and calls `cx.notify()`; the dialog builder reruns on the next frame.
- Render
  - Root: `v_flex().size_full().debug_selector("settings-modal")`.
  - Header, 48 px: `settings-title` reads "Settings" in `DISPLAY_FONT`, then `context` in muted text, then maximize (`IconName::Maximize`) and × (`IconName::Close`) as ghost small buttons.
  - Body: `h_flex().flex_1().min_h_0()`.
    - Nav: `v_flex().id("settings-nav").w(px(230.)).overflow_y_scroll()`. Each group label is uppercase 11 px, `nevoa_2`. Each item is `aurora::tab(active, cx).id(ElementId::Name(format!("settings-nav-{key}").into()))` with the count on the right, `.test_support()`.
    - Content: `v_flex().id("settings-content").flex_1().min_w_0().overflow_y_scroll().p(px(30.))`. It holds the title (26 px `DISPLAY_FONT` bold), the description, the error banner (unchanged), `Input::new(&self.search)`, then the page.
- `show(section, …)` sets the section, empties the search value and calls `set_placeholder(format!("Search {}…", title.to_lowercase()))`.
- The wai pages render exactly today's rows for `self.section` only, filtered by the search (same label / agent-name matching).
- The AI page ends with group title "Advanced" and one row: "settings.toml", "Everything above, as a file", and a `Button::new("settings-open-file").label("Open")`. Remove the old footer and the draggable title.
- `nav_count(Ai)` is `Some(self.data.agents.len())`. Every other section returns `None` for now.
- `settings_data` sets `context`:
  - with a default agent: `format!("({} · {})", agent_label(&a), folder_name(&self.project))`
  - without one: `format!("({})", folder_name(&self.project))`
- `topbar.rs`: `.selected(self.settings_view().is_some())`, and the click runs `open_settings(Section::Ai, None, window, cx)`.
- `app.rs`: move `let pending = …; w.ask_approval(pending, window, cx);` below the block that opens Settings, so the approval dialog lands on top.
- Removing `Dragged::Settings` makes the dropped `session` always present. Drop the `Option` there.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p wai -- settings_view:: workspace::tests:: palette::`
Expected: PASS. Then run the full gate.

- [ ] **Step 5: Commit**

```bash
git add crates/wai/src/settings_view.rs crates/wai/src/workspace.rs crates/wai/src/topbar.rs crates/wai/src/app.rs
git commit -m "feat: open settings in a modal over the window"
```

---

### Task 2: Extensions moves into the modal

**Files:**
- Modify: `crates/wai/src/settings_view.rs` (`Section::Extensions`, page, nav count)
- Modify: `crates/wai/src/extensions_view.rs` (row layout, tests)
- Modify: `crates/wai/src/workspace.rs` (`show_extensions`, `InstallPlugin` handler, `settings_data`)
- Modify: `crates/wai/src/sidebar.rs` (drop `SidebarTab::Extensions` and its arm)
- Modify: `crates/wai/src/topbar.rs` (drop `.selected` on `sidebar-extensions`)
- Modify: `crates/wai/src/palette.rs` (test)
- Modify: `crates/wai/src/aurora.rs` (`badge`)

**Interfaces:**
- Consumes: Task 1's `open_settings`, `settings_view()`, `close_settings`, `nav_count`.
- Produces:
  - `Section::Extensions` (key `extensions`), appended to the wai group in `NAV`.
  - `SettingsData.extensions: Option<Entity<ExtensionsView>>`
  - `Workspace::show_extensions(window, cx) -> Entity<ExtensionsView>` keeps its signature. It now opens the modal on Extensions.
  - `pub(crate) fn badge(text: impl Into<SharedString>, cx: &App) -> Div` in `aurora.rs`.

- [ ] **Step 1: Write the failing tests**

`extensions_view.rs` tests:
- Add a helper `fn consent(cx) -> bool`: `cx.update(|w, cx| { w.render_frame(cx); w.try_find("consent-install").is_some() })`.
- In `consent_dialog_lists_permissions`, `long_description_cannot_hide_permissions` and the `suggest-review` check of `project_suggestion_needs_review_and_consent`, replace each `dialog(cx)` that asks about the consent dialog with `consent(cx)`. The settings modal is a dialog too.
- `extensions_page_shows_installed`
  - Commit `sample` with `ExtStore` (as `quit_stops_extensions` does), then `open_settings(Section::Extensions, None, …)`.
  - Assert `view.read_with(cx, |v, cx| v.nav_count(Section::Extensions, cx)) == Some(1)`.
  - After `render_frame`: `window.try_find(("ext-enabled", 0usize)).is_some()` and `cx.debug_bounds("ext-install-folder").is_some()`.
- `topbar_extensions_opens_the_page`: `click("sidebar-extensions", cx)` gives `settings_view()` with `section() == Section::Extensions`.
- `plugin_install_types_without_enter`: after clicking `ext-plugin:sample:0`, assert `workspace.read_with(cx, |w, _| w.settings_view().is_none())`.

`palette.rs` `palette_lists_builtin_templates_and_ext_commands`:
- Replace `run("extensions", cx)` and its assertion with inline steps:
  - ⌘⇧P, `simulate_input("extensions")`, `enter`.
  - Assert `w.settings_view().map(|v| v.read(cx).section()) == Some(Section::Extensions)`.
  - `escape`, then `assert!(!dialog(cx))`.
- The later `assert_eq!(w.sidebar_tab, SidebarTab::Extensions)` becomes `SidebarTab::Files`.

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wai -- extensions_view:: palette::`
Expected: FAIL (`Section::Extensions` unknown).

- [ ] **Step 3: Implement**

- `open_settings` calls `self.extensions(window, cx)` before `settings_data`, so the view exists, and puts it in `SettingsData.extensions`.
- The Extensions page is the header plus `data.extensions.clone()`.
- `nav_count(Extensions)` is `ext.read(cx).store().list().len()`.
- `show_extensions`: `self.open_settings(Section::Extensions, None, window, cx); self.extensions(window, cx)`.
- The `InstallPlugin` handler calls `close_settings` before `type_in_terminal`.
- `ExtensionsView` render
  - Drop `size_full`, `overflow_y_scroll` and `p_2`; the page scrolls. Keep `.id("extensions")`.
  - Actions row: `ext-install-folder` "Install from folder…", the URL `Input` (`flex_1`), `ext-install-git` "Install from git URL…". Then "Preparing…" while staging.
  - Then a group header "Installed" with the count, then one row per extension (rename `card` to `row`). The row has:
    - name with `aurora::badge(version)`
    - the permissions line, the problem line, the plugin suggestions
    - right side: `("ext-enabled", ix)` Switch and `("ext-uninstall", ix)` ghost "Uninstall"
  - All ids stay. "No extensions installed" stays.
- `aurora::badge`: 11 px text, `tinta_2` on `vidro_forte`, 1 px `borda`, radius 6, px 6. In light mode use the theme's `muted_foreground` on `muted`.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p wai -- extensions_view:: palette:: ext_host::`
Expected: PASS. Then the gate.

- [ ] **Step 5: Commit**

```bash
git add crates/wai/src
git commit -m "feat: move extensions into the settings modal"
```

---

### Task 3: Claude readers: plugins and MCP servers

**Files:**
- Create: `crates/wai-settings/src/claude.rs`
- Modify: `crates/wai-settings/src/lib.rs` (`pub mod claude;`)
- Modify: `crates/wai-settings/Cargo.toml`: `serde_json = { workspace = true, features = ["preserve_order"] }` (indexmap is already in `Cargo.lock`; plugin order and later writes keep the file's key order)

**Interfaces:**
- Produces (all `pub` in `wai_settings::claude`):

```rust
pub fn claude_home() -> PathBuf;                          // dirs::home_dir().unwrap_or_default()
pub fn short(path: &Path, home: &Path) -> String;         // "~/…" or shown(path)
pub fn project_key_for(path: &Path) -> String;            // shown(path), '\\' -> '/' on Windows
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stamp(Option<u64>);                            // None: the file was missing
impl Stamp { pub fn of(bytes: Option<&[u8]>) -> Stamp }   // std DefaultHasher over the bytes
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClaudeData {
    pub plugins: Vec<Plugin>, pub mcp: Vec<McpServer>,
    pub skills: Vec<Entry>, pub agents: Vec<Entry>, pub instructions: Vec<Entry>, pub hooks: Vec<Entry>,
    pub problems: Vec<String>,          // "<short path> does not parse: <error>"
    pub settings_stamp: Stamp,          // ~/.claude/settings.json
    pub config_stamp: Stamp,            // ~/.claude.json
    pub project_key: Option<String>,    // the existing `projects` key for this project
}
#[derive(Clone, Debug, PartialEq)]
pub struct Plugin { pub id: String, pub name: String, pub marketplace: String, pub version: String,
    pub root: PathBuf, pub enabled: bool, pub locked: Option<String>, pub summary: String }
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum McpSource { Plugin(String), User, Local, Project }
#[derive(Clone, Debug, PartialEq)]
pub struct McpServer { pub name: String, pub label: String, pub source: McpSource,
    pub transport: &'static str, pub enabled: bool, pub locked: Option<String> }
#[derive(Clone, Debug, PartialEq)]
pub struct Entry { pub group: String, pub name: String, pub path: PathBuf, pub summary: String, pub read_only: bool }
pub fn read_claude(home: &Path, project: &Path) -> ClaudeData;   // `project` canonical
```

**Rules** (from the spec's "What Claude Code actually stores"):
- **Which plugins.** For each id of `installed_plugins.json` `plugins`, in file order, take this project's `project`/`local` install (`projectPath == project`), else the `user`/`managed` install. With neither, the id is not listed.
- **Enabled.** The first `enabledPlugins[id]` found in:
  1. `<git root or project>/.claude/settings.local.json`
  2. `<project>/.claude/settings.local.json`
  3. `<project>/.claude/settings.json`
  4. `~/.claude/settings.json`

  `false` means off and any other value means on. With no entry, use `plugin.json` `defaultEnabled`, which defaults to `true`.
- **Locked**, from the first of:
  - "Set in this project's .claude/settings.local.json"
  - "Set in this project's .claude/settings.json"
  - "Managed by your organization"
  - "Fix ~/.claude/settings.json first" (user settings broken)
- **Summary.** Non-zero counts joined with `" · "`, in this order: skills, agents, commands, hooks (handlers), MCP servers, language servers. Use singular or plural (`1 skill`, `2 MCP servers`). With no counts, the description's first line; else `No components`.
- **Count sources.**
  - Skills: `skills/*/SKILL.md`, plus each manifest `skills` dir. A dir that holds `SKILL.md` directly is one skill. A root `SKILL.md` is one skill when there is no `skills/` and no manifest key.
  - Agents: `*.md` under `agents/`, recursive. The manifest's `agents` replaces it.
  - Commands: `commands/*.md`. The manifest's `commands` replaces it (a path, an array or an object map; count its entries).
  - Hooks: handlers in `hooks/hooks.json` (`{"hooks":{…}}`) plus the manifest's `hooks` (path, inline map or array).
  - MCP: `.mcp.json` (`{"mcpServers":{…}}` or a bare `{name: {…}}` map) plus the manifest's `mcpServers` (path, map or array; `.mcpb` skipped).
  - LSP: `.lsp.json` plus the manifest's `lspServers`. With no `plugin.json`, the description and `lspServers` come from the marketplace entry: `known_marketplaces.json` → `installLocation` → `.claude-plugin/marketplace.json` → `plugins[]` by `name`.
- **MCP order.** Enabled plugins' servers as `plugin:<name>:<server>`, then `~/.claude.json` `mcpServers` (User), then `projects[key].mcpServers` (Local), then `<project>/.mcp.json` (Project).
- **Transport.** From `type`: `http` → `HTTP`, `sse` → `SSE`, `ws` → `WebSocket`, anything else → `stdio`.
- **MCP enabled** = `!projects[key].disabledMcpServers.contains(name)`.
- **MCP locked**, from the first of:
  - "Fix ~/.claude.json first"
  - "Start a Claude session in this project first" (no project key)
- **Project key.** The first of `project_key_for(project)` and `project_key_for(git root)` that exists in `projects`. The git root is the nearest ancestor holding a `.git` entry.
- **File limits.** `~/.claude.json` is read with a 32 MB cap; other JSON files through `read_config`. A broken file adds a problem and yields nothing. A broken `~/.claude.json` also drops the User and Local servers.

- [ ] **Step 1: Write the failing tests**

In `claude.rs` `mod tests`, add a `fn fixture(home: &Path, project: &Path)`. It writes this tree (`<home>`, `<project>` and `<key>` = `project_key_for(project)` substituted):

```text
<home>/.claude/plugins/installed_plugins.json
  {"version":2,"plugins":{
    "alpha@m":[{"scope":"user","installPath":"<home>/.claude/plugins/cache/m/alpha/1.2.0","version":"1.2.0"}],
    "beta@m":[{"scope":"project","projectPath":"<project>","installPath":"<home>/.claude/plugins/cache/m/beta/abc1234","version":"abc1234"}],
    "gamma@n":[{"scope":"project","projectPath":"/elsewhere","installPath":"<home>/x/gamma","version":"1.0.0"}],
    "delta@n":[{"scope":"user","installPath":"<home>/.claude/plugins/cache/n/delta/2.0.0","version":"2.0.0"}]}}
alpha: .claude-plugin/plugin.json {"name":"alpha","description":"Alpha tools"}
       skills/one/SKILL.md, skills/two/SKILL.md, agents/rev.md, agents/sub/deep.md, commands/go.md
       hooks/hooks.json {"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"echo s3cr3t"}]}],
                                  "Stop":[{"hooks":[{"type":"command","command":"a"},{"type":"command","command":"b"}]}]}}
       .mcp.json {"mcpServers":{"db":{"command":"db-mcp","args":["--token","s3cr3t"],"env":{"KEY":"s3cr3t"}}}}
beta:  .claude-plugin/plugin.json {"name":"beta"}
       .mcp.json {"web":{"type":"http","url":"https://x.test/?k=s3cr3t","headers":{"Authorization":"Bearer s3cr3t"}}}
delta: (no plugin.json; empty dir)
<home>/.claude/plugins/known_marketplaces.json {"n":{"installLocation":"<home>/.claude/plugins/marketplaces/n"}}
<home>/.claude/plugins/marketplaces/n/.claude-plugin/marketplace.json
  {"plugins":[{"name":"delta","description":"Go language server","lspServers":{"gopls":{"command":"gopls"}}}]}
<home>/.claude/settings.json {"model":"opus","enabledPlugins":{"delta@n":false}}
<home>/.claude.json {"oauthAccount":{"emailAddress":"s3cr3t@x.test"},"mcpServers":{"u":{"command":"u-mcp"}},
  "projects":{"<key>":{"mcpServers":{"l":{"type":"sse","url":"https://l.test"}},"disabledMcpServers":["plugin:alpha:db"]}}}
<project>/.mcp.json {"mcpServers":{"p":{"type":"ws","url":"wss://p.test"}}}
```

Each test makes a `tempdir()` and canonicalizes it, with `home = tmp/home` and `project = tmp/proj`.

- `plugins_follow_installs_and_enabled_plugins`
  - ids are `["alpha@m","beta@m","delta@n"]`
  - `enabled` is `[true,true,false]`
  - `version` of alpha is `"1.2.0"`
  - every `locked` is `None`
- `plugin_summary_counts_components`
  - alpha: `"2 skills · 2 agents · 1 command · 3 hooks · 1 MCP server"`
  - beta: `"1 MCP server"`
  - delta: `"1 language server"`
- `default_enabled_false_starts_off`: add `"defaultEnabled":false` to alpha's manifest. alpha `enabled == false`.
- `project_and_local_settings_lock_the_toggle`
  - Write `<project>/.claude/settings.json` `{"enabledPlugins":{"alpha@m":false}}`.
    - alpha is off.
    - `locked == Some("Set in this project's .claude/settings.json")`.
  - Then add `<project>/.claude/settings.local.json` `{"enabledPlugins":{"alpha@m":true}}`.
    - alpha is on.
    - `locked == Some("Set in this project's .claude/settings.local.json")`.
- `manifest_paths_add_or_replace`
  - Alpha's manifest gains `"skills":"./extra/","agents":["./more/x.md"],"mcpServers":"./m.json"`.
  - The files: `extra/three/SKILL.md`, `more/x.md`, `m.json` `{"mcpServers":{"db2":{"command":"y"}}}`.
  - Summary: `"3 skills · 1 agent · 1 command · 3 hooks · 2 MCP servers"`.
- `mcp_servers_from_plugins_user_local_and_project`
  - names: `["plugin:alpha:db","plugin:beta:web","u","l","p"]`
  - labels: `["db","web","u","l","p"]`
  - transports: `["stdio","HTTP","stdio","SSE","WebSocket"]`
  - sources: `[Plugin("alpha"),Plugin("beta"),User,Local,Project]`
  - enabled: `[false,true,true,true,true]`
  - `project_key == Some(project_key_for(&project))`
- `disabled_plugins_add_no_servers`
  - Write `delta/.mcp.json` `{"mcpServers":{"z":{"command":"z"}}}`.
  - No name starts with `plugin:delta:`.
  - delta's summary is `"1 MCP server · 1 language server"`.
- `project_key_prefers_an_existing_entry`
  - Make `project = tmp/repo/sub` with `tmp/repo/.git/`, and key `projects` by `project_key_for(tmp/repo)`. Then `project_key == Some(that)`.
  - With no matching key: `project_key == None`, and every MCP `locked == Some("Start a Claude session in this project first")`.
- `nothing_secret_is_kept`: `!format!("{:?}", read_claude(&home, &project)).contains("s3cr3t")`.
- `broken_files_are_reported_and_lock_their_toggles`
  - `settings.json` = `"{oops"`
    - `problems[0].starts_with("~/.claude/settings.json does not parse")`
    - 3 plugins listed, all `locked == Some("Fix ~/.claude/settings.json first")`
  - Restored, and `~/.claude.json` = `"{"`
    - a problem starting `"~/.claude.json does not parse"`
    - MCP names `["plugin:alpha:db","plugin:beta:web","p"]`, all `locked == Some("Fix ~/.claude.json first")`
- `short_paths_use_a_tilde`
  - `short(&home.join(".claude/x"), &home) == "~/.claude/x"`
  - `short(Path::new("/opt/x"), &home) == "/opt/x"`
- `stamps_follow_the_bytes`
  - `Stamp::of(None) == Stamp::default()`
  - `Stamp::of(Some(b"a")) != Stamp::of(Some(b"b"))`
  - `read_claude(..).settings_stamp == Stamp::of(Some(&fs::read(settings)?))`

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wai-settings claude::`
Expected: compile errors (no `claude` module).

- [ ] **Step 3: Implement**

Implement `read_claude` and the helpers in `claude.rs` following the rules above.
- Parse with `serde_json::Value` and keep only the fields the structs name.
- Paths in manifests are relative to the plugin root. Skip any path whose resolved form is outside the root (`..` or absolute), as Claude does.
- `skills`, `agents`, `instructions` and `hooks` stay empty in this task.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p wai-settings claude::`
Expected: PASS. Then the gate.

- [ ] **Step 5: Commit**

```bash
git add crates/wai-settings
git commit -m "feat: read Claude's plugins and MCP servers"
```

---

### Task 4: Claude readers: skills, agents, instructions, hooks

**Files:**
- Modify: `crates/wai-settings/src/claude.rs`

**Interfaces:**
- Consumes: Task 3's types and `fixture`.
- Produces:
  - `read_claude` fills `skills`, `agents`, `instructions`, `hooks`. Their rows only come from enabled plugins; `read_only` is true under a plugin root.
  - `fn frontmatter(text: &str) -> (Option<String>, Option<String>)` (name, description)
  - `fn imports(text: &str) -> Vec<String>`

**Rules:**
- **Skills.**
  - Groups `User` (`~/.claude/skills/*/SKILL.md`, depth 1) and `Project` (`<project>/.claude/skills/*/SKILL.md`), each sorted by name. Then one group per enabled plugin, named after the plugin, in plugin order, sorted.
  - name: frontmatter `name`, else the dir name.
  - summary: the description's first line, else `""`.
  - Read at most the first 8 KB, lossy UTF-8.
- **Agents.** Same shape. User is `~/.claude/agents/**/*.md` and Project is `<project>/.claude/agents/**/*.md` (recursive, depth ≤ 3). Plugins come from Task 3's agent sources. name: frontmatter `name`, else the file stem.
- **Instructions**, group `User`:
  - `~/.claude/CLAUDE.md`. Summary: its first `# ` heading, else `"Every project"`.
  - Then its imports, depth first. Summary `"Included by <file name>"`.
- **Instructions**, group `Project`:
  - The folders from the filesystem root down to the project. In each: `CLAUDE.md`, `.claude/CLAUDE.md`, `CLAUDE.local.md`, each followed by its imports.
  - Summaries: `"This project"`, `"This project, yours only"` (`CLAUDE.local.md`), `"Parent folder"` (ancestors).
- **Imports.**
  - Skip fenced blocks (a line starting with ```` ``` ```` or `~~~` toggles them) and inline code spans.
  - Tokens start with `@`; `\ ` is a space.
  - Resolve `~/` against `home`, keep absolute paths, resolve others against the importing file's folder.
  - List only regular files, each canonical path once, at most 4 hops from the root file.
- **Hooks.**
  - Group `User`: one entry per event in `~/.claude/settings.json` `hooks`, in file order. Summary `"1 hook"` / `"<n> hooks"` (handlers). Path: the settings file.
  - Group `Project`: the same from `<project>/.claude/settings.json`, then `settings.local.json`.
  - Group `From plugins`: one entry per enabled plugin with hooks. name: the plugin name. summary: the events joined with `" · "`. path: `hooks/hooks.json`, else the manifest's hooks file, else `.claude-plugin/plugin.json` for inline hooks. `read_only: true`.

- [ ] **Step 1: Write the failing tests**

Extend `fixture` with:
- `~/.claude/skills/mine/SKILL.md` = `"---\nname: mine\ndescription: \"My skill\"\n---\nbody"`
- `~/.claude/skills/synced/abc/SKILL.md`
- `<project>/.claude/skills/local/SKILL.md` = `"no frontmatter"`
- alpha's `skills/one/SKILL.md` = `"---\nname: one\ndescription: >\n  Folded\n  text\n---\n"`
- `~/.claude/agents/helper.md` with `name: helper`, `description: Helps`
- `<project>/.claude/agents/proj.md`
- `~/.claude/CLAUDE.md` = ``"# Global rules\n- @instructions/a.md here\n`@instructions/no.md`\n```\n@instructions/no.md\n```\n@~/extra.md\n@missing.md\n"``
- `~/.claude/instructions/a.md` = `"@b.md"`, `b.md` = `"@a.md"`, `no.md`
- `<home>/extra.md`
- `<project>/CLAUDE.md`
- settings `hooks` `{"SessionStart":[{"hooks":[{"type":"command","command":"echo s3cr3t"}]}],"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"x"},{"type":"command","command":"y"}]}]}`

Tests:

```rust
#[test]
fn frontmatter_reads_name_and_description() {
    assert_eq!(frontmatter("---\nname: a\ndescription: \"Quoted\"\n---\n"), (Some("a".into()), Some("Quoted".into())));
    assert_eq!(frontmatter("---\ndescription: >\n  Folded\n  text\n---\n").1.as_deref(), Some("Folded text"));
    assert_eq!(frontmatter("no frontmatter"), (None, None));
}

#[test]
fn imports_skip_code() {
    assert_eq!(imports("a @x.md `@y.md`\n```\n@z.md\n```\n@my\\ file.md"), ["x.md", "my file.md"]);
}
```

- `skills_from_user_project_and_enabled_plugins`
  - `(group, name)` pairs: `[("User","mine"),("Project","local"),("alpha","one"),("alpha","two")]`
  - `mine` has summary `"My skill"`; `one` has `"Folded text"`
  - `read_only`: `[false,false,true,true]`
  - nothing from `synced`
- `agents_from_user_project_and_plugins`: `(group, name)` `[("User","helper"),("Project","proj"),("alpha","deep"),("alpha","rev")]`
- `instructions_follow_imports`
  - User names `["CLAUDE.md","a.md","b.md","extra.md"]`
  - summaries `["Global rules","Included by CLAUDE.md","Included by a.md","Included by CLAUDE.md"]`
  - the cycle is listed once; `no.md` and `missing.md` are absent
- `imports_stop_after_four_hops`: a chain `CLAUDE.md → d1 → d2 → d3 → d4 → d5` lists `d1..d4` and not `d5`.
- `project_instructions_walk_up`
  - `project = tmp/root/proj` with `root/CLAUDE.md`, `proj/CLAUDE.md`, `proj/.claude/CLAUDE.md`, `proj/CLAUDE.local.md`
  - Project summaries in order: `["Parent folder","This project","This project","This project, yours only"]`
- `hooks_show_events_not_commands`
  - User: `[("SessionStart","1 hook"),("PreToolUse","2 hooks")]`
  - From plugins: `[("alpha","SessionStart · Stop")]`, path `…/alpha/1.2.0/hooks/hooks.json`, `read_only`
  - `!format!("{data:?}").contains("s3cr3t")`
- `disabled_plugins_contribute_nothing`: with `enabledPlugins["alpha@m"] = false`, no skill, agent or hook entry has group or name `alpha`.
- `odd_files_do_not_break_the_read` (Review Focus 4):
  - beta's `plugin.json` = `"[]"`
  - a skill whose `SKILL.md` bytes are `[0xff, 0xfe, b'x']`
  - `hooks/hooks.json` = `{"hooks":[]}`
  - an install whose `installPath` does not exist

  `read_claude` returns. beta is still listed; the odd skill is listed by its dir name with summary `""`; the missing install is listed with summary `"No components"`.

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wai-settings claude::`
Expected: FAIL (`frontmatter`, `imports` not found; empty lists).

- [ ] **Step 3: Implement**

Implement the rules above in `claude.rs`. `frontmatter` reads `key: value` lines between the first two `---` lines, strips quotes, and joins the indented lines after a `>` or `|` value with spaces.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p wai-settings claude::`
Expected: PASS. Then the gate.

- [ ] **Step 5: Commit**

```bash
git add crates/wai-settings/src/claude.rs
git commit -m "feat: read Claude's skills, agents, instructions and hooks"
```

---

### Task 5: Safe writes to Claude's files

**Files:**
- Modify: `crates/wai-settings/src/claude.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Debug)]
pub enum WriteError { Changed, Busy, Invalid(String), Io(std::io::Error) }
// Display: Changed "it changed since it was shown", Busy "Claude is writing it; try again",
// Invalid(s) s, Io(e) e.
pub fn set_plugin_enabled(home: &Path, id: &str, on: bool, seen: Stamp) -> Result<Stamp, WriteError>;
pub fn set_mcp_enabled(home: &Path, project_key: &str, name: &str, on: bool, seen: Stamp) -> Result<Stamp, WriteError>;
```

The returned `Stamp` is the stamp of the bytes written.

- `set_plugin_enabled` sets `enabledPlugins[id] = on` in `~/.claude/settings.json`. It creates the object, or the file, when missing.
- `set_mcp_enabled` adds `name` to `projects[project_key].disabledMcpServers` (off) or removes it (on). It creates the array when missing. It never creates the project entry: `Invalid("~/.claude.json has no entry for this project")`.
- A non-object `enabledPlugins` or `projects[key]`, or a non-array `disabledMcpServers`, is `Invalid` and nothing is written.

**Algorithm:** one private `fn replace_json(path: &Path, seen: Stamp, edit: impl FnOnce(&mut Value) -> Result<(), String>) -> Result<Stamp, WriteError>`.
1. Resolve symlinks with `try_canonical` when the file exists.
2. Lock: `create_dir("<target>.lock")`.
   - It exists and its mtime is under 10 s old: retry every 100 ms for 1 s, then `Busy`.
   - Older: `remove_dir` and retry once.
   - Always `remove_dir` the lock at the end.
3. Read the bytes. If `Stamp::of(bytes) != seen`, return `Changed`.
4. Parse, run `edit`, then serialize with `PrettyFormatter::with_indent`. The indent is the leading whitespace of the file's first indented line, else two spaces. Keep a trailing `\n` if the original had one.
5. Write `<target>.wai.tmp` and give it the original's permissions (`set_permissions(tmp, meta.permissions())`).
6. Re-read the target and compare stamps again. On a mismatch, remove the temp file and return `Changed`. Otherwise `rename` the temp over the target.

- [ ] **Step 1: Write the failing tests**

- `plugin_toggle_changes_only_its_key`
  - The file is 4-space-indented, `{"model":"opus","enabledPlugins":{"a@m":true},"hooks":{"Stop":[]},"zeta":1}`, with a trailing newline.
  - After `set_plugin_enabled(&home, "a@m", false, Stamp::of(Some(&before)))`:
    - the parsed keys are `["model","enabledPlugins","hooks","zeta"]`
    - `enabledPlugins["a@m"] == false`
    - the text ends with `"\n"` and its second line starts with exactly 4 spaces
    - the returned stamp equals `Stamp::of(Some(&after))`
- `plugin_toggle_creates_a_missing_file`: no `settings.json` and `seen = Stamp::default()`. The file becomes `{"enabledPlugins":{"a@m":true}}` (pretty).
- `changed_file_is_left_alone`: take the stamp, overwrite the file with `{"model":"other"}`, call. The result is `Err(WriteError::Changed)` and the text is `{"model":"other"}`.
- `invalid_files_are_left_alone`: `"{oops"`, then `{"enabledPlugins":[]}`, each with a matching stamp. Each gives `Err(Invalid(_))` and the bytes are unchanged.
- `toggling_off_and_on_restores_the_bytes`
  - `~/.claude.json` is 2-space-indented, `{"numStartups":3,"oauthAccount":{"emailAddress":"a@b.test"},"projects":{"<key>":{"allowedTools":[],"lastCost":0.123456,"disabledMcpServers":[]}}}`, plus a newline.
  - Off: `disabledMcpServers == ["plugin:a:db"]`, and every other value equals the original `Value`.
  - On with the returned stamp: the file bytes equal the original bytes.
- `missing_project_entry_is_not_created`: `set_mcp_enabled` with an unknown key gives `Err(Invalid(_))` and the bytes are unchanged.
- `symlink_target_is_written` (`#[cfg(unix)]`, Review Focus 3): `settings.json` is a symlink to `dotfiles/settings.json`. After the write, the link is still a symlink and the target holds the change.
- `mode_is_kept` (`#[cfg(unix)]`): a 0o600 `~/.claude.json` is still `mode & 0o777 == 0o600` after the write.
- `busy_lock_gives_up`: with a fresh `settings.json.lock` dir, the result is `Err(Busy)` within 2 s, the bytes are unchanged and the lock dir is still there.
- `stale_lock_is_replaced` (`#[cfg(unix)]`): set the lock dir's mtime to 60 s ago with `File::open(dir)?.set_modified(..)`. The write succeeds and no `.lock` is left.
- `lock_is_released`: after any successful write, `!target.with_extension("json.lock").exists()`.

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wai-settings claude::`
Expected: compile errors (`set_plugin_enabled` not found).

- [ ] **Step 3: Implement**

Implement `replace_json`, `set_plugin_enabled` and `set_mcp_enabled` per the algorithm above. `WriteError` implements `Display` and `std::error::Error`.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p wai-settings claude::`
Expected: PASS. Then the gate.

- [ ] **Step 5: Commit**

```bash
git add crates/wai-settings/src/claude.rs
git commit -m "feat: write Claude's settings without clobbering them"
```

---

### Task 6: Claude group in the modal, and the Plugins page

**Files:**
- Create: `crates/wai/src/claude_config.rs` (`impl Workspace`: `refresh_claude`, `toggle_plugin`; test fixture)
- Create: `crates/wai/src/settings_view/claude.rs` (`mod claude;` inside `settings_view.rs`: groups, rows, page render)
- Modify:
  - `crates/wai/src/settings_view.rs` (Claude state, `Section::Plugins`, events)
  - `crates/wai/src/workspace.rs` (fields, `open_settings` calls `refresh_claude`, the event arm, `open_at` defaults)
  - `crates/wai/src/main.rs` (`mod claude_config;`)
  - `crates/wai/src/aurora.rs` (`soon`)

**Interfaces:**
- Consumes: Tasks 3–5 (`read_claude`, `ClaudeData`, `Stamp`, `set_plugin_enabled`, `short`, `claude_home`).
- Produces:

```rust
// settings_view/claude.rs
#[derive(Debug, PartialEq)] pub(crate) struct Group { pub title: String, pub rows: Vec<Row>, pub add_soon: bool }
#[derive(Debug, PartialEq)] pub(crate) struct Row { pub name: String, pub badge: Option<String>,
    pub path: Option<String>, pub summary: Option<String>, pub status: Option<(Tone, String)>, pub control: Control }
#[derive(Clone, Copy, Debug, PartialEq)] pub(crate) enum Tone { Ok, Warn, Err, Muted }
#[derive(Debug, PartialEq)] pub(crate) enum Control { None, Soon,
    Open { kind: &'static str, ix: usize, path: PathBuf, read_only: bool },
    Plugin { ix: usize, on: bool, locked: bool }, Mcp { ix: usize, on: bool, locked: bool } }
pub(crate) fn plugin_groups(data: &ClaudeData, home: &Path) -> Vec<Group>;
pub(crate) fn filter(groups: Vec<Group>, query: &str) -> Vec<Group>;   // name/badge/path/summary, lowercase
pub(crate) fn default_open(total_rows: usize, ix: usize) -> bool;      // total <= 20 || ix == 0
```

- `SettingsEvent::TogglePlugin { id: String, on: bool, seen: Stamp }`
- `SettingsView::set_claude(&mut self, data: ClaudeData, cx)` (clears `busy`)
- `SettingsView::claude(&self) -> Option<&ClaudeData>`
- `SettingsData.home: PathBuf`
- `Workspace.claude_home: PathBuf` (`pub(crate)`; `claude_home()` in `new`, `temp_dir().join("wai-no-claude")` in `open_at`)
- `Workspace.claude_task: Option<Task<()>>`
- `Workspace::refresh_claude(&mut self, cx: &mut Context<Self>)` (`pub(crate)`)
- `Workspace::toggle_plugin(&mut self, id: String, on: bool, seen: Stamp, window: &mut Window, cx: &mut Context<Self>)` (`pub(crate)`)
- `pub(crate) fn soon(cx: &App) -> Div` in `aurora.rs`
- `#[cfg(test)] pub(crate) fn claude_fixture(home: &Path, project: &Path)` and `claude_ws(cx) -> (Entity<Workspace>, &mut VisualTestContext, TempDir)` in `claude_config.rs` tests. They write Task 3's and Task 4's tree with these differences:
  - no `gamma` or `delta`
  - `beta@m` turned off in `enabledPlugins`
  - `disabledMcpServers: []`
  - `~/.claude.json` written with mode 0o600 on unix

  `claude_ws` sets `claude_home` and `project`.
- Element ids:
  - `("plugin-toggle", ix)` and `("plugin-menu", ix)`
  - `soon-browse`, `soon-plugins-menu`, `plugins-learn-more`
  - `("settings-group", ix)` and `("soon-add", ix)`

- [ ] **Step 1: Write the failing tests**

Pure tests (`#[test]`) in `settings_view/claude.rs`:
- `plugin_rows_group_by_marketplace`
  - Input: a hand-built `ClaudeData` with `alpha@m` (v `1.2.0`, on), `beta@m` (v `abc1234`, off, locked `"Set in this project's .claude/settings.json"`) and `delta@n`.
  - group titles: `["m","n"]`
  - badges: `[Some("v1.2.0"), Some("abc1234")]`
  - alpha's path: `Some("~/.claude/plugins/cache/m/alpha/1.2.0")`
  - alpha's control: `Plugin{ix:0,on:true,locked:false}`
  - beta's control: `Plugin{ix:1,on:false,locked:true}`, status `Some((Tone::Muted, "Set in this project's .claude/settings.json".into()))`
  - `add_soon` is true on every group
- `filter_keeps_matching_rows_and_drops_empty_groups`
  - `"ALP"` gives one group with one row
  - `""` returns the input
- `big_pages_open_only_the_first_group`: `default_open(10,1)`, `default_open(21,0)` and `!default_open(21,1)`.

`gpui_kit::test`s in `claude_config.rs`:

```rust
#[gpui_kit::test]
fn modal_opens_before_claude_is_read(cx: &mut TestAppContext) {
    let (workspace, cx, _tmp) = claude_ws(cx);
    workspace.update_in(cx, |w, window, cx| w.open_settings(Section::Plugins, None, window, cx));
    let view = workspace.read_with(cx, |w, _| w.settings_view()).expect("open at once");
    assert!(view.read_with(cx, |v, _| v.claude().is_none()), "read off the UI thread");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| v.claude().map(|d| d.plugins.len())), Some(2));
    assert_eq!(view.read_with(cx, |v, cx| v.nav_count(Section::Plugins, cx)), Some(2));
}
```

- `plugin_toggle_writes_enabled_plugins`
  - Open on Plugins and park, then `click(("plugin-toggle", 0usize))` and park.
  - `~/.claude/settings.json` parsed: `enabledPlugins["alpha@m"] == false`, `model == "opus"`.
  - `v.claude().unwrap().plugins[0].enabled == false`.
  - `window.notifications(cx).len() == 1`.
- `changed_settings_are_not_overwritten` (Review Focus 1)
  - Open and park, write `{"model":"other"}` to `settings.json`, click the toggle, park.
  - The file text is still `{"model":"other"}`.
  - One notification.
  - After the refresh, `plugins[0].enabled == true` (no entry means default on).
- `toggles_wait_for_the_write` (Review Focus 2)
  - Click without the `click` helper, which parks: `cx.update(|window, cx| { window.render_frame(cx); window.click(("plugin-toggle", 0usize), cx); window.render_frame(cx); })`.
  - Before any park, `window.find(("plugin-toggle", 0usize)).disabled() == Some(true)`.
- `coming_soon_controls_are_disabled`: after a park and a render, `soon-browse`, `soon-plugins-menu`, `("plugin-menu", 0usize)` and `("soon-add", 0usize)` all report `disabled() == Some(true)`.
- `groups_collapse_and_search_opens_them`
  - `click(("settings-group", 0usize))` hides `("plugin-toggle", 0usize)` (`try_find` is none).
  - A search of `"alpha"` shows it again.

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wai -- claude_config:: settings_view::claude::`
Expected: compile errors.

- [ ] **Step 3: Implement**

- `open_settings` (open or re-show) calls `refresh_claude`.
- `refresh_claude` replaces `claude_task` with `cx.spawn`. The task runs `cx.background_spawn(read_claude(home, project))`, then `set_claude` on `settings_view()` when it is still open.
- `NAV` gains the group label `"Claude"` with `[Section::Plugins]`.
  - The nav draws the label as `✳` in `aurora::brasa()` followed by `CLAUDE`.
  - `nav_count(Plugins) = claude().map(|d| d.plugins.len())`.
- Claude pages
  - While `claude()` is `None`, a page says `Reading ~/.claude…`.
  - `problems` show as a danger banner.
  - Each group header (`("settings-group", ix)`, `.test_support()`) has a chevron, the bold title, a count pill and an `("soon-add", ix)` disabled `IconName::Plus` button with tooltip `"Coming soon"` when `add_soon` is set.
  - A group is open when the search is non-empty or `default_open(total, ix) != toggled.contains(&(section, ix))`. `toggled: HashSet<(Section, usize)>` lives in the view.
  - Each row: name (semibold, `clamp` 80) plus `aurora::badge`, then the path in mono xs muted, the summary (`clamp` 160), and the status as a 7 px dot and text in the tone's colour.
  - Plugins controls: `Switch::new(("plugin-toggle", ix))`, disabled when locked, `busy` or not loaded. Then a ghost `IconName::Ellipsis` `("plugin-menu", ix)`, disabled, tooltip `"Update, uninstall: coming soon"`.
- Plugins header actions
  - `soon-browse`: "Browse" disabled, with `aurora::soon(cx)` beside it.
  - `soon-plugins-menu`: a disabled ellipsis.
  - Under the description, `plugins-learn-more` runs `cx.open_url("https://code.claude.com/docs/en/plugins")`.
- `aurora::soon`: "Coming soon", 10.5 px semibold `ambar` on `ambar` at 10% alpha, 1 px `ambar` at 35%, fully rounded. In light mode use the theme's `warning`.
- Toggle flow: the view sets `busy = true` and emits `TogglePlugin`. `toggle_plugin` spawns `set_plugin_enabled` on the background executor, then:
  - `Ok`: `Notification::success("Applies to new Claude sessions")`
  - `Err(Changed)`: `Notification::warning("Claude's settings changed since they were shown; showing them again")`
  - other errors: `Notification::warning(format!("Could not change ~/.claude/settings.json: {e}"))`

  Then `refresh_claude`.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p wai -- claude_config:: settings_view::`
Expected: PASS. Then the gate.

- [ ] **Step 5: Commit**

```bash
git add crates/wai/src
git commit -m "feat: show Claude's plugins in settings, with a safe toggle"
```

---

### Task 7: MCP Servers page with the per-project toggle

**Files:**
- Modify: `crates/wai/src/settings_view/claude.rs`, `crates/wai/src/settings_view.rs`, `crates/wai/src/claude_config.rs`, `crates/wai/src/workspace.rs` (event arm)

**Interfaces:**
- Consumes: Task 6's `Group`/`Row`/`Control`, `refresh_claude`, `claude_fixture`; Task 5's `set_mcp_enabled`.
- Produces:
  - `pub(crate) fn mcp_groups(data: &ClaudeData) -> Vec<Group>`. Task 8 adds a `check` parameter.
  - `SettingsEvent::ToggleMcp { key: String, name: String, on: bool, seen: Stamp }`
  - `Workspace::toggle_mcp(&mut self, key: String, name: String, on: bool, seen: Stamp, window, cx)` (`pub(crate)`)
  - `Section::Mcp` (key `mcp`, title "MCP Servers")
  - ids `("mcp-toggle", ix)` and `soon-add-server`

- [ ] **Step 1: Write the failing tests**

- `mcp_rows_show_source_and_transport` (pure, on Task 3's server list built by hand)
  - groups `["From plugins","User","This project","claude.ai connectors"]`; an empty group is dropped, but connectors always shows
  - badges `["plugin: alpha","plugin: beta","user","local",".mcp.json"]`
  - summaries are the transports
  - enabled rows: status `(Muted,"Not checked")`; disabled rows: `(Muted,"Off for this project")`
  - controls `Mcp{ix,on,locked}`
  - the last group has one row, `"Connectors from your claude.ai account"`, with summary `"They live in your Claude account, not in a local file"`, `Control::Soon` and `add_soon == false`
- `mcp_rows_carry_no_secret`: `!format!("{:?}", mcp_groups(&read_claude(&home, &project))).contains("s3cr3t")` on `claude_fixture`.
- `mcp_toggle_writes_disabled_mcp_servers` (gpui)
  - Open on Mcp, park, click `("mcp-toggle", 0usize)`, park.
  - `~/.claude.json` `projects[key].disabledMcpServers == ["plugin:alpha:db"]`, and `oauthAccount` is unchanged.
  - Under `#[cfg(unix)]`, mode `0o600` is kept (the fixture writes it 0600).
  - One notification.
  - Click again and park: the list is `[]`.
- `mcp_toggle_is_locked_without_a_project_entry`: with the fixture's `projects` emptied, `window.find(("mcp-toggle", 0usize)).disabled() == Some(true)`, and the status reads `"Start a Claude session in this project first"`.

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wai -- claude_config:: settings_view::claude::`
Expected: FAIL.

- [ ] **Step 3: Implement**

- Append `Section::Mcp` to the Claude group. `nav_count = d.mcp.len()`.
- Description per the spec. Actions: "Check status" is added in Task 8; `soon-add-server` "Add server…" is disabled, with `aurora::soon`.
- The row toggle emits `ToggleMcp` with `key = data.project_key` (a locked row cannot emit) and `seen = data.config_stamp`.
- `toggle_mcp` mirrors `toggle_plugin` with `set_mcp_enabled`. Notifications:
  - success: `"Applies to new Claude sessions in this project"`
  - `Changed`: same text as Task 6
  - other errors: `"Could not change ~/.claude.json: {e}"`

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p wai -- claude_config:: settings_view::`
Expected: PASS. Then the gate.

- [ ] **Step 5: Commit**

```bash
git add crates/wai/src
git commit -m "feat: list Claude's MCP servers with a per-project toggle"
```

---

### Task 8: Check MCP status with `claude mcp list`

**Files:**
- Modify: `crates/wai-settings/src/claude.rs` (`McpStatus`, `parse_mcp_list`)
- Modify: `crates/wai/src/claude_config.rs` (`run_mcp_list`, `check_mcp`)
- Modify: `crates/wai/src/settings_view/claude.rs`, `crates/wai/src/settings_view.rs` (status, button)
- Modify: `crates/wai/src/workspace.rs` (`claude_program` field, `open_at` default, event arm)

**Interfaces:**
- Produces:

```rust
// wai_settings::claude
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum McpStatus { Connected, NeedsAuth, Failed(String), Pending, Disabled, Other(String) }
pub fn parse_mcp_list(text: &str) -> BTreeMap<String, McpStatus>;
// wai::settings_view::claude
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum McpCheck { #[default] NotRun, Running, Done(BTreeMap<String, McpStatus>), Failed(String) }
pub(crate) fn status_line(check: &McpCheck, name: &str, enabled: bool) -> (Tone, String);
pub(crate) fn mcp_groups(data: &ClaudeData, check: &McpCheck) -> Vec<Group>;   // replaces Task 7's
// wai::claude_config
pub(crate) async fn run_mcp_list(program: PathBuf, cwd: PathBuf, timeout: Duration) -> Result<String, String>;
```

- `Workspace.claude_program: Option<PathBuf>` (`pub(crate)`). `None` means `find_program("claude")`; `open_at` sets `Some(temp_dir().join("wai-no-claude/claude"))`.
- `SettingsEvent::CheckMcp`
- `SettingsView::set_mcp_check(&mut self, check: McpCheck, cx)`
- id `mcp-check`

**Parsing.**
- Ignore lines starting with `claude.ai ` and lines without `": "`.
- name: the text before the first `": "`.
- The status starts after the last `" - "` that is followed by `✔`, `!`, `✘`, `⏸` or `⊘`:
  - `✔` → `Connected`
  - `! Needs authentication` → `NeedsAuth`
  - `✘ Failed to connect` → `Failed(reason after " — ", trimmed, or "")`
  - `⏸` → `Pending`
  - `⊘` → `Disabled`
  - any other → `Other(rest)`
- The command or URL part is never kept.

**Status line**, in order:
- `!enabled` → `(Muted,"Off for this project")`
- `NotRun` → `(Muted,"Not checked")`
- `Running` → `(Muted,"Checking…")`
- `Failed(t)` → `(Warn,t)`
- `Done(map)`:
  - `Connected` → `(Ok,"Connected")`
  - `NeedsAuth` → `(Warn,"Needs sign-in · run /mcp in Claude")`
  - `Failed(r)` → `(Err,"Failed to connect · <clamp(r,160)>")`, or `"Failed to connect"` when `r` is empty
  - `Pending` → `(Warn,"Pending approval · start Claude in this project to approve")`
  - `Disabled` → `(Muted,"Off for this project")`
  - `Other(t)` → `(Muted,clamp(t,160))`
  - missing → `(Muted,"Not listed by claude mcp list")`

**Runner.**
- `smol::process::Command::new(program).args(["mcp","list"]).current_dir(cwd)`, stdin null, `kill_on_drop(true)`, `.output()`.
- Race it with `smol::Timer::after(timeout)` via `smol::future::or`.
- A timeout is `Err(format!("claude mcp list took over {}s", timeout.as_secs()))`.
- A non-zero exit is `Err(last stderr line)`.
- `check_mcp`
  - resolves the program; when none is executable it is `McpCheck::Failed("Needs the claude CLI".into())`
  - otherwise sets `Running`, spawns `run_mcp_list(program, project, Duration::from_secs(60))` and sets `Done(parse_mcp_list(&out))` or `Failed(format!("Check failed: {e}"))`
  - keeps the task in `mcp_task: Option<Task<()>>`; a new check replaces it
- The check result lives only in the open view; a new modal starts at `NotRun`.

- [ ] **Step 1: Write the failing tests**

- `parse_mcp_list_reads_claude_output` (wai-settings). The input is these lines, verbatim from a real run (2026-11-11):

```text
Checking MCP server health…

claude.ai Figma: https://mcp.figma.com/mcp - ✔ Connected
plugin:context7:context7: https://mcp.context7.com/mcp?client=claude-code-plugin (HTTP) - ! Needs authentication
plugin:github:github: https://api.githubcopilot.com/mcp/ (HTTP) - ✘ Failed to connect — HTTP 400: Error POSTing to endpoint: bad request: Authorization header is badly formatted
plugin:playwright:playwright: npx @playwright/mcp@latest - ✔ Connected
u: u-mcp - ⏸ Pending approval (run `claude` to approve)
l: https://l.test (SSE) - ⊘ Disabled for this project (re-enable via /mcp)
```

  The map has exactly the keys `plugin:context7:context7`, `plugin:github:github`, `plugin:playwright:playwright`, `u` and `l`:
  - `NeedsAuth`
  - `Failed("HTTP 400: Error POSTing to endpoint: bad request: Authorization header is badly formatted")`
  - `Connected`
  - `Pending`
  - `Disabled`
- `status_lines_read_plainly` (pure, wai): one assertion per rule above, including the `!enabled` override and the missing-name case.
- `check_status_runs_claude_mcp_list_in_the_project` (`#[cfg(unix)]`, `allow_parking`)
  - Write an executable `tmp/bin/claude`:
    `#!/bin/sh\npwd > "$0.cwd"\necho "$@" > "$0.args"\nprintf '%s\n' 'plugin:alpha:db: db-mcp - ✔ Connected'`
  - Set `claude_program = Some(it)`, open on Mcp, park, `click("mcp-check")`.
  - `wait_until` the row for `plugin:alpha:db` shows `(Ok,"Connected")`.
  - `claude.args` is `"mcp list\n"`, and the canonical `claude.cwd` is the project.
- `check_status_without_the_cli_says_so`: with the `open_at` default, `click("mcp-check")` and park. Every enabled row's status is `(Warn,"Needs the claude CLI")`.
- `mcp_list_gives_up_after_the_timeout` (`#[cfg(unix)]`): a script `exec sleep 5`. `smol::block_on(run_mcp_list(script, tmp, Duration::from_millis(200)))` is `Err("claude mcp list took over 0s")` and returns in under 2 s.

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wai-settings claude::parse && cargo test -p wai -- claude_config:: settings_view::claude::`
Expected: FAIL.

- [ ] **Step 3: Implement**

- The MCP header action `mcp-check` reads "Check status" ("Checking…" and disabled while `Running`) and emits `CheckMcp`.
- `mcp_groups` uses `status_line` for every row.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: same as Step 2.
Expected: PASS. Then the gate.

- [ ] **Step 5: Commit**

```bash
git add crates/wai-settings/src/claude.rs crates/wai/src
git commit -m "feat: check Claude's MCP server status on request"
```

---

### Task 9: Skills page, and Open into the editor

**Files:**
- Modify: `crates/wai/src/settings_view/claude.rs`, `crates/wai/src/settings_view.rs`, `crates/wai/src/workspace.rs` (event arm; make `open` `pub(crate)`)

**Interfaces:**
- Consumes: Task 4's `Entry`; Task 6's `Group`/`Row`/`Control::Open`.
- Produces:
  - `pub(crate) fn entry_groups(entries: &[Entry], kind: &'static str, home: &Path) -> Vec<Group>`. Groups keep the entries' order; `add_soon` only for `User` and `Project`. Row: name, summary, path `short(path)` (`plugin: <group>` when `read_only`), `Control::Open { kind, ix, path, read_only }`. Tasks 10–12 reuse it with kinds `instruction`, `agent`, `hook`.
  - `SettingsEvent::Open { path: PathBuf, read_only: bool }`
  - `Section::Skills` (key `skills`)
  - ids `(format!("open-{kind}"), ix)` as `ElementId::NamedInteger`. Here: `open-skill`.

- [ ] **Step 1: Write the failing tests**

- `entry_rows_group_and_open` (pure)
  - Entries: `User/mine`, `Project/local`, `alpha/one` (`read_only`).
  - groups `["User","Project","alpha"]` with `add_soon` `[true,true,false]`
  - path of `one`: `Some("plugin: alpha")`
  - controls `Open{kind:"skill",ix:0..2,..}`
- `skills_page_opens_skill_md` (gpui, `claude_ws`)
  - Open on Skills, park. `nav_count(Skills) == Some(<fixture skills count>)`.
  - `click(ElementId::NamedInteger("open-skill".into(), 0))`, then `wait_until` an editor tab exists.
  - Its `path == canonical(home/.claude/skills/mine/SKILL.md)`, `read_only == false` and `settings_view().is_none()`.
- `plugin_skill_opens_read_only`: the index of the first entry with group `"alpha"` opens a tab with `read_only == true`.

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wai -- claude_config:: settings_view::claude::`
Expected: FAIL.

- [ ] **Step 3: Implement**

- Append `Section::Skills`. Description from the spec; `nav_count = d.skills.len()`.
- The row renders a ghost small "Open" button with `IconName::ExternalLink` that emits `Open`.
- The workspace arm: `close_settings`, then `self.open(path, None, read_only, window, cx)`.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: same as Step 2.
Expected: PASS. Then the gate.

- [ ] **Step 5: Commit**

```bash
git add crates/wai/src
git commit -m "feat: list Claude's skills in settings"
```

---

### Task 10: Instructions page

**Files:**
- Modify: `crates/wai/src/settings_view/claude.rs`, `crates/wai/src/settings_view.rs`

**Interfaces:**
- Consumes: Task 9's `entry_groups` (kind `instruction`).
- Produces: `Section::Instructions` (key `instructions`). Rows show the file name, path and summary.

- [ ] **Step 1: Write the failing test**

`instructions_page_lists_imports` (gpui, `claude_ws`):
- `nav_count(Instructions) == Some(<fixture count>)`.
- Through the view: the `User` group row names are `["CLAUDE.md","a.md","b.md","extra.md"]` and the Project group has `CLAUDE.md` with `"This project"`.
- Clicking `open-instruction` 0 opens `~/.claude/CLAUDE.md` editable.

- [ ] **Step 2: Run the test and confirm it fails**

Run: `cargo test -p wai -- claude_config::tests::instructions`
Expected: FAIL.

- [ ] **Step 3: Implement**

Append `Section::Instructions` with the spec's description. `nav_count = d.instructions.len()`.

- [ ] **Step 4: Run the test and confirm it passes**

Run: same.
Expected: PASS. Then the gate.

- [ ] **Step 5: Commit**

```bash
git add crates/wai/src
git commit -m "feat: list Claude's instruction files in settings"
```

---

### Task 11: Agents page

**Files:**
- Modify: `crates/wai/src/settings_view/claude.rs`, `crates/wai/src/settings_view.rs`

**Interfaces:**
- Consumes: `entry_groups` (kind `agent`).
- Produces: `Section::Agents` (key `agents`).

- [ ] **Step 1: Write the failing test**

`agents_page_lists_user_project_and_plugin_agents` (gpui):
- groups `["User","Project","alpha"]`
- the alpha rows' path is `"plugin: alpha"`
- `open-agent` 0 opens `~/.claude/agents/helper.md`

- [ ] **Step 2: Run the test and confirm it fails**

Run: `cargo test -p wai -- claude_config::tests::agents`
Expected: FAIL.

- [ ] **Step 3: Implement**

Append `Section::Agents` with "Subagents Claude can hand work to." `nav_count = d.agents.len()`.

- [ ] **Step 4: Run the test and confirm it passes**

Run: same.
Expected: PASS. Then the gate.

- [ ] **Step 5: Commit**

```bash
git add crates/wai/src
git commit -m "feat: list Claude's agents in settings"
```

---

### Task 12: Hooks page

**Files:**
- Modify: `crates/wai/src/settings_view/claude.rs`, `crates/wai/src/settings_view.rs`

**Interfaces:**
- Consumes: `entry_groups` (kind `hook`). Plugin hook rows open read-only.
- Produces: `Section::Hooks` (key `hooks`).

- [ ] **Step 1: Write the failing test**

`hooks_page_shows_events_not_commands` (gpui):
- groups `["User","From plugins"]`
- the User rows are `[("SessionStart","1 hook"),("PreToolUse","2 hooks")]`
- `!format!("{:?}", groups).contains("s3cr3t")`
- `open-hook` 0 opens `~/.claude/settings.json` with `read_only == false`

- [ ] **Step 2: Run the test and confirm it fails**

Run: `cargo test -p wai -- claude_config::tests::hooks`
Expected: FAIL.

- [ ] **Step 3: Implement**

Append `Section::Hooks` with the spec's description. `nav_count = d.hooks.len()`.

- [ ] **Step 4: Run the test and confirm it passes**

Run: same.
Expected: PASS. Then the gate.

- [ ] **Step 5: Commit**

```bash
git add crates/wai/src
git commit -m "feat: list Claude's hooks in settings"
```

---

### Task 13: Other agents as Coming soon pages

**Files:**
- Modify: `crates/wai/src/settings_view.rs`

**Interfaces:**
- Produces:
  - `Section::Codex` / `Copilot` / `Antigravity` (keys `codex`, `copilot`, `antigravity`)
  - `NAV` group `"Other agents"`
  - debug selector `settings-soon-page`

- [ ] **Step 1: Write the failing tests**

- `nav_groups_match_the_spec` (pure): `NAV` equals:
  - `[("wai",[Ai,Appearance,Conversation,Terminal,Updates,Extensions]),`
  - `("Claude",[Plugins,Mcp,Skills,Instructions,Agents,Hooks]),`
  - `("Other agents",[Codex,Copilot,Antigravity])]`
- `other_agents_are_coming_soon` (gpui)
  - `click("settings-nav-codex")`, then `debug_bounds("settings-soon-page").is_some()`.
  - `nav_count(Codex) == None`.
  - The page text holds `"Today wai shows what Claude uses. Codex comes next; its own settings stay in its CLI meanwhile."`. Expose `fn soon_text(section) -> String` and assert on it.

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p wai -- settings_view::`
Expected: FAIL.

- [ ] **Step 3: Implement**

- The nav rows of this group show a small `aurora::soon`-style "Soon" in place of a count.
- The page: the title, the description `"Plugins, MCP servers, skills, instructions, agents and hooks of <X>, like the Claude ones."`, then a centred block with `aurora::soon`, "Not in wai yet" (20 px display font) and `soon_text`.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: same.
Expected: PASS. Then the gate.

- [ ] **Step 5: Commit**

```bash
git add crates/wai/src/settings_view.rs
git commit -m "feat: show other agents as coming soon in settings"
```

---

### Task 14: User guide, and the real-machine validation list

**Files:**
- Modify: `docs/user-guide.md`
  - top bar (line ~7)
  - Panes (line ~10): Settings is no longer a pane
  - "The Settings screen" (~298–316): opening, layout, closing and maximize, the three groups, each Claude page, Coming soon, Check status, what the toggles write and when they apply
  - shortcuts table (⌘W no longer closes Settings; Esc does)

- [ ] **Step 1: Rewrite the sections**

Rewrite them from the spec's Behaviour section, with the spec's exact copy. Remove "Arranging: drag its Settings title".

- [ ] **Step 2: Gate and commit**

Run the gate (docs only; the tests still pass). Then:

```bash
git add docs/user-guide.md
git commit -m "docs: describe the settings modal"
```

- [ ] **Step 3: Hand the controller this validation list**

Do not run it yourself; the owner or the controller runs it on the real app after merge:
1. Open Settings: the counts match the mockup's real data (17 plugins, 4 MCP servers from plugins).
2. Turn `ralph-loop` off. `~/.claude/settings.json` then has `"ralph-loop@claude-plugins-official": false` and the rest is unchanged (`git -C ~/.claude diff`). A new wai Claude session has no `/ralph-loop`. Turn it back on.
3. Turn `plugin:playwright:playwright` off. **Check status** reads "Off for this project", and so does `claude mcp list` in that project. Then let a running Claude session finish a turn and confirm `disabledMcpServers` survived in `~/.claude.json`. Turn it back on.
4. Check status on this machine gives Connected for playwright, Needs sign-in for context7, Failed for github.
5. Open on Skills, Instructions, Agents and Hooks: the files open, and plugin files open read-only.
6. Idle CPU with the modal closed stays < 0.5%.
