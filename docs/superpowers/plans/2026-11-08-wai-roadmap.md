# wai — Roadmap and Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build **wai**, a native AI-first IDE for macOS and Windows that drives coding agents (Claude first) through the Agent Client Protocol. It ships with an extension system and a first WayCarbon extension.

**Architecture:**
- One native Rust process with a GPU UI (GPUI via `gpui-kit`).
- Every agent session is a child process speaking ACP (JSON-RPC over stdio).
- Every extension with logic is a child process speaking JSON-RPC over stdio.
- Model crates (agent, history, terminal, settings, extensions) do not depend on GPUI and are unit-tested. The `wai` binary crate holds the views.
- Everything is event-driven. Nothing polls.

**Tech Stack:**

| Area | Choice |
|---|---|
| Language | Rust (stable) |
| UI | `gpui-kit =0.9.0` (pins `gpui-pre =0.3.11`) |
| Agent protocol | `agent-client-protocol 5` |
| Claude adapter | `@agentclientprotocol/claude-agent-acp` (npm, run with `npx`) |
| Terminal | `alacritty_terminal 0.26` + `portable-pty 0.9` |
| File watching / search | `notify 8`, `ignore` |
| Diff | `similar 3` |
| Config and serialization | `serde`, `toml`, `serde_json` |
| Async | `smol` (same runtime family GPUI uses) |

**Spec:** `docs/ESTUDO.md` (study, decisions in §1, architecture §8, features §9, metrics and phases §10, rewrite rationale §12)

**Plan style:**
- **Rolling wave.** Phase 0 is detailed below.
- **One plan per phase.** Each later phase gets its own detailed plan file (`docs/superpowers/plans/2026-11-08-wai-fase-N.md`), written at the start of that phase. Phase 0 spikes fix the real GPUI/ACP APIs first, and exact signatures cannot honestly be written before that.
- **Fixed now:** the deliverables, interfaces and acceptance tests of every phase, listed below.

**Execution:**
- **Phase 0** (spikes, learning): native, inline.
- **Phases 1–4:** subagent-driven, with one reviewer pass per task and a whole-phase review before merge.
- **Knowledge handoff:** API knowledge learned in Phase 0 is written to `docs/NOTES-gpui.md` and `docs/NOTES-acp.md` so fresh implementers do not rediscover it.

## Global Constraints

- **Platforms:**
  - macOS arm64 is primary and verified on this machine.
  - Windows x86_64 is a required target, verified by CI (`windows-latest`).
  - Linux is best-effort and not gated.
- **GPUI:**
  - consumed only through `gpui-kit = "=0.9.0"`;
  - must build with Xcode Command Line Tools only (no `metal` binary on this machine); the `runtime_shaders` feature is required.
- **Agent:**
  - the default Claude agent command is `npx -y @agentclientprotocol/claude-agent-acp@0.101.0`;
  - the command must be configurable per agent in settings.
- **Credentials:**
  - wai never reads, stores, forwards or prompts for credentials of any provider;
  - wai never shows a provider login screen;
  - if an agent reports it is unauthenticated, wai shows: `"<agent> is not signed in. Open a terminal and run its login (for Claude: claude /login)."`
- **No polling:** no polling timers for agent, session, file or history state. Use streams and file-system events. UI animation frames are allowed only while something animates.
- **Licensing:**
  - no code copied from `central-ia-waycarbon-claude` (it has no license);
  - no code copied from Zed's GPL crates (`editor`, `rope`, `terminal`, `terminal_view`, …);
  - dependencies must be permissive (MIT/Apache-2.0/BSD/ISC/Zlib/Unicode/MPL-2.0); enforced by `cargo deny check licenses` in CI.
- **Performance budgets** (measured on this Mac, release build):
  - idle CPU ≈ 0% (< 0.5% average over 30 s);
  - RAM < 300 MB with 1 project, 3 sessions and 1 terminal;
  - window visible < 1 s after launch;
  - a 100 MB file opens and scrolls without a freeze > 100 ms.
- **Language:**
  - code, comments, UI copy, commit messages and README are in English;
  - owner-facing reports (`docs/ESTUDO.md`, `docs/spikes/*`, `docs/RELATORIO-*.md`) are in pt-BR.
- **Commits:**
  - format `feat:` / `fix:` / `refactor:` / `test:` / `docs:` / `chore:`, without scope;
  - no `Co-Authored-By` line;
  - no Jira reference (personal project, no card);
  - no pushing: there is no remote until the owner creates one.
- **Branches:**
  - `main` holds finished phases;
  - `development` is the integration branch;
  - each phase works on `feat/fase-N-<slug>`, merged into `development` with `--no-ff` after the phase review, then fast-forwarded to `main` and tagged `fase-N`.
- **Quality gate per task:**
  - `cargo fmt --all --check`;
  - `cargo clippy --workspace --all-targets -- -D warnings`;
  - `cargo test --workspace`.
- **Quality gate for UI tasks:** in addition, run the app and capture a screenshot with `screencapture` into `docs/screens/`.
- **Disk:**
  - only ~34 GB are free, so all crates and spikes share one workspace `target/`;
  - run `cargo clean -p <spike>` when spikes are deleted.

## Review Focus

1. **Agent process crashes, is missing (`npx` absent, offline) or exits mid-turn.**
   - The session shows an error state with the last 20 lines of agent stderr and a Restart action.
   - The app never hangs.
   - Test: `wai-agent` integration test where `wai-fake-agent --crash-after 1` dies mid-stream, asserting an `AgentEvent::Exited { code, stderr_tail }`.
   - Owner: Phase 1, Task 1.2.
2. **Agent not signed in.**
   - The ACP `authenticate` requirement or an auth error is shown with the exact copy from Global Constraints. No login UI.
   - Test: fake agent `--require-auth` → `AgentEvent::AuthRequired`.
   - Owner: Phase 1, Task 1.2.
3. **Huge outputs: multi-MB assistant text or tool output.**
   - The chat list stays virtualized.
   - Tool output is truncated to 2,000 lines with "Show all".
   - Test: fake agent `--flood 5MB`; assert that the chat model keeps every chunk and the view model exposes a truncated tool output.
   - Owner: Phase 1, Task 1.3.
4. **Permission request in a session that is not focused, or answered after the agent cancelled it.**
   - The session gets a "needs you" badge and a native notification.
   - A late answer is ignored without panic.
   - Test: `PermissionBroker` test answering after cancel returns `Err(Stale)`.
   - Owner: Phase 1, Task 1.2.
5. **Quitting with running sessions or terminals.**
   - Ask for confirmation, then kill child process trees.
   - No orphan `node`/`claude` processes remain.
   - Test: `wai-agent` test drops `AgentConnection` and asserts the child PID is gone within 2 s.
   - Owner: Phase 1, Task 1.2.

---

## Phase 0 — Spikes (throwaway, native execution)

**Exit gate:**
- `docs/spikes/S1-acp.md`, `docs/spikes/S2-ui.md` and `docs/spikes/S3-baseline.md` exist, each with a go/no-go verdict.
- `docs/NOTES-gpui.md` and `docs/NOTES-acp.md` exist.
- The Phase 1 plan is written.

**Kill criteria:**
- **S2 fails** (cannot build on this Mac without full Xcode, idle CPU > 1%, RAM > 300 MB, or the 100 MB file freezes > 1 s): switch the UI to Tauri 2 + React and rewrite this roadmap's stack before Phase 1.
- **S1 fails with subscription login:** switch the Claude path to `claude -p --input-format stream-json --output-format stream-json` behind the same `wai-agent` interface.

### Task 0.1: Repository and toolchain bootstrap

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, `README.md`, `deny.toml`, `.github/workflows/ci.yml`

- [ ] **Step 1:** Install Rust with `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal -c rustfmt,clippy`.
  - Run `~/.cargo/bin/rustc --version`.
  - Expected: `rustc 1.x.y`.
- [ ] **Step 2:** Pin the toolchain.
  - `rust-toolchain.toml` with `channel = "<installed stable>"`, `components = ["rustfmt", "clippy"]`, `targets = ["aarch64-apple-darwin", "x86_64-pc-windows-msvc"]`.
- [ ] **Step 3:** Create the workspace `Cargo.toml`.
  - `members = ["crates/*", "spikes/*"]`, `resolver = "3"`.
  - Shared `[workspace.package]` with `edition = "2024"`.
  - `[workspace.dependencies]` pins as in the Tech Stack table.
- [ ] **Step 4:** Add `deny.toml`.
  - Allow-list the licenses from Global Constraints and deny `GPL-*`/`AGPL-*`.
- [ ] **Step 5:** Add `.github/workflows/ci.yml`.
  - Matrix `macos-latest` and `windows-latest`.
  - Steps: fmt check, clippy `-D warnings`, test, `cargo deny check licenses`.
- [ ] **Step 6:** `git init -b main`.
  - Commit `chore: bootstrap wai workspace`.
  - Then `git checkout -b development` and `git checkout -b feat/fase-0-spikes`.

### Task 0.2: S1 — ACP with Claude

**Files:**
- Create: `spikes/s1-acp/Cargo.toml`, `spikes/s1-acp/src/main.rs`, `docs/spikes/S1-acp.md`, `docs/NOTES-acp.md`

- [ ] **Step 1:** Read the `agent-client-protocol` 5.x client example in `~/.cargo/registry/src/*/agent-client-protocol-5.*/examples/`. Write `docs/NOTES-acp.md` with:
  - the client trait methods: request permission, session notification, read/write text file;
  - `initialize` / `new_session` / `load_session` / `prompt` / `cancel`;
  - the `SessionUpdate` variants.
- [ ] **Step 2:** Implement `spikes/s1-acp` as a CLI with the following behavior:
  - spawns the default Claude agent command with stdio pipes;
  - runs `initialize` with fs read/write capabilities and opens `new_session(cwd = a temp dir)`;
  - sends the prompt `Create hello.txt containing the word wai, then reply with DONE.`;
  - prints every `SessionUpdate` as one line;
  - answers permission requests from stdin (first option when piped);
  - sends `cancel` on Ctrl+C;
  - at the end, tries `load_session` for the same id.
- [ ] **Step 3:** Run it with the owner's existing Claude Code login (no API key in env).
  - Record: did auth work without a key?
  - Record: which update kinds arrived, whether the permission request arrived, whether `hello.txt` was written through the client fs, whether `load_session` works, and time-to-first-token.
- [ ] **Step 4:** Write `docs/spikes/S1-acp.md`.
  - Findings, a verdict (go ACP / fallback stream-json), and open risks.
  - Commit `docs: record S1 ACP spike results`.

### Task 0.3: S2 — GPUI window, editor, terminal, streaming markdown

**Files:**
- Create: `spikes/s2-ui/Cargo.toml`, `spikes/s2-ui/src/main.rs`, `docs/spikes/S2-ui.md`, `docs/NOTES-gpui.md`

- [ ] **Step 1:** Read the `gpui-kit`/`gpui-component` 0.9 examples in the cargo registry. Write `docs/NOTES-gpui.md` covering:
  - app bootstrap;
  - window open;
  - `Entity`/`Context`;
  - `Render`;
  - actions and keybindings;
  - async (`cx.spawn`, background executor);
  - the dock/resizable panels;
  - the code editor (`InputState` code editor mode);
  - markdown (`TextView`);
  - virtual list;
  - theming.
- [ ] **Step 2:** Implement `spikes/s2-ui` with one window and three panes:
  - **left:** the code editor opening a generated 100 MB file (`spikes/s2-ui/gen-big.sh` writes it to the target dir, not the repo);
  - **right:** a markdown view fed by a `smol` channel. A background task appends ~40 tokens per second of sample markdown for 20 s;
  - **bottom:** a terminal grid. Use `portable-pty` to spawn `$SHELL`, feed `alacritty_terminal::Term`, and render cells as text rows with a monospace font. Key input is forwarded.
- [ ] **Step 3:** Run the release build.
  - Record launch-to-window time.
  - Record idle CPU and RSS (`top -l 6 -s 5 -pid <pid> -stats cpu,mem`) after the stream ends.
  - Record scroll behaviour in the 100 MB file.
  - Take a screenshot to `docs/screens/s2-ui.png`.
- [ ] **Step 4:** Run `rustup target add x86_64-pc-windows-msvc && cargo check -p s2-ui --target x86_64-pc-windows-msvc`. Record the result; the expected failure modes are C-toolchain related.
- [ ] **Step 5:** Write `docs/spikes/S2-ui.md` with the numbers versus the budgets and a verdict.
  - Commit `docs: record S2 GPUI spike results`.

### Task 0.4: S3 — Baseline of the current Central

**Files:**
- Create: `docs/spikes/S3-baseline.md`

- [ ] **Step 1:** Copy the measurements already taken on 2026-11-08 (ESTUDO §12).
  - Add RSS of the Chrome renderer serving `https://localhost:8443`, if identifiable via `ps` plus open connections. Otherwise mark it unmeasured.
- [ ] **Step 2:** Commit `docs: record S3 Central baseline`.

### Task 0.5: Gate and Phase 1 plan

- [ ] **Step 1:** Apply the kill criteria. Update this roadmap's Tech Stack if anything changed.
- [ ] **Step 2:** Write `docs/superpowers/plans/2026-11-08-wai-fase-1.md`, using the real APIs from the NOTES files.
- [ ] **Step 3:** Delete `spikes/` and run `cargo clean`. Merge `feat/fase-0-spikes` → `development` → `main`, tag `fase-0`.

---

## Phase 1 — Core MVP on macOS (detailed plan written in Task 0.5)

**Target workspace:**

```
crates/
  wai/              bin: main.rs, app.rs (bootstrap, actions, keymap), views/ (sidebar, chat, terminal, notifications)
  wai-agent/        ACP client, no GPUI
  wai-fake-agent/   bin: scripted ACP agent for tests (--crash-after N, --require-auth, --flood SIZE, --ask-permission)
  wai-history/      ~/.claude/projects index, incremental, no GPUI
  wai-terminal/     PTY + alacritty_terminal model, no GPUI
  wai-settings/     settings.toml + layout.json persistence, no GPUI
```

**Interfaces fixed now** (names are binding; field details are finalized in the Phase 1 plan):

- **`wai-agent`:**
  - `AgentSpec { name: String, command: String, args: Vec<String>, env: Vec<(String,String)> }`
  - `AgentConnection::spawn(spec: &AgentSpec, cwd: &Path) -> Result<AgentConnection>`
  - `AgentConnection::new_session(&self, cwd: &Path, mcp: Vec<McpServer>) -> Result<SessionHandle>`
  - `AgentConnection::load_session(&self, id: &SessionId, cwd: &Path) -> Result<SessionHandle>`
  - `SessionHandle::prompt(&self, blocks: Vec<PromptBlock>) -> Result<StopReason>`
  - `SessionHandle::cancel(&self)`
  - `SessionHandle::events(&self) -> async_channel::Receiver<AgentEvent>`
  - `AgentEvent`: `MessageChunk{role, text}`, `ThoughtChunk(String)`, `ToolCall(ToolCallView)`, `ToolCallUpdate(ToolCallView)`, `Plan(Vec<PlanEntry>)`, `PermissionRequested(PermissionRequest)`, `TurnEnded(StopReason)`, `AuthRequired{agent}`, `Exited{code: Option<i32>, stderr_tail: Vec<String>}`
  - `PermissionBroker::answer(id: RequestId, option: OptionId) -> Result<(), BrokerError>`, where `BrokerError::Stale` covers a cancelled or unknown request.
- **`wai-history`:**
  - `HistoryIndex::open(root: &Path) -> Result<HistoryIndex>`
  - `HistoryIndex::sessions(&self) -> Vec<SessionSummary{id, title, cwd, updated_at, message_count}>`
  - `HistoryIndex::apply_fs_event(&mut self, ev: notify::Event)`
  - Reads only appended bytes (tracks `(inode, offset)` per file).
- **`wai-terminal`:**
  - `TerminalModel::spawn(shell: &str, cwd: &Path, size: GridSize) -> Result<TerminalModel>`
  - `write(&self, bytes: &[u8])`
  - `resize(&self, size: GridSize)`
  - `snapshot(&self) -> GridSnapshot{rows: Vec<Row>, cursor}`
  - `events() -> Receiver<TerminalEvent{Wakeup, Title(String), Exited(i32)}>`
- **`wai-settings`:**
  - `Settings::load(user_dir: &Path, project_dir: Option<&Path>) -> Settings`; project `.wai/settings.toml` overrides user `~/.config/wai/settings.toml`.
  - `Layout::load/save(path)`.

**Tasks** (each has unit tests; UI tasks add a screenshot):

| Task | Deliverable |
|---|---|
| 1.1 | Workspace crates skeleton + `wai-settings` with tests for override precedence and a missing-file default |
| 1.2 | `wai-agent` + `wai-fake-agent`. Integration tests for streaming, permission, cancel, crash, auth-required, and no orphan process (Review Focus 1, 2, 4, 5) |
| 1.3 | Chat view: virtualized message list, streaming markdown, tool-call rows with truncation (Review Focus 3), permission card with option buttons, prompt input (Enter sends, Shift+Enter newline, Esc cancels), image/file drop → ACP content blocks |
| 1.4 | Sessions sidebar: live sessions with state dot (working / needs-you / done / error) + history list from `wai-history` (watcher-driven), New, Resume (`load_session`), Fork (only when the agent advertises it), Close, "Open to the side" (up to 4 chat panes side by side) |
| 1.5 | Terminal panel: `wai-terminal` model + grid view, key input, resize, cmd+` toggle |
| 1.6 | Native notifications on needs-you / done while the window is unfocused; dock badge count |
| 1.7 | Layout and open-session persistence; restore on launch with `load_session` |
| 1.8 | Phase review + perf measurement against budgets → `docs/RELATORIO-fase-1.md` |

## Phase 2 — IDE

| Task | Deliverable |
|---|---|
| 2.1 | File tree (gitignore-aware via `ignore`, live via `notify`) |
| 2.2 | Editor tabs with gpui-component code editor, tree-sitter highlighting, save, large-file read-only mode above 20 MB |
| 2.3 | Agent edit review. wai advertises ACP client fs capabilities; agent writes go through `write_text_file`, are staged, and are shown as hunks (`similar`) with Accept / Reject per hunk and Accept all. Nothing touches disk before acceptance unless the session is in auto-accept mode |
| 2.4 | Project search (`ignore` + `grep-regex`/`grep-searcher`), results open in the editor |
| 2.5 | Agents panel: every session with plan entries, current tool call, sub-tool-calls; replaces the Central's "Escritório" in 2D |
| 2.6 | `.wai/` project config: conversation templates (name, agent, cwd, first prompt not sent), default agent |
| 2.7 | Phase review + `docs/RELATORIO-fase-2.md` |

## Phase 3 — Extensions v1 + Windows

| Task | Deliverable |
|---|---|
| 3.1 | `wai-ext` manifest (`wai-extension.toml`): `id`, `name`, `version`, `[contributes]` (templates, commands, agents, mcp_servers, claude_plugins, themes), `[process]` (command, args), `[permissions]` (process, network, listen, paths, control_sessions). Parser tests including unknown-field rejection |
| 3.2 | Install / uninstall from a local folder or git URL into `~/.config/wai/extensions/<id>`; consent dialog listing permissions; enable/disable |
| 3.3 | Extension host: JSON-RPC 2.0 over stdio. Host → ext: `initialize`, `event/*`, `command/run`, `panel/action`. Ext → host: `sessions/list`, `sessions/open`, `sessions/prompt`, `notify`, `panel/render`. Each call is checked against declared permissions |
| 3.4 | Declarative panel schema (list, tree, table, markdown, form, button, badge, progress) rendered with gpui-component; schema tests + screenshot |
| 3.5 | Contributions wired: templates, commands in the command palette, agents in the agent picker, MCP servers passed into `new_session`, `claude_plugins` shown as a one-click `claude plugin install` in the terminal (wai runs nothing silently) |
| 3.6 | `extensions/wai-waycarbon` (Go): templates per WayCarbon repo, `beehive-waycarbon` plugin + Jira MCP declared, "My cards" panel reading the `jira` CLI (uses the owner's existing CLI auth; wai never sees tokens) |
| 3.7 | Windows: CI green on `windows-latest`; ConPTY terminal path; path/CRLF tests |
| 3.8 | Phase review + `docs/RELATORIO-fase-3.md` |

## Phase 4 — Publication readiness

| Task | Deliverable |
|---|---|
| 4.1 | Packaging: `cargo-packager` config for `.app`/`.dmg` and NSIS `.exe`; app icon |
| 4.2 | Release workflow (tag `v*` → build both OSes → draft GitHub release). Signing and notarization steps read secrets that do not exist yet and are skipped when absent |
| 4.3 | Auto-update check against GitHub releases (opt-in setting, no background polling; checks on launch only) |
| 4.4 | First-party extensions: `wai-rotinas` (scheduled scripts from an allow-list, notify on news), `wai-navegador` (manifest-only: Playwright MCP). `wai-remoto` (phone access): design doc + skeleton only; its security review needs the owner |
| 4.5 | README, user guide, extension authoring guide; `docs/RELATORIO-final.md` listing what needs the owner (license, Apple/Windows certificates, Anthropic confirmation, remote repo) |

## Owner-only items (cannot be done unattended)

- Choose the license and confirm with WayCarbon who owns the code.
- Create the remote repository and push. CI and Windows verification run only after that.
- Get an Apple Developer ID and a Windows signing certificate.
- Confirm with Anthropic that subscription use via `claude-agent-acp` in a third-party editor is acceptable before public launch.
