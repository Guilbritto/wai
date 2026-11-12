# wai Onda 2: extensions and settings track Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extensions get a data folder that survives reinstall and can be updated in place (API 1.1); `wai-rotinas` kills everything a timed-out script started and has its sleep policy pinned; the MCP Servers toggle writes Claude's own "disabled servers" list for the servers Claude can disable per project; the Onda 1 settings minors that still apply are fixed.

**Architecture:**
- `wai-ext` learns a per-extension data folder (`<config>/extension-data/<id>/`, outside `extensions/<id>/` so replacing or removing the code never touches it) and `ExtStore::update`, which swaps the code folder and keeps the record's `enabled`.
- `extensions_view.rs` turns "install an id that is already installed" into an update, with the same consent dialog.
- `wai-settings/src/claude.rs` gains `set_mcp_enabled`, built on the existing `replace_json` (lock dir, temp file in the same directory, rename, mode kept, symlink resolved), made hardened for `~/.claude.json`.
- `settings_view` enables the MCP switch for plugin, user and local servers and keeps "Coming soon" for the rest.

**Tech Stack:** Rust 2024; GPUI via `gpui-kit = "=0.9.0"`; `serde_json` with `preserve_order`; Go (stdlib only) for `wai-rotinas`.

**Spec:** none. Sources: `docs/superpowers/plans/2026-11-12-wai-execucao-pendencias.md` (track "extensões e settings"), master plan rows 2.5 and 3.14, `docs/RELATORIO-final.md` (R66), `docs/extensions.md`, `docs/superpowers/plans/2026-11-11-wai-onda-1-settings.md` (the page's design), `/Users/guilherme.brito/Project/Guilherme/wai/.superpowers/sdd/master-onda-1/track-c-report.md` (minors).

## Decisions to confirm

1. **Data folder lives at `<config>/extension-data/<id>/`, not under `extensions/<id>/`, and uninstall keeps it.** Cost if wrong: if the owner wants "uninstall removes everything", a user who uninstalls leaves files behind; fix is one `remove_dir_all` plus a checkbox in the uninstall flow. Not deleting is the safe default for credentials and state a user would lose.
2. **API 1.1 is additive.** `initialize` keeps `"protocol": 1` (an extension that checks `== 1` keeps working) and gains `"api": "1.1"` and `"data_dir"`. Cost if wrong: an extension that rejects unknown fields breaks; docs already say to ignore unknown ones, to be re-checked in Task 1.
3. **Update = installing the same id again (folder or git URL), not a button on the row.** `extensions.json` does not record the source, so a row button would have nothing to pull from. The consent dialog is shown again with the new permissions and reads "Update <name>?". Cost if wrong: the owner wants a one-click "Update" on each row; that needs the source persisted in `extensions.json` (a new optional `source` field) and is a follow-up.
4. **`wai-rotinas` timeout kills the script's descendant tree, not a new process group.** The owner wrote "its whole process group". A separate group per run would make a script outlive wai: wai stops extensions with a SIGKILL of the extension's group and no shutdown message (docs/extensions.md "Stopping"), so a script in its own group would survive every quit, disable and uninstall. The tree walk keeps the script in the extension's group (still killed with it) and kills the script plus every descendant on timeout. Ceiling: a descendant that was already orphaned to launchd or that called `setsid` escapes. Cost if wrong: an owner who wants the literal group kill needs a supervisor that outlives the extension; say so and it is a separate task.
5. **Sleep policy: run once on wake (already the behaviour), pinned by a test and documented.** A routine whose time fell during sleep runs once within 1 minute of waking, whatever the number of missed slots. No skip-and-log: a daily report that fires at 14:00 instead of 09:30 is still wanted, and a skipped one is silent. Cost if wrong: a routine with side effects that must not run late (a "good morning" message) needs a per-routine grace; one new optional key.
6. **MCP toggle writes `disabledMcpServers` (project entry of `~/.claude.json`) for plugin, user and local servers only.** `.mcp.json` servers and claude.ai connectors keep "Coming soon". Cost if wrong: if `.mcp.json` rows should toggle too, the reader must also honour `disabledMcpjsonServers` (Claude's "rejected" list), otherwise the switch lies; one more task.
7. **The MCP write does not compare a whole-file stamp.** Claude rewrites `~/.claude.json` every few seconds while a session runs, so the Onda 1 "unchanged since shown" check would reject most toggles. The edit is applied to the bytes read under the lock; a change between that read and the rename still aborts (`Changed`) and the UI retries once. Cost if wrong: none for data (other keys are copied from fresh bytes); the cost is a short window where Claude's own write can win and wai reports "try again".
8. **Turning a server off when the project has no entry in `projects` creates the entry** with only `disabledMcpServers`. Claude fills defaults for missing keys on read. Cost if wrong: an odd-looking minimal entry; turning on with no entry is a no-op.

## Dropped items

- **M1** (commit-hash version gets a `v` badge): already fixed, `plugin_groups` adds `v` only when the version starts with a digit.
- **M2** (marketplace groups alphabetical): `BTreeMap` order is the current, deliberate behaviour; no ordering requirement exists.
- **M3** (problem banners on every Claude page): cosmetic, needs an owner call on which page owns which file; not worth a task.
- **M9** (Windows JSON fixture), **M13** (`preserve_order` workspace-wide): Windows is Fase 4; `wai-settings` already enables `preserve_order`, which is what Task 4 needs.
- **M10** (no search on Extensions): hidden on purpose by Onda 1 Task 2.
- **M11** (guide summaries for "From plugins") and the `Err(String::new())` sentinel for a missing CLI: cosmetic, no behaviour at stake.
- **Validation item 3 of the Onda 1 list** ("MCP toggle disabled, `~/.claude.json` byte-identical"): obsolete, the toggle now writes. Task 6 replaces it.
- **Removing R66's one-wakeup-a-minute polling**: stays; a host "system woke" event would need a protocol change outside this track.

## Global Constraints

- Binding: `/Users/guilherme.brito/Project/Guilherme/wai/.superpowers/sdd/master-onda-1/global-constraints.md`, with this update: a remote `origin` exists, but there is still **no push** and no merge into `development` (the controller merges). Every shell starts with `export PATH="$HOME/.cargo/bin:$PATH"`. Run `df -h /System/Volumes/Data` before a build; under 8 GB free, stop and report. `cargo clean` in the worktree when done.
- Worktree `.claude/worktrees/ext-settings`, branch `feat/ext-settings`, from `development` (`git log development -3` first).
- **Owner's pace.** Per task: write its tests, run only `cargo check --workspace --all-targets` (plus the extension's own build and test command when the task touches `extensions/wai-rotinas`: `cd extensions/wai-rotinas && go vet ./... && go test ./...`), then commit. Do not run `cargo test` or `clippy` per task. The full gate runs once, in Task 8: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build -p wai-agent --bin wai-fake-agent && cargo build -p wai-ext --bin wai-fake-ext`, `cargo test --workspace`, and the extension tests per `docs/building.md`. Then one final review and one fix wave.
- Commits: `feat:` / `fix:` / `test:` / `docs:`. No scope, no Co-Authored-By, no Jira ref. Imperative, first line under 72 chars.
- Not in this track, do not touch: `crates/wai/src/update.rs`, the Updates section of `settings_view.rs`, the quit path in `workspace.rs`, `.github/workflows/*`, `CLAUDE.md`. (`settings_view.rs` is touched only for the MCP page description and its tests; keep edits away from `Section::Updates`.)
- The core never touches user credentials and opens no ports. `~/.claude.json` holds account data: no code or test logs, prints or asserts on its values other than the `disabledMcpServers` array. No struct carries a server's `env`, `headers`, `args`, `command` or `url`.
- Tests never read or write the real `~/.claude.json`, `~/.claude` or `~/.config/wai`. `open_at` points `claude_home` at a temp dir. Never write into `std::env::temp_dir()` itself.
- Never touch `/Applications/wai.app` or kill `wai`, `claude-agent-acp` or `claude` you did not start.
- Idle CPU stays under 0.5%: no timer, poll or animation added anywhere. The MCP write runs on the background executor.
- UI copy is English. Keep existing ids (`ext-*`, `consent-*`, `mcp-check`, `mcp-toggle`, `plugin-toggle`, ...). Windows must still build: process and symlink tests are `#[cfg(unix)]`.
- `wai-rotinas`: stdlib only, no new Go dependency.

## Review Focus

1. **Update while the extension runs.** The old process must be stopped before its folder is swapped, and restarted from the new code. Tests: Task 2 `update_swaps_code_keeps_flags`, Task 3 `update_stops_then_restarts`.
2. **Update fails half way** (rename or `extensions.json` save errors). The old extension must still be there and start. Test: Task 2 `failed_update_restores_the_old_folder`.
3. **Uninstall then reinstall keeps the data; the data folder is never inside a folder wai replaces.** Test: Task 1 `data_survives_uninstall_and_reinstall`.
4. **A timed-out script that started children** (`sleep 300 &`, a pipeline, a double fork). None may survive the timeout. Test: Task 5 `timeout_kills_descendants`.
5. **`~/.claude.json` is a symlink, has mode 0600, is large, has keys in a given order, floats and non-ASCII.** The write must keep the link, the mode, the order and every other value. Tests: Task 4 `mcp_write_keeps_everything_else`, `mcp_write_keeps_mode_and_symlink`.
6. **Claude rewrites the file between wai's read and rename**, or a second click lands while the first write runs. No lost update, no stale re-enable. Tests: Task 4 `mcp_write_aborts_if_the_file_changes_underneath`, Task 6 `mcp_toggles_wait_for_the_write`.
7. **A Mac that sleeps through several slots of an `every` or `at` routine.** One run on wake, then the next slot is in the future. Test: Task 5 `slept_through_slots_run_once`.

---

### Task 1: Extension data folder and API 1.1 handshake

**Files:**
- Modify: `crates/wai-ext/src/store.rs` (`Installed`, `load`, `commit`)
- Modify: `crates/wai-ext/src/host.rs` (`Ext::new`, `Ext::start` env, `init`)
- Modify: `crates/wai-ext/src/install.rs` (add `pub(crate) fn data_root(config_dir: &Path) -> PathBuf`)
- Modify: `crates/wai-ext/src/bin/wai-fake-ext.rs` (echo `WAI_EXT_DATA`)
- Modify: `crates/wai-ext/tests/host.rs`, `crates/wai-ext/tests/install.rs`
- Modify: `crates/wai/src/ext_host.rs` (the one `Installed { .. }` literal in its tests, ~line 1041)
- Modify: `docs/extensions.md`

**Interfaces:**
- Produces: `Installed.data: PathBuf` (`<config>/extension-data/<id>`; the folder may not exist yet). `ExtHandle::spawn` creates it before starting the process and passes it as env `WAI_EXT_DATA` and as `data_dir` in `initialize`.
- Produces: `initialize` params now `{"protocol": 1, "api": "1.1", "wai_version", "ext_id", "granted", "project", "ext_dir", "data_dir"}`.

- [ ] **Step 1: Write the failing tests**
  - `tests/host.rs`: extend the existing `initialize` echo test (it asserts `env_ext_dir`): the fake ext also reports `env_ext_data`; assert `init["env_ext_data"] == data` and `init["params"]["data_dir"] == data`, `init["params"]["api"] == "1.1"`, `init["params"]["protocol"] == 1`, and that `data` exists as a directory after spawn. The `installed()` helper gains the `data` field (a sibling temp dir).
  - `tests/install.rs` `data_survives_uninstall_and_reinstall`: commit a sample, write `<data>/state.txt`, `uninstall`, `commit` again; the file is still there and `store.list()[0].data` ends with `extension-data/sample`.
- [ ] **Step 2: Run `cargo check --workspace --all-targets`**
  Expected: errors (no field `data`). That is the red state; do not run the tests.
- [ ] **Step 3: Implement**
  - `data_root(config_dir) = config_dir.join("extension-data")`. `load` and `commit` set `data: data_root(config_dir).join(id.as_str())`. Ids are validated and never start with a dot, so the path stays inside `extension-data`.
  - `Ext::new` stores `data` and does `std::fs::create_dir_all(&installed.data)`, mapping an error to `Err("cannot create its data folder: {e}")` (the extension is then `Failed`, as for other start errors). `start` adds `.env("WAI_EXT_DATA", &self.data)`. `init` gains `api` and `data_dir` (through `dunce::simplified`, like `ext_dir`).
  - `uninstall` and `commit` never touch `data`.
- [ ] **Step 4: `docs/extensions.md`**
  - Line 5: "This guide describes API version 1.1 (protocol 1), as implemented by wai 0.6.2." (Keep the wai version the doc already states; the release process bumps it.)
  - Replace the **Data.** bullet: "**Data.** A folder of your own arrives as `WAI_EXT_DATA` and as `data_dir` in `initialize`, `<config>/extension-data/<id>/`, created before you start. It is not inside your extension folder, so updating or reinstalling does not touch it, and uninstalling leaves it. Keep state and credentials-free caches there."
  - `initialize` row (line 135): add `"api": "1.1"` and `"data_dir": "<folder>"`. Add a sentence: "API 1.1 only adds fields; ignore the ones you do not know."
  - "Where things go" line 94 gains the data folder.
- [ ] **Step 5: `cargo check --workspace --all-targets`**, then commit.

```bash
git add crates/wai-ext crates/wai/src/ext_host.rs docs/extensions.md
git commit -m "feat: give each extension a data folder that outlives reinstalls"
```

### Task 2: `ExtStore::update`

**Files:**
- Modify: `crates/wai-ext/src/store.rs`
- Modify: `crates/wai-ext/src/install.rs` (`InstallError` variant)
- Test: `crates/wai-ext/tests/install.rs`

**Interfaces:**
- Consumes: `Staged`, `Installed.data` (Task 1).
- Produces: `ExtStore::update(&mut self, staged: Staged, granted: Vec<Permission>) -> Result<(ExtId, PathBuf), InstallError>`. The `PathBuf` is the old code folder, moved into the staging area; the caller deletes it off the main thread (same contract as `uninstall`). Callers stop the extension's processes first. New `InstallError::NotInstalled(String)`: "{0} is not installed". `commit` keeps refusing an installed id.

- [ ] **Step 1: Write the failing tests**
  - `update_swaps_code_keeps_flags`: install `sample` v0.1.0, disable it, put a marker in its data folder; stage a folder with the same id at v0.2.0 and another file; `update` with the manifest's permissions. Assert: `list()[0].manifest.version == "0.2.0"`, `enabled == false`, the new file exists in `extensions/sample/`, the old-only file does not, the data marker survives, `extensions.json` says `0.2.0`, the returned trash path exists and holds the old files, and `ExtStore::open` of the same config shows the same.
  - `update_refuses_unknown_id_and_wrong_consent`: not installed gives `NotInstalled`; `granted` different from the manifest's gives `Consent`; in both cases nothing changed on disk.
  - `failed_update_restores_the_old_folder`: make the swap fail after the old folder is moved (for example make `extensions.json.tmp` a directory so `save` fails; Unix only if needed). Assert the error, that `extensions/sample/` holds the old version again, `list()` still reports the old version and the staged copy is intact.
- [ ] **Step 2: Run `cargo check --workspace --all-targets`**: errors (no `update`). Red.
- [ ] **Step 3: Implement `update`.** Order: validate consent like `commit`; `position(&id)` or `NotInstalled`; `trash = staging_path`; `rename(dir, trash)`; `rename(staged.dir(), dir)` (on failure rename `trash` back); replace `manifest` and `granted`, keep `enabled` and `data`; `save()`; on a save error put everything back (rename `dir` to the staged location is not needed: rename `dir` back to `staged.dir()`, `trash` back to `dir`, restore the in-memory record) and return the error. Success returns `(id, trash)`.
- [ ] **Step 4: `cargo check --workspace --all-targets`**, then commit.

```bash
git add crates/wai-ext
git commit -m "feat: update an installed extension in place"
```

### Task 3: Update flow in the Extensions page

**Files:**
- Modify: `crates/wai/src/extensions_view.rs` (`review`, `consent`, `install`; tests)
- Modify: `docs/extensions.md` ("Updating" bullet)

**Interfaces:**
- Consumes: `ExtStore::update` (Task 2), `ExtHandle` stop through `self.host.update(cx, |host, cx| host.stop(id, cx))` and `self.sync_host(cx)`, both already used by `uninstall` and `install`.
- Produces: `review` no longer refuses an installed id; it passes `updating: bool` to `consent` and `install`. The refusal for a `expect` mismatch stays. Dialog title `Update {name}?` and button label `Update` (id stays `consent-install`); the version line shows `{old} → {new}`. Success notice `Updated {name}`.

- [ ] **Step 1: Write the failing tests** (pattern of the existing `uninstall_stops_processes`, which uses a running fake extension)
  - `update_stops_then_restarts`: install a running extension, `review` a staged copy with the same id and a new version, click `consent-install`; assert the old process is gone (its pid file or `wait_gone` helper as the sibling test does), the row shows the new version, the extension is running again from the new folder, `enabled` kept.
  - `update_shows_old_and_new_version`: the consent dialog contains a line `consent: 0.1.0 → 0.2.0` (the `line()` helper makes each line findable by `consent: <text>`) and the title is `Update Sample?`; Cancel leaves the old version and no staged leftovers (`staged(&config) == 0`).
  - Replace any existing test that asserts the "already installed; uninstall it first" refusal in the UI with an assertion of the update dialog. The `expect` mismatch test stays.
- [ ] **Step 2: `cargo check --workspace --all-targets`**, fix compile errors in the tests; they fail at run time (not run here).
- [ ] **Step 3: Implement.** In `review`, `installed` only matters to decide `updating`; keep the `Some(want) != id` refusal. `install(staged, updating)`: if updating, `host.stop(&id)` first, then `store.update(...)`, then `cx.background_spawn(remove_dir_all(trash)).detach()`, then `sync_host`. A project-suggested update is allowed but still goes through consent.
- [ ] **Step 4: `docs/extensions.md`.** Replace the "Updating" bullet: "**Updating.** Install the same id again (folder or git URL). wai asks for consent again, stops the extension, replaces its folder and starts it again. It stays enabled or disabled as it was, and its data folder is untouched. A project suggestion of an installed id also goes through this."
- [ ] **Step 5: `cargo check --workspace --all-targets`**, then commit.

```bash
git add crates/wai/src/extensions_view.rs docs/extensions.md
git commit -m "feat: install over an installed extension to update it"
```

### Task 4: `set_mcp_enabled` in `wai-settings`

**Files:**
- Modify: `crates/wai-settings/src/claude.rs` (`replace_json`, new `set_mcp_enabled`, tests)

**Interfaces:**
- Produces: `pub fn set_mcp_enabled(home: &Path, project_key: &str, server: &str, on: bool) -> Result<(), WriteError>`. It edits `<home>/.claude.json`, `projects[project_key].disabledMcpServers` only: `on == false` appends `server` if absent; `on == true` removes it. Preserves the array's order, creates `projects` and the entry only when turning off and they are missing, writes nothing when nothing would change. Errors: `Invalid` when `projects`, the entry or the list is not the expected JSON type (nothing written), `Busy`, `Changed`, `Io`.
- Changes: `replace_json(path, seen: Option<Stamp>, edit)`. `None` skips the "seen" comparison but still re-reads the file right before the rename and returns `Changed` if it differs from what the edit started from. `set_plugin_enabled` passes `Some(seen)`; its behaviour is unchanged.
- Hardening (minors M5, M6): the temp file is created with `OpenOptions::create_new` after removing whatever held its name (never follows a symlink planted at `<file>.wai.tmp`); a dangling symlink at `path` is resolved with `read_link` joined to the parent, not replaced by a regular file.

- [ ] **Step 1: Write the failing tests** (temp dirs only; fixture JSON is built in code, never from the real file)
  - `mcp_write_keeps_everything_else`: fixture with an `oauthAccount` object, numbers `0.1`, `1e21`, `12345678901234567890`, a non-ASCII string, nested arrays, `projects` with two entries and keys in a deliberate non-alphabetical order, 4-space indent, trailing newline. Toggle one server off, then on. Assert that parsed-and-reserialized original equals the result except the one array, that key order of every object is the same (compare `Value` with `preserve_order` through `to_string`), the indent and trailing newline are kept, and after the second toggle `disabledMcpServers == []` (the empty array may remain).
  - `mcp_write_keeps_mode_and_symlink` (`#[cfg(unix)]`): file `0600` and also `0640`; and a symlink `~/.claude.json` into another temp dir: the link survives, the target changes, mode unchanged, no `*.wai.tmp` or `.lock` left over.
  - `mcp_write_creates_missing_entry_only_when_turning_off`; `mcp_on_without_entry_writes_nothing` (mtime and bytes unchanged).
  - `mcp_write_rejects_wrong_shapes`: `projects` is an array; `disabledMcpServers` is a string: `Invalid`, bytes unchanged.
  - `mcp_write_aborts_if_the_file_changes_underneath`: call the inner `replace_json` with an edit closure that rewrites the file from another thread/handle before returning; expect `Changed` and the file as the other writer left it.
  - `planted_tmp_symlink_is_not_followed` (`#[cfg(unix)]`): `<file>.wai.tmp` is a symlink to a victim file; after the write the victim is unchanged.
  - `dangling_symlink_is_kept` (`#[cfg(unix)]`): the link still points at the same target and the target now exists.
- [ ] **Step 2: `cargo check --workspace --all-targets`**: errors. Red.
- [ ] **Step 3: Implement** as in Interfaces. The edit closure: `root.as_object_mut()`, `projects` (create when off), entry (create when off), `disabledMcpServers` array of strings; non-string elements are kept untouched.
- [ ] **Step 4: `cargo check --workspace --all-targets`**, then commit.

```bash
git add crates/wai-settings/src/claude.rs
git commit -m "feat: write a project's disabled MCP servers to Claude's config"
```

### Task 5: `wai-rotinas`: kill the tree on timeout, pin the sleep policy

**Files:**
- Create: `extensions/wai-rotinas/killtree_unix.go` (`//go:build unix`), `extensions/wai-rotinas/killtree_other.go` (`//go:build !unix`)
- Modify: `extensions/wai-rotinas/rotinas.go` (`runScript`, remove the stale `ponytail:` note)
- Modify: `extensions/wai-rotinas/main_test.go`, `extensions/wai-rotinas/README.md`, `docs/RELATORIO-final.md` is NOT edited (history)

**Interfaces:**
- Produces: `killTree(pid int) error` (unix: snapshot descendants with `ps -A -o pid=,ppid=`, SIGKILL them and then the root; repeat up to 3 passes until a pass finds none; errors only if the root cannot be signalled for another reason than "no such process"). `other`: `os.FindProcess(pid)` then `Kill()`.
- Produces: `descendants(rows [][2]int, root int) []int` (pure, breadth first, never includes `root`, handles cycles) used by `killTree`.
- `runScript` sets `cmd.Cancel = func() error { return killTree(cmd.Process.Pid) }` (called by `exec` when the context ends: timeout or stop). The script stays in the extension's process group (Decision 4); no `Setpgid`.

- [ ] **Step 1: Write the failing tests**
  - `TestDescendants`: rows `[[10,1],[11,10],[12,11],[13,10],[20,1]]`, root 10 gives `{11,12,13}` in any order; a cycle does not loop.
  - `timeout_kills_descendants` (unix): script `sleep 300 &\necho $! > PIDFILE\nwait\n`; `runScript(ctx, path, time.Second)`; status `Timed out`; read the pid, poll up to 3 s with `syscall.Kill(pid, 0)` until `ESRCH`; also a grandchild through a subshell: `( sleep 300 & echo $! > PIDFILE2 ; wait ) &`.
  - `stop_kills_descendants`: same script, cancel the context instead of timing out; status `Stopped`, child gone.
  - `slept_through_slots_run_once` (Review Focus 7), using `newServer` with `seed`-style state: an `every = "1h"` routine whose `last_run` is 3 days ago and an `at = "09:30"` routine whose `last_run` is 3 days ago; `pick(now)` returns the routine at once; after `state[name] = record{LastRun: now}` (what `run` stores) `due(r)` is after `now` and `pick` sleeps. Name the case `R66` in a comment. This pins "run once on wake".
- [ ] **Step 2: Run `cd extensions/wai-rotinas && go vet ./...`**: compile errors (no `killTree`). Red.
- [ ] **Step 3: Implement** as in Interfaces. Remove the `ponytail:` comment above `runScript` and say instead why the tree, not a group: "a group of its own would outlive wai's SIGKILL of the extension's group". Keep `WaitDelay`.
- [ ] **Step 4: README "How runs work".** Schedule bullet: replace the R66 sentence with "A routine whose time passes while the computer sleeps runs once, within a minute of waking, however many slots it missed; the next run is counted from that run." Limits bullet: "A run is stopped after 60 minutes (`Timed out`); the script and everything it started are killed." Stopping bullet unchanged.
- [ ] **Step 5: `cd extensions/wai-rotinas && go vet ./... && go test ./...`** (Expected: PASS) and `cargo check --workspace --all-targets`; commit.

```bash
git add extensions/wai-rotinas
git commit -m "fix: kill everything a timed-out routine started"
```

### Task 6: MCP toggle in Settings, honest page copy

**Files:**
- Modify: `crates/wai/src/settings_view/claude.rs` (`Control::Mcp`, `mcp_groups`, `toggle_mcp`, `claude_row`, `set_claude`, tests)
- Modify: `crates/wai/src/settings_view.rs` (`SettingsEvent::ToggleMcp`, `Section::Mcp` description only)
- Modify: `crates/wai/src/workspace.rs` (the `SettingsEvent` match, one arm, like `TogglePlugin` at ~2430)
- Modify: `crates/wai/src/claude_config.rs` (`toggle_mcp` on `Workspace`; tests)

**Interfaces:**
- Consumes: `set_mcp_enabled` (Task 4); `ClaudeData.project_key: Option<String>` and `project_key_for(&Path)`.
- Produces: `SettingsEvent::ToggleMcp { name: String, on: bool }`. `Control::Mcp { ix, on, toggleable: bool }` where `toggleable` is true for `McpSource::Plugin | User | Local` and false for `Project`. The switch is `disabled(!toggleable || self.busy || self.claude.is_none())`; non-toggleable rows keep `aurora::soon(cx)`, toggleable rows show no badge.
- Produces: `Workspace::toggle_mcp(&mut self, name: String, on: bool, window, cx)`. Key = `data.project_key` or `project_key_for(&self.project)`. Runs `set_mcp_enabled` on the background executor; on `Changed` or `Busy` retries once; success notice `Applies to new Claude sessions`; failure notice `Could not change ~/.claude.json: {e}`; always ends with `refresh_claude`.
- Minor M8: `set_claude` no longer clears `busy`. A new `SettingsView::write_done(&mut self, cx)` does (called by both `toggle_plugin` and `toggle_mcp` completions, before `refresh_claude`), so a stale read finishing mid-write cannot re-enable the switches.
- Page copy (`Section::Mcp`): "Servers that give Claude more tools. Turning one off applies to this project, in new Claude sessions, and keeps its configuration. Servers from a .mcp.json file and your claude.ai connectors cannot be turned off here yet." This is true after the change for the rows that have an active switch; before it the old sentence promised behaviour the disabled switch did not have.

- [ ] **Step 1: Write the failing tests** (pattern of `claude_config.rs` tests: temp `home`, `open_at` with `claude_home`, project temp dir; the fixture has `projects` with a plugin server and a local server, a `.mcp.json` server, and secrets such as `s3cr3t` that must never reach a row)
  - `mcp_toggle_writes_disabled_list`: click `mcp-toggle` for `plugin:alpha:db` (currently off in the fixture, so it turns on): after the write `disabledMcpServers` no longer has it; click again: it has it; the rest of the file is equal as parsed JSON; `~/.claude/settings.json` is untouched.
  - `mcp_project_servers_stay_coming_soon`: the `.mcp.json` row's switch is disabled and clicking changes no byte of `~/.claude.json`; the claude.ai connectors row is `Control::Soon`.
  - `mcp_toggles_wait_for_the_write` (Review Focus 6): after the first click `busy` is true and the switches are disabled; a second click before the write ends writes nothing; a `set_claude` that lands mid-write (simulated by calling `refresh_claude` while the write task is pending) does not clear `busy`.
  - `mcp_toggle_reports_invalid_file`: `projects` is a string: the warning notice shows, bytes unchanged, switches usable again.
  - Update `mcp_rows_show_source_and_transport` for the new `Control::Mcp` field and for the missing Coming soon badge on toggleable rows. Update the test that asserts the page description text, if any.
- [ ] **Step 2: `cargo check --workspace --all-targets`**: fix compile errors in tests and callers. Red at run time.
- [ ] **Step 3: Implement** as in Interfaces. `status_line` is unchanged (the "Off for this project" line already follows `enabled`).
- [ ] **Step 4: `cargo check --workspace --all-targets`**, then commit.

```bash
git add crates/wai/src/settings_view.rs crates/wai/src/settings_view crates/wai/src/workspace.rs crates/wai/src/claude_config.rs
git commit -m "feat: turn Claude MCP servers on and off for the project"
```

### Task 7: Onda 1 settings minors that still apply

**Files:**
- Modify: `crates/wai-settings/src/claude.rs` (M4, M12, M15; tests)
- Modify: `crates/wai/src/settings_view.rs`, `crates/wai/src/settings_view/claude.rs` (M7; tests)

**Interfaces:** none new; behaviour only.

- [ ] **Step 1: Write the failing tests**
  - M4 `empty_install_path_is_skipped`: `installed_plugins.json` entry with `"installPath": ""` (and one missing the key): the plugin is listed with no skills, hooks or MCP servers and nothing under the process's cwd is scanned (run with the cwd set to a temp dir holding a `skills/x/SKILL.md`: it must not show up). Use a plugin row with an empty `path`.
  - M12 `git_root_local_settings_lock_the_toggle`: project is a subfolder of a git root whose `.claude/settings.local.json` sets `enabledPlugins` for the plugin; `Plugin.locked` is `Some("Set in the repository's .claude/settings.local.json")`. And a home folder that is itself a git repo does not lock anything (the existing `at_home` rule still wins).
  - M15 `default_manifest_paths_are_not_counted_twice`: a plugin whose manifest names `"skills": "./skills/"` (the default folder) and `"hooks": "./hooks/hooks.json"` lists each skill and hook once, and `Plugin.summary` counts them once.
  - M7 `collapsed_state_follows_the_group_not_its_position`: on a page with more than 20 rows, collapse the second group, type a filter that removes the first group, and the group that was second is still collapsed (the state is keyed by `(section, group title)`).
- [ ] **Step 2: `cargo check --workspace --all-targets`**; Red at run time.
- [ ] **Step 3: Implement.** M4: `scan` is skipped when `installPath` is empty. M12: give the `root_local` layer the lock text above. M15: collect skill and hook paths into a de-duplicated set (by canonical path) before counting. M7: `toggled: HashSet<(Section, String)>` and `default_open` receives the group's position in the unfiltered list (compute it before `filter`).
- [ ] **Step 4: `cargo check --workspace --all-targets`**, then commit.

```bash
git add crates/wai-settings/src/claude.rs crates/wai/src/settings_view.rs crates/wai/src/settings_view
git commit -m "fix: settle the leftover Claude settings page edge cases"
```

### Task 8: Final gate, review, fix wave

**Files:** none planned.

- [ ] **Step 1: Full gate** (once, at the end of the track):
  ```bash
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo build -p wai-agent --bin wai-fake-agent && cargo build -p wai-ext --bin wai-fake-ext
  cargo test --workspace
  (cd extensions/wai-rotinas && go vet ./... && go test ./...)
  ```
  Plus the other extensions' tests per `docs/building.md` if their files changed (they should not). Expected: all green.
- [ ] **Step 2: One final review** of the whole branch (`git diff development...HEAD`) with a fresh reviewer, focused on the Review Focus list.
- [ ] **Step 3: One fix wave** for what the review finds, one commit per fix (`fix:`); re-run Step 1.
- [ ] **Step 4: Manual validation on the real machine, after the controller merges** (owner or controller, not the implementer):
  1. In a project, Settings > Claude > MCP Servers: turn `plugin:playwright:playwright` off. `python3 -I -c` printing only `sorted(projects[key]["disabledMcpServers"])` shows it; `ls -l ~/.claude.json` mode is still `-rw-------`; `claude mcp list` in that folder says `Disabled for this project`. Turn it on: gone from the list.
  2. A `.mcp.json` server and the claude.ai connectors row keep Coming soon.
  3. Run a Claude session while toggling; no "try again" loop and no corrupted file (`python3 -I -c 'import json,os;json.load(open(os.path.expanduser("~/.claude.json")))'` exits 0).
  4. Install `wai-rotinas` twice from a changed folder: second time shows "Update Rotinas?", the process restarts, `~/.config/wai/extension-data/wai-rotinas/` exists and survives an uninstall.
  5. A routine whose script runs `sleep 300 &` with `runTimeout` lowered by hand (or the test) leaves no `sleep` behind.
  6. Sleep the Mac across a routine's slot: one run within a minute of waking.
