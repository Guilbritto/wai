# wai Fase 2 core: adapter version, log and diagnostics, one instance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Settings > AI shows each adapter's version and says when a newer `claude-agent-acp` is out; wai writes a log file and a "Copy diagnostics" action copies a secret-free report; a second wai instance is refused, without breaking self-update's **Restart now**.

**Architecture:**
- The adapter version already reaches `AgentRegistry::apply` in `AgentEvent::Ready { version, .. }` and is dropped. The registry keeps it; `agent_rows` hands it to the AI page. "Newer adapter" is one more GET (npm registry `latest` of the package the claude command pins), fired from `Workspace::check_updates` under the same switch and trigger as the wai check, with its own failure path (logged, never an `UpdateStatus`).
- The log is stderr itself: at startup, on macOS, fd 2 is pointed at `logs/wai.log` (rotated once above 1 MiB), so every existing `eprintln!` in all crates lands in it with no call-site change. `diagnostics.rs` builds the report as a pure function (`report`) from plain inputs; the palette entry and a Settings button call it on the background executor and copy the result.
- One instance = an advisory lock (`File::try_lock`, std) on `wai.lock` next to `state.json`. The kernel releases it when the holder exits, and both relaunch helpers (`macos::relaunch_after_exit`, the PowerShell in `windows::install_after_exit`) start the new wai only after the old process is gone, so Restart now needs no change; the new process still retries for 3 s as slack.

**Tech Stack:** Rust 2024 (toolchain 1.99.1, `File::try_lock` is std); GPUI via `gpui-kit = "=0.9.0"`; hyper/rustls already used by `update.rs`; `rustix` (macOS, add feature `stdio`); `notify-rust` already a dependency.

**Spec:** none. Sources: `docs/superpowers/plans/2026-11-11-wai-master-plan.md` rows 2.2, 2.3, 3.13; `docs/superpowers/plans/2026-11-12-wai-execucao-pendencias.md` "Fase 2"; `docs/superpowers/specs/2026-11-12-wai-release-and-auto-update-design.md`.

## Decisions to confirm

1. **Second instance: warn and quit, not "focus the running one".** Focusing needs either a socket/IPC (new surface; the core opens none) or OS activation, and on macOS the second process is the same bundle, so `open` cannot tell it from the first. Warn-and-quit is one `try_lock` plus a native alert naming the folder the user asked for. Cost if wrong: the owner wants the existing window raised; upgrade is a pid in the lock file plus `NSRunningApplication.activate` (macOS) behind the same lock check, no change to the lock itself.
2. **The lock waits 3 s before giving up (100 ms polls).** Restart now never needs it (the helpers wait for the old pid to exit, and the OS drops the lock at exit), but a slow teardown (agent group kill on quit) must not turn an update into "already running". Cost: a real second launch shows its alert 3 s late. If that feels slow, drop to 1 s.
3. **Lock file is `<dir of state.json>/wai.lock`**, so `WAI_STATE_PATH` manual runs lock their own state and never collide with the installed wai. The lock guards `state.json` and `recovery/`, which live in the same folder.
4. **Log is stderr redirected on macOS only: `logs/wai.log` beside `state.json`, mode 0600, rotated at startup when above 1 MiB to `wai.log.1` (one generation, so at most ~2 MiB).** Windows keeps plain stderr (it is the Fase 4 target; no log means the report says "no log on this platform"). No size cap while running: sources are rare and rate-limited; a long-lived wai that floods it grows until the next launch. Cost if wrong: a hand-rolled logger with a size-checked writer replaces the dup2 and the ~25 `eprintln!` sites change.
5. **What the log holds:** whatever the core, `wai-agent`, `wai-ext` and `wai-settings` already print with `eprintln!` (extension crashes, failed saves, update check failures, PATH import), plus a start line `wai <version> started <unix seconds>`. If the agent adapter's stderr is inherited (Task 4 step 1 checks), it lands here too. Nothing new is logged: no env, no prompts, no settings values.
6. **The report is best-effort scrubbed, not proven clean.** Settings are printed by allow-list (see Task 5), commands that differ from the built-in one are collapsed to their program name, env and `~/.claude.json` are never read, and the log tail goes through `scrub` (home -> `~`, long opaque tokens and `key=`/`token=`/`secret=`/`password=`/`Bearer` values masked). A secret a third-party child prints in an unusual shape could still pass; the owner reads the clipboard before pasting it anywhere.
7. **Where the action lives:** palette row "Copy diagnostics" and a button on the Settings Updates page (`settings-diagnostics`). Not on the AI page, which is the sidebar track's neighbourhood.
8. **The npm lookup uses the registry's `latest` for the package named in the claude command** (`@agentclientprotocol/claude-agent-acp`). It is opt-in with `updates.check` (and runs on "Check now"), sends only `User-Agent: wai/<version>`, and reads one small JSON. The note says "newer than the pinned one"; wai never installs or edits the command itself. Cost if wrong: if the owner wants it on a separate switch, one `updates.adapter` bool.

## Dropped items

None. Each of 2.2, 2.3 and 3.13 is still open on `development` (0fc0a5a): `Ready.version` is still dropped in `AgentRegistry::apply`, `main` only uses `eprintln!`, and nothing locks the state folder.

## Global Constraints

- Worktree is created by the controller from `development` (`git log --oneline -5 development` first); the owner's pace applies. **Per task: write its tests, run only `cargo check --workspace --all-targets`, commit.** The full gate runs once, in Task 7: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build -p wai-agent --bin wai-fake-agent && cargo build -p wai-ext --bin wai-fake-ext`, `cargo test --workspace --no-fail-fast`. Then one opus final review and one fix wave. Every shell starts with `export PATH="$HOME/.cargo/bin:$PATH"`; `df -h /System/Volumes/Data` before a build, under 8 GB free stop and report; `cargo clean` when done.
- Commits: `feat:` / `fix:` / `test:` / `docs:`, no scope, no Co-Authored-By, no Jira ref. Imperative, first line under 72 chars. No push, no merge into `development`.
- Idle CPU stays under 0.5%: no timer, poll loop or animation added. The only wait is the 3 s lock retry before the window exists.
- The core opens no ports and never touches user credentials. No code or test reads `~/.claude.json`, env values or tokens; the diagnostics never include them.
- UI copy is English. Existing ids kept.
- Tests never touch the real `~/.config/wai`, `~/.claude` or `~/.claude.json`: every test uses a dir under `std::env::temp_dir()/wai-<name>-<pid>` it removes, and nothing calls `config_dir()`.
- **Self-update behaviour is unchanged.** `update.rs`, `update/macos.rs`, `update/windows.rs`, `UpdateStatus`, `check_and_stage`, `install` and the relaunch helpers are not edited. `update/adapter.rs` is a new sibling that reuses the private `fetch`, `version`, `newer` (it is a child module of `update`). The only edit near them is one call in `Workspace::check_updates`, placed after its existing guards so the wai check runs exactly as before.
- Avoid the workspace track's area (`.claude/worktrees/onda2-workspace`, `workspace.rs` panels, sessions, terminals) and the sidebar redesign (`sidebar.rs`, `agents_view.rs`, the Sessions/History panel). Allowed `workspace.rs` touches, by function name: `agent_rows`, `check_updates`, the `SettingsEvent` match in the settings subscription, and one new field on `Workspace`. New workspace methods live in `diagnostics.rs` (`impl Workspace`, the way `palette.rs` does).
- Merge `development` into the branch before the final gate.

## Review Focus

1. **Restart now with the lock.** Old wai holds the lock until it exits; the new one must start. Tests: Task 3 `waits_for_the_holder_to_exit`, `a_child_does_not_keep_the_lock` (unix: a spawned `sh` must not inherit the fd, or an extension daemon outliving wai would block every launch).
2. **Crash or kill -9 of the holder, then launch.** The lock must not be stale. Test: Task 3 `a_dead_holder_leaves_no_lock` (drop = exit, file stays on disk).
3. **Lock file cannot be created or locked (read-only folder, odd filesystem).** wai must still start, logging why. Test: Task 3 `unusable_lock_does_not_block`.
4. **Report with no log, a huge log, a log with a non-UTF-8 byte or a cut multi-byte char at the tail boundary.** Test: Task 5 `tail_survives_missing_big_and_binary_logs`.
5. **A secret in a custom agent command, in a log line or in an extension setting.** Tests: Task 5 `custom_command_is_collapsed`, `scrub_masks_tokens_and_home`, `report_has_no_env_or_extension_values`.
6. **npm answers 404, 429, a body without `version`, a version that is not semver, or an older version than the pinned one.** None may show a note or disturb the wai update status. Test: Task 2 `adapter_check_handles_failures`.
7. **Adapter exits or is signed out after `Ready`.** The row must not keep showing a stale "running version" for a dead process. Test: Task 1 `exit_clears_the_adapter_version`.

---

### Task 1: Keep the adapter version and show it in Settings > AI

**Files:**
- Modify: `crates/wai/src/agents.rs` (`Agent`, `AgentRegistry::apply`, new accessor)
- Modify: `crates/wai/src/workspace.rs` (`agent_rows`: one field)
- Modify: `crates/wai/src/settings_view.rs` (`AgentRow`, `agent_row`; keep edits out of `Section::Updates` and `version_row`)
- Test: tests module of `agents.rs`; tests module of `settings_view.rs`

**Interfaces:**
- Produces: `Agent.adapter: Option<(String, String)>` (name, version) set from `AgentEvent::Ready { name, version, .. }`, cleared on `AgentEvent::Exited`; `AgentRegistry::adapter(&self, id: &str) -> Option<&(String, String)>`.
- Produces: `AgentRow.adapter: Option<String>`, the text `"<name> <version>"`, `None` until `Ready`.

- [ ] **Step 1: Write the failing tests.** In `agents.rs` tests, using the fake agent the existing tests use: `ready_keeps_the_adapter_version` (after `Ready`, `registry.adapter("fake")` is `Some((name, version))` with the fake agent's values) and `exit_clears_the_adapter_version` (after `Exited`, `None`). In `settings_view.rs` tests, next to `settings_shows_update_status`: `ai_row_shows_the_adapter_version` asserting the row's text node `settings-adapter-fake` contains the version once `Ready`, and does not exist before it.
- [ ] **Step 2: Implement.** Add the field and accessor; in `apply`, match `Ready { name, version, caps }` and store; in the `Exited` arm clear it beside `caps = None`. `agent_rows` sets `adapter: registry.adapter(&id).map(|(n, v)| format!("{n} {v}"))`. In `agent_row`, under the status text, render a muted 12 px line `Adapter <adapter>` with `debug_selector`/id `settings-adapter-<id>` when `Some`.
- [ ] **Step 3: `cargo check --workspace --all-targets`**, expect success. Commit: `feat: show the adapter version in Settings AI`.

### Task 2: Notice when a newer `claude-agent-acp` is out

**Files:**
- Create: `crates/wai/src/update/adapter.rs`
- Modify: `crates/wai/src/update.rs`: only `mod adapter;` (no other line)
- Modify: `crates/wai/src/agents.rs` (`AgentRegistry.latest`, setter)
- Modify: `crates/wai/src/workspace.rs` (`check_updates`: one call; new `check_adapter`)
- Modify: `crates/wai/src/settings_view.rs` (`AgentRow`, `agent_row`)
- Modify: `docs/user-guide.md` (Updates section: the adapter line)

**Interfaces:**
- Produces: `adapter::pinned(command: &str) -> Option<(String, String)>`: the package and version of an `npx` command's `<pkg>@<version>` word (a scoped name keeps its leading `@`); `None` for anything else, including a version that fails `super::version`.
- Produces: `adapter::check(pkg: &str, pinned: &str, fetch: impl FnOnce(&str) -> Fetched) -> anyhow::Result<Option<String>>`: GET `https://registry.npmjs.org/<pkg>/latest` (pkg already validated: `[A-Za-z0-9@/._-]`, one optional leading `@`, at most one `/`); `Ok(Some(latest))` only when `super::newer(pinned, latest)`; `Err` for a non-200 status or a body with no valid `version`.
- Produces: `AgentRegistry::set_latest(&mut self, pkg: String, version: Option<String>)`, `AgentRegistry::newer_adapter(&self, id: &str) -> Option<String>` (compares `pinned(command(id))` with `latest[pkg]`), and `AgentRow.adapter_note: Option<String>` = `"<version> is out (wai runs <pinned>)"`.

- [ ] **Step 1: Write the failing tests in `update/adapter.rs`.** `pinned_reads_the_npx_word` (`"npx -y @agentclientprotocol/claude-agent-acp@0.103.0"` -> `Some(("@agentclientprotocol/claude-agent-acp","0.103.0"))`; `"codex-acp"`, `"npx foo"`, `"npx foo@latest"` -> `None`). `adapter_check_handles_failures` with injected `fetch`: 200 + `{"version":"0.104.0"}` -> `Some("0.104.0")`; same version, older version, and a pre-release when pinned is a release -> `None`; 404, 429, `{}`, `{"version":"x"}`, a fetch `Err` -> `Err`. `adapter_url_is_fixed`: the closure receives exactly `https://registry.npmjs.org/@agentclientprotocol/claude-agent-acp/latest`; a pkg with `..` or a space is rejected before `fetch` runs.
- [ ] **Step 2: Implement `adapter.rs`**, reusing `super::{fetch, version, newer, logged}`; parse with `serde_json::Value`, read `version` only.
- [ ] **Step 3: Wire it.** `Workspace::check_adapter(&mut self, cx)`: for each registry agent whose command `pinned`s, spawn `cx.background_spawn(async move { adapter::check(.., fetch) })` once per distinct package, then `registry.set_latest` and `sync_settings_view`; failure is `eprintln!("wai: adapter version check failed: {}", update::logged(&e))`. In `check_updates`, call it right after the existing `from_launch && !self.settings.updates.check` guard and the `settings.check |= !from_launch` line, guarded by the same `settings.check`; it must not touch `self.update`. `agent_row` shows `adapter_note` as a muted line `settings-adapter-note-<id>`.
- [ ] **Step 4: Workspace test** `no_adapter_request_when_checks_are_off` in `workspace.rs` tests (call `check_updates(true, ..)` with `updates.check = false`; assert `registry.newer_adapter` stays `None` and no task was spawned; use a registry-level injected result, no network). Settings test `ai_row_shows_the_adapter_note`.
- [ ] **Step 5: Document** one bullet in `docs/user-guide.md` under Updates: Settings > AI shows each adapter's version and, when `updates.check` is on, says if a newer `claude-agent-acp` exists on npm.
- [ ] **Step 6: `cargo check --workspace --all-targets`**; commit `feat: tell when a newer claude-agent-acp is out`.

### Task 3: One wai at a time

**Files:**
- Create: `crates/wai/src/instance.rs`
- Modify: `crates/wai/src/main.rs` (`mod instance;`; call in `main`)

**Interfaces:**
- Produces: `instance::acquire(lock_path: &Path, wait: Duration) -> Acquired` with `enum Acquired { Held(Guard), Taken, Unusable(String) }`. `Guard` owns the open `File`; dropping it (or process exit) releases. `Taken`: another process holds it after `wait`. `Unusable`: the file cannot be created or locked for a reason other than `WouldBlock`.
- Produces: `instance::LOCK_WAIT: Duration = 3 s`, `instance::tell_taken(project: &Path)`: native alert (macOS `osascript display alert "wai is already running" message "Quit it first, then open <folder>." giving up after 30`, fixed strings plus the folder name passed as an `argv` item, never interpolated into the script; elsewhere `notify_rust` synchronously) plus an `eprintln!`.

- [ ] **Step 1: Write the failing tests** (temp dir, `wai-instance-<pid>`): `second_acquire_is_taken` (`acquire(p, 0)` twice -> `Held`, `Taken`); `waits_for_the_holder_to_exit` (hold in a thread that drops after 300 ms; `acquire(p, 3 s)` -> `Held`); `a_dead_holder_leaves_no_lock` (acquire, drop, file still exists, `acquire` -> `Held`); `unusable_lock_does_not_block` (lock path inside a non-existent parent that cannot be created, e.g. a path under a regular file -> `Unusable`); `#[cfg(unix)] a_child_does_not_keep_the_lock` (acquire, spawn `sh -c "sleep 2"`, drop guard, `acquire(p, 0)` -> `Held`).
- [ ] **Step 2: Implement `acquire`:** `OpenOptions::new().create(true).write(true).open`, then `try_lock` in a loop with 100 ms sleeps until `wait` elapses; `TryLockError::WouldBlock` is the only `Taken` case; every other error is `Unusable(message)`. Std opens with close-on-exec; do not clear it.
- [ ] **Step 3: Wire `main`.** Right after `state_path` is computed and before `AppState::load`: `let lock = state_path.with_file_name("wai.lock")`; `Taken` -> `tell_taken(&project_arg_or_state_project)` and `return Ok(())` (resolve the folder name for the message only from the argument, else say "wai"); `Unusable(e)` -> `eprintln!("wai: not locking the state folder: {e}")` and carry on; keep the guard in a `let _instance` that lives to the end of `main`. It sits after `update::macos::on_start()` and `shell_path::import()`, so the relocated-copy hand-off (old copy exits before taking the lock) and the PATH import are untouched.
- [ ] **Step 4: Check the hand-off by reading, and record it in the commit body:** `macos::relaunch_after_exit` polls `kill -0 <old pid>` (up to 60 s) before `open -n`, and the Windows script waits for every wai process; so the new process only runs once the old lock is released. Do not edit either.
- [ ] **Step 5: `cargo check --workspace --all-targets`**; commit `feat: refuse a second wai on the same state folder`.

### Task 4: wai writes a log file

**Files:**
- Create: `crates/wai/src/log.rs`
- Modify: `crates/wai/src/main.rs` (`mod log;`; call after the lock)
- Modify: `crates/wai/Cargo.toml` (`rustix` macOS features: add `"stdio"`)

**Interfaces:**
- Produces: `log::init(dir: &Path) -> Option<PathBuf>`: creates `dir/logs/` (0700), rotates `wai.log` to `wai.log.1` when it is over `MAX_BYTES = 1 MiB` (overwriting the old `.1`), opens `wai.log` append (0600), writes the start line, and on macOS makes fd 2 that file (`rustix::stdio::dup2_stderr`). Returns the path, also stored in a `OnceLock` read by `log::path() -> Option<&'static Path>`. Elsewhere returns `None` and changes nothing. Any failure: `None`, stderr untouched.
- Produces: `log::rotate(path: &Path, max: u64) -> std::io::Result<()>` (pure file step, testable).

- [ ] **Step 1: Check how the agent child's stderr is set** in `wai-agent` (the `AcpAgent` spawn and `crates/wai-agent/src/actor.rs`): if inherited it flows into the log (Decision 5); if piped and dropped, note that adapter stderr is not in the log. Put the answer in the commit body. Do not change it.
- [ ] **Step 2: Write the failing tests** (temp dir): `rotate_moves_a_big_log_aside` (2 KiB file, `max` 1 KiB -> `wai.log` gone, `wai.log.1` has the bytes); `rotate_keeps_a_small_log`; `rotate_overwrites_the_old_generation`; `init_on_a_bad_dir_is_none` (parent is a file). The dup2 itself is not unit-tested (it would redirect the test harness's stderr).
- [ ] **Step 3: Implement.** `main` calls `log::init(state_path.parent())` only after the lock is `Held` or `Unusable`, never on `Taken`, so a refused second launch cannot rotate or truncate the first's log.
- [ ] **Step 4: `cargo check --workspace --all-targets`**; commit `feat: write wai's stderr to a log file`.

### Task 5: The diagnostics report

**Files:**
- Create: `crates/wai/src/diagnostics.rs`
- Modify: `crates/wai/src/main.rs` (`mod diagnostics;`)

**Interfaces:**
- Consumes: `log::path()` (Task 4), `AgentRegistry::adapter` (Task 1), `wai_settings::{Settings, builtin}`.
- Produces: `struct Inputs { wai: &'static str, os: String, settings: Settings, home: Option<PathBuf>, agents: Vec<AgentLine>, extensions: Vec<(String, String, bool)>, log_tail: String }` with `AgentLine { id: String, status: String, adapter: Option<String>, command: String }`.
- Produces: `fn report(inputs: &Inputs) -> String` (plain text, sections `wai`, `System`, `Agents`, `Settings`, `Extensions`, `Log (last N lines)`); `fn scrub(text: &str, home: Option<&Path>) -> String`; `fn tail(path: &Path, max_bytes: u64, max_lines: usize) -> String` (reads only the last `max_bytes` = 32 KiB, drops a cut first line, lossy UTF-8, `"(no log)"` when absent); `fn os_line() -> String` (`std::env::consts::{OS, ARCH}` plus `sw_vers -productVersion` on macOS).
- Settings are printed by allow-list: `default_agent`, `theme`, `terminal.shell` (program basename only), `updates.check`, `updates.repo`, `conversation.processing`, agent ids and each `models` entry. A command equal to its built-in's prints as is; any other prints as `<custom: <program basename>, N args>`. Extensions: id, version, enabled only.

- [ ] **Step 1: Write the failing tests.** `custom_command_is_collapsed` (a command containing `--api-key=sk-abc` never appears in `report`; the program name does). `scrub_masks_tokens_and_home` (`/Users/x/p` with home `/Users/x` -> `~/p`; `Bearer abc123`, `token=abcdef`, a 40-char `[A-Za-z0-9_]` word -> masked; ordinary words and a 20-char path unchanged). `report_has_no_env_or_extension_values` (build `Inputs` from a `Settings` whose `terminal.shell` is `/bin/zsh -c hunter2`; assert `hunter2` is absent and `zsh` present; assert the report has no line from `std::env::vars()` for any variable whose name contains `TOKEN`, `KEY` or `SECRET`; the test does not mutate the environment). `tail_survives_missing_big_and_binary_logs` (no file -> `"(no log)"`; 1 MiB file -> at most `max_lines` lines and under `max_bytes`; bytes `0xff 0xfe` and a 3-byte char cut at the boundary -> no panic, replacement char only). `report_lists_each_agent_with_its_version`.
- [ ] **Step 2: Implement** the functions above; tokens: split on whitespace, mask a word of 32+ chars drawn from `[A-Za-z0-9_+/=.-]` that contains no `/`-separated path shape of 3+ segments, and the value after `=` or `:` of a key in `{key, token, secret, password, authorization}` (case-insensitive), and the word after `Bearer`.
- [ ] **Step 3: `cargo check --workspace --all-targets`**; commit `feat: build a secret-free diagnostics report`.

### Task 6: Copy diagnostics action

**Files:**
- Modify: `crates/wai/src/diagnostics.rs` (`impl Workspace { pub(crate) fn copy_diagnostics(&mut self, window, cx) }`)
- Modify: `crates/wai/src/palette.rs` (`Entry::CopyDiagnostics`, the "wai" group row, its handler)
- Modify: `crates/wai/src/settings_view.rs` (`SettingsEvent::CopyDiagnostics`; a row `settings-diagnostics` with a "Copy diagnostics" button on the Updates page, as its own row below `version_row`, not inside it)
- Modify: `crates/wai/src/workspace.rs` (the `SettingsEvent` match: one arm)
- Modify: `docs/user-guide.md` (one line: where it is and what it leaves out)

**Interfaces:**
- Consumes: `diagnostics::{Inputs, report, tail, os_line}`.
- Produces: `Workspace::copy_diagnostics(&mut self, window: &mut Window, cx: &mut Context<Self>)`: snapshots `Inputs` on the main thread (settings, `agent_rows`, extension store, home), reads the log tail and `os_line` on `cx.background_spawn`, then `cx.write_to_clipboard(ClipboardItem::new_string(report))` and `Notification::success("Diagnostics copied")`; on a panic-free `Err` a `Notification::error`.

- [ ] **Step 1: Write the failing tests.** palette: `palette_lists_copy_diagnostics` (the "wai" group has the row). workspace (`TestAppContext`, `state_path: None`, no real dirs): `copy_diagnostics_fills_the_clipboard` (clipboard text starts with the wai version line, has an `Agents` section, and does not contain the test's temp dir home string). settings: `settings_button_copies_diagnostics`.
- [ ] **Step 2: Implement.** The Settings button emits the event; the workspace arm calls `copy_diagnostics`.
- [ ] **Step 3: `cargo check --workspace --all-targets`**; commit `feat: add Copy diagnostics to the palette and Settings`.

### Task 7: Full gate, final review, fix wave

- [ ] **Step 1:** merge `development` into the branch (resolve conflicts in `workspace.rs`, `settings_view.rs`, `main.rs` by keeping both sides' functions).
- [ ] **Step 2: Full gate, once:** `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo build -p wai-agent --bin wai-fake-agent && cargo build -p wai-ext --bin wai-fake-ext`; `cargo test --workspace --no-fail-fast`. Expect all green. Fix only what the gate names.
- [ ] **Step 3: One opus final review** of the branch diff against this plan, with the Review Focus list and Global Constraints (self-update files untouched: `git diff development --stat -- crates/wai/src/update.rs crates/wai/src/update/macos.rs crates/wai/src/update/windows.rs` shows only the `mod adapter;` line).
- [ ] **Step 4: One fix wave** for the review's findings, re-run the gate, commit `fix: apply the core track review`.
- [ ] **Step 5: Owner checks (not run by the agent):** launch wai twice (`wai` then `wai` again from a terminal): second shows the alert and exits; with `WAI_STATE_PATH=/tmp/x/state.json` the second wai starts normally; Settings > Updates > **Restart now** on a staged build starts the new wai (0.6.x to next release); `~/.config/wai/logs/wai.log` exists and Copy diagnostics pastes without tokens; idle CPU < 0.5% with 3 sessions.
