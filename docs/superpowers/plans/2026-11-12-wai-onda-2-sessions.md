# wai Onda 2, sessions track (2.6, 1.10, 2.1, Onda 1 minors, 0.8 proof): Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Route an agent's events to the right session when two open sessions share an id (2.6), make the prompt box's folder and branch chips follow the folder and worktree the agent actually works in (1.10), warn before resuming a Claude conversation that is open elsewhere (2.1), close the still-valid deferred minors of Onda 1's sessions track, and prove the suite stable (0.8).

**Architecture:** 2.6 gives every `ChatSession` a binding stamp, taken each time the agent gives or loads its id; `Workspace::session` picks the latest stamp, and a session that loses its id to a newer one says so. 1.10 reads the session's own tool-call `locations`, resolves each new directory with git on the background executor (cached per directory, last read wins), and stores the result as `ChatSession::place` next to the existing `head`; the strip draws `place` when set. 2.1 adds `elsewhere.rs`: one background check (a process whose argv names the id, or a transcript written in the last 120 s) used by History's resume paths. The "open elsewhere" mark on History rows is deferred until the sidebar redesign (`feat/sidebar-redesign`, another session) lands, because History moves into the Sessions panel there. The minors are small fixes in `start.rs`, `chat/draft.rs`, `chat/input.rs` and `git.rs`.

**Tech Stack:** Rust workspace, GPUI + gpui-kit/gpui-component, ACP (`agent-client-protocol`, `claude-agent-acp` 0.103.0), the `git` CLI through `git::command`, `ps` (macOS/Linux).

**Spec:** this plan's items come from `docs/superpowers/plans/2026-11-12-wai-execucao-pendencias.md` (track "sessões", new item 1.10 with the owner's words) and rows 0.8, 2.1, 2.6 of `docs/superpowers/plans/2026-11-11-wai-master-plan.md` (its line numbers are from 0.5.0). R84 is described in `docs/RELATORIO-final.md` (lines 278, 324). The Decisions below are binding for the implementer.

## Global Constraints

- Repo `/Users/guilherme.brito/Project/Guilherme/wai`, base `development` at `a818cc8` (0.6.2). Work in the worktree `.claude/worktrees/onda2-sessions` on branch `feat/onda2-sessions`. Run `git log --oneline -5 development` first: another session commits here.
- Merge order into `development`: chat → extensions and settings → **sessions** → workspace. Before handing the branch over, merge `development` into it if the earlier tracks (or the self-update work) landed; resolve conflicts in this worktree. Do not merge into `development` yourself.
- `origin` exists now, but **no push**. Commits: `feat:` / `fix:` / `test:` / `docs:` with no scope, no Co-Authored-By, no Jira ref; imperative, first line < 72 chars.
- **Owner's pace.** Per task: write the task's tests and code, run only `cargo check --workspace --all-targets` (expected: no errors), then commit. No per-task `cargo test`, clippy, reviewer or fix loop. Failures surface in Task 8 (one full gate, one final review, one fix wave), then Task 9 (the 0.8 proof).
- Every shell: `export PATH="$HOME/.cargo/bin:$PATH"`. Before a build, `df -h /System/Volumes/Data`; under 8 GB free, stop and report. `cargo clean` in the worktree when the track is done.
- Gate (Task 8 only): `cargo build -p wai-agent --bin wai-fake-agent && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`. One test: `cargo test -p wai <name>`.
- Not ours (another agent builds self-update; another track edits panes): never touch `crates/wai/src/update.rs`, the Updates section of `settings_view.rs`, the quit/close-confirmation path of `workspace.rs` (R28/R29, `quit_asks_while_a_session_works`, `proceed`, `on_window_should_close`), `.github/workflows/*`, `CLAUDE.md`. Keep `workspace.rs` edits to the functions this plan names; new `impl Workspace` code goes in `elsewhere.rs`.
- Tests never read or write the real `~/.claude`, `~/.claude.json` or `~/.config/wai` (set `w.claude_home` to a temp dir; state files in temp dirs), never the developer's gitconfig (all git goes through `git::command`). Never kill `wai`, `claude` or `claude-agent-acp` processes you did not start; a test kills only children it spawned.
- No timers and no polling in the product (idle CPU < 0.5%): git and `ps` run on events (a tool call, a turn end, a History refresh, a resume click), on the background executor.
- UI copy in English; strings in quotes in this plan are exact. Never break existing test ids. Follow existing patterns; comments only for a non-obvious why.
- The draft screen's "New worktree" stays disabled in repositories with submodules (`git::worktree_blocked`, owner confirmed). Do not change it.

## Decisions to confirm

| # | Decision | Cost if wrong |
|---|---|---|
| D1 | 2.6 rule: of the open sessions with one (agent, id), events go to the one the agent bound to that id **last** (`ChatSession::bound`, stamped at start, at `started`, at every `load`). When a session takes an id another open session holds, the older one ends its turn and shows "The agent gave this session's id to another session. Restart loads it here again." (Restart takes the id back the same way.) Process tags were rejected: the reported case (a restored session, then a new one from the same process) shares the process. | None for Claude, Codex, Gemini (unique ids). An agent that repeats ids ping-pongs the notice between two sessions instead of mixing them silently. |
| D2 | 1.10 signal: the `locations` of the session's **own** tool calls (`ToolCall`, and `ToolCallUpdate` that carries locations; `subagent == None`). Claude sends them for Read, Edit, Write, NotebookEdit, Glob and Grep paths. Bash `cd …` and `EnterWorktree` carry none, so the chips move on the agent's first file touch in the new folder. Relative paths are joined to `cwd`. | A lag between `cd`/`git worktree add` and the first read or edit there. Parsing Bash commands was rejected as fragile. |
| D3 | Several repositories: the last location that resolves to a git repository wins, reads included. A location outside any repository (`/tmp`, `~/.claude/plans`) changes nothing. Back in `cwd`'s repository, the chips return to today's (`cwd`'s folder name and branch). | Chips hop between submodules while the agent explores. Upgrade: once a turn has edited, only edits move them. |
| D4 | Folder chip = the repository's **main** working tree (`Head::folder`): `inventories` for a worktree at `inventories/.claude/worktrees/x`; branch chip = the worktree's branch with `GitFork`. A detached HEAD (common for submodules) shows no branch chip, as today. | Cosmetic. |
| D5 | Resume and restore re-derive the place from the agent's replay (Claude replays tool calls with their locations): nothing new in `state.json`. While a load replays, only the last location is kept and followed once the load ends. | An agent that replays no tool calls reopens on the start folder until its next tool call. |
| D6 | The workspace (Files, Search, History, editor) does **not** follow the agent; it keeps following the user to the session's start folder (`follow_session`, a24e9f9: only the user's own moves move the workspace). | The user opens the worktree with ⌘O to browse it. Upgrade: `follow_session` uses `place.folder`. |
| D7 | The branch is read again at each turn end (one git call per session turn), never on a timer. | A branch switched mid-turn shows at the turn's end. |
| D8 | 2.1 detection, only for agent id `claude` (as "Continue in terminal", D8 of Onda 1): open elsewhere = a process whose command line has a token equal to the id (or ending in `=<id>`) and a token containing `claude`, **or** `<claude_home>/projects/*/<id>.jsonl` modified in the last 120 s, unless this wai ended that session in the last 120 s. Sessions open in this wai are never checked. | A `claude` TUI resumed with `/resume` (no id in argv) and idle for over 2 min is not detected. |
| D9 | 2.1 checks on a History row's click and side button (both via `resume`) and on History drags; not on restore at launch. The dialog offers "Cancel", "Open anyway" and "Fork" (Fork only when `can_fork`). | A session restored at launch while also open in a terminal is not flagged. |
| D10 | No "open elsewhere" mark on History rows in this track: History moves into the Sessions panel in the sidebar redesign (another session), so the mark is added there afterwards. | A conversation open elsewhere shows nothing until you click it; the resume check still asks. |
| D11 | Minor 10: `sessions/list` titles and the Rename prefill use the name or the agent's title only (no first-message fallback); Fork keeps the shown title for the copy's title. | Cosmetic. |
| D12 | Extensions' `sessions/open` with `send` sends its prompt through the same wait as drafts (after the default model and mode settle). | None expected. |

## Verified and dropped

- **0.8 fixes:** in. `new_session_appears_in_open_list` uses the gated fake (`workspace.rs:3374`), `crash_loop_stops_after_three` has the 30 s hang guard (`crates/wai-ext/tests/host.rs:406`). Only the 20-run proof remains (Task 9).
- **R84:** still present. `Workspace::session` (`workspace.rs:902`) returns the first match.
- **Minor 4:** fixed during Onda 1 (the report says so).
- **Minor 11** (owner's in-app checks): not code; it is item 0.6 of the execution plan.
- **"No test for I8 subfolder":** `start::tests::worktree_keeps_the_subfolder` covers it.
- Still valid and planned: Minors 1, 2, 7, 9, 12 and the extension prompt race (Task 6); Minors 3, 5, 6, 8, 10 and the I10 `--no-track` test (Task 7).

## Review Focus

1. **The agent reads outside any repository** (`/tmp`, `~/.claude/plans`): the strip stays where it was. Test `outside_a_repository_the_strip_stays` (Task 3).
2. **Two locations in quick succession whose git reads finish out of order:** the later location wins. Test `strip_comes_back_with_the_agent` (Task 3).
3. **A submodule on a detached HEAD** (what `git submodule update` leaves): folder chip `inventories`, no branch chip. Tests `detached_submodule_has_no_branch` (Task 2), `detached_submodule_shows_no_branch_chip` (Task 3).
4. **A conversation closed in wai and reopened from History within 2 min** (wai's own write is fresh): no warning. Test `a_conversation_this_wai_closed_does_not_ask` (Task 5).
5. **Subagents reading other repositories** (Explore agents): the strip ignores them. Test `subagent_locations_do_not_move_the_strip` (Task 3).

## File map

| File | Responsibility | Tasks |
|---|---|---|
| `crates/wai/src/chat/view.rs` | `bound`, `ChatEvent::Bound`, `lose_id`, `REUSED`; `place`, `roots`, `follow`, `set_home`, turn-end re-read; `leftover`, `send_settled`; tests | 1, 3, 6 |
| `crates/wai/src/chat/model.rs` | `on_exited` cleanup shared with `lose_id`; `named_title` | 1, 7 |
| `crates/wai/src/workspace.rs` | `session`, `claim`, `add_session`; `resume` split, History drop, `end_session` stamp, two fields; `start_session` send; doc comments | 1, 5, 6, 7 |
| `crates/wai/src/git.rs` | `Head { root, folder, .. }`; fetch only a pushed base; test fixtures | 2, 7 |
| `crates/wai/src/chat/input.rs` | strip draws `place`; Send off while starting | 3, 6 |
| `crates/wai-agent/src/bin/wai-fake-agent.rs` + `tests/fake_agent.rs` | `--read <path>`; tempdir in the close test | 3, 7 |
| `crates/wai/src/elsewhere.rs` (new) | detection; `impl Workspace` check, dialog | 4, 5 |
| `crates/wai/src/sidebar.rs` | Rename prefill | 7 |
| `crates/wai/src/start.rs`, `crates/wai/src/chat/draft.rs` | draft minors | 6 |
| `docs/user-guide.md` | strip and History bullets | 3, 5 |

---

### Task 1: 2.6, route events to the session that holds the id now (R84)

**Files:**
- Modify: `crates/wai/src/chat/view.rs` (`ChatEvent`, `ChatSession` fields, `new`, `started`, `load`, new `lose_id`), `crates/wai/src/chat/model.rs` (`on_exited`)
- Modify: `crates/wai/src/workspace.rs` (`session` ~902, `add_session` ~1023, new `claim`)
- Test: `crates/wai/src/chat/view.rs` `mod tests` (beside `restart_reloads_session_after_crash`)

**Interfaces:**
- Produces:
  - `pub(crate) const REUSED: &str = "The agent gave this session's id to another session. Restart loads it here again.";` in `chat/view.rs`.
  - `ChatSession.bound: u64` (`pub(crate)`): from a `static BINDINGS: AtomicU64` (`fetch_add(1) + 1`), set in `new` when `started.id` is non-empty, in `started()` and at the top of `load()`. `started()` and `load()` then emit `ChatEvent::Bound` (new variant: "The agent gave or loaded this session's id").
  - `ChatSession::lose_id(&mut self, cx: &mut Context<Self>)`: runs `on_exited`'s cleanup (turn over, waiting cards cancelled, running subagents parked), sets `TurnState::Error(REUSED.into())` and pushes `REUSED` as an Error notice, so Restart shows. Split `ChatModel::on_exited` so the cleanup is shared and its own notice text stays "Agent exited: …".
  - `pub(crate) fn Workspace::session(&self, agent, id, cx)`: same signature, now `pub(crate)`; among matches, `max_by_key(bound)`.
  - `Workspace::claim(&self, session: &Entity<ChatSession>, cx: &mut Context<Self>)`: every *other* open session with the same agent and id gets `lose_id`. Called in `add_session` before the push, and from the `add_session` subscription on `ChatEvent::Bound` (then `return`).

- [ ] **Step 1: Write the tests** (helper `two_sessions_one_id(cx) -> (Entity<Workspace>, Entity<ChatSession>, Entity<ChatSession>)`: `open(cx, fake_settings("--crash-after 1"))`; `a = start_session`; `a.send_text("hi")`; `wait_until` `a` is `TurnState::Error(_)` (process 1 crashed after one chunk); `b = start_session` (process 2; the fake's ids restart); assert both ids are `"fake-1"`. Before the fix, `b`'s events land in `a`):
  - `an_id_the_agent_reuses_goes_to_its_new_session`: `b.send_text("go")`; `wait_until` `last_agent_text(&b) == Some("Hello")`; then `a` holds exactly one `ChatItem::Agent` (its own "Hello") and some `ChatItem::Notice` whose text is `REUSED`.
  - `restart_takes_a_reused_id_back`: after the helper (process 2 alive, `b` idle), `a.load(false, window, cx)`; `wait_until` `a` is Idle; `a.model.items` equals the fake's replay (`User "previous prompt"`, `Agent "previous answer"`), `b` has none of those items and has a Notice `REUSED`.
- [ ] **Step 2: Implement** the interfaces above.
- [ ] **Step 3:** `cargo check --workspace --all-targets`. Expected: no errors.
- [ ] **Step 4: Commit.** `git commit -m "fix: route agent events to the session that holds the id now"`

---

### Task 2: 1.10, `git::head` names the repository root and its main folder

**Files:**
- Modify: `crates/wai/src/git.rs` (`Head`, `head`, tests and the `tests` fixtures)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq)]
  pub(crate) struct Head {
      pub root: PathBuf,           // `--show-toplevel` of the folder: a linked worktree's own root
      pub folder: PathBuf,         // the repository's main working tree
      pub branch: Option<String>,  // None: detached HEAD
      pub worktree: bool,          // git-dir != common-dir
  }
  pub(crate) fn head(folder: &Path) -> Option<Head>;
  ```
  In `#[cfg(test)] pub(crate) mod tests`: `pub(crate) fn submodule_worktree(tmp: &Path) -> (PathBuf, PathBuf)`: `repo_with(tmp/"inv-src")`, `repo_with(tmp/"super")`, `git(super, ["-c", "protocol.file.allow=always", "submodule", "add", <inv-src>, "inventories"])`, commit, then in `super/inventories` `worktree add -b feat-x .claude/worktrees/x`. Returns `(super, super/inventories/.claude/worktrees/x)`, both canonical.
- `head` algorithm (verified by the planner on git 2.50.1, where `worktree list` names a submodule's `.git/modules/<name>` as its main worktree, and `--git-dir <common>` alone gives the cwd for a plain repo):
  1. `rev-parse --path-format=absolute --show-toplevel --git-dir --git-common-dir` → `root`, `git_dir`, `common`; any failure → `None`.
  2. Not a worktree → `folder = root`.
  3. Else `folder` = the first `worktree <path>` line of `worktree list --porcelain`; when that path equals `common`, `run(dir, ["--git-dir", common, "rev-parse", "--show-toplevel"])` instead (reads the submodule's `core.worktree`); any failure → `root`.
  4. `branch = current_branch(folder_arg)`.

- [ ] **Step 1: Write the tests** (plain `#[test]`):
  - Update `head_tells_a_worktree_apart`: the repo gives `Head { root: r, folder: r, branch: Some("main"), worktree: false }`; the created worktree gives `root == w.path`, `folder == r`, `branch == Some("wai/x")`, `worktree`.
  - `head_names_the_submodule_of_a_submodule_worktree`: with `submodule_worktree`, `head(wt) == Head { root: wt, folder: super/inventories, branch: Some("feat-x"), worktree: true }`; `head(super/inventories)` has `root == folder == super/inventories`, `!worktree`.
  - `detached_submodule_has_no_branch`: `checkout --detach` in `super/inventories` → `head(..).unwrap().branch == None`, `folder` still `super/inventories`.
- [ ] **Step 2: Implement.** Callers compile unchanged (`workspace.rs` `add_session`, `start.rs` test).
- [ ] **Step 3:** `cargo check --workspace --all-targets`. Expected: no errors.
- [ ] **Step 4: Commit.** `git commit -m "feat: name a worktree's repository folder in git::head"`

---

### Task 3: 1.10, the strip follows the agent's folder and worktree

**Files:**
- Modify: `crates/wai-agent/src/bin/wai-fake-agent.rs` (doc list, `Opts`, `parse_args`, prompt handler, `LoadSessionRequest` handler); Test: `crates/wai-agent/tests/fake_agent.rs`
- Modify: `crates/wai/src/chat/view.rs` (fields, `handle_event`, `handle_turn_ended`, `load`, `set_home`, `follow`), `crates/wai/src/chat/input.rs` (`render_strip`), `crates/wai/src/workspace.rs` (`add_session`: `s.head = head` → `s.set_home(head, cx)`, one line)
- Modify: `docs/user-guide.md` (the "strip on the prompt box" bullet, line ~50)
- Test: `crates/wai/src/chat/view.rs` `mod tests` (beside `strip_shows_folder_branch_and_agent`)

**Interfaces:**
- Consumes: `Head`, `git::head`, `tests::submodule_worktree` (Task 2).
- Produces:
  - Fake flag doc line: "`--read <path>` (repeatable, in order): the turn starts with a Read tool call `read-<i>` titled `Read <file name>`, kind read, location `<path>`, reported in progress then completed. `session/load` replays the same calls after `previous answer`."
  - `ChatSession` fields: `pub place: Option<Head>` ("where the agent works when that is another repository or worktree than `cwd`'s; `None`: `cwd`"), `roots: HashMap<PathBuf, Option<PathBuf>>` (directory → its repository root; `None`: in none), `place_read: u64`, `replayed: Option<PathBuf>`.
  - `ChatSession::set_home(&mut self, head: Option<Head>, cx)`: sets `head`; drops a `place` whose `root` equals the new head's.
  - `fn follow(&mut self, path: &Path, cx)`, called in `handle_event` right after `apply` returns `Some(ix)` when the event is a `ToolCall`, or a `ToolCallUpdate` with `locations: Some(_)`, and `items[ix]` is a `ChatItem::Tool` with `subagent == None` and a last location. Algorithm:
    ```text
    path = cwd.join(path) if relative
    dir  = path if path.is_dir() else path.parent()      (none: return)
    if loading: replayed = Some(dir); return
    current = place.root or head.root
    roots[dir] == Some(None)                 -> return   (not in a repository)
    roots[dir] == Some(Some(r)), r == current -> return   (already there)
    place_read += 1; n = place_read
    background git::head(&dir), then on the UI thread:
        roots[dir] = result.map(root)
        if n != place_read or result is None -> stop
        if result.root == head.root -> place = None, head = result   (back home, fresh branch)
        else place = Some(result)
        notify
    ```
  - In `load()`, right after `this.loading = false`: `if let Some(dir) = this.replayed.take() { this.follow(&dir, cx) }`.
  - In `handle_turn_ended`: read `git::head` of `place.root` (else `head.root`) in the background; on return, it replaces whichever of `place`/`head` has that root. No `place_read` bump.
  - `render_strip`: `(folder, head) = place.map(|p| (folder_name(&p.folder), Some(p))).unwrap_or((folder_name(&cwd), head))`; the branch chip and its `GitFork`/`GitBranch` mark come from that `head`. Ids unchanged (`pill-folder`, `pill-branch`, `pill-agent`).
  - User guide, replace "They are fixed once the session runs, so they are labels, not menus." with: "The folder and branch follow the agent: when its own reads, edits and searches work in another repository or worktree than the session's folder (a submodule it entered, a worktree it created there), the strip shows that repository's folder and the worktree's branch. They come back when it works in the session's folder again, the branch is read again at the end of each turn, and a reopened session shows where it last worked. They are labels, not menus. Files, Search and History stay on the session's folder."

- [ ] **Step 1: Write the fake-agent test** `read_flag_reports_its_location`: `spawn(&["--read", "/tmp/wai-read/a.txt"])`; after a prompt, the events hold a `ToolCall` whose `locations[0].path == "/tmp/wai-read/a.txt"`; after `load_session`, the replay holds the same.
- [ ] **Step 2: Write the wai tests** (`set_project(super)` then `start_session`, `send("go")`, `wait_until` the condition; `pill` reads labels; end with `leave`):
  - `strip_follows_the_agent_into_a_submodule_worktree`: `--read <wt>/a.txt` → `place.is_some()`; `pill-folder == "inventories"`, `pill-branch == "feat-x"`, `place.worktree`.
  - `strip_comes_back_with_the_agent`: `--read <wt>/a.txt --read <super>/b.txt` → `wait_until` `roots.len() == 2` and Idle → `place.is_none()`, `pill-folder == "super"`, `pill-branch == "main"`.
  - `outside_a_repository_the_strip_stays`: `--read <tmp>/plain/n.txt` (`plain` exists, no repo) → `wait_until` `roots.len() == 1` → `place.is_none()`, pills as before the turn.
  - `subagent_locations_do_not_move_the_strip`: `handle_event(&SessionEvent::ToolCall(ToolCallView { id: "c", kind: "read", status: Completed, locations: vec![Location { path: wt/a.txt, line: None }], subagent: Some("child".into()), .. }))`; `run_until_parked` → `roots.is_empty()`, `place.is_none()`.
  - `detached_submodule_shows_no_branch_chip`: `checkout --detach` in `super/inventories`; `--read <super>/inventories/a.txt` → `pill-folder == "inventories"`, `pill("pill-branch") == None`.
  - `resumed_session_shows_where_the_agent_worked`: as `restored_session_shows_its_strip` (state.json, `cwd = super`, `launch`) with `--read <wt>/a.txt` → `wait_until` `place.is_some()` → `pill-folder == "inventories"`.
  - `strip_reads_the_branch_again_after_a_turn`: `repo_with(r, ["feature"])`; after the head lands, `git(r, ["checkout", "-b", "other"])`; send → `wait_until` `head.branch == Some("other")`; `pill-branch == "other"`.
- [ ] **Step 3: Implement** the interfaces above and the user-guide sentence.
- [ ] **Step 4:** `cargo check --workspace --all-targets`. Expected: no errors.
- [ ] **Step 5: Commit.** `git commit -m "feat: the prompt box strip follows the agent's folder and worktree"`

---

### Task 4: 2.1, detect a Claude conversation open in another process

**Files:**
- Create: `crates/wai/src/elsewhere.rs` (with `#[cfg(test)] mod tests`)
- Modify: `crates/wai/src/main.rs` (`mod elsewhere;`)

**Interfaces:**
- Produces:
  ```rust
  pub(crate) const RECENT: Duration = Duration::from_secs(120);
  /// Of `ids`, those open in another process: one whose command line names the id, or a
  /// transcript `<claude_home>/projects/*/<id>.jsonl` written within RECENT (not for `quiet`
  /// ids: this wai ended them lately, so the write is its own). Blocking: background executor.
  pub(crate) fn open_elsewhere(claude_home: &Path, ids: &[String], quiet: &HashSet<String>) -> HashSet<String>;
  /// A line of `ps` output names `id`: a token equal to it or ending in `=<id>`, and a token containing "claude".
  pub(crate) fn named_by(ps: &str, id: &str) -> bool;
  ```
  Private: `fn processes() -> String` runs `ps -axww -o command=` once per call (stdin null); `""` on Windows or on failure. `open_elsewhere` reads `projects/` once and stats `<dir>/<id>.jsonl` per id; it runs `ps` only when `ids` is non-empty.

- [ ] **Step 1: Write the tests** (plain `#[test]`, temp homes only):
  - `a_process_naming_the_id_counts`: `named_by` is true for `"/u/.local/bin/claude --resume abc-1\n"`, `"claude --resume=abc-1"`, `"node /x/claude-agent-sdk/cli.js --session-id abc-1"`; false for `"tail -f /h/.claude/projects/p/abc-1.jsonl"`, `"grep abc-1"`, `"claude --resume abc-12"`.
  - `a_fresh_transcript_counts`: `home/projects/-p/abc.jsonl` just written → `open_elsewhere(home, ["abc"], {})` contains `abc`; after `File::set_modified(now - 10 min)` it does not; fresh again but `quiet = {"abc"}` → not.
  - `a_running_claude_resume_counts` (`#[cfg(unix)]`): spawn `sh -c "sleep 30" claude --resume <uuid>` (`<uuid>` from the pid and a counter); `open_elsewhere(empty home, [uuid], {})` contains it; kill and wait the child.
- [ ] **Step 2: Implement.**
- [ ] **Step 3:** `cargo check --workspace --all-targets`. Expected: no errors.
- [ ] **Step 4: Commit.** `git commit -m "feat: detect a Claude conversation open in another process"`

---

### Task 5: 2.1, warn before resuming, offer a fork

**Files:**
- Modify: `crates/wai/src/elsewhere.rs` (`impl Workspace`, tests)
- Modify: `crates/wai/src/workspace.rs`: field `ended` (+ `new`); `resume` (~1292) becomes the check around the old body, renamed `resume_now`; the `drop_zones` listener wraps a `Dragged::History` drop; `end_session` stamps `ended`
- Modify: `docs/user-guide.md` (Sessions tab bullets)

**Interfaces:**
- Consumes: `open_elsewhere`, `RECENT` (Task 4); `Workspace::session` (Task 1, now `pub(crate)`: `elsewhere.rs` calls it); `fork`, `can_fork`.
- Produces:
  - `Workspace.ended: HashMap<String, Instant>` ("ids this wai ended, and when: their transcript's fresh write is ours").
  - `Workspace::quiet(&mut self) -> HashSet<String>`: drops `ended` entries older than `RECENT`, returns the rest.
  - `Workspace::check_elsewhere(&mut self, summary: &SessionSummary, then: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static, window, cx)`: runs `then` at once when the default agent is not `claude` or `self.session("claude", &summary.id, cx)` is open; else `open_elsewhere(claude_home, [id], quiet)` in the background, then `then` when empty, else the dialog.
  - Dialog (style of `confirm`): heading with `debug_selector("confirm-title")` and `aria_label` "This conversation is open elsewhere"; body "Claude Code is running it in another window or terminal, or wrote to it in the last 2 minutes. Opening it here too can mix the two sessions' turns."; footer `Button::new("cancel")` "Cancel", `Button::new("open-anyway")` "Open anyway" (runs `then`), and, when `can_fork("claude")`, `Button::new("confirm").primary()` "Fork" (runs `fork("claude", id, title, cwd)`).
  - `resume(summary, side)` = `check_elsewhere(summary, move |w, window, cx| w.resume_now(&summary, side, window, cx))`. The drop listener: a `Dragged::History(row)` drop runs `check_elsewhere(row, move |w, window, cx| w.drop_session(&dragged, ix, zone, window, cx))`; other drops as today.
  - User guide, after the History bullet: "- **Open elsewhere:** a Claude conversation that another process runs (`claude --resume` in a terminal, another wai) or that changed in the last 2 minutes asks first when you open it from History: **Fork** opens a copy beside, **Open anyway** opens it here."

- [ ] **Step 1: Write the tests** in `elsewhere::tests`. Helper `claude_history(n) -> Settings`: `history_settings(n)` with its `fake` agent re-keyed as `claude` and `default_agent = Some("claude")`. Each test sets `w.claude_home = <tmp>/home`, calls `load_history(&workspace, 2, cx)`, opens the History list as the existing History tests do, and writes `<tmp>/home/projects/-p/<id>.jsonl` where it says "fresh":
  - `resume_asks_when_the_conversation_is_open_elsewhere`: fresh `session-0`; click the "Session 0" row → `wait_until` `find_by_label("This conversation is open elsewhere")`; `sessions.is_empty()`; `click("open-anyway")` → `wait_until` one session with id `session-0`.
  - `fork_from_the_warning_opens_a_copy`: fresh `session-0`; click; `click("confirm")` → `wait_until` one session whose id is not `session-0`.
  - `cancel_leaves_it_closed`: fresh; click; `click("cancel")`; `run_until_parked` → no session, no `confirm-title`.
  - `a_conversation_this_wai_closed_does_not_ask`: click `session-1` (no file) → it opens at once; close it with `("open-close", 0usize)`; write fresh `session-1`; click its row again → `wait_until` one session, and `confirm-title` never showed.
- [ ] **Step 2: Implement.**
- [ ] **Step 3:** `cargo check --workspace --all-targets`. Expected: no errors.
- [ ] **Step 4: Commit.** `git commit -m "feat: warn before resuming a conversation open elsewhere"`

---

### Task 6: Onda 1 minors, draft and start (Minors 1, 2, 7, 9, 12, extension race)

**Files:**
- Modify: `crates/wai/src/chat/input.rs` (`render_bar`: Send off while starting), `crates/wai/src/chat/view.rs` (`leftover`, `send_settled`), `crates/wai/src/chat/draft.rs` (`fail`, `can_send`, `creating`, branch menu), `crates/wai/src/start.rs` (`send_draft`, `revert_draft`), `crates/wai/src/workspace.rs` (`start_session`'s `send_text` → `send_settled`, ~1726)
- Test: `crates/wai/src/start.rs` `mod tests`, `crates/wai/src/chat/view.rs` `mod tests`

**Interfaces:**
- Produces:
  - Minor 1: `render_bar`'s `disabled` also holds while `self.is_starting()`.
  - Minor 2: `ChatSession::leftover(&self, cx) -> (String, Vec<Attachment>)` (typed text and queued texts as today; the chat's attachments then the queued ones). `DraftView::fail(&mut self, error: Option<String>, leftover: (String, Vec<Attachment>), window, cx)` appends the attachments. Other callers pass `Default::default()`.
  - Minor 7: `can_send` is false while `worktree && repo.is_none()`; `send_draft`'s `(None, true, _)` arm returns without a notice (the copy "Still reading the repository…" goes).
  - Minor 9: `DraftView.creating: bool` ("a worktree is being created for this draft: the next send waits for it to be recorded"); set when a `GitJob::Worktree` starts, cleared when its result comes back (every outcome, Esc included); `can_send` requires `!creating`.
  - Minor 12: in Local mode, when `repo.current == Some(repo.default)`, the "Default branch" section and row are left out (the Current row already names it).
  - Race (D12): `ChatSession::send_settled(&mut self, text: &str, cx) -> bool`: false when not Idle or `text.trim()` is empty; else pushes the user bubble, sets `first`, calls `send_first`.

- [ ] **Step 1: Write the tests:**
  - `pending_chat_send_is_off_while_starting` (start.rs, `gated`): send a draft, type "more" in the pending chat → `find("send").disabled() == Some(true)`; open the gate → `wait_until` the session lands.
  - `attachments_added_while_starting_go_back_to_the_draft` (gated): send; push `Attachment::new(&file)` into the chat; Esc → the draft's `attachments` contain `file`, the draft's own attachments too.
  - `worktree_send_waits_for_the_repository`: draft with `worktree = true`, `repo = None` → `!can_send`.
  - `send_waits_while_a_worktree_is_created`: `d.creating = true` → `!can_send`; after a normal worktree send lands, `!d.creating`.
  - `current_default_branch_is_listed_once`: repo on `main`, Local → no branch item has the value `main` (the Current row is titled `main`); with `set_worktree(true)` the `main` row is back.
  - `extension_prompt_waits_for_the_default_model` (view.rs, `--models`, `settings.models = {"fake": "fake-large"}`): `start_session("fake", Some("hi"), true, None)` → `wait_until` `last_agent_text == Some("model:fake-large")`.
- [ ] **Step 2: Implement.**
- [ ] **Step 3:** `cargo check --workspace --all-targets`. Expected: no errors.
- [ ] **Step 4: Commit.** `git commit -m "fix: tidy the draft and pending-chat starts"`

---

### Task 7: Onda 1 minors, git, fixtures and titles (Minors 3, 5, 6, 8, 10; I10 test)

**Files:**
- Modify: `crates/wai/src/git.rs` (`create_worktree`, `tests::repo_with`), `crates/wai-agent/tests/fake_agent.rs` (`close_session_reaches_the_agent`), `crates/wai/src/workspace.rs` (doc comments at ~162 and ~1831; `sessions/list` title ~1596), `crates/wai/src/chat/model.rs` (`named_title`), `crates/wai/src/sidebar.rs` (Rename prefill ~359)

**Interfaces:**
- Produces:
  - Minor 8: `create_worktree` fetches only when `refs/remotes/origin/<base>` already exists; otherwise `fetch_failed = None` and `from = <base>`.
  - Minor 5: `repo_with` gives commit `n` (0-based, across the whole helper) `GIT_AUTHOR_DATE` and `GIT_COMMITTER_DATE` `@<1700000000 + n> +0000` (through `command(dir)`, keeping the `-c` user flags).
  - Minor 6: the close test writes into a `tempfile::tempdir()`.
  - Minor 3: "Whether the batch names `path`…" moves above `fn touches`; "An Open row's ×: drops `session`…" moves above `fn end_session`.
  - Minor 10 (D11): `ChatModel::named_title(&self) -> Option<&str>` = name, else the agent's title; `sessions/list`'s `"title"` and the Rename prefill use it. `shown_title`'s doc says it falls back to the first message's first line.

- [ ] **Step 1: Write the tests:**
  - `never_pushed_base_does_not_report_a_fetch_failure` (git.rs): `cloned`; `checkout -b local-only` with a commit → `create_worktree(.., "local-only")` has `fetch_failed == None`, `from == "local-only"`.
  - `worktree_branch_tracks_nothing` (I10): after `create_worktree` from `origin/main`, `rev-parse --abbrev-ref wai/x@{upstream}` fails.
  - `repo_with_orders_branches_by_creation`: `repo_with(["a", "b"])` → `repo().branches == ["b", "a", "main"]`.
  - `named_title_has_no_fallback` (model.rs): a model with a User item and no title → `shown_title() == Some(<first line>)`, `named_title() == None`; with `name = Some("Mine")` → both `Some("Mine")`.
- [ ] **Step 2: Implement.**
- [ ] **Step 3:** `cargo check --workspace --all-targets`. Expected: no errors.
- [ ] **Step 4: Commit.** `git commit -m "fix: tidy git fetch, test fixtures and session titles"`

---

### Task 8: Track gate, final review, one fix wave

- [ ] **Step 1: Merge `development`** into the branch if earlier tracks landed (`git log --oneline feat/onda2-sessions..development`); resolve in the worktree; `cargo check --workspace --all-targets`.
- [ ] **Step 2: Full gate** (Global Constraints). Record pass/fail counts and each failure's test name and first error line.
- [ ] **Step 3: One whole-branch review** (most capable model) against this plan: Decisions, Review Focus, copy, the "not ours" files untouched, no polling.
- [ ] **Step 4: One fix wave** for gate failures and the review's Critical/Important findings (Minors deferred, listed in the report). Commit `fix: apply the sessions track review`. Re-run the gate. Expected: fmt and clippy clean, `cargo test --workspace` 0 failed.
- [ ] **Step 5: Report** to the controller: commits, gate counts, deferred minors, and what the owner checks in the app (1.10 with real Claude in `waycarbon-ecosystem-local`: enter `inventories`, create a worktree, read a file there; 2.1 with `claude --resume <id>` in the wai terminal).

---

### Task 9: 0.8 proof, 20 consecutive full-suite runs under load

Run after Task 8 is green, in the background (long), with nothing else building.

- [ ] **Step 1: Run:**
  ```sh
  export PATH="$HOME/.cargo/bin:$PATH"
  df -h /System/Volumes/Data      # stop if under 8 GB free
  cargo build -p wai-agent --bin wai-fake-agent && cargo test --workspace --no-run
  n=$(( $(sysctl -n hw.ncpu) / 2 )); : > /tmp/wai-load.pids
  for i in $(seq $n); do yes > /dev/null & echo $! >> /tmp/wai-load.pids; done
  pass=0; for i in $(seq 20); do cargo test --workspace -q > /tmp/wai-run-$i.log 2>&1 && pass=$((pass+1)) || break; done
  kill $(cat /tmp/wai-load.pids); echo "passed $pass/20"
  ```
  Kill only the pids in `/tmp/wai-load.pids`.
- [ ] **Step 2: Pass criterion:** the output ends with `passed 20/20`. On a failure, read `/tmp/wai-run-<i>.log`. Fix it here only when its cause is the same class as 0.8's (a wall-clock budget on work that grows with load, or a race against a real process), commit `test: …`, and restart the count from 0. Any other cause: stop and report the test name and the log to the controller.
- [ ] **Step 3:** `cargo clean` in the worktree. Report the count.

---

## Coverage

| Item | Task |
|---|---|
| 2.6 R84: events reach the session holding the id; the older holder says so; Restart takes it back | 1 |
| 1.10 signal, git resolution off the UI thread, per-directory cache, last read wins, no polling | 2, 3 |
| 1.10 several repositories, outside a repository, subagents, detached submodule | 3 |
| 1.10 resume/restore restores the place; workspace does not follow (D6); branch re-read per turn | 3 |
| 1.10 "New worktree" stays disabled with submodules | untouched |
| 2.1 detection (process, fresh transcript, own recent close) | 4 |
| 2.1 warn on resume (click, side button, drag), offer Fork (History mark deferred to the sidebar redesign) | 5 |
| Onda 1 sessions minors still valid | 6, 7 |
| Gate, review, fix wave; 0.8 proof | 8, 9 |
