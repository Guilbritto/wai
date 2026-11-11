# wai Onda 0/1, sessions track (0.8, 1.1, 1.3, 1.5): Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stabilise the two flaky tests (0.8), ask before ending sessions that are still working (1.1), add "Continue in terminal" (1.3), and replace "New session starts at once" with the new-session draft (1.5).

**Architecture:** 0.8 gives the fake agent a file gate on `session/new`, so tests that look at the "Preparing" state no longer race the agent. 1.1 adds one `Workspace::confirm` dialog helper, which 1.3 and 1.5 reuse. 1.3 adds ACP `session/close` to `wai-agent` and types `cd … && claude --resume …` into the terminal. 1.5 adds three files, so `workspace.rs` (the conflict hot spot) grows little: `git.rs` holds every git call (blocking, run off the UI thread), `chat/draft.rs` holds `DraftView` (the draft screen and its chips), and `start.rs` holds `impl Workspace` for opening, discarding and sending drafts. On send, the agent is spawned and a `ChatSession` with an empty id (the "pending chat") shows the user's bubble and one step line per action. It joins `Workspace::sessions` only once `session/new` succeeds. Any failure or Esc before that returns the pane to the untouched draft.

**Tech Stack:** Rust workspace, GPUI + gpui-component 0.9 (`Select`/`SelectState` with custom `SelectItem::render`, `SearchableVec<SearchableGroup<_>>`, `window.open_dialog`), ACP via `agent-client-protocol` 5 (schema 1.16: `CloseSessionRequest`), the `git` CLI through `std::process::Command`.

**Spec:** `docs/superpowers/specs/2026-11-11-wai-new-session-draft-design.md` (1.5, binding). Mockup: `.superpowers/brainstorm/44632-1794399351/content/new-session-draft-v2.html`. Master plan rows 0.8, 1.1, 1.3, 1.5: `docs/superpowers/plans/2026-11-11-wai-master-plan.md`.

## Global Constraints

Copied from `.superpowers/sdd/master-onda-1/global-constraints.md` (binding):

- Repo: /Users/guilherme.brito/Project/Guilherme/wai (Rust workspace, GPUI + gpui-component, ACP). Base: branch `development` at 172c454.
- Every shell: `export PATH="$HOME/.cargo/bin:$PATH"` (background shells have no cargo on PATH).
- Disk is tight (~33 GB free on /System/Volumes/Data). Run `df -h /System/Volumes/Data` before a build; if under 8 GB free, stop and report. `cargo clean` in your worktree when done.
- Never `cargo run` with a fake HOME. Tests use temp dirs; never the developer's real config, gitconfig or shell rc. Real Claude only in #[ignore] smoke tests or explicit validation work.
- Do NOT touch /Applications/wai.app or the running wai process (it hosts the owner's live sessions, including this one). Never kill `wai`, `claude-agent-acp` or `claude` processes you did not start.
- Another session may also commit to this repo: `git log` before branching/merging. `crates/wai/src/workspace.rs` (~4.9k lines) is the conflict hot spot.
- Idle CPU < 0.5%, streaming CPU < 10%. Nothing animated or polling in the core when idle.
- UI strings stay in English. Aurora theme tokens/spec: docs/superpowers/specs/2026-11-09-wai-visual-aurora-design.md.
- Never break existing test ids (`tab-*`, `sidebar-*`, `new-session`, `send`, `stop`, permission/question button ids).
- The core never touches user credentials and opens no ports.
- Follow existing patterns; no unrequested abstractions; no comments/docs unless they carry a non-obvious why.
- Work in a git worktree under `.claude/worktrees/<name>` on a branch `feat/<name>` created from `development`.
- Commit messages: `feat:` / `fix:` / `test:` / `docs:` / `refactor:` with NO scope, NO Co-Authored-By line, NO Jira refs. Imperative, first line < 72 chars.
- No push (there is no remote). Do not merge into `development`; the controller does that.
- Gate (before saying done): `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (check docs/building.md for the project's exact gate and Go tests under extensions/ if you touch them).

Added for this track:

- **Gate command (G)**, run at the end of every task:
  ```sh
  export PATH="$HOME/.cargo/bin:$PATH"
  df -h /System/Volumes/Data          # stop if under 8 GB free
  cargo build -p wai-agent --bin wai-fake-agent
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```
  A single test: `cargo test -p wai <test_name>` (wai is a binary crate; its tests live in `src`).
- **Branches.** Items merge in this order: `feat/flaky-tests` (Task 1), `feat/confirm-end` (Task 2), `feat/continue-in-terminal` (Task 3), `feat/session-draft` (Tasks 4–12). Create each branch from `development` once the controller has merged the item before it. If that merge has not happened yet, branch from the previous item's branch and say so in the report: Task 3 uses Task 2's `confirm`, and Task 5 uses Task 1's `start_now`. Run `git log` first.
- **No timers, no polling** in the product. Git data is read when a draft opens and when its folder changes, never on a timer. Each fake-agent gate in tests polls a file every 10 ms, but only in the test binary.
- **Git env.** Every git invocation goes through the single builder `git::command(dir)`, which sets `GIT_TERMINAL_PROMPT=0` and null stdin and uses the user's PATH. Under `#[cfg(test)]` it also sets `GIT_CONFIG_GLOBAL` (`/dev/null`, `NUL` on Windows) and `GIT_CONFIG_NOSYSTEM=1`, so no test reads the developer's gitconfig. wai never stashes, resets, cleans or forces a checkout.
- **state.json.** Only additive keys, each with a serde default. Drafts are never written to it.
- **New test ids (1.5):** `draft-folder`, `draft-workspace`, `draft-branch`, `draft-agent`, `draft-mode`, `draft-model`, `draft-notice`, `draft-install`, `draft-row-<n>`, `draft-<n>`, `draft-close-<n>`, `pill-folder`, `pill-branch`, `pill-agent`. Dialogs: `confirm-title`, `confirm`, `cancel`. Keep `send` and `stop` on the draft's and the pending chat's buttons.
- **Copy is fixed by this plan.** Strings in quotes below are exact; the spec's and mockup's strings are used verbatim where they exist.

## Review Focus

The five inputs most likely to bite that the spec does not name, each pinned by a test in its task:

1. **The draft's folder disappears before send** (deleted, unmounted, renamed). Send keeps the draft, its text and its choices, and shows "The folder <path> no longer exists." Test: `send_to_a_missing_folder_keeps_the_draft` (Task 5).
2. **Repository edge cases:** a detached HEAD, a repo with no `main`/`master`/`origin/HEAD`, a subfolder of a repo. Chips stay sane, the base falls back, and a worktree session opens at the same subfolder. Tests: `detached_head_has_no_current_branch`, `default_branch_falls_back` (Task 4); `worktree_keeps_the_subfolder` (Task 9).
3. **Esc after the worktree exists but before the agent answers.** No session starts, the worktree stays, the chips point at it, and the next send does not create another. Test: `escape_after_the_worktree_keeps_it_for_the_retry` (Task 9).
4. **Quitting while a draft is still starting** (git step or `session/new` in flight). ⌘Q asks as it does for a working session. Test: `starting_draft_counts_as_working` (Task 8).
5. **Shell metacharacters in what wai types into the terminal**, from the folder path or the session id. Both are quoted. Test: `resume_command_quotes_the_folder_and_id` (Task 3).

## Decisions (planner's calls)

Choices the owner and the spec did not make, each with its cost if wrong:

| # | Decision | Cost if wrong |
|---|---|---|
| D1 | 0.8: the fake agent's new `--new-session-gate <path>` holds `session/new` until the file exists. Tests that assert the Preparing state then create it. | None (test-only). |
| D2 | 0.8: `crash_loop_stops_after_three` is fixed by evidence (Task 1 Part B). Expected outcome: its 10 s guard is a hang guard on four process starts, so it becomes 30 s. "Under load" means `hw.ncpu / 2` busy `yes` loops. | If the evidence is misread and there is a real hang, the flake stays. Part B forbids raising the guard without timestamps that show every event arriving. |
| D3 | 1.1: a session counts as "still working" when its turn runs, Working **or NeedsYou** (what `stop()` would cancel). A draft whose send is starting also counts. | One extra dialog when closing a session that waits for you. Flip one `matches!` in `ChatSession::busy`. |
| D4 | 1.1: quit and window close ask about unsaved tabs first, then about working sessions (two dialogs in a row). Drafts never ask at quit; the spec does not keep them. | A long draft is lost on ⌘Q. |
| D5 | 1.1 copy, Quit: title "1 session is still working" / "N sessions are still working", body "Quitting ends their turns. End them anyway?", button "Quit anyway". Close window: same title, body "Closing the window ends their turns. End them anyway?", button "Close anyway". Row ×: title "This session is still working", body "Ending it stops its turn. End it anyway?", button "End session". | Wording only. |
| D6 | 1.3: the ACP session id **is** the Claude Code session id. Checked in claude-agent-acp 0.103.0 `dist/v1/acp-agent.js` `createSession`: a new session gets `sessionId = randomUUID()` and passes `options.sessionId = sessionId` to the SDK; load/resume reuse the id; a fork gets a fresh uuid, also passed as `options.sessionId`. **One exception:** leaving plan mode with "clear context" (`dist/clear-context-coordinator.js`, `publicSessionId`) restarts Claude Code under a new random id while the ACP id stays. After that flow, `--resume <id>` opens the conversation from before the clear. Task 3 adds an `#[ignore]` smoke test that pins the normal case. | In the clear-context flow, the terminal resumes the pre-clear conversation. Upgrade path: pick the newest `~/.claude/projects/<cwd>/*.jsonl` written after the clear. Not done: it couples wai to Claude's file layout. |
| D7 | 1.3: `end_session` sends ACP `session/close` whenever the agent advertises it (the adapter advertises `close: {}`). This applies to every Open-row ×, not only to Continue in terminal. It frees the adapter's per-session Claude process and keeps two Claudes off one `.jsonl`. | None expected. Reopening from History loads from disk, as for any closed session. |
| D8 | 1.3: wai types `cd '<cwd>' && claude --resume '<id>'` with POSIX quoting, because the running shell may be in another folder. The menu item shows only for agent id `claude`. | Windows shells (CI only) get POSIX syntax. A Claude agent configured under another id has no item. |
| D9 | 1.5 file split: `git.rs`, `chat/draft.rs`, `start.rs`. `Pane::Preparing` stays for `start_session` (extensions' `sessions/open`, and tests). | None. |
| D10 | 1.5 "pending chat": the agent process spawns at send (`registry.handle`), and a `ChatSession` with an empty id shows the bubble and the steps at once. If `session/new` answers after Esc, wai closes that session in the agent (D7) and never shows it. While starting, Send in the pending chat is off; text typed there goes back into the draft on revert. | An agent process may start for a send whose git step then fails. That is harmless: it is the same process a Test starts. |
| D11 | Step lines are ordinary `ChatItem::Tool` rows (kind `other`) with plain-text titles (no backticks), so the processing modes apply to them (Hidden hides them). The "✦ <Agent>" header stays above the first agent message, not above the steps; 1.8 reworks this view. | Looks a little different from the mockup until 1.8. |
| D12 | Slug: lower-case Unicode alphanumerics (`char::is_alphanumeric`), the first 4 words, at most 40 chars, joined by `-`; "session" when empty. | Branch names can carry accents ("adicionar-validação"). The rule lives in one fn. |
| D13 | Worktree from a subfolder `<root>/a/b`: the session's cwd is `<worktree>/a/b`. | None. |
| D14 | No `origin` remote: no fetch and no step line. Default branch: `origin/HEAD`, else `main`, else `master`, else the current branch, else `HEAD`. Detached HEAD: the Local chip reads "Detached HEAD". | None. |
| D15 | "Uncommitted files" = every `git status --porcelain` entry, untracked included. The chip and the guard use the same count, and the guard re-checks in the background right before checkout. | The guard refuses a switch the user considers safe (untracked files only). The answer is New worktree. |
| D16 | Submodules (controller's ruling): `git::worktree_blocked(&Repo)` plus `const SUBMODULE_HINT` is the only place the rule lives. | None; change it there. |
| D17 | Known folders: the project, then open sessions newest first, then History as the agent lists it, deduplicated by `canonical`. Pushed into drafts by `Workspace::sync_drafts` (on draft open, history refresh, a session landing, settings and extension changes). | A folder can appear late if History loads after the menu opened. |
| D18 | Draft rows sit at the top of the ungrouped Open rows, in creation order. They never join groups, and the session rows keep their `open-*` indexes. | Order only. |
| D19 | Agent glyphs from the mockup: ✳ Claude, ◎ Codex, ◉ Copilot, ▲ Antigravity; a generic icon for others. | Cosmetic. |
| D20 | The model chip is hidden until the agent has reported models (`models_seen`). The mode chip is hidden until `modes_seen` has the agent (spec). With no remembered model and no Settings model, send sets nothing and the agent's own default applies. | The chip does not name the model the agent will pick on its own. |
| D21 | `last_models` and `last_bases` are written at send ("Last used"), not at pick. | A pick in a discarded draft is not remembered. |
| D22 | Pane header: the three pills are added, and the existing model/effort/mode pickers stay. Pills shrink and truncate before the title disappears. | A crowded header in four panes. Owner may ask to drop one. |
| D23 | "How to install": a URL opens it; any other text is revealed under the notice. Extension agents get no button. | None. |
| D24 | Clone: the destination is `<parent>/<repo name from URL>`. An existing destination is refused, and so is a URL that starts with `-`. It runs `git clone -- <url> <dest>` with `GIT_TERMINAL_PROMPT=0` and no ssh overrides, so the user's ssh and credential setup applies. | A URL with an unusual name gets an odd folder name. |
| D25 | Menu footers are a final disabled row. | Cosmetic. |
| D26 | ⌘W on a draft pane hides it; the draft stays in Open, as a session does. | None. |
| D27 | Error copy. Dirty checkout: "Can't switch to <branch>: <folder> has <N> uncommitted files. Pick New worktree to work on <branch> without touching them." Shared folder: "Can't switch to <branch>: another open session works in <folder>. Pick New worktree instead." git refusal: "git checkout <branch> failed: <git's last line>". Worktree: "Could not create the worktree: <git's last line>". Unknown agent: "No agent named <agent> in the settings or extensions." Agent start: "Could not start a <agent> session: <error>". Sign-in: the existing `signed_out(agent)`. | Wording only. |
| D28 | A draft's × asks "Discard this draft?" when it has text **or** attachments. The spec asks only for text; attachments alone would be lost silently. | One extra dialog for an attachment-only draft. |

## File map

| File | Responsibility | Tasks |
|---|---|---|
| `crates/wai-agent/src/bin/wai-fake-agent.rs` | `--new-session-gate`, `--close-file` | 1, 3 |
| `crates/wai-agent/src/{types.rs,actor.rs}` | `AgentCaps::close`, `AgentHandle::close_session`, `program_name` | 3, 7 |
| `crates/wai-ext/tests/host.rs` | crash-loop guard | 1 |
| `crates/wai-settings/src/lib.rs` | `AppState::{last_models,last_bases,modes_seen}`, `SeenModes` | 11 |
| `crates/wai/src/git.rs` (new) | every git call; `Repo`, `Head`, `Worktree`; `worktree_blocked` | 4, 9, 10, 12 |
| `crates/wai/src/chat/draft.rs` (new) | `DraftView`, `DraftEvent`, `DraftData`, chips and menus | 5, 7, 8, 9, 11, 12 |
| `crates/wai/src/start.rs` (new) | `impl Workspace`: `open_draft`, `show_draft`, `close_draft`, `discard_draft`, `send_draft`, `sync_drafts`, `known_folders` | 5–9, 11 |
| `crates/wai/src/chat/{view.rs,input.rs,model.rs,render.rs,mod.rs}` | `busy`, pending chat, steps, `send_with`, title fallback, pills | 2, 5, 8, 10 |
| `crates/wai/src/workspace.rs` | `Pane::Draft`, `drafts`, `confirm`, `land`, `end_session` close, `continue_in_terminal`, test helpers | 1, 2, 3, 5, 6, 8 |
| `crates/wai/src/sidebar.rs` | draft rows, row × asking, "Continue in terminal" | 2, 3, 6, 8 |
| `crates/wai/src/{palette.rs,topbar.rs,app.rs,main.rs}` | entry points, top-bar count, demo script, `mod git; mod start;` | 4, 5, 8 |

---

### Task 1: 0.8, deterministic Preparing tests and the crash-loop test

**Files:**
- Modify: `crates/wai-agent/src/bin/wai-fake-agent.rs` (doc list at the top, `Opts`, `parse_args`, the `NewSessionRequest` handler)
- Test: `crates/wai-agent/tests/fake_agent.rs`
- Modify (tests only): `crates/wai/src/workspace.rs` (`mod tests`), `crates/wai/src/ext_host.rs` (`new_session_takes_over_an_extensions_preparing_pane`)
- Modify (test only): `crates/wai-ext/tests/host.rs` (`crash_loop_stops_after_three`)

**Interfaces:**
- Produces, in `workspace::tests`, both `pub(crate)`:
  - `fn gated(flags: &str, dir: &Path) -> (Settings, PathBuf)`: `fake_settings(&format!("{flags} --new-session-gate {}", quote(dir.join("gate"))))` and the gate path.
  - `fn start_now(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<ChatSession>`: calls `w.start_session(w.default_agent().unwrap(), None, false, None, window, cx)`, waits until `sessions.len()` grew by one, and returns `sessions[n]`. Later tasks use it wherever a test needs a session and not the entry point.

**Root cause (Part A).** `simulate_keystrokes` ends with `run_until_parked()`. With `allow_parking`, the fake agent's `session/new` reply can arrive on its own thread while that run is still going. The Preparing pane then turns into a session before the test asserts `Pane::Preparing`, and the second ⌘N starts a second session. The same race exists in `events_before_the_session_lands_are_replayed` and `new_session_takes_over_an_extensions_preparing_pane`. One flag in the shared fake fixes all three.

- [ ] **Step 1: Write the failing fake-agent test** in `crates/wai-agent/tests/fake_agent.rs`:

```rust
#[test]
fn new_session_waits_for_its_gate() {
    run(async {
        let dir = tempfile::tempdir().unwrap();
        let gate = dir.path().join("gate");
        let (agent, rx) = spawn(&["--new-session-gate", gate.to_str().unwrap()]);
        ready(&rx).await;
        let pending = agent.new_session(cwd(), vec![]);
        let held = smol::future::or(async { pending.await.ok().map(|_| ()) }, async {
            smol::Timer::after(Duration::from_millis(300)).await;
            None
        });
        assert_eq!(held.await, None, "session/new answered before the gate opened");
        std::fs::write(&gate, "").unwrap();
        assert!(agent.new_session(cwd(), vec![]).await.is_ok());
    });
}
```
(The first future is dropped. The second `new_session` proves the agent answers once the file exists; the dropped request's reply is ignored.)

- [ ] **Step 2: Run it.** `cargo test -p wai-agent --test fake_agent new_session_waits_for_its_gate`. Expected: FAIL (the fake panics on the unknown flag, so `ready` sees `Exited`).

- [ ] **Step 3: Implement `--new-session-gate <path>`** in the fake. Add the doc line "`--new-session-gate <path>`: `session/new` answers only once `<path>` exists (checked every 10 ms)." and the field `new_session_gate: Option<PathBuf>`. In the `NewSessionRequest` handler, move the body (respond, then the `available_commands_update`) into `cx.spawn(async move { … })`, as the prompt handler does, so the dispatch loop is not held. Before responding, `while !gate.exists() { smol::Timer::after(Duration::from_millis(10)).await }`. Without the flag, the handler behaves as today.

- [ ] **Step 4: Run it.** Same command. Expected: PASS.

- [ ] **Step 5: Make the three wai tests deterministic.** Add `gated` and `start_now` to `workspace::tests`, then rewrite:
  - `new_session_appears_in_open_list`: `gated("", tmp)`. Call `start_session("fake", None, false, None)` directly, not ⌘N, so the test keeps its meaning after Task 5. Assert `Pane::Preparing { agent: "fake", .. }`. Call it again and assert `w.starting.get("fake") == Some(&1)`, the second start waiting. Write the gate. `wait_until` a session shows in pane 0, then assert `sessions.len() == 1` and `panes[0].session() == sessions.first()`.
  - `events_before_the_session_lands_are_replayed`: `gated`, `start_session` directly, same assertions as today. Write the gate before `wait_until`.
  - `ext_host.rs` `new_session_takes_over_an_extensions_preparing_pane`: build `fake` with `gated` ("slow" wraps it and shares the gate). Keep ⌘N (Task 5 rewrites this test). Write the gate after the `Pane::Preparing … "fake"` assertion.

  Run: `cargo test -p wai new_session_appears_in_open_list events_before_the_session_lands_are_replayed new_session_takes_over_an_extensions_preparing_pane`. Expected: PASS. Then run each 30 times in a loop (`for i in $(seq 30); do cargo test -p wai <name> -q || break; done`). Expected: 30 passes.

- [ ] **Step 6: Commit.** `git commit -m "test: gate the fake agent's session/new so Preparing tests cannot race"`

**Part B, `crash_loop_stops_after_three`** (use superpowers:systematic-debugging; evidence before any fix):

- [ ] **Step 7: Reproduce with timestamps.** Temporarily (not committed), give the test an `Instant` and `eprintln!("{:?} {event:?}", start.elapsed())` per received event. Load half the cores and keep their pids:
  ```sh
  n=$(( $(sysctl -n hw.ncpu) / 2 )); : > /tmp/wai-load.pids
  for i in $(seq $n); do yes > /dev/null & echo $! >> /tmp/wai-load.pids; done
  for i in $(seq 30); do cargo test -p wai-ext --test host -- --nocapture 2>&1 | grep -E "Ready|Crashed|timed out|test result"; done
  kill $(cat /tmp/wai-load.pids)
  ```
  (Kill only the pids in that file. Run the whole `host` binary, not the single test: the sibling tests are part of the load.)
- [ ] **Step 8: Decide from the output.**
  - **(a) Expected:** every run shows 4 `Ready` and 1 `Crashed` in order, and a failing run just crosses 10 s. The guard is a hang guard on four process starts. Change that test's `run(Duration::from_secs(10), …)` to 30 s, with a one-line comment: "a hang guard on four process starts, not a speed limit".
  - **(b)** An event never arrives (a generation stuck after `Ready`). Stop and debug which process holds the crashed child's stdout (`lsof -p <test pid>` during the hang). Report to the controller before changing `host.rs`: the fix belongs in the spawn lock (`SPAWN`, `host.rs:38`), not in the test.
- [ ] **Step 9: Remove the instrumentation and commit (a).** `git commit -m "test: give the crash-loop test a hang guard that survives load"`

- [ ] **Step 10: 0.8 done-check, 20 suite runs under load** (long: run it in the background and report the count):
  ```sh
  cargo build -p wai-agent --bin wai-fake-agent && cargo test --workspace --no-run
  n=$(( $(sysctl -n hw.ncpu) / 2 )); : > /tmp/wai-load.pids
  for i in $(seq $n); do yes > /dev/null & echo $! >> /tmp/wai-load.pids; done
  pass=0; for i in $(seq 20); do cargo test --workspace -q > /tmp/wai-run-$i.log 2>&1 && pass=$((pass+1)) || break; done
  kill $(cat /tmp/wai-load.pids); echo "passed $pass/20"
  ```
  Expected: `passed 20/20`. If another test fails, fix it in this task only when its cause is the same class (a wall-clock budget on work that grows with load, or a race against a real process); otherwise report it to the controller with its log.
- [ ] **Step 11: Gate G.** `cargo clean` when the item is done.

---

### Task 2: 1.1, confirm before ending sessions that are still working

**Files:**
- Modify: `crates/wai/src/chat/view.rs` (`busy`; `stop` uses it)
- Modify: `crates/wai/src/workspace.rs` (`confirm`, `still_working`, `busy_sessions`, `close_session`; `proceed`; the `on_window_should_close` closure at ~299; tests)
- Modify: `crates/wai/src/sidebar.rs` (the `open-close` × calls `close_session`)
- Modify (test): `crates/wai/src/chat/view.rs` `close_drops_session_and_pane_and_ends_its_turn`

**Interfaces:**
- Consumes: `start_now` (Task 1).
- Produces:
  - `ChatSession::busy(&self) -> bool`: state is `Working | NeedsYou` (D3).
  - `Workspace::busy_sessions(&self, cx: &App) -> usize` (Task 8 adds starting drafts).
  - `Workspace::confirm(&mut self, title: String, body: &'static str, confirm: &'static str, then: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static, window: &mut Window, cx: &mut Context<Self>)` opens a dialog in the style of `leave`'s: title in a `div().debug_selector("confirm-title").aria_label(title)`, `body` as text, and a footer with `Button::new("cancel")` ("Cancel") and `Button::new("confirm").danger()` labelled `confirm`. `then` is kept in an `Rc` and runs on Confirm after `close_dialog`.
  - `pub(crate) fn still_working(n: usize) -> String`: "1 session is still working" / "{n} sessions are still working".
  - `Workspace::close_session(&mut self, session: &Entity<ChatSession>, window, cx)`: busy → `confirm("This session is still working", "Ending it stops its turn. End it anyway?", "End session", end_session)`; otherwise `end_session` at once. `end_session` itself stays unconditional.

- [ ] **Step 1: Write the failing tests** in `workspace::tests`. Settings: `fake_settings("--reply 'a b c d e f g h i j' --chunk-delay-ms 1000")`, so a sent turn stays Working for 10 s. A session becomes Working at once with `session.update(cx, |s, cx| s.send_text("go", cx))`. Helpers: `asked = cx.debug_bounds("confirm-title").is_some()`; the title via `window.find_by_label(..)` (`TestQueryExt`).
  - `quit_asks_while_a_session_works`. With the session idle, `secondary-q` opens no dialog. After `send_text`, `secondary-q` shows `find_by_label("1 session is still working")`. `click("cancel")` closes the dialog and the session's status is still `TurnState::Working`.
  - `closing_the_window_asks_while_a_session_works`: `assert!(!cx.simulate_close())` and the title shows; `click("cancel")` leaves the window open; `assert!(!cx.simulate_close())` again, then `click("confirm")` leaves `cx.cx.update(|app| app.windows().is_empty())` true.
  - `open_row_close_asks_while_its_session_works`: `send_text`, then click `("open-close", 0usize)` → asked with title "This session is still working"; `cancel` → `sessions.len() == 1`; click × again → `confirm` → `sessions.is_empty()`.
  - `idle_session_row_close_does_not_ask`: idle → × → no dialog, `sessions.is_empty()`.
- [ ] **Step 2: Run them.** `cargo test -p wai quit_asks_while closing_the_window_asks open_row_close_asks idle_session_row_close`. Expected: the first three FAIL (no dialog); the fourth passes already.
- [ ] **Step 3: Implement.**
  - `busy`, with `stop()` rewritten to `if self.busy() { … }`.
  - `proceed`: `Leave::Open` → `set_project`, as today. For `Leave::Quit`/`Leave::Close`: when `busy_sessions == 0`, quit or `remove_window` at once; otherwise `confirm(still_working(n), "Quitting ends their turns. End them anyway?", "Quit anyway", |_, _, cx| cx.quit())`, or for Close the body "Closing the window ends their turns. End them anyway?", the label "Close anyway" and `window.remove_window()`.
  - The window-close closure: `let ask = !dirty.is_empty() || busy > 0; if ask { this.leave(Leave::Close, window, cx) } !ask`.
  - sidebar `open-close` → `close_session`.
  - `quit_stops_agents` stays as is. It drives `app.shutdown()`, the platform path (Dock, logout) that never asks.
- [ ] **Step 4: Update `close_drops_session_and_pane_and_ends_its_turn`.** Its session is NeedsYou, so after the × click it now asks: add `click("confirm", cx)` (import `click` from `workspace::tests`) before the assertions.
- [ ] **Step 5: Run.** The four new tests, the updated one, and the editor's `quit_and_close_ask_about_unsaved_edits`. Expected: PASS.
- [ ] **Step 6: Gate G, then commit.** `git commit -m "feat: confirm before ending sessions that are still working"`

---

### Task 3: 1.3, close ACP sessions and "Continue in terminal"

**Files:**
- Modify: `crates/wai-agent/src/types.rs` (`AgentCaps::close`), `crates/wai-agent/src/actor.rs` (`Command::CloseSession`, `close_session`, caps at ~616)
- Modify: `crates/wai-agent/src/bin/wai-fake-agent.rs` (`--close-file <path>`)
- Test: `crates/wai-agent/tests/fake_agent.rs` (existing caps literal gains `close: false`; new test), `crates/wai-agent/tests/real_claude.rs` (new `#[ignore]` test)
- Modify: `crates/wai/src/workspace.rs` (`end_session`, `continue_in_terminal`, `resume_command`, tests), `crates/wai/src/sidebar.rs` (`session_menu`)

**Interfaces:**
- Consumes: `Workspace::confirm`, `ChatSession::busy` (Task 2); `start_now` (Task 1).
- Produces:
  - `AgentCaps { …, close: bool }`, from `caps.session_capabilities.close.is_some()`.
  - `AgentHandle::close_session(&self, session: SessionId) -> Result<(), AgentError>` (async; sends `acp::CloseSessionRequest::new(session)` and maps the reply to `()`).
  - `pub(crate) fn resume_command(cwd: &Path, id: &str) -> String` = `format!("cd {} && claude --resume {}", wai_agent::quote(cwd), wai_agent::quote(id))`.
  - `Workspace::continue_in_terminal(&mut self, session: &Entity<ChatSession>, window, cx)`.

- [ ] **Step 1: Failing fake-agent test** `close_session_reaches_the_agent`: `spawn(&["--close-file", file])`; `ready` has `caps.close == true`; `new_session`, then `close_session(id).await.is_ok()`; the file's content is `"fake-1\n"`. Also add `close: false` to the caps literal in `streams_agent_text_and_ends_turn`.
- [ ] **Step 2: Run** `cargo test -p wai-agent --test fake_agent close_session`. Expected: FAIL (it does not compile: no `close_session`, no `close` field).
- [ ] **Step 3: Implement** the caps field, the `CloseSession` command and handle method, and in the fake: doc line "`--close-file <path>`: advertise `session/close`; each closed id is appended to `<path>`, one per line." Advertise via `SessionCapabilities::new()…close(acp::SessionCloseCapabilities::new())` only with the flag. Handle `acp::CloseSessionRequest` by appending the id and responding `CloseSessionResponse::new()`.
- [ ] **Step 4: Run** the fake-agent tests. Expected: PASS.
- [ ] **Step 5: Failing wai tests** (`workspace::tests`):
  - `resume_command_quotes_the_folder_and_id` (plain `#[test]`): `resume_command(Path::new("/tmp/it's here"), "abc") == r"cd '/tmp/it'\''s here' && claude --resume 'abc'"`.
  - `ending_a_session_closes_it_in_the_agent`: fake with `--close-file`; `start_now`; click `("open-close", 0)` (idle, so it does not ask); `wait_until` the file contains "fake-1".
  - `continue_in_terminal_ends_the_session_and_types_the_resume`. Settings: the agent id must be `claude`, run by the fake with `--close-file`: `settings.agents = [("claude", fake)]`, `default_agent = Some("claude")`. `start_now`, then `menu_item(("open", 0usize), "Continue in terminal", cx)`. Assert: `sessions.is_empty()`, the close file has "fake-1", `terminal_visible`, and `wait_until` a terminal snapshot row contains `claude --resume 'fake-1'` (read rows as `quit_signals_agents_before_the_terminal` does).
  - `continue_in_terminal_is_only_for_claude`: agent `fake`; right-click `("open", 0)`; `window.try_find_by_label("Continue in terminal").is_none()`.
- [ ] **Step 6: Run** them. Expected: FAIL.
- [ ] **Step 7: Implement.**
  - `end_session`: after `stop()`, when the registry status is `Ready(caps)` with `caps.close`, take the handle and `cx.background_spawn(async move { handle.close_session(id).await })`. Log an `Err` with `eprintln!`. Do not await before the pane goes.
  - `continue_in_terminal`: let `run = move |w, window, cx| { w.end_session(&s, window, cx); w.type_in_terminal(&resume_command(&cwd, &id), window, cx) }`. Busy → `confirm("This session is still working", "Ending it stops its turn. End it anyway?", "Continue in terminal", run)`; otherwise `run` now.
  - `session_menu`: `.when(agent == "claude", |m| m.item(item(workspace, "Continue in terminal", …)))` after "Fork".
- [ ] **Step 8: Run** them. Expected: PASS.
- [ ] **Step 9: Smoke test** (not part of G) in `real_claude.rs`, `#[ignore = "needs network, npx and a Claude Code login"]` `new_session_id_names_the_claude_code_transcript`. In a temp dir: `new_session`, prompt "Reply with the word ok.", wait for `TurnEnded`. Then assert some `$HOME/.claude/projects/*/<id>.jsonl` exists (read_dir over the project dirs). It only reads a file name; it never writes there. Run it once yourself and paste the result into the report: `cargo test -p wai-agent --test real_claude new_session_id_names -- --ignored --nocapture`.
- [ ] **Step 10: Gate G, then commit.** `git commit -m "feat: continue a Claude session in the terminal"`

---

### Task 4: 1.5, the git module

**Files:**
- Create: `crates/wai/src/git.rs` (with `#[cfg(test)] mod tests`)
- Modify: `crates/wai/src/main.rs` (`mod git;`)

**Interfaces (produces; all blocking, so callers run them on the background executor):**

```rust
pub(crate) const SUBMODULE_HINT: &str = "Pick the submodule's folder to work in a worktree";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Repo {
    pub root: PathBuf,             // canonical `rev-parse --show-toplevel`
    pub current: Option<String>,   // None: detached HEAD
    pub default: String,           // D14 fallback chain
    pub branches: Vec<String>,     // local, `for-each-ref --sort=-committerdate`
    pub dirty: usize,              // `status --porcelain` entries (D15)
    pub submodules: bool,          // root/.gitmodules exists
    pub origin: bool,              // an `origin` remote exists
}
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Head { pub branch: Option<String>, pub worktree: bool }   // worktree: git-dir != common-dir
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Worktree { pub path: PathBuf, pub branch: String, pub from: String, pub fetch_failed: Option<String> }

pub(crate) fn command(dir: &Path) -> std::process::Command;   // the only builder (Global Constraints)
pub(crate) fn repo(folder: &Path) -> Option<Repo>;            // None: not a repo, or no git
pub(crate) fn head(folder: &Path) -> Option<Head>;
pub(crate) fn worktree_blocked(repo: &Repo) -> Option<&'static str>; // D16: the submodule rule
pub(crate) fn slug(text: &str) -> String;                     // D12
pub(crate) fn create_worktree(repo: &Repo, slug: &str, base: &str) -> Result<Worktree, String>;
pub(crate) fn checkout(folder: &Path, branch: &str) -> Result<(), String>; // refuses when dirty (D27 copy)
```

`create_worktree`, in order: (1) when `repo.origin`, `git fetch origin <base>`; a failure becomes `fetch_failed = Some(<git's last line>)`, never an `Err`. (2) `from` = `origin/<base>` when `rev-parse --verify --quiet refs/remotes/origin/<base>` succeeds, else `<base>`. (3) Pick the first free `n` (none, `-2`, `-3`, …) where both `refs/heads/wai/<slug><n>` and `<root>/.worktrees/<slug><n>` are free. (4) When `git check-ignore -q .worktrees/` fails, append `.worktrees/` to `<git-common-dir>/info/exclude` (create `info/` if missing; never twice). (5) `git worktree add -b <branch> <path> <from>`. An error carries git's last stderr line. Error strings follow D27.

Test helper, in `#[cfg(test)] pub(crate) mod tests` so Task 9's tests in `start.rs` can use it: `fn git(dir: &Path, args: &[&str]) -> String` runs `command(dir)` with `-c user.name=wai -c user.email=wai@example.invalid -c commit.gpgsign=false`, asserts success and returns stdout. Plus `fn repo_with(dir: &Path, branches: &[&str]) -> PathBuf`: `git init -b main` with one commit, then one commit per extra branch. The branch created last has the newest commit.

- [ ] **Step 1: Failing tests** (plain `#[test]`, temp dirs):
  - `slug_takes_the_first_words`: `slug("Add a tag filter to the energy dashboard!") == "add-a-tag-filter"`; `slug("  ?? ") == "session"`; `slug("Adicionar validação de CPF") == "adicionar-validação-de-cpf"`; `slug(&"x".repeat(60)).len() == 40`.
  - `not_a_repository_is_none`: `repo(tempdir) == None`.
  - `repo_reads_branches_dirty_and_default`: `repo_with(["feature"])`, checked out on `feature`, one modified tracked file and one untracked file → `current == Some("feature")`, `branches[0] == "feature"`, `branches.contains("main")`, `dirty == 2`, `default == "main"`, `!origin`, `!submodules`.
  - `detached_head_has_no_current_branch`: `git checkout --detach` → `current == None`.
  - `default_branch_falls_back`: a repo whose only branch is `trunk` → `default == "trunk"`.
  - `default_branch_prefers_origin_head`: a bare origin whose HEAD is `development`, cloned → `default == "development"`, `origin`.
  - `submodules_block_worktrees`: write `<root>/.gitmodules` → `worktree_blocked(&repo(root).unwrap()) == Some(SUBMODULE_HINT)`; without the file → `None`.
  - `worktree_starts_from_the_fetched_base`: origin gets a new commit C2 on `main` after the clone. `create_worktree(&repo, "x", "main")` → `path == root/.worktrees/x`, `branch == "wai/x"`, `from == "origin/main"`, `fetch_failed == None`, and `git rev-parse HEAD` in `path` equals C2. `<common-dir>/info/exclude` contains `.worktrees/` exactly once (and still once after a second create). `git status --porcelain` in root is empty.
  - `worktree_name_clash_gets_a_suffix`: create "x" twice → the second is `.worktrees/x-2` on `wai/x-2`.
  - `offline_fetch_starts_from_the_local_base`: `git remote set-url origin <missing path>` → `fetch_failed.is_some()`, `from == "main"`, `path.is_dir()`.
  - `ignored_worktrees_dir_leaves_exclude_alone`: `.gitignore` holds `.worktrees/` → after create, `info/exclude` does not contain it.
  - `checkout_refuses_uncommitted_files`: dirty → `Err(e)` with `e.contains("uncommitted files")`, current branch unchanged; clean → `Ok`, `repo(root).current == Some("feature")`.
  - `head_tells_a_worktree_apart`: `head(root) == Some(Head { branch: Some("main"), worktree: false })`; for a created worktree, `worktree: true`, `branch: Some("wai/x")`.
- [ ] **Step 2: Run** `cargo test -p wai git::`. Expected: FAIL (it does not compile).
- [ ] **Step 3: Implement** `git.rs` to the interfaces above. A private `fn run(dir, args) -> Result<String, String>` returns trimmed stdout, or `Err(last stderr line, else the exit status)`.
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Gate G, then commit.** `git commit -m "feat: git helpers for new-session drafts"`

---

### Task 5: 1.5, the draft pane, every entry point, and a basic send

**Files:**
- Create: `crates/wai/src/chat/draft.rs`, `crates/wai/src/start.rs`
- Modify: `crates/wai/src/chat/{mod.rs,input.rs,view.rs}`, `crates/wai/src/workspace.rs`, `crates/wai/src/{palette.rs,topbar.rs,sidebar.rs,main.rs,app.rs}`
- Modify (test migration): `workspace.rs`, `chat/view.rs`, `agents_view.rs`, `ext_host.rs`, `sidebar.rs`, `editor/mod.rs` tests

**Interfaces:**
- Consumes: `start_now` (Task 1).
- Produces:
  - `pub enum Pane { …, Draft(Entity<DraftView>) }` and `Pane::draft(&self) -> Option<&Entity<DraftView>>`.
  - `Workspace.drafts: Vec<Entity<DraftView>>` ("the Open list's drafts, in the order opened; never saved").
  - In `chat/draft.rs`:
    ```rust
    pub struct DraftView { pub agent: String, pub folder: PathBuf, pub model: Option<String>,
        pub notice: Option<String>, pub starting: bool /* Task 8: Option<Entity<ChatSession>> */,
        pub(super) input: Entity<TextareaState>, pub(super) attachments: Vec<Attachment>, … }
    pub enum DraftEvent { Send, Changed }
    impl DraftView {
        pub fn new(agent: String, folder: PathBuf, model: Option<String>, prompt: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self;
        pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>);
        pub fn text(&self, cx: &App) -> String;                         // trimmed
        pub fn can_send(&self, cx: &App) -> bool;                       // !starting && !text.is_empty()
        pub(crate) fn message(&self, cx: &App) -> (String, Vec<Attachment>); // clones; the draft keeps both
        pub fn fail(&mut self, error: Option<String>, cx: &mut Context<Self>); // back to editing, notice = error
    }
    ```
  - In `chat/mod.rs`: `mod draft; pub use draft::{DraftEvent, DraftView};` and `pub use input::Attachment` (now `#[derive(Clone)]`); `ChatSession::send_with(&mut self, text: String, attachments: Vec<Attachment>, cx)` (it builds a `Message` and calls `send_message`); `pub(super)` helpers pulled out of `render_prompt` and `paste`, which both the chat and the draft use: `send_button(disabled: bool, cx) -> Button` (id `send`), `attach_button() -> Button` (id `attach`), `paste_into(attachments: &mut Vec<Attachment>, item: &ClipboardItem, window, cx) -> bool`. `DraftView::pick_files` mirrors `ChatSession::pick_files` (files and folders, several at once).
  - In `start.rs`: `Workspace::open_draft(&mut self, agent: String, prompt: Option<String>, window, cx)`, `Workspace::send_draft(&mut self, draft: Entity<DraftView>, window, cx)`.
  - In `workspace.rs`: `fn land(&mut self, session: Entity<ChatSession>, early: Vec<SessionEvent>, model: Option<String>, window, cx)`. It replays `early`, inserts `models_seen[agent]` from `session.model.config` when non-empty, applies `model.or(settings.models[agent])` with `apply_default_model`, calls `add_session`, and calls `refresh_history`. `start_session`'s `Ok` branch is rewritten to use it, with no behavior change.

**Behavior.**
- `new_session` (⌘N, top bar, the Sessions tab double click, palette "New session") calls `open_draft(default, None)`; with no default it opens Settings with `CHOOSE_AI`, as today.
- Palette "New session with X" calls `open_draft(X, None)`. `start_template` and prompt-only `run_ext_command` call `open_draft(agent, Some(prompt))`. A template with an unknown agent keeps today's `Pane::Empty` error. Extensions' `sessions/open` keeps `start_session`.
- `open_draft` creates the draft with `folder = self.project` and `model = settings.models[agent]`, subscribes to it (`Send` → `send_draft`; `Changed` → `cx.notify()`), pushes it to `drafts`, sets `panes[session_pane()] = Pane::Draft(..)` and focuses it. It never touches the registry.
- `focus_pane` gives `Pane::Draft` the caret (`draft.focus`). `render` draws it. `follow_session` ignores drafts (they are not `session()`).
- Draft screen (mockup; Aurora tokens). Header (`aurora::card_bar`): "New session", then a dashed `aurora::pill` (`border_dashed()`) reading "Draft · starts when you send". Body, centered at max width 640 px: "What are we building?" in `DISPLAY_FONT` at 26 px bold, the muted line "Pick where and with whom. Nothing starts until you send.", the prompt box, and the hint row "⏎ start session", "⇧⏎ new line", "`START_KEY` another draft". The prompt box: a notice area (id `draft-notice`, shown when `notice` is set; the text in `theme.danger`), the textarea (`auto_grow(3, 12)`, placeholder "Pitch your idea", submit on Enter, Shift+Enter a new line), the attachment chips, and a bar holding `attach_button`, a spacer and `send_button(!can_send)`. Paste and drop attach files, as in the chat.
- `send_draft` (basic; Tasks 8 and 9 extend it):
  1. Return unless `can_send`.
  2. If `!folder.is_dir()`: `fail(Some("The folder <shown> no longer exists."))`.
  3. `registry.handle(agent)`; `None` → `fail(Some(D27 unknown agent))`.
  4. Set `starting = true`, clear `notice`, emit `Changed`; `*starting[agent] += 1`.
  5. Spawn `handle.new_session(folder, mcp)`, then `landed()`.
  6. On `Ok`: `mark_ready`; create the `ChatSession` and call `land(session, early, draft.model)`, then `session.send_with(text, attachments)`. The pane showing the draft, if any, becomes `Pane::Session`; drop the draft from `drafts`; focus and `follow_session` when that pane is focused; `save_state`.
  7. `Err(AuthRequired)`: registry `SignedOut`, `fail(Some(signed_out(agent)))`. Any other `Err(e)`: `fail(Some(D27 agent start))`.
- Demo script (`app.rs` `send_prompt`, feature `demo`): wait for `Pane::Draft` in pane 0, type the prompt, dispatch Enter. Verify with `cargo check -p wai --features demo`.

- [ ] **Step 1: Failing tests** (`workspace::tests` unless noted):
  - `new_session_opens_a_draft_without_starting_anything`: `fake_settings("--pid-file <tmp>/pid")`. `secondary-n` → `matches!(panes[0], Pane::Draft(_))`, `drafts.len() == 1`, `sessions.is_empty()`, `registry.status("fake").is_none()`; after `run_until_parked`, the pid file does not exist.
  - `every_entry_point_opens_a_draft`. Each of these adds one draft and the registry still has no status for `fake`: click `sidebar-new`; double click `tab-sessions`; `w.start_template(ix)` for a template `("t", Some("fake"), "Write tests")`, whose draft text is "Write tests"; `w.run_ext_command(&ext, CommandSpec { id: "review".into(), title: "Review".into(), prompt: Some("Review".into()) })`, whose draft text is "Review".
  - `palette.rs` tests: `palette_new_session_with_opens_a_draft`: `run_entry(Entry::NewSessionWith("b"))` gives a draft whose `agent == "b"`.
  - `new_session_twice_gives_two_drafts`: ⌘N ⌘N → `drafts.len() == 2`, `panes[0].draft() == Some(&drafts[1])`.
  - `sending_a_draft_starts_one_session_in_its_folder`: ⌘N, `simulate_input("hello")`, `enter`. `wait_until` `sessions.len() == 1`, then: `drafts.is_empty()`, `panes[0].session() == sessions.first()`, `session.cwd == canonical(temp_dir)`, the first `ChatItem::User` text is "hello", and `wait_until` an Agent item reads "Hello from fake agent".
  - `failed_start_keeps_the_draft`: `fake_settings("--require-auth")`. ⌘N, attach one file (`draft.update(|d, cx| d.attachments.push(Attachment::new(&file)))`), type "hello", Enter. `wait_until` `notice == Some(signed_out("fake"))`; then `!starting`, `text == "hello"`, `attachments.len() == 1`, `sessions.is_empty()`, the pane still shows the draft.
  - `send_to_a_missing_folder_keeps_the_draft`: `d.folder = <tmp>/gone` → after Enter, `notice == Some("The folder <tmp>/gone no longer exists.")` (via `shown`), `registry.status("fake").is_none()`.
  - `drafts_are_not_saved` (`open_at` with a temp state path, then `launch`): ⌘N, type "x", `w.save_state(cx)` → `AppState::load(path).open_sessions.is_empty()`. Relaunch (second `TestAppContext`, `open_at` + `launch`) → `drafts.is_empty()` and `matches!(panes[..], [Pane::Empty(None)])`.
  - `no_default_opens_settings_on_new_session` stays unchanged and must still pass.
- [ ] **Step 2: Run** them. Expected: FAIL (no `Pane::Draft`).
- [ ] **Step 3: Implement** the interfaces and behavior above.
- [ ] **Step 4: Migrate the tests whose subject is a session, not the entry point**, from ⌘N to `start_now` (or, where the test needs the Preparing state, to a direct `start_session` without waiting):
  - `workspace.rs`: `choosing_a_default_starts_it` (keep the Ready wait; then ⌘N must give a draft whose agent is "fake"), `hide_while_preparing_still_opens_the_session` (`start_session` then `secondary-w`, both before any run), `lone_empty_pane_has_no_close`, `registry_reports_auth_required`, `successful_session_clears_signed_out` (both starts), `state_saved_on_new_session`, `extension_opens_never_replace_user_sessions`.
  - `chat/view.rs`: the `start_session` helper's body becomes `start_now`; `landing_session_keeps_focus_on_focused_pane` uses `start_session` without waiting.
  - `agents_view.rs`: the `new_session` helper uses `start_now`.
  - `ext_host.rs`: `mcp_servers_passed_to_new_session` (both), `navegador_mcp_reaches_new_session`, `sessions_list_and_prompt`, `session_events_follow_permissions` use `start_now`. `new_session_takes_over_an_extensions_preparing_pane` is rewritten: ⌘N in pane 1 now gives `Pane::Draft` there (drop the gate; nothing races), the extension's session still opens (`sessions.len() == 1`, in no pane, prefilled "theirs"), and `panes[1]` is still the draft.
  - `sidebar.rs`: `double_click_on_sessions_tab_starts_a_session` is renamed `double_click_on_sessions_tab_opens_a_draft` and asserts `drafts.len() == 1`, `sessions.is_empty()`; `names_and_groups_survive_a_relaunch` uses `start_now`.
  - `editor/mod.rs` `new_session_keeps_the_editor_pane`: ⌘N gives `[Pane::Editor(_), Pane::Draft(_)]` (a draft never takes the editor pane).
- [ ] **Step 5: Run** `cargo test -p wai`. Expected: PASS. Also `cargo check -p wai --features demo`.
- [ ] **Step 6: Gate G, then commit.** `git commit -m "feat: new session opens a draft that starts on send"`

---

### Task 6: 1.5, drafts in the Open list

**Files:** Modify `crates/wai/src/sidebar.rs`, `crates/wai/src/start.rs`, `crates/wai/src/workspace.rs` (`Dragged`, `drop_session`, `lit`), `crates/wai/src/chat/draft.rs` (header drag).

**Interfaces:**
- Consumes: `Workspace::confirm` (Task 2), `Pane::Draft`, `drafts` (Task 5).
- Produces: `Dragged::Draft(Entity<DraftView>)`; `Workspace::show_draft(&mut self, draft: Entity<DraftView>, side: bool, window, cx)` (the `show` rules: focus its pane if one shows it, else the session pane or a side pane); `Workspace::close_draft(&mut self, draft: &Entity<DraftView>, window, cx)` (empty text and no attachments → `discard_draft`, else `confirm("Discard this draft?", "What you typed is lost.", "Discard", discard_draft)`); `Workspace::discard_draft(&mut self, draft: &Entity<DraftView>, window, cx)` (leaves `drafts`; the pane showing it goes with `drop_pane`).

**Behavior.** `render_sessions` draws the draft rows before `rows(None)` (D18). A draft row is `row(active)` with id `("draft-row", ix)`. Inside it, `h_flex` id `("draft", ix)` holds an 8 px dot with a dashed `nevoa` border and no fill, "New session" truncating, and the tag "Draft" (`text_xs`, muted). A click calls `show_draft(d, false)`; `on_drag` carries `Dragged::Draft(d)` titled "New session". The ×, `Button ("draft-close", ix)` (tooltip "Discard draft"), calls `close_draft`. `active` = the focused pane shows it. The draft's header title drags like a session's (R89). `drop_session` and `lit` treat `Dragged::Draft` like `Dragged::Open`: `from` is the pane showing it, and `Slot::Dropped` builds `Pane::Draft`. `reorder` and `group_session` ignore drafts.

- [ ] **Step 1: Failing tests** (`sidebar.rs` tests):
  - `draft_row_shows_and_discards`. ⌘N gives the label `("draft", 0)` == "New session". `start_now` then puts a session in pane 0. Clicking `("draft", 0)` brings the draft back into pane 0 (`panes[0].draft().is_some()`). With empty text, `("draft-close", 0)` removes it without a dialog (`drafts.is_empty()`, `!dialog`). ⌘N, type "x", × → `find_by_label("Discard this draft?")`; `cancel` keeps it; × → `confirm` → gone, and pane 0 is no longer a draft.
  - `draft_drags_to_the_right_edge` (reuse `drag_to_pane` from `workspace::tests`, made `pub(crate)`): ⌘N ⌘N, drag `("draft", 0)` to pane 0 at x = 0.9 → `panes.len() == 2`, `panes[1].draft() == Some(&drafts[0])`, `focused_pane == 1`.
  - `session_rows_keep_their_indexes_next_to_drafts`: `start_now`, then ⌘N → `label(("open", 0))` is the session's title and `("draft", 0)` exists.
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the sidebar tests and `cargo test -p wai drag_`. Expected: PASS.
- [ ] **Step 5: Gate G, then commit.** `git commit -m "feat: drafts in the Open list"`

---

### Task 7: 1.5, folder, agent and model chips with the installed check

**Files:** Modify `crates/wai/src/chat/draft.rs`, `crates/wai/src/start.rs`, `crates/wai/src/workspace.rs` (`sync_drafts` calls), `crates/wai-agent/src/actor.rs` (+ `lib.rs` export).

**Interfaces:**
- Consumes: Task 5's `DraftView`, `open_draft`, `send_draft`, `land(model)`.
- Produces:
  - `#[derive(Clone, Default)] pub struct DraftData { pub known: Vec<PathBuf>, pub agents: Vec<String>, pub commands: BTreeMap<String, String>, pub models: BTreeMap<String, Vec<SeenModel>>, pub default_models: BTreeMap<String, String> }`. Task 11 adds fields. `DraftView::new` takes `data: DraftData` instead of `model`; `DraftView::set_data(&mut self, data: DraftData, cx)`.
  - `Workspace::draft_data(&self, cx: &App) -> DraftData`, `Workspace::known_folders(&self, cx: &App) -> Vec<PathBuf>` (D17), `Workspace::sync_drafts(&self, cx)`. It is called from `open_draft`, after `refresh_history` sets `history`, in `add_session`, `apply_settings` and `sync_contributions`, and wherever `models_seen` changes.
  - `DraftView::set_folder(&mut self, folder: PathBuf, cx)` (canonical; emits `Changed`; never calls `set_project`), `set_agent(&mut self, agent: String, cx)` (resets `model` to `default_models[agent]`), `set_model(&mut self, model: Option<String>, cx)`, `installed(&self) -> Option<bool>` (the chosen agent's).
  - None of these take a `Window`: `Select` subscriptions get none. Each marks the menus stale, and `render` rebuilds their items with `set_items`/`set_selected_value`, as `ChatSession::sync_pickers` does with `pickers_stale`.
  - `wai_agent::program_name(command: &str) -> Option<PathBuf>`: the parsed command's program as written (`AcpAgent::from_str(command).ok()?.into_config().command()`).

**Behavior** (strip above the prompt box; chips styled as `aurora::pill` with a chevron; menus are `Select` with `appearance(false)`, custom `SelectItem::render`, `menu_max_h` within the window, closing on Esc, on a pick and on a click outside):
- **Folder** `draft-folder`: icon `Folder`, the folder's name. The menu (searchable, placeholder "Search projects") lists `known` (name plus its parent path in `nevoa`, the current one checked), then "Open folder…", which calls `cx.prompt_for_paths` (directories) and then `set_folder`. A pick calls `set_folder`. The workspace's project, title, Files and terminal do not change.
- **Agent** `draft-agent`: the D19 glyph plus `agent_label`. The menu lists `data.agents` in order; rows whose program is missing say "Not installed"; footer "Default agent comes from Settings → Agents." (D25). A pick calls `set_agent`.
- **Installed check:** on `new`, on `set_data` and on `set_agent`, `cx.background_spawn` checks `wai_agent::program(command).is_none()` for every agent in `data.commands` and stores a `HashMap<String, bool>`. Not installed shows in `draft-notice`: title "<Label> is not installed", text "wai runs <Label> through its CLI and could not find <program_name> on your PATH.". For built-in agents only, a "How to install" button `draft-install` (D23): `builtin(id).install` starting with `https://` calls `cx.open_url`; otherwise it reveals that text under the notice. `can_send` also requires `installed() != Some(false)`.
- **Model** `draft-model` (right side of the bar, next to Send): hidden while `data.models[agent]` is empty (D20). The menu lists those models with the current one checked; footer "Last used for <Label>. The list refreshes when the session starts." `send_draft` passes `draft.model` to `land`.
- `send_draft` uses `draft.folder` and `draft.agent`, as Task 5 already does.

- [ ] **Step 1: Failing tests** (`start.rs` tests, using `workspace::tests` helpers):
  - `folder_menu_lists_known_folders`. History has one session in `<tmp>/a` (`fake_settings("--sessions 1 --sessions-cwd <tmp>/a")`, `load_history`). After `start_now` and ⌘N, the draft's `data.known == [canonical(temp_dir), canonical(<tmp>/a)]` (the project first, no duplicates).
  - `picking_a_folder_keeps_the_workspace`: `d.set_folder(<tmp>/b)` → `d.folder == canonical(<tmp>/b)`, `w.project` unchanged.
  - `send_uses_the_drafts_folder_and_agent`. Agents `fake` and `b` (both the fake). `set_agent("b")`, `set_folder(<tmp>/b)`, type, Enter → the session's `agent == "b"` and `cwd == <tmp>/b`, and `w.project == <tmp>/b` (it followed). Switch the project away before the temp dir drops, as `workspace_follows_the_focused_session` does.
  - `missing_program_shows_the_notice_and_disables_send`: agent `ghost` with command `wai-no-such-program-7c1e --acp`; `set_agent("ghost")`, type "x". `wait_until` `installed() == Some(false)`; `window.try_find("draft-notice").is_some()`; `!can_send`; Enter starts nothing (`registry.status("ghost").is_none()`).
  - `changing_the_agent_resets_the_model`: `settings.models = {"b": "fake-large"}` → `set_agent("b")` gives `model == Some("fake-large")`; `set_agent("fake")` gives `None`.
  - `model_pick_applies_to_the_new_session`: `fake_settings("--models")`. `start_now` fills `models_seen`. ⌘N, `set_model(Some("fake-large"))`, type, Enter → the reply (`reply` helper logic) is "model:fake-large".
  - `model_chip_waits_for_seen_models`: before any session, `try_find("draft-model").is_none()`.
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Gate G, then commit.** `git commit -m "feat: folder, agent and model chips in the draft"`

---

### Task 8: 1.5, send shows the chat at once; failure and Esc go back to the draft

**Files:** Modify `crates/wai/src/chat/{view.rs,input.rs,model.rs,draft.rs}`, `crates/wai/src/start.rs`, `crates/wai/src/workspace.rs` (`busy_sessions`), `crates/wai/src/topbar.rs`, `crates/wai/src/sidebar.rs` (draft row while starting).

**Interfaces:**
- Consumes: Task 5's `send_draft` and `land`, Task 2's `busy_sessions`, Task 3's close on orphans.
- Produces:
  - `DraftView.starting: Option<Entity<ChatSession>>` replaces the bool. `DraftView` renders `starting` (the chat) when set; `focus` goes to it; `can_send` is false.
  - `ChatSession::pending(agent: String, cwd: PathBuf, handle: AgentHandle, registry: WeakEntity<AgentRegistry>, text: String, attachments: Vec<Attachment>, window, cx) -> Self` (empty id, `push_user(Message::shown)`, keeps the message as `first`), `ChatSession::is_starting(&self) -> bool` (`id.is_empty()`), `ChatSession::step(&mut self, title: String, cx) -> usize` (pushes `ChatItem::Tool { id: "wai-step-<ix>", kind: "other", status: InProgress, .. }`), `ChatSession::end_step(&mut self, ix: usize, title: String, failed: bool, cx)`, `ChatSession::started(&mut self, started: SessionStarted, window, cx)` (sets `id`; takes modes and config through `take_started`, extracted from `load`'s `Ok` branch and used by both; then prompts `first` with `blocks`, without a second bubble), `ChatSession::leftover(&self, cx: &App) -> String` (the input text and any queued texts, blank-line separated).
  - `ChatEvent::StopStart`: `cancel()` emits it, instead of sending an ACP cancel, while `is_starting()`. `send()` returns early while `is_starting()`.
  - `ChatModel::shown_title()` falls back to the first line of the first `User` item.

**Behavior** (`send_draft` reworked; D10, D11):
1. The guards from Task 5.
2. `handle = registry.handle(agent)`, then the pending chat with the draft's `message()`. Subscribe to it: `StopStart` → `cancel_start(draft)`; `Hide` → close the pane showing the draft. Set `draft.starting = Some(chat)`, emit `Changed`, and when the draft's pane is the focused one, focus the chat's input (Esc reaches it through the `ChatSession` key context).
3. `ix = chat.step("Starting <Label>")`. Then `new_session(cwd, mcp)` and `landed()`.
4. After every await, check `draft.starting == Some(chat)`. If not (Esc, or a replaced draft) and `session/new` succeeded: close that session in the agent when `caps.close` (D7). Return without landing.
5. On `Ok`: `end_step(ix, "Started <Label> in <shown cwd> · <mode name> · <model name>")`, leaving out missing parts. Then `mark_ready`; `chat.started(started)`; `land(chat, early, draft.model)`; replace the draft's pane, remove the draft, `follow_session`/`save_state` as in Task 5.
6. On any failure before that: `fail(draft, error)`, which drops the pending chat, appends `leftover` to the draft's text after a blank line when it is non-empty, and sets `notice`.
7. `cancel_start(draft)` is the same without a notice.
- `busy_sessions` adds the drafts whose `starting` is set. The top bar's working count adds them too. A starting draft's Open row shows its chat's status dot and `shown_title`, with no "Draft" tag.

- [ ] **Step 1: Failing tests** (`start.rs` tests; use `gated` from Task 1):
  - `sending_shows_the_chat_at_once`: `gated("")`. ⌘N, type "build it", Enter. Then `panes[0]` is still the draft with `starting.is_some()`; the chat's items are `[User { text: "build it" }, Tool { title: "Starting fake", status: InProgress, .. }]`; `chat.model.activity() == Some("Starting fake…")`; `sessions.is_empty()`. Write the gate. `wait_until` `sessions.len() == 1`: `drafts.is_empty()`, `items[0]` is the User item, `items[1]` is a Tool that is `Completed` with a title starting "Started fake in ", and `wait_until` an Agent item reads "Hello from fake agent".
  - `escape_before_the_session_starts_returns_to_the_draft`: `gated("--close-file <tmp>/closed")`. ⌘N, type "build it", Enter, then type "and tests" into the pending chat. `simulate_keystrokes("escape")` → `starting.is_none()`, `notice.is_none()`, `text == "build it\n\nand tests"`. Write the gate; `wait_until` the closed file contains "fake-1"; `sessions.is_empty()`.
  - `failed_start_keeps_the_draft` (Task 5) still passes with the pending chat in between.
  - `starting_draft_counts_as_working`: `gated`, send → `busy_sessions == 1`; `secondary-q` shows `find_by_label("1 session is still working")`.
  - `untitled_session_shows_its_first_message`: after landing (no `Title` event), `label(("open", 0)) == "build it"` and the pane's `session-title` label is "build it".
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement.** Remove `ChatSession::send_with` (Task 5) if nothing calls it any more; clippy's `-D warnings` fails on dead code.
- [ ] **Step 4: Run** these tests and all of `chat::view::tests` (load and restore paths share `take_started`). Expected: PASS.
- [ ] **Step 5: Gate G, then commit.** `git commit -m "feat: a sent draft shows its chat and steps at once"`

---

### Task 9: 1.5, Local/New worktree and branch chips; checkout and worktree on send

**Files:** Modify `crates/wai/src/chat/draft.rs`, `crates/wai/src/start.rs`.

**Interfaces:**
- Consumes: `git::{repo, worktree_blocked, slug, create_worktree, checkout, Repo, SUBMODULE_HINT}` (Task 4); the pending chat and `step`/`end_step`/`fail` (Task 8).
- Produces:
  - `DraftView.repo: Option<Repo>` (None while reading, or not a repo), `DraftView.reading: bool` (true while `read_repo` runs), `DraftView.worktree: bool`, `DraftView.branch: Option<String>` (Local: the branch to check out, None = stay; worktree: the base).
  - `DraftView::read_repo(&mut self, cx)`: background `git::repo(folder)`. On load: Local sets `branch = None`; worktree sets `branch = Some(repo.default)` (Task 11 adds the remembered base).
  - `DraftView::set_worktree(&mut self, on: bool, cx)`: refused (stays false) while `worktree_blocked` is `Some`. `DraftView::set_branch(&mut self, branch: Option<String>, cx)`. `DraftView::worktree_blocked(&self) -> Option<&'static str>`.
  - `set_folder` (Task 7) now also clears `repo` and calls `read_repo`. `new` calls `read_repo`.

**Behavior:**
- `draft-workspace` chip, hidden while `repo` is None: icon `Monitor` + "Local", or `GitFork` + "New worktree". Menu rows: "Local" / "Works in the folder already on this computer"; "New worktree" / "Creates a separate copy for this session, on send". The latter is disabled with the second line `SUBMODULE_HINT` when blocked.
- `draft-branch` chip, hidden while `repo` is None. Local reads the branch or the current branch ("Detached HEAD" when None), with an `ambar` count pill of `repo.dirty` when it shows the current branch and `dirty > 0`. Worktree reads "from <base>". Menu: filter placeholder "Filter branches". Local first shows the current branch with "Current · N uncommitted files" (or "Current"). Then the "Default branch" section, then "Branches" (the rest of `repo.branches`). Footer: Local "Picking another branch checks it out when the session starts."; worktree "The new worktree starts from this branch."
- `send_draft`, before "Starting <Label>":
  - **Worktree:** `ix = step("Creating worktree")`. Background `create_worktree(&repo, &slug(text), base)`. On `Ok(wt)`: if `wt.fetch_failed`, add a completed step "Could not fetch origin/<base> (<why>); started from the local <base>". Then `end_step(ix, "Created worktree <shown path> on branch <branch> from <from>")`. `cwd = wt.path.join(folder.strip_prefix(&repo.root))` (D13). **Flip the draft** to that worktree: `folder = cwd`, `worktree = false`, `branch = None`, `read_repo`, so a retry reuses it. On `Err(e)`: `fail(Some(e))`.
  - **Local, `branch = Some(b)` and `b != repo.current`:** refuse with D27 copy when any session, or another starting draft, has `canonical(cwd) == folder`. Otherwise `ix = step("Checking out <b>")`; background `git::checkout(folder, b)`; `Ok` → `end_step(ix, "Checked out <b>")`; `Err(e)` → `fail(Some(e))`.
  - The check from Task 8 that the start is still current also runs after the git step.

- [ ] **Step 1: Failing tests** (`start.rs` tests; repos from `git::tests::{git, repo_with}`; project = temp_dir):
  - `git_chips_hidden_outside_a_repository`: ⌘N; `wait_until` `!reading`. Then `try_find("draft-workspace")` and `try_find("draft-branch")` are None.
  - `git_data_is_read_off_the_ui_thread`: `set_folder(repo)`; right after, `repo.is_none()`; `wait_until` `repo.current == Some("main")`; both chips exist.
  - `local_send_on_another_branch_checks_it_out`: repo with `feature`, clean, on `main`. `set_branch(Some("feature"))`, send → the session's `cwd == root`, `git::repo(root).current == Some("feature")`, and the chat has a Tool step "Checked out feature".
  - `local_switch_refuses_uncommitted_files`: dirty repo, `set_branch(Some("feature"))`, send → `notice` contains "uncommitted files", `current == Some("main")`, `sessions.is_empty()`.
  - `local_switch_refuses_a_folder_another_session_uses`: `w.set_project(root)`, `start_now` (cwd = root), ⌘N, `set_branch(Some("feature"))`, send → `notice == "Can't switch to feature: another open session works in <root name>. Pick New worktree instead."`.
  - `new_worktree_send_creates_it_from_the_fetched_base`: origin + clone, origin ahead by C2. `set_worktree(true)`, `set_branch(Some("main"))`, type "Add tag filter", send → session `cwd == root/.worktrees/add-tag-filter`, `git::head(cwd).branch == Some("wai/add-tag-filter")`, HEAD == C2, and a step starts "Created worktree".
  - `offline_fetch_still_starts_the_session`: origin URL set to a missing path → the session starts and a Completed step starts "Could not fetch origin/main".
  - `worktree_keeps_the_subfolder`: `set_folder(root/sub)`, worktree, send → session `cwd == <wt>/sub`.
  - `retry_after_a_created_worktree_reuses_it`: agent `--require-auth`; worktree send → `notice == signed_out`, `!worktree`, `folder == <root>/.worktrees/<slug>`. Send again → `read_dir(root/.worktrees).count() == 1`.
  - `escape_after_the_worktree_keeps_it_for_the_retry`: `gated` agent. Worktree send; `wait_until` a "Created worktree" step exists; `escape` → back on the draft, `folder` is the worktree, `!worktree`. Open the gate and send again → the session's `cwd` is that worktree and `.worktrees` holds 1 entry.
  - `submodule_repo_disables_new_worktree`: root has `.gitmodules` → `worktree_blocked() == Some(SUBMODULE_HINT)`; `set_worktree(true)` leaves `worktree == false`.
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Gate G, then commit.** `git commit -m "feat: drafts start in a branch or a new worktree"`

---

### Task 10: 1.5, the folder, branch and agent·model pills in the session header

**Files:** Modify `crates/wai/src/chat/{view.rs,render.rs}`, `crates/wai/src/workspace.rs` (`add_session`).

**Interfaces:**
- Consumes: `git::{head, Head}` (Task 4).
- Produces: `ChatSession.head: Option<Head>`, filled by `add_session` through `cx.background_spawn(git::head(cwd))` when it is still None. Every new, restored, resumed and forked session passes through it.

**Behavior** (D22): after the title in `render_header`, three `aurora::pill`s. `pill-folder`: icon `Folder` + `folder_name(cwd)`. `pill-branch`, only when `head.branch` is Some: icon `GitFork` for a worktree, else `GitBranch`, plus the branch. `pill-agent`: `agent_label(agent)`, then " · <name of the current Model option value>" when there is one. Each pill has `min_w_0().truncate()` and `aria_label` = its text. The pickers and the usage pill stay after them. A pending chat shows the folder and agent pills from its own fields; the branch pill appears once it lands.

- [ ] **Step 1: Failing tests** (`chat::view::tests`):
  - `header_pills_show_folder_branch_and_agent_model`: `w.set_project(repo on "feature")`; `fake_settings("--models")`; `start_now`. `wait_until` `head.is_some()`; within the pane: `find("pill-folder").label() == root name`, `find("pill-branch").label() == "feature"`, `find("pill-agent").label() == "fake · Fake Small"`.
  - `restored_session_shows_its_pills`: a saved state whose cwd is the repo, then `launch` → `pill-branch` "main" once loaded.
  - `no_branch_pill_outside_a_repository`: `start_now` in temp_dir → `pill-folder` exists, `pill-branch` does not.
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Gate G, then commit.** `git commit -m "feat: folder, branch and model pills in the session header"`

---

### Task 11: 1.5, remembered model and base; modes_seen and the mode chip

**Files:** Modify `crates/wai-settings/src/lib.rs`, `crates/wai/src/workspace.rs` (fields, `new`, `save_state`, `land`, `test_agent`), `crates/wai/src/chat/{draft.rs,view.rs}`, `crates/wai/src/start.rs`.

**Interfaces:**
- Produces, in wai-settings: `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)] pub struct SeenModes { pub modes: Vec<SeenModel>, pub current: Option<String> }` and new `AppState` fields (struct-level `#[serde(default)]` already applies): `pub last_models: BTreeMap<String, String>` (agent → model id), `pub last_bases: BTreeMap<PathBuf, String>` (repo root → branch), `pub modes_seen: BTreeMap<String, SeenModes>`.
- Workspace mirrors the three like `models_seen`: loaded in `new`, written in `save_state`. `fn seen_modes(session: &ChatModel) -> Option<SeenModes>` takes `modes`/`current_mode` when non-empty, else the `ConfigCategory::Mode` option's values and current. `land` and `test_agent` insert it.
- `DraftData` gains `modes`, `last_models`, `last_bases`. A draft starts with `model = last_models[agent] → default_models[agent]` and `mode = modes[agent].current`, also on `set_agent`. For a worktree, `branch = last_bases[repo.root] → repo.default`.
- `ChatSession::set_mode` becomes `pub(crate)`. `DraftView.mode: Option<String>`, `DraftView::set_mode(&mut self, mode: Option<String>, cx)`.

**Behavior:**
- The `draft-mode` chip (icon `Shield`, the mode's name; inside the prompt bar after +) is hidden while `modes` lacks the agent. Its menu lists those modes with the current one checked.
- After `started()`, when the draft's mode differs from the session's current mode: `set_option(<Mode option id>, mode)` if the session has a Mode config option, else `set_mode(mode)`. The step line uses the chosen names.
- At land for a draft, write `last_models[agent] = model` when Some and `last_bases[repo.root] = base` for a worktree, then `save_state` (D21).
- A pick never changes `settings` or `settings.toml`.

- [ ] **Step 1: Failing tests.** wai-settings: `app_state_keeps_remembered_choices` (round trip of all three maps; a state JSON without the keys loads with empty maps). wai (`start.rs` tests, `fake_settings("--models")`):
  - `new_draft_takes_the_last_model_used_for_its_agent`: send a draft with `set_model(Some("fake-large"))`, then ⌘N → the new draft's `model == Some("fake-large")`; `w.settings.models` is unchanged.
  - `new_draft_never_preselects_a_picked_mode`: send a draft with `set_mode(Some("plan"))` and check the session's `current_mode == Some("plan")`. Then ⌘N → `mode == Some("default")`, the agent's last reported current mode.
  - `mode_chip_waits_for_the_agent_to_report_modes`: before any session, `try_find("draft-mode").is_none()`; after `start_now`, a new draft shows it.
  - `worktree_base_is_remembered_per_repository`: worktree send from base `feature` → a new draft on that repo with `set_worktree(true)` has `branch == Some("feature")`; a draft on another repo has its own default.
  - `picks_never_change_settings` (`live` with a user `settings.toml`): picks and sends leave the file's bytes unchanged.
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** these and `cargo test -p wai-settings`. Expected: PASS.
- [ ] **Step 5: Gate G, then commit.** `git commit -m "feat: drafts remember the last model and worktree base"`

---

### Task 12: 1.5, "Clone repository…" in the folder menu

**Files:** Modify `crates/wai/src/git.rs`, `crates/wai/src/chat/draft.rs`.

**Interfaces:**
- Produces: `git::clone(url: &str, parent: &Path) -> Result<PathBuf, String>` (D24): refuses an empty URL, one starting with `-`, or an existing destination ("<dest> already exists"), then runs `git clone -- <url> <dest>` from `parent`. `DraftView.cloning: Option<String>` (the URL). `DraftView::clone_into(&mut self, url: String, parent: PathBuf, cx)` runs the background clone, then `set_folder(dest)` or `notice = "Could not clone <url>: <why>"`.

**Behavior:** the folder menu's last row is "Clone repository…". It opens a dialog titled "Clone repository" with an `Input` (id `clone-url`, placeholder "https://github.com/owner/repo.git"), Cancel and "Choose folder…" (id `clone-choose`). The latter calls `prompt_for_paths` (a directory) and then `clone_into`. While cloning, `draft-notice` reads "Cloning <url>…" and `can_send` is false.

- [ ] **Step 1: Failing tests.** git.rs: `clone_makes_a_folder_named_after_the_repo` (a local bare `origin.git` → `clone(path, parent) == Ok(parent/"origin")`, a repo); `clone_refuses_an_existing_folder`; `clone_refuses_an_option_as_url` (`"-u x"` → Err, no process run). start.rs: `cloned_folder_becomes_the_drafts_folder` (`d.clone_into(bare, parent)` → `wait_until` `folder == parent/"origin"` and `repo.is_some()`; while cloning, `!can_send`).
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Gate G, then commit.** `git commit -m "feat: clone a repository from the draft's folder menu"`. `cargo clean` in the worktree when the item is done.

---

## Spec coverage (1.5)

| Spec requirement | Task |
|---|---|
| Every entry point opens a draft; none spawns an agent or calls `new_session` before send | 5 |
| ⌘N twice gives two drafts; a draft replaces what the focused pane showed | 5 |
| Screen layout, copy, hint row; prompt box reuses the chat input (paste, attach) | 5 |
| Folder chip: known folders, Open folder…, Clone repository…; the workspace does not switch folders | 7, 12 |
| Local / New worktree; branch chip, uncommitted count, menus and footers; hidden outside git | 9 |
| Agent chip, "Not installed", notice, "How to install", Send disabled | 7 |
| Mode chip (not remembered; `modes_seen`); model chip (remembered; "Last used for") | 7, 11 |
| Send: worktree (fetch, `-2` suffix, exclude), checkout (guards), start, mode/model, message | 8, 9, 11 |
| Chat at once: bubble, step lines, loading line with Esc | 8 |
| Failure and Esc back to the draft intact; worktree reused on retry | 5, 8, 9 |
| Open list draft row, ×, "Discard this draft?", drag between panes; not in History, not saved | 6, 5 |
| Title = first line of the first message until the agent names it | 8 |
| Header pills, restored sessions included | 10 |
| Remembered base per repository; model per agent; picks never change Settings | 11 |
| Submodule ruling (one place) | 4, 9 |
