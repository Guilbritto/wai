# wai Onda 2, workspace track (3.3, 3.5, 3.4, 3.11, 3.12): Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the terminal panel several shells as tabs (3.3), make the mouse wheel work inside `less`/`vim` (3.5), bring editor tabs back on reopen (3.4), make the file tree and the search respect `.gitignore` outside a git repository (3.11), and harden the hot-exit recovery copies (3.12).

**Architecture:** 3.3 replaces `Workspace::terminal: Option<Entity<TerminalView>>` with a list of tabs. All terminal logic for `Workspace` moves to a new `crates/wai/src/terminals.rs` (`impl Workspace`, the way `start.rs` does for drafts), so `workspace.rs`, the merge hot spot, only changes its fields, three loops, the render call and the tests that read `w.terminal`. 3.5 adds a pure `wheel_bytes` encoder and a `wheel_mode()` query to `wai-terminal`; the view picks history scroll, arrow keys or mouse reports from the live terminal mode. 3.4 adds two additive keys to `AppState`; the workspace restores them after the hot-exit copies, so a recovered edit is never shadowed by a clean reopen. 3.11 puts both walkers behind one builder in `wai-project`. 3.12 is two small edits in `editor/recovery.rs`.

**Tech Stack:** Rust workspace, GPUI + gpui-component (via `gpui_kit`), `alacritty_terminal` 0.26 (`TermMode`), `portable-pty` 0.9, `ignore` 0.4.

**Spec:** none. Inputs: `docs/superpowers/plans/2026-11-12-wai-execucao-pendencias.md` (track "workspace"), master plan rows 3.3, 3.4, 3.5, 3.10, 3.11, 3.12 (`docs/superpowers/plans/2026-11-11-wai-master-plan.md`; its line numbers are stale), "Lacunas conhecidas" of `docs/RELATORIO-final.md`, and the owner's answer of 2026-11-12: he uses the terminal for the Claude TUI, for `/mcp` and `/plugin`, and for his own commands.

## Dropped

- **3.10 (folder switch rereads settings): already done.** `Workspace::set_project` calls `reload_settings`, which reads `Settings::load(user_dir, Some(project))` and runs `apply_settings`: it changes the theme (`Theme::change`) and, when `default_agent` changed, restarts the agent and refreshes the history. The test `project_switch_reloads_settings` covers the theme. The "Lacunas" line in `docs/RELATORIO-final.md` is stale; Task 8 removes it. A project's `default_agent` is honoured only when the user already chose one (`Settings::load`, R15): that is by design.

## Global Constraints

From `/Users/guilherme.brito/Project/Guilherme/wai/.superpowers/sdd/master-onda-1/global-constraints.md` (binding), updated for today:

- Repo: /Users/guilherme.brito/Project/Guilherme/wai. Base: `development` at a818cc8 (0.6.2). Work in a worktree `.claude/worktrees/workspace` on branch `feat/workspace` from `development`. A remote `origin` now exists; still **no push**, and no merge into `development` (the controller does that).
- Every shell: `export PATH="$HOME/.cargo/bin:$PATH"`. Disk is tight: `df -h /System/Volumes/Data` before a build, stop under 8 GB free, `cargo clean` in the worktree when done.
- Tests never read or write the real `~/.config/wai`, `~/.claude`, `~/.gitconfig` or the developer's shell rc. Terminal tests use `/bin/sh` (`open()` sets it). Never touch `/Applications/wai.app` or the running wai.
- Idle CPU < 0.5%, streaming CPU < 10%. Nothing animated or polling in the core when idle: no timer per terminal tab, no timer for the tab strip. Event-driven only.
- UI copy in English. Follow existing patterns; no comments unless they carry a non-obvious why.
- Never break existing test ids (`tab-*`, `sidebar-*`, `new-session`, `send`, `stop`, `terminal`, `confirm`, `cancel`, `confirm-title`, ...). New ids are listed per task.
- State file: additive keys only, each with a serde default (`AppState` has `#[serde(default)]`).
- **Owner's pace.** Per task: write its tests, implement, run only `cargo check --workspace --all-targets`, commit. Do not run `fmt`, `clippy` or `test` per task. The full gate (`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, after `cargo build -p wai-agent --bin wai-fake-agent`) runs once, in Task 8, followed by one final review and one fix wave.
- Commits: `feat:` / `fix:` / `test:` / `docs:`, no scope, no Co-Authored-By, no Jira ref, imperative, first line under 72 chars.
- **Do not touch** (another agent builds self-update): `crates/wai/src/update.rs`, the Updates section of `settings_view.rs`, the quit-confirmation path in `workspace.rs` (R28/R29: `leave`, `Leave`, the `Quit` action handler), `.github/workflows/*`, `CLAUDE.md`. Task 2 does edit the `on_app_quit` closure, but only the three lines that stop the terminal; say so in the commit body so the merge is easy.
- **Keep `workspace.rs` edits to the terminal and editor panes.** A "sessions" track merges before this one and also edits `workspace.rs`.

## Decisions to confirm

1. **"Install in terminal" and "Continue in terminal" type into a fresh tab.** `type_in_terminal` keeps its name and callers but now opens a new tab and types there. The active tab may be the Claude TUI or a running build, and typing a shell command into it is wrong; a fresh shell is the only prompt that is certainly idle. Cost if wrong: one extra tab per use (close it with the shortcut).
2. **Shortcuts.** New tab `⌘T` (`ctrl-t`; `ctrl-shift-t` while a terminal has focus off macOS). Close tab `⌘W` while a terminal has focus (`ctrl-shift-w` off macOS). Next / previous tab `⌘⇧]` / `⌘⇧[` (terminal focused only). Cost if wrong: `⌘W` with the terminal focused used to close the focused pane, which was almost certainly accidental; it now closes the terminal tab. All four are plain keymap lines in `app.rs`.
3. **Tabs are not restored on reopen.** `terminal_visible` stays the only terminal state: a reopen shows one fresh shell, as today. A shell cannot be resumed, and its scrollback is gone with it. Cost if wrong: the owner reopens to one tab instead of three.
4. **The tab strip is always visible and `TERMINAL_HEIGHT` goes from 260 to 288.** The strip (28 px) holds the tabs and a `+`; without it new tabs are undiscoverable, and without the extra height the grid loses a row and a half. Cost if wrong: 28 px less for the panes; revert the constant and the test literal.
5. **Closing a tab asks when a job runs in it** (the pty's foreground process group is not the shell: `tcgetpgrp`), and the Claude TUI counts as one. A tab whose shell exited or sits at a prompt closes at once. At most **8** tabs (each holds a pty, two threads and up to 10,000 lines of scrollback); the 9th asks nothing and shows a notice. Cost if wrong: an extra confirm on `⌘W`, or a limit the owner hits.
6. **Tab label = the shell's title** (what the Claude TUI or the shell prompt sets), sanitized and cut to 24 characters; `Terminal N` when none. `N` is a counter that is never reused, so tabs do not renumber when one closes.
7. **Wheel: only the wheel.** In mouse-reporting mode wai sends wheel reports only; clicks and drags still select text in wai (no click or drag reporting, no shift-to-bypass). Cost if wrong: vim's `mouse=a` clicks do not move the cursor.
8. **Editor tabs return only when the saved project equals the opened one** (a launch with another folder opens none), skip files that no longer read (no notice, one log line), do not steal focus, and do not keep caret or scroll position. Scratch tabs (tool outputs) are never saved.
9. **`.gitignore` outside git: `require_git(false)`, and ancestors above the project root are not read.** Only the project's own `.gitignore` files (root and below) apply; a stray `~/.gitignore` must not hide a project under `~`. Inside a git repo nothing changes. Cost: a few lines of inherited-rule matching in `list_dir`.
10. **Hot-exit: `fsync` every recovery write on the main thread** (as the write already is), and delete a copy at launch when it is over 50 MB or does not parse (empty included). A copy that merely fails to read (an I/O error) stays, so a transient error cannot cost data. Cost: `File::sync_all` is a full flush on macOS (tens of ms) once per typing pause and per dirty tab at quit; Task 7 measures it and, if a single write passes 50 ms on this Mac, moves it to the background executor.

## Review Focus

Failure modes the work implies that no feature description names, each pinned by a test in its task:

1. **A tab's shell exits or fails to start while others live.** The tab stays with its notice, the others are untouched, and closing it asks nothing. Test: `exited_tab_closes_without_asking` (Task 3).
2. **Closing the last tab, or the active one in the middle.** The last one hides the panel and the next `ctrl-\`` starts a fresh shell; a middle one selects its neighbour, never an index out of range. Tests: `closing_the_last_terminal_hides_the_panel`, `closing_the_active_tab_selects_its_neighbour` (Task 2).
3. **Leaving `less`/`vim`.** The wheel follows the live mode: after the alternate screen ends it scrolls the history again; X10 mouse reports for columns or rows past 222 are dropped, never wrapped. Tests: `wheel_mode_follows_the_screen`, `x10_reports_past_222_are_dropped` (Task 4).
4. **A saved editor tab whose file is gone, or a recovered edit for a file also in the saved list.** No notice for the first; the second stays the dirty, recovered tab. Tests: `restore_skips_a_file_that_is_gone`, `recovered_edits_win_over_a_clean_reopen` (Task 5).
5. **A stray `.gitignore` above a project that is not in git, and a corrupt recovery copy.** The first hides nothing; the second is deleted at launch and never blocks the others. Tests: `ancestor_gitignore_is_ignored_outside_git` (Task 6), `corrupt_and_oversized_copies_are_deleted` (Task 7).

---

## File Structure

- Create `crates/wai/src/terminals.rs`: the terminal tabs' `impl Workspace`, the strip's render, `tab_label`.
- Modify `crates/wai/src/workspace.rs`: fields, `new`, the quit hook's terminal stop, `enforce`, `set_project`, `restore`, the render's terminal card and actions, `open` (split into `open_loaded`), `recover`, `save_state`, `editor_pane`'s subscription, and the tests that read `w.terminal`.
- Modify `crates/wai/src/terminal_view.rs`, `crates/wai/src/app.rs`, `crates/wai/src/main.rs` (`mod terminals;`), `crates/wai/src/editor/pane.rs`, `crates/wai/src/editor/recovery.rs`.
- Modify `crates/wai-terminal/src/lib.rs`, `crates/wai-terminal/src/keys.rs`, `crates/wai-settings/src/lib.rs`.
- Create `crates/wai-project/src/walk.rs`; modify `tree.rs`, `search.rs`, `lib.rs`.
- Modify `docs/user-guide.md`, `docs/RELATORIO-final.md` (Task 8).

---

### Task 1: Terminal model and view hooks for tabs

**Files:**
- Modify: `crates/wai-terminal/src/lib.rs`
- Modify: `crates/wai/src/terminal_view.rs`

**Interfaces:**
- Produces: `TerminalModel::busy(&self) -> bool` (a job other than the shell owns the pty's foreground; `false` off unix).
- Produces: `TerminalView::title(&self) -> Option<&str>`, `TerminalView::busy(&self) -> bool` (`false` when no shell runs), and `pub struct TerminalChanged;` with `impl EventEmitter<TerminalChanged> for TerminalView`, emitted when the title changes. The title is `None` after the shell exits or restarts.
- Consumes: `TerminalEvent::Title(String)` (already produced by the model, ignored by the view today).

- [ ] **Step 1: Tests.** In `wai-terminal` (module `tests`, unix): `busy_follows_the_foreground_job`: spawn `/bin/sh`, assert `!busy()`, write `sleep 5\r`, poll (10 ms sleeps, 5 s deadline, like `wait_for`) until `busy()`, write `\x03`, poll until `!busy()`. In `terminal_view.rs` tests: `title_is_kept_and_announced`: toggle the terminal (`ctrl-\``), subscribe to the active view's `TerminalChanged` with a counter, type `printf '\033]0;hello\007'` + enter, `wait_until` `title() == Some("hello")` and the counter is 1. Both tests use `w.terminal.clone()` as today; Task 2 renames the accessor.
- [ ] **Step 2: Implement `TerminalModel::busy`** with `MasterPty::process_group_leader()` against `Child::process_id()`: busy when the leader is `Some` and differs from the shell's pid. Gate with `#[cfg(unix)]` if the trait method is not available elsewhere in portable-pty 0.9; return `false` otherwise.
- [ ] **Step 3: Implement the view side.** In the event task of `start`, keep the last `TerminalEvent::Title` of each burst next to `exited` and pass it to `refresh(exited, title, cx)`. `refresh` stores a changed title and `cx.emit(TerminalChanged)`; on exit or `start` it clears the title (emit if it was set). Add the two getters.
- [ ] **Step 4:** `cargo check --workspace --all-targets`. Expected: no errors.
- [ ] **Step 5: Commit**

```bash
git add crates/wai-terminal/src/lib.rs crates/wai/src/terminal_view.rs
git commit -m "feat: report a terminal's title and whether a job runs in it"
```

---

### Task 2: The workspace holds several terminals

**Files:**
- Create: `crates/wai/src/terminals.rs`
- Modify: `crates/wai/src/workspace.rs` (field `terminal` at `:204`, `new` at `:411`, quit hook at `:320`, `enforce` at `:633`, `restore` at `:818`, `set_project` at `:2505`, render at `:2808-2872`, imports at `:34`/`:51`; remove `toggle_terminal` and `type_in_terminal` at `:1769-1811`; tests that read `w.terminal`)
- Modify: `crates/wai/src/app.rs` (actions, `bind_keys`, `terminal_ctrl_keymap`, its test)
- Modify: `crates/wai/src/main.rs` (`mod terminals;`), `crates/wai/src/terminal_view.rs` (tests)

**Interfaces:**
- Consumes (Task 1): `TerminalView::{title, busy}`, `TerminalChanged`.
- Produces in `terminals.rs`:
  - `pub(crate) const MAX_TERMINALS: usize = 8;`
  - `pub(crate) struct TerminalTab { pub(crate) view: Entity<TerminalView>, pub(crate) number: u32, _changed: Subscription }`; the subscription notifies the workspace on `TerminalChanged`.
  - On `Workspace` (fields `pub(crate) terminals: Vec<TerminalTab>`, `pub(crate) active_terminal: usize`, `pub(crate) next_terminal: u32` replace `terminal`): `pub(crate) fn terminal(&self) -> Option<&Entity<TerminalView>>` (the active one); `pub(crate) fn toggle_terminal(&mut self, &ToggleTerminal, window, cx)` (moved, same behaviour: the first show creates a tab; showing activates the active tab; hiding refocuses the pane); `pub(crate) fn new_terminal(&mut self, &NewTerminal, window, cx)`; `pub(crate) fn close_terminal_action(&mut self, &CloseTerminal, window, cx)`, `next_terminal_action` / `prev_terminal_action` (wrap around); `pub(crate) fn open_terminal(&mut self, window, cx) -> Option<Entity<TerminalView>>` (shows the panel, adds and activates a tab in the project folder, `None` plus a warning notification `"At most 8 terminals"` at `MAX_TERMINALS`); `pub(crate) fn select_terminal(&mut self, ix, window, cx)`; `pub(crate) fn close_terminal(&mut self, ix, window, cx)` (Task 3 adds the confirm); `pub(crate) fn type_in_terminal(&mut self, text: &str, window, cx)` (same signature, now opens a fresh tab).
- Produces in `app.rs`: actions `NewTerminal`, `CloseTerminal`, `NextTerminal`, `PrevTerminal`.

- [ ] **Step 1: Tests** (in `terminals.rs` `#[cfg(all(test, unix))]`, using `workspace::tests::{open, wait_until}`; each calls `cx.executor().allow_parking()`):
  - `new_terminal_adds_a_tab_and_shows_the_panel`: `ctrl-\`` then `secondary-t`: `terminals.len() == 2`, `active_terminal == 1`, `terminal_visible`; the numbers are `[1, 2]`.
  - `type_in_terminal_opens_a_fresh_tab`: with one tab open, call `type_in_terminal("printf wai-%s typed", ...)`; two tabs, and the new one's grid shows the text (`wait_until`); the first tab's grid does not.
  - `closing_the_last_terminal_hides_the_panel`: one tab, `close_terminal(0)`: `terminals` empty, `!terminal_visible`; `ctrl-\`` again: one tab, number 2 (the counter is not reset).
  - `closing_the_active_tab_selects_its_neighbour`: three tabs, active 1: close 1, `active_terminal == 1` (the old third); then close 1 again, `active_terminal == 0`.
  - `ninth_terminal_is_refused`: open `MAX_TERMINALS` tabs, then `new_terminal` again: still 8 and one notification.
  - `terminal_tab_numbers_are_never_reused`: open 2, close the first, open one: numbers `[2, 3]`.
  - Update `windows_terminal_keymap_table` in `app.rs` (Step 3) and the tests that read `w.terminal`: `w.terminal.clone()` becomes `w.terminal().cloned()`, `w.terminal.is_some()` becomes `!w.terminals.is_empty()`, `w.terminal.as_ref().unwrap()` becomes `w.terminal().unwrap()` (`workspace.rs` tests near `:4328`, `:4522`, `:4871`, `:5164`, `:5491`; `terminal_view.rs` tests).
- [ ] **Step 2: Implement `terminals.rs`** and move the two functions out of `workspace.rs`. `open_terminal` builds `TerminalView::new(self.shell.clone(), self.project.clone(), cx)`, subscribes with `cx.subscribe(&view, |_, _, _: &TerminalChanged, cx| cx.notify())`, pushes the tab, sets `active_terminal`, sets `terminal_visible = true`, calls `save_state`, and `activate`s the view. `close_terminal` removes the tab (dropping the view stops its shell), then: empty list means `terminal_visible = false` and `focus_pane(self.focused_pane)`; else `active_terminal = min(ix, len - 1)` and the new active view is activated. Both save state.
- [ ] **Step 3: `workspace.rs` edits, localized.**
  - Fields and `new`: `terminals: Vec::new(), active_terminal: 0, next_terminal: 1`.
  - Quit hook: replace `if let Some(terminal) = &this.terminal { terminal.update(cx, |t, _| t.stop()); }` with a loop over `this.terminals`. Do not touch anything else in that closure or in `leave`.
  - `enforce` and `set_project`: loop over all tabs for `t.shell = shell` and `t.cwd = self.project.clone()`.
  - `restore`: `if state.terminal_visible { self.toggle_terminal(...) }` is unchanged.
  - Render: `div().size_full().children(self.terminal.clone())` becomes `self.render_terminal_panel(window, cx)` (defined in Task 3; until then return `div().size_full().children(self.terminal().cloned())` from a stub of that name in `terminals.rs`). Register `.on_action(cx.listener(Self::new_terminal))`, `close_terminal_action`, `next_terminal_action`, `prev_terminal_action` next to `toggle_terminal`.
  - Imports: add the four actions.
- [ ] **Step 4: `app.rs`.** Declare the four actions in `actions!`. In `bind_keys` add `("secondary-t", NewTerminal, workspace)`, `("secondary-w", CloseTerminal, Some("Terminal"))`, `("secondary-shift-]", NextTerminal, Some("Terminal"))`, `("secondary-shift-[", PrevTerminal, Some("Terminal"))`. In `terminal_ctrl_keymap` (now `[...; 12]`): add `("ctrl-t", NoAction)` after `ctrl-w` and `("ctrl-shift-t", NewTerminal)` after `ctrl-shift-n`; `ctrl-shift-w` becomes `CloseTerminal`. Update `windows_terminal_keymap_table`'s `expected` to the same order. Every window-opening test runs `bind_keys`, which unwraps, so a key string `KeyBinding::load` rejects fails them all: confirm `]` and `[` parse.
- [ ] **Step 5:** `mod terminals;` in `main.rs`. `cargo check --workspace --all-targets`.
- [ ] **Step 6: Commit** (body: "The quit hook now stops every terminal; no change to the quit confirmation.")

```bash
git add crates/wai/src crates/wai-terminal
git commit -m "feat: hold several terminals and open one per terminal command"
```

---

### Task 3: Tab strip, close confirmation, labels

**Files:**
- Modify: `crates/wai/src/terminals.rs`
- Modify: `crates/wai/src/workspace.rs` (`TERMINAL_HEIGHT` at `:56`; test `sidebar_and_terminal_keep_260_px` compares with `px(super::TERMINAL_HEIGHT)` instead of `px(260.)`)

**Interfaces:**
- Consumes (Task 2): `TerminalTab`, `select_terminal`, `close_terminal`, `open_terminal`; (Task 1) `title`, `busy`.
- Produces: `fn tab_label(number: u32, title: Option<&str>) -> String` (title with control characters removed and whitespace trimmed, cut to 24 characters with `…`, else `Terminal {number}`); `Workspace::render_terminal_panel(&self, window, cx) -> impl IntoElement` (the strip over the active view); `close_terminal` now asks first.
- New test ids: `terminal-tab-<i>`, `terminal-tab-close-<i>`, `new-terminal`. The existing `terminal` selector stays on the card.

- [ ] **Step 1: Tests.**
  - `tab_label_cleans_the_title` (pure): `(1, None)` gives `Terminal 1`; `(2, Some("  \u{7}build "))` gives `build`; a 40-character title gives 23 characters then `…`; `(3, Some("   "))` gives `Terminal 3`.
  - `strip_selects_adds_and_closes`: `ctrl-\`` then click `new-terminal`: ids `terminal-tab-0` and `terminal-tab-1` are drawn, the second active; click `terminal-tab-0`: `active_terminal == 0`; click `terminal-tab-close-0`: one tab left.
  - `closing_a_tab_with_a_running_job_asks`: in the active tab type `sleep 30` + enter, `wait_until` `busy()`; click its close: `confirm-title` shows `"A command is still running"` and the tab count is unchanged; click `cancel`: unchanged; click close again then `confirm`: closed.
  - `exited_tab_closes_without_asking`: type `exit` + enter, `wait_until` the view has no shell (as `exited_shell_restarts_in_current_project` does), click its close: no dialog, the tab is gone.
  - `tab_label_follows_the_title`: print an OSC title as in Task 1; the strip's tab label (`window.find("terminal-tab-0").label()`) is `hello`.
- [ ] **Step 2: Implement the strip.** A 28 px row above the view inside the existing card (so the `terminal` selector and the 5 px card padding stay): a tab per `TerminalTab` (label from `tab_label`, active one highlighted with the Aurora tokens used by the editor's tab strip in `editor/pane.rs`, truncated, a small `×` button), then a `+` button at the end. Click on a tab: `select_terminal` (activates and focuses the view). `TERMINAL_HEIGHT` becomes `288.`.
- [ ] **Step 3: Implement the confirm.** `close_terminal(ix)`: when `tabs[ix].view.read(cx).busy()`, `self.confirm("A command is still running".into(), "Closing this terminal ends it. Close it anyway?", "Close terminal", move |w, window, cx| w.remove_terminal(ix', window, cx), ...)`, where the closure re-finds the tab by its `number` (an index can shift while the dialog is open) and does nothing if it is gone. `confirm` is the existing helper at `workspace.rs:2030`. The shortcut and the `×` both go through `close_terminal`.
- [ ] **Step 3b:** `cargo check --workspace --all-targets`.
- [ ] **Step 4: Commit**

```bash
git add crates/wai/src/terminals.rs crates/wai/src/workspace.rs
git commit -m "feat: add a terminal tab strip and ask before closing a running one"
```

---

### Task 4: Wheel inside `less`, `vim` and mouse-aware programs (3.5)

**Files:**
- Modify: `crates/wai-terminal/src/keys.rs`, `crates/wai-terminal/src/lib.rs` (export, `wheel_mode`)
- Modify: `crates/wai/src/terminal_view.rs` (`on_scroll`, `select`, remove the stale `ponytail` note about the wheel)

**Interfaces:**
- Produces in `wai-terminal`: `pub enum WheelMode { History, Arrows { app_cursor: bool }, Mouse { sgr: bool } }` (derive `Debug, Clone, Copy, PartialEq, Eq`); `pub fn wheel_bytes(mode: WheelMode, lines: i32, col: u16, row: u16) -> Vec<u8>` (`lines > 0` is the wheel moving up, as `on_scroll` computes today; `col`/`row` are 0-based visible cells; `History` returns empty); `TerminalModel::wheel_mode(&self) -> WheelMode`.
- Rule for `wheel_mode` (alacritty's): any of `TermMode::MOUSE_MODE` gives `Mouse { sgr: SGR_MOUSE }`; else `ALT_SCREEN` with `ALTERNATE_SCROLL` (on by default) gives `Arrows { app_cursor: APP_CURSOR }`; else `History`.
- Encoding: `Arrows`: `|lines|` copies of `ESC [ A` (up) or `ESC [ B` (down), `ESC O A/B` when `app_cursor`. `Mouse`: per line one report, button 64 (up) or 65 (down), at 1-based `col + 1`, `row + 1`; SGR is `ESC [ < 64 ; c ; r M`; X10 is `ESC [ M` then bytes `32 + 64 (+1)`, `32 + c`, `32 + r`, and a report whose `c` or `r` is over 223 is dropped.

- [ ] **Step 1: Tests.**
  - `keys.rs` (pure): `arrows_follow_app_cursor` (`lines = 2` gives `\x1b[A\x1b[A`; `-1` with `app_cursor` gives `\x1bOB`); `sgr_wheel_reports` (`Mouse { sgr: true }`, `lines = 1`, `col = 4`, `row = 9` gives `\x1b[<64;5;10M`; `-1` gives `...65...`); `x10_wheel_reports` (`\x1b[M` + `[96, 37, 42]` for the same cell); `x10_reports_past_222_are_dropped` (`col = 223` gives empty); `history_sends_nothing`.
  - `lib.rs` (real `/bin/sh`): `wheel_mode_follows_the_screen`: `History`; `printf '\033[?1049h'; echo wai-alt` then wait for `wai-alt` then `Arrows { app_cursor: false }`; `printf '\033[?1049l'` returns to `History`; `printf '\033[?1000h\033[?1006h'` gives `Mouse { sgr: true }`; `\033[?1000l` back to `History`.
  - `terminal_view.rs` (real shell): `wheel_in_the_alternate_screen_sends_arrows`: run `printf '\033[?1049h'; cat`, scroll up one line over the grid (as `wheel_scrolls_back_through_history` does), `wait_until` a visible row contains `^[[A` (the tty echoes the control bytes `cat` receives). `wheel_in_mouse_mode_sends_reports`: `printf '\033[?1000h\033[?1006h'; cat`, scroll up one line, a row contains `^[[<64;`. `wheel_without_modes_still_scrolls_history`: the existing `wheel_scrolls_back_through_history` keeps passing.
- [ ] **Step 2: Implement** the three items in `wai-terminal` (re-export from `lib.rs`).
- [ ] **Step 3: Implement the view.** Factor the cell mapping out of `select` into `fn cell_at(&self, at: Point<Pixels>) -> (u16, u16)` (clamped to the grid) and reuse it. `on_scroll`: when `lines != 0`, match `model.wheel_mode()`: `History` keeps `model.scroll(lines)` and `refresh`; otherwise `self.write(&wheel_bytes(mode, lines as i32, col, row), cx)` with `(col, row) = cell_at(event.position)`. The pixel remainder logic is unchanged.
- [ ] **Step 4:** `cargo check --workspace --all-targets`.
- [ ] **Step 5: Commit**

```bash
git add crates/wai-terminal crates/wai/src/terminal_view.rs
git commit -m "feat: send the wheel to programs in the alternate screen or mouse mode"
```

---

### Task 5: Editor tabs come back on reopen (3.4)

**Files:**
- Modify: `crates/wai-settings/src/lib.rs` (`AppState` at `:466`, its `Default`, tests)
- Modify: `crates/wai/src/editor/pane.rs` (`EditorPane` events)
- Modify: `crates/wai/src/workspace.rs` (`save_state` at `:866`, `restore`, `recover` at `:835`, `open` at `:2113`, `editor_pane` at `:2237`)

**Interfaces:**
- Produces in `AppState`: `open_files: Vec<PathBuf>` (the editor tabs in order, scratch tabs excluded) and `active_file: Option<PathBuf>`; both default empty/`None`.
- Produces in `editor/pane.rs`: `pub struct TabsChanged;` with `impl EventEmitter<TabsChanged> for EditorPane`, emitted by `add`, `activate` and `remove`.
- Produces in `workspace.rs`: `fn open_loaded(&mut self, path: PathBuf, loaded: editor::Loaded, line: Option<u32>, scratch: bool, window, cx)` (the body of `open`'s success branch, extracted unchanged: recovery file, `FileEditor::new`, `add_tab`; the `demo` timing stays); `fn restore_files(&mut self, files: Vec<PathBuf>, active: Option<PathBuf>, window, cx)`; `recover` takes `(files: Vec<PathBuf>, active: Option<PathBuf>, window, cx)` and calls `restore_files` after it has added the recovered tabs.
- Ordering rule: recovered (dirty) tabs are added first, so a path that has a recovered tab is skipped by `restore_files`; a clean reopen would otherwise shadow the copy and leave it for a launch that never comes.

- [ ] **Step 1: Tests.**
  - `wai-settings`: `state_without_open_files_still_loads` (JSON from before: `{"project": "/p"}` loads with `open_files` empty and `active_file` `None`); `open_files_round_trip`.
  - `workspace.rs` (`open_at` with a temp `state.json`, files in a temp dir canonicalized like the other tests; wait with `wait_until`):
    - `editor_tabs_come_back_on_reopen`: open two files via `open_file`, `save_state`, then a second `open_at` with the same state file whose project is the same: after `launch`, the editor pane has the same two paths in order, the saved active one active, and `focused_pane` is where it was.
    - `restore_ignores_the_files_of_another_project`: state saved with project `/other`: no editor pane.
    - `restore_skips_a_file_that_is_gone`: one of two files deleted before the reopen: one tab, no notification.
    - `recovered_edits_win_over_a_clean_reopen`: a dirty tab saved by `keep_edits` and listed in `open_files`: after the reopen exactly one tab for it, `dirty` and showing the recovered banner.
    - `scratch_tabs_are_not_saved`: a tool-output tab is absent from `AppState::load(path).open_files`.
    - `closing_a_tab_saves_the_state`: open two, close one: the state file lists one (no quit needed).
- [ ] **Step 2: Implement `AppState`'s keys** and their defaults.
- [ ] **Step 3: Implement `TabsChanged`** and subscribe in `editor_pane` next to `Emptied`: `this.save_state(cx)`.
- [ ] **Step 4: Implement `save_state` and `restore_files`.** `save_state` fills the two keys from the editor pane (`tab.read(cx).scratch` excluded; `active` is the active tab's path). `restore` reads `state.open_files` and `state.active_file` only when `state.project.as_deref() == Some(self.project.as_path())`, and passes them to `recover`. `restore_files` skips paths that already have a tab, loads the rest with `editor::load` on the background executor (one task, in order), drops failures with one `eprintln!`, adds the rest through `open_loaded`, then reveals `active` if open, and finally refocuses the pane that had focus before (the editor pane takes the first empty pane or a new one on the right, so earlier indices still hold).
- [ ] **Step 5:** `cargo check --workspace --all-targets`.
- [ ] **Step 6: Commit**

```bash
git add crates/wai-settings/src/lib.rs crates/wai/src/editor/pane.rs crates/wai/src/workspace.rs
git commit -m "feat: reopen the editor tabs of the last run"
```

---

### Task 6: `.gitignore` outside a git repository (3.11)

**Files:**
- Create: `crates/wai-project/src/walk.rs`
- Modify: `crates/wai-project/src/lib.rs`, `crates/wai-project/src/tree.rs` (`list_dir`), `crates/wai-project/src/search.rs` (`run`)

**Cause (verified):** `ignore`'s `WalkBuilder` defaults to `require_git(true)`: `.gitignore` files apply only inside a git repository. Both `list_dir` and the search's `run` build their walker the same way.

**Interfaces:**
- Produces in `walk.rs`: `pub(crate) fn walker(start: &Path, root: &Path, global_ignores: bool) -> WalkBuilder` (hidden files shown, `git_global(global_ignores)`, `current_dir(root)`, `require_git(false)`, and `parents(false)` when no ancestor of `root` (itself included) holds `.git`); `pub(crate) fn inherited(root: &Path, dir: &Path) -> Vec<ignore::gitignore::Gitignore>` (the `.gitignore` of each folder from `root` down to, excluding, `dir`; empty inside a repo, where `parents(true)` already does this); `pub(crate) fn hidden_by(inherited: &[Gitignore], path: &Path, is_dir: bool) -> bool` (deepest rule that matches wins; `matched_path_or_any_parents`; a whitelist un-ignores).
- `list_dir`/`run` keep their public signatures and callers.

- [ ] **Step 1: Tests** (temp dirs with no `.git`; the temp dir's ancestors have none):
  - `tree.rs` `gitignore_applies_outside_git`: root `.gitignore` is `target/\n*.log\n`; `list_dir(root, root)` leaves out `target` and `a.log`; a `src/.gitignore` (`gen/`) hides `src/gen` when listing `src`; listing `src` also honours the root's `*.log` (the `inherited` path); a negation `!keep.log` in `src/.gitignore` shows `src/keep.log`.
  - `tree.rs` `ancestor_gitignore_is_ignored_outside_git`: `<tmp>/.gitignore` is `*.txt`, the project is `<tmp>/proj` with `a.txt`: `a.txt` is listed.
  - `tree.rs`: the existing `tree_lists_lazily_and_respects_gitignore` and `global_ignores_follow_the_flag` still hold (they use a `.git`).
  - `search.rs` `search_respects_gitignore_outside_git`: no `.git`, `.gitignore` with `target/`, a match in `target/out.rs` and one in `a.rs`: only `a.rs`; and the ancestor case: `<tmp>/.gitignore` `*.rs`, project `<tmp>/proj`: the `a.rs` match is found.
- [ ] **Step 2: Implement `walk.rs`** and switch `list_dir` and `run` to `walker(...)`. `list_dir` computes `inherited(root, dir)` once and drops entries for which `hidden_by` is true, after the walker's own filtering. Update the doc comment on `list_dir` ("inside a git repo" becomes "inside the project").
- [ ] **Step 3:** `cargo check --workspace --all-targets`.
- [ ] **Step 4: Commit**

```bash
git add crates/wai-project
git commit -m "fix: respect .gitignore in a project outside a git repository"
```

---

### Task 7: Hot-exit hardening (3.12)

**Files:**
- Modify: `crates/wai/src/editor/recovery.rs` (`write`, `read_entry`, the `ponytail` note on `write`, tests)

**Interfaces:**
- `recovery::write(file, entry)` and `recovery::load(dir)` keep their signatures.
- `write`: `sync_all()` the temp file before the rename (replace the "no fsync" `ponytail` note with the reason now: a power loss must not leave an empty entry). The directory is not synced: losing the rename leaves the previous entry.
- `read_entry`: an entry over `MAX_BYTES` is deleted (log one line); an entry that does not parse (including an empty file) is deleted (log one line); an I/O error while reading the metadata or the bytes leaves the file and logs. Update the doc comment of `load` accordingly.

- [ ] **Step 1: Tests.** `corrupt_and_oversized_copies_are_deleted`: in a temp `recovery/`, `0.json` valid, `1.json` empty, `2.json` `{"path":`, `3.json` sparse 51 MB (as `oversized_recovery_is_ignored` makes it): `load` returns only `0.json`'s entry, `1`, `2` and `3` no longer exist, `0.json` does. Change `oversized_recovery_is_ignored` to assert the file is now deleted (its comment says "left alone"). `unreadable_copy_is_left` (unix): a `4.json` with mode `000`: skipped, still exists (skip the assertion when running as root). `write_leaves_a_complete_entry`: `write` then read the file back and compare with the entry (guards the temp-file path after the `sync_all` edit).
- [ ] **Step 2: Implement both edits.**
- [ ] **Step 3: Measure** once on this Mac with a throwaway `#[test] #[ignore]` or a scratch run: time `write` of a 100 KB entry. If it passes 50 ms, move `keep_or_warn`'s write to the background executor and say so in the commit; if not, leave it. Record the number in the commit body. Delete any throwaway code.
- [ ] **Step 4:** `cargo check --workspace --all-targets`.
- [ ] **Step 5: Commit**

```bash
git add crates/wai/src/editor/recovery.rs
git commit -m "fix: fsync hot-exit copies and delete the ones that never come back"
```

---

### Task 8: Docs, the gate, the review

**Files:**
- Modify: `docs/user-guide.md` (line `:11`, `:15`, the "The terminal" section from `:216`, the shortcuts table `:240-256`), `docs/RELATORIO-final.md` ("Lacunas conhecidas", "Uso")

Do not change the version strings in either file: the release process owns them.

- [ ] **Step 1: `user-guide.md`.** The terminal now has tabs (strip, `+`, `×`, labels, the 8-tab limit, closing a running tab asks); its keys: new tab `⌘T` / `ctrl-t` (`ctrl-shift-t` in the terminal off macOS), close `⌘W` (`ctrl-shift-w`), next/previous `⌘⇧]` / `⌘⇧[`; "Install in terminal" and "Continue in terminal" open a new tab; a relaunch shows one fresh terminal. The wheel works in `less` and `vim`. Line `:15`: editor tabs are restored (same project only), and unsaved ones as before. The shortcuts table gets the new rows; the `⌘W` row's terminal note changes.
- [ ] **Step 2: `RELATORIO-final.md`.** Remove: the "tema e `default_agent` do projeto novo não" half of the folder-switch gap (keep nothing for 3.10), the two hot-exit gaps (fsync, copies that do not come back), the editor-tabs and `.gitignore`-outside-git lines of "Editor e busca", and the terminal wheel line if listed. Leave the other gaps.
- [ ] **Step 3: Commit docs.**

```bash
git add docs/user-guide.md docs/RELATORIO-final.md
git commit -m "docs: describe terminal tabs and the restored editor tabs"
```

- [ ] **Step 4: The gate, once.**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
df -h /System/Volumes/Data          # stop if under 8 GB free
cargo build -p wai-agent --bin wai-fake-agent
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: all pass. Fix what fails in small `fix:` or `test:` commits; formatting goes in one `refactor: format` commit if needed.
- [ ] **Step 5: Final review.** One reviewer on the whole branch diff, checking the Global Constraints (no change to `update.rs`, the Updates section, the quit confirmation, workflows, `CLAUDE.md`), the Review Focus tests, the idle-CPU rule (no timer or poll was added), and that `workspace.rs` changes stay in the terminal and editor panes.
- [ ] **Step 6: One fix wave** for the review's findings, one commit per fix, then re-run Step 4 once. `cargo clean` in the worktree.

---

## Self-review

- Coverage: 3.3 (Tasks 1-3), 3.5 (4), 3.4 (5), 3.11 (6), 3.12 (7), docs and gate (8); 3.10 dropped with its reason.
- Names used across tasks: `terminal()`, `terminals`, `active_terminal`, `next_terminal`, `TerminalTab`, `MAX_TERMINALS`, `open_terminal`, `select_terminal`, `close_terminal`, `type_in_terminal`, `TerminalChanged`, `TabsChanged`, `open_loaded`, `restore_files`, `WheelMode`, `wheel_bytes`, `wheel_mode`, `walker`, `inherited`, `hidden_by`: each is defined in the task that produces it and used only in later ones.
- Known risks: `workspace.rs` conflicts with the sessions track (moved functions, `open`, `save_state`, `restore`, render); the `on_app_quit` edit sits next to the self-update agent's area; `process_group_leader` availability in portable-pty 0.9 on every target (gate with `cfg(unix)`); `KeyBinding::load` for `secondary-shift-]`.
