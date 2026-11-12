# wai sidebar follow-ups (1.11, 2.1b, 3.1, 3.2): Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On top of the merged sidebar redesign: a double click on the Sessions panel's empty space opens a draft (1.11); History rows mark a Claude conversation open elsewhere (2.1b); one subagent can be stopped from the agent list and its drawer (3.1); the drawer shows the subagent's own text, thinking and checklist (3.2).

**Architecture:** 1.11 wraps the History section's body (everything under its header) in one element whose double click opens a draft; History rows stop their clicks there. 2.1b runs the existing `elsewhere::open_elsewhere` in `refresh_history`'s background step and keeps the result in one `Workspace` field that `history_row` reads. 3.1/3.2 start from the deferred Tasks 3–4 of the chat track: `wai-agent` stops dropping a child's text, thinking and plan (two new `SessionEvent`s) and reads its `cancel` right; each `model::Subagent` keeps an ordered log of what it said and where its tool calls fell, its checklist and its stop right; the redesign's drawer draws that log, and a Stop square sits on the agent-list row and in the drawer header, sending `session/cancel` for the child id through the existing `AgentHandle::cancel`.

**Tech Stack:** Rust workspace, GPUI + gpui-kit 0.9.0, ACP (`agent-client-protocol-schema`, `claude-agent-acp` 0.103.0), `wai-fake-agent` for tests.

**Spec:** rows 1.11, 2.1b, 3.1 and 3.2 of `docs/superpowers/plans/2026-11-12-wai-execucao-pendencias.md` (pt-BR). Built on the sidebar redesign: spec `docs/superpowers/specs/2026-11-12-wai-sidebar-redesign-design.md` and plan `docs/superpowers/plans/2026-11-12-wai-sidebar-redesign.md` (both on `feat/sidebar-redesign`; its non-goal "showing a subagent's own text or checklist" is 3.2). Starting points: Tasks 3–4 (DEFERRED) and Decisions D3, D8 of `docs/superpowers/plans/2026-11-12-wai-onda-2-chat.md`; D10 and Task 5 of `docs/superpowers/plans/2026-11-12-wai-onda-2-sessions.md`.

**Planned against:** `feat/sidebar-redesign` at `85a1bf1` (worktree `.claude/worktrees/sidebar-redesign`), `development` at `0fc0a5a`. Line numbers below are from `85a1bf1`.

## Global Constraints

- Start only after `feat/sidebar-redesign` is merged into `development`. Run `git log --oneline -5 development` first: another session commits here. Work in a worktree `.claude/worktrees/sidebar-followups` on branch `feat/sidebar-followups` from `development` (the controller's names win if it gives others). **No push, no merge into `development`.**
- Commits: `feat:` / `fix:` / `test:` / `docs:`, no scope, no Co-Authored-By, no Jira ref; imperative, first line < 72 chars.
- **Owner's pace.** Per task: write its tests and code, run only `cargo check --workspace --all-targets` (expected: no errors), then commit. No per-task `cargo test`, clippy or review. Once, in Task 6: the full gate `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build -p wai-agent --bin wai-fake-agent && cargo build -p wai-ext --bin wai-fake-ext`, `cargo test --workspace --no-fail-fast`; then one opus final review and one fix wave.
- Every shell: `export PATH="$HOME/.cargo/bin:$PATH" _ZO_DOCTOR=0`. Before a build, `df -h /System/Volumes/Data`; under 8 GB free, stop and report. `cargo clean` in the worktree when done.
- **Idle CPU < 0.5 %.** No timers, no polling. The elsewhere check runs only in `refresh_history`'s background step (agent ready, each turn's end, each new session). A subagent's words wake nothing while its drawer is closed, and reach an open drawer on the existing `STREAM_FLUSH` (100 ms).
- UI copy in English, exactly as quoted here: `Stop subagent`, `Open elsewhere`, `+N more`.
- Existing test ids stay: `tab-sessions`, `draft-row`, `open-row`, `open`, `history`, `history-row`, `history-side`, `history-section`, `all-projects`, `history-hint`, `agent-list`, `agent-main`, `("agent-row", ix)`, `agent-hint`, `subagent-drawer`, `subagent-drawer-close`, `subagent-answer`, `("subagent-card", ix)`, `("tool", ix)`, `("step", ix)`, `waiting`, `stop`.
- Tests never touch the real `~/.config/wai`, `~/.claude` or `~/.claude.json` (`workspace::tests::open` points `claude_home` and `user_dir` at temp dirs; the elsewhere tests set `w.claude_home` to their own temp dir). Never kill `wai`, `claude` or `claude-agent-acp` processes a test did not start.
- Not ours: never touch `crates/wai/src/update.rs`, the Updates section of `settings_view.rs`, the quit-confirmation path of `workspace.rs`, `.github/workflows/*`, `CLAUDE.md`.
- **Shared Sessions panel.** The work-areas plan (`2026-11-12-wai-work-areas.md`) also edits the redesign's Sessions panel. Here, in `sidebar.rs`, touch only `history_section`, `history_row` and `refresh_history` (and add tests); in `workspace.rs`, only one field and its initializer in `Workspace::new`. Never `render_sessions`, `open_row`, `draft_row`, `group_header`. Whichever branch lands second merges `development` in and resolves.
- Follow existing patterns (`aurora::*` colours, `Button` ghost/xsmall, `.test_support()` to register ids, `cx.stop_propagation()` in a button inside a clickable row). Comments only for a non-obvious why; `ponytail:` comments for deliberate ceilings.

## Decisions to confirm

| # | Decision | Cost if wrong |
|---|---|---|
| D1 | 1.11 "empty part" = the History section's body, under its header: the space below the last History row, under a folded History header, or around the "Choose an AI" hint. Only a double click (`click_count() == 2`) opens a draft, as **New session** does. History rows keep their own clicks (they stop propagation); the Open section, group and section headers, the filter, the title row and the All switch are outside the body. | With a History long enough to fill the panel there is no empty space; ⌘N, **+** and the rail icon still open a draft. |
| D2 | 2.1b mark: computed in `refresh_history`'s background step, only when the default agent is `claude`, for every listed History id except sessions open in this wai (their own agent process names them). `open_elsewhere`'s rules are unchanged (sessions D8). The click on a row still runs its fresh check and dialog. | A conversation opened in a terminal while wai sits idle shows no mark until the next refresh. A conversation wai just ended may show the mark until the next refresh if its `claude` process outlives the close (process check; `quiet` covers only the transcript check). |
| D3 | The mark: an amber `Monitor` icon (already in `ExtraIcons`, the draft's Local chip) at 11 px before the row's date; tooltip and aria label `Open elsewhere`. | Cosmetic. |
| D4 | Stop places: a square (`IconName::Square`, tooltip `Stop subagent`) at the right end of each agent-list row whose subagent is stoppable, and in the drawer header before **×**. Not on the transcript card (it opens the drawer), not in the Agents panel, no key. | One more click for keyboard users: ↓ into the list, Enter opens the drawer, then the mouse. |
| D5 | Stop sends `session/cancel` with the child's id (`AgentHandle::cancel`) only while the subagent is Running or NeedsYou **and** the agent granted `capabilities.cancel`. Its open cards close as `Cancelled` at once (the broker answers them). Nothing changes optimistically: the row reads `✗` when the agent reports `idle`/`cancelled` (chat D3: accepting the stop "is no evidence that the child stopped"). | A second or so before the mark flips; a second click sends a harmless second cancel. |
| D6 | Drawer layout: the checklist is pinned between the header and the body (first `MAX_PLAN_ROWS` = 8 rows with `agents_view::plan_row`, then `+N more`). The body follows the subagent in arrival order: each tool call as today's step line, its thinking as an open muted box (like an unfolded **Thought**), its text as Markdown. The answer block (`subagent-answer`) shows only while the subagent said no text of its own: claude-agent-acp streams the child's final answer as its own agent message and sends it again as the reply (`native-subagents.js` `launchReply`). | A subagent whose text is not its answer hides the reply; the reply is still on the card's `text` for a later change. |
| D7 | Not shown: the subagent's prompt (not asked for; the header's task names it), its title changes, modes and commands. The prompt would be one route arm (`SessionMessage` to the child) and one log entry kind. | The drawer starts with the first thing the subagent does, not what it was told. |
| D8 | Restart and restore clear `model.subagents` and the drawer's Markdown before the replay (chat D8): claude-agent-acp re-announces and replays the children (`subagent-replay.js`). | A subagent the replay does not bring back leaves the list. |

## Verified and dropped

- Facts checked for this plan: claude-agent-acp 0.103.0 (`~/.npm/_npx/46301f59f66b1e3b/node_modules/@agentclientprotocol/claude-agent-acp/dist/native-subagents.js`) announces each child with `capabilities: { cancel: {} }` (`announceNativeSubagent` ~1370) and `cancelChild` (~682) stops only that child; the child's answer is its own agent message as well as the reply (`launchReply` ~1520). In wai: `AgentHandle::cancel` sends `session/cancel` then `cancel_pending(Some(id))` (`actor.rs` ~776), and a subagent's requests are parked under the child id (`Broker::attribute` ~350); `Subagents::route` drops a child's text, thinking and plan (`_ => None`, `subagents.rs` ~56); `ChatSession::load` does not clear `model.subagents` (`view.rs` ~1015); the fake already sends the child's plan `Sub step` and text `sub text` (`subagent_start`).
- Dropped, already done by the redesign: **finished subagents in the list.** `ChatModel::shown_subagents` lists the current turn's subagents, finished included, plus live ones from earlier turns; older ones open from their transcript card.
- Dropped, replaced by the redesign's drawer: chat Task 5's Stop on the transcript card (D4), chat Task 6's side `Sheet` (`SubagentTranscript`, `Open transcript`, `transcript_panel`), chat Task 7's Agents-tab lines for finished subagents (the agent list under the prompt is where subagents are seen now; the Agents panel keeps its live lines), chat D6's single Markdown transcript with `→ title` lines (the drawer already draws tool calls as real step lines).
- Dropped: `SubagentPart::Prompt` / `convert::subagent_prompt` (D7).

## Review Focus

1. **Claude sends a subagent's final answer twice** (as its own text, then as the reply): the drawer shows it once. Tests: `drawer_shows_the_answer_only_for_a_silent_subagent` and the updated `subagent_shows_in_the_chat` (Task 3).
2. **Stop clicked on an agent-list row**: it stops that subagent only and opens no drawer; once it is stopped or finished, or when the agent granted no `cancel`, no Stop shows and nothing is sent. Tests: `stoppable_needs_a_live_subagent_that_allows_it` (Task 2), `a_subagent_stops_from_the_agent_list` (Task 4).
3. **Restart or restore of a session that ran subagents**: the replay doubles no subagent's words, and no log entry points at a cleared transcript row. Test: `restart_clears_the_subagents` (Task 2).
4. **A double click on a History row** opens that conversation, never a draft too. Test: `double_click_on_the_empty_panel_opens_a_draft` (Task 1).
5. **A History row whose conversation is open in this wai**: never marked, though wai's own agent process names it. Test: `history_marks_conversations_open_elsewhere` (Task 5).

## File map

| File | Responsibility | Tasks |
|---|---|---|
| `crates/wai/src/sidebar.rs` | `history_section` body + double click; `history_row` stops its click, draws the mark; `refresh_history` computes the marks; tests | 1, 5 |
| `crates/wai/src/workspace.rs` | field `elsewhere` + its initializer | 5 |
| `crates/wai/src/elsewhere.rs` | the mark's test | 5 |
| `crates/wai-agent/src/types.rs`, `convert.rs`, `subagents.rs` | `SubagentSaid`, `SubagentPlan`, `SubagentPatch::can_stop`; routing; tests | 2 |
| `crates/wai-agent/src/bin/wai-fake-agent.rs`, `tests/fake_agent.rs` | `--subagent` grants `cancel`, thinks, honours a child cancel; tests | 2 |
| `crates/wai/src/chat/model.rs` | `Said`, `Subagent::{said, plan, can_stop}`, `stoppable`; tests | 2 |
| `crates/wai/src/chat/view.rs` | `load` clears; `sub_texts`, `sync_markdown`, `handle_event`/`schedule_flush` wake; `stop_subagent`; tests | 2, 3, 4 |
| `crates/wai/src/chat/agents.rs` | drawer body, checklist, Stop on the row and in the header | 3, 4 |
| `crates/wai/src/chat/render.rs`, `crates/wai/src/agents_view.rs` | `output_box`, `plan_row` visibility | 3 |
| `docs/user-guide.md` | lines 25, 66, 68, 71, 129 (at `85a1bf1`) | 1, 3, 4, 5 |

---

### Task 1: 1.11, a double click on the Sessions panel's empty space opens a draft

**Files:**
- Modify: `crates/wai/src/sidebar.rs` (`history_section` ~836, `history_row` ~1173; test beside `double_click_on_sessions_tab_opens_a_draft` ~1675)
- Modify: `docs/user-guide.md` (line 25)

**Interfaces:**
- Produces: element id `sessions-blank`: a `v_flex().flex_1().min_h_0()` under the History header row in `history_section`, always present (folded or not), holding today's hint or `uniform_list("history", …)` when unfolded. Its `on_click`: `click_count() == 2` → `self.new_session(&NewSession, window, cx)`.

- [ ] **Step 1: Re-verify the named functions after the redesign merges.** Confirm `85a1bf1` (or the redesign's later head) is in `development` (`git merge-base --is-ancestor 85a1bf1 development`). Then:
  ```sh
  grep -n -E "fn (history_section|history_row|refresh_history|render_drawer|render_agents|prune_agents|open_drawer|sub_stat|state_look|shown_subagents|patch_subagent|route|subagent_patch|subagent_start|subagent_finish|sync_row|schedule_flush|load|handle_event|plan_row|output_box|check_elsewhere|open_elsewhere|quiet|new_session)\b" \
    crates/wai/src/*.rs crates/wai/src/chat/*.rs crates/wai-agent/src/*.rs crates/wai-agent/src/bin/wai-fake-agent.rs
  ```
  Expected: each name is found (at `85a1bf1` some have namesakes elsewhere: `load` in `app.rs`, `route` in `workspace.rs`, `render_agents` in `agents_view.rs`, `new_session` in `actor.rs`). A renamed or moved one: follow it and note it in the Task 6 report. If the drawer, the agent list or the History section changed shape (not just lines), stop and report before writing code.
- [ ] **Step 2: Write the test** `double_click_on_the_empty_panel_opens_a_draft` (sidebar tests): `open(cx, history_settings(1))`, `load_history(&workspace, 1, cx)`; `window.double_click("sessions-blank", cx)` (after `render_frame`) → `drafts.len() == 1`, `sessions.is_empty()`; then `window.double_click(("history", 0usize), cx)`, `run_until_parked` → `drafts.len()` is still `1` (Review Focus 4).
- [ ] **Step 3: Implement.** In `history_section`, move the hint / list into `sessions-blank` (with `.test_support()`), keeping the list's `flex_1().min_h_0().pl(px(4.))`. In `history_row`, the `("history", ix)` click calls `cx.stop_propagation()` first.
- [ ] **Step 4: User guide**, line 25 becomes: "- **⌘N**, the **+** button, a double click on the **Sessions** icon, or a double click on the empty part of the **Sessions** panel (under **History**) starts one with the default agent (`default_agent`, which you choose in [Settings](#the-settings-screen); until you do, these open Settings instead)."
- [ ] **Step 5:** `cargo check --workspace --all-targets`. Expected: no errors.
- [ ] **Step 6: Commit.** `git commit -m "feat: open a draft on a double click in the empty sessions panel"`

---

### Task 2: 3.1/3.2 data, a subagent's words, checklist and stop right reach its model

**Files:**
- Modify: `crates/wai-agent/src/types.rs` (`SessionEvent` ~135, `SubagentPatch` ~174), `convert.rs` (`subagent_patch` ~84), `subagents.rs` (`route` and its tests), `actor.rs` (doc of `AgentHandle::cancel` only)
- Modify: `crates/wai-agent/src/bin/wai-fake-agent.rs` (`--subagent` doc ~18, `subagent_start` ~435, `subagent_finish` ~481); Test: `crates/wai-agent/tests/fake_agent.rs`
- Modify: `crates/wai/src/chat/model.rs` (`Subagent` ~207, `apply`, `patch_subagent`, new `stoppable`; tests), `crates/wai/src/chat/view.rs` (`load` ~1015; test), `crates/wai/src/chat/agents.rs` (test literal only)

**Interfaces:**
- Produces (`wai_agent`):
  ```rust
  // in `SessionEvent`:
  /// A subagent's text, or with `thought` its thinking, a chunk at a time.
  SubagentSaid { id: SessionId, thought: bool, text: String },
  /// A subagent's checklist, replaced whole.
  SubagentPlan { id: SessionId, entries: Vec<PlanEntry> },
  // in `SubagentPatch`:
  /// The agent lets the client stop it alone (`capabilities.cancel`); `None` means unchanged.
  pub can_stop: Option<bool>,
  ```
  `subagent_patch`: `capabilities` `Value(c)` → `Some(c.cancel.is_some())`, `Null` → `Some(false)`, `Undefined` → `None`. `route`'s child branch maps `AgentText` / `AgentThought` to `SubagentSaid` (`thought` false / true) and `Plan` to `SubagentPlan`, all with `id: session`, to the root; the ponytail line becomes "a subagent's title changes, modes and commands are not shown". `AgentHandle::cancel` doc: "`session` may be a subagent's id the agent granted `cancel`: only that subagent stops, and only its own open requests are answered `cancelled`."
- Produces (`chat::model`):
  ```rust
  /// What a subagent said, and where its tool calls fell, in order.
  #[derive(Debug, Clone, PartialEq)]
  pub enum Said {
      Text(String),
      Thought(String),
      /// The `items` row of one of its tool calls.
      Call(usize),
  }
  // `Subagent` gains:
  pub said: Vec<Said>,
  /// Its checklist, replaced whole.
  pub plan: Vec<PlanEntry>,
  /// The agent lets wai stop it alone.
  pub can_stop: bool,
  impl ChatModel {
      /// Whether wai can stop subagent `id` now: Running or NeedsYou, and its agent allows it.
      pub fn stoppable(&self, id: &str) -> bool;
  }
  ```
  `apply`: `SubagentSaid` appends to the known subagent's last entry when it is the same kind (`Text`/`Thought`), else pushes a new one, and returns `None` (unknown id: nothing); `SubagentPlan` replaces its `plan`, `None`; a `ToolCall` with `subagent: Some(id)` of a known subagent also pushes `Said::Call(<the row it was pushed at>)`. `patch_subagent` sets `can_stop` from `patch.can_stop` when `Some`. The four `Subagent { .. }` literals (`patch_subagent`; tests `agents.rs` `stat_says_time_then_tokens`, `view.rs` ~1718 and ~1835) gain `said: Vec::new(), plan: Vec::new(), can_stop: false`.
- `ChatSession::load`: `self.model.subagents.clear()` beside `self.model.items.clear()` (D8).
- Fake `--subagent` doc gains: "announced with `capabilities.cancel`; on the child, after the plan, the thought `sub thought` before the text; a `session/cancel` for the child stops it: `sub-read` completes `failed`, the parent gets `idle` (`cancelled`) and no reply."

- [ ] **Step 1: Write the `wai-agent` tests.**
  - `subagents.rs`: in `subagent_updates_route_to_the_root`, the two `None` asserts become `s.route("c", agent_chunk("x")) == Some(("r", SubagentSaid { id: "c", thought: false, text: "x" }))`, the same with an `AgentThoughtChunk("hm")` and `thought: true`, and `s.route("c", plan()) == Some(("r", SubagentPlan { id: "c", entries: [PlanEntry { content: "Sub step", status: "pending" }] }))`. `child_usage_goes_to_its_subagent`'s literal gains `can_stop: None`. In `states_and_replies_map` add: `can_stop` of `SubagentUpdate::new("c").capabilities(SubagentSessionCapabilities::new().cancel(SessionCancelCapabilities::new()))` is `Some(true)`, of `SubagentSessionCapabilities::new()` is `Some(false)`.
  - `fake_agent.rs`: in `subagent_traffic_reaches_the_parent`, the `patch` closure gains `can_stop: Some(true)`; add asserts that `updates` contains `SubagentSaid { id: sub, thought: true, text: "sub thought" }` and `SubagentSaid { id: sub, thought: false, text: "sub text" }`, and a `SubagentPlan { id: sub, entries }` with one entry; the existing `!…SessionEvent::Plan(_)` and `agent_text == "Hello from fake agent"` asserts stay.
  - New `cancelling_a_subagent_stops_only_it` (chat plan Task 3, unchanged): `spawn(["--subagent", "--background", "--ask-permission", "--reply", <50 × "word">, "--chunk-delay-ms", "20"])`; prompt; `request = permission(&rx)`; `agent.cancel(request.subagent.expect(..))`; `until` the request's `PermissionResolved`, a `Stopped` subagent state (`subagent_state(e, SubagentState::Stopped)`) and `TurnEnded`; assert the turn ended `StopReason::EndTurn`.
- [ ] **Step 2: Write the `wai` tests.**
  - `model.rs` (helper `said(id, thought, text) -> SessionEvent` beside `call`):
    ```rust
    #[test]
    fn a_subagent_keeps_what_it_said_in_order() {
        let mut m = ChatModel::default();
        m.apply(&subagent("c", Some("Explore"), Some(SubagentState::Running)));
        m.apply(&said("c", true, "Look"));
        m.apply(&said("c", true, "ing"));
        let ix = m.apply(&call("t1", "Read lib.rs", ToolStatus::InProgress, Some("c"))).unwrap();
        m.apply(&said("c", false, "Found "));
        m.apply(&said("c", false, "it"));
        let want = [Said::Thought("Looking".into()), Said::Call(ix), Said::Text("Found it".into())];
        assert_eq!(m.subagents[0].said, want);
        m.apply(&call("t2", "Run ls", ToolStatus::InProgress, None));
        assert_eq!(m.apply(&said("x", false, "?")), None);
        assert_eq!(m.subagents[0].said, want, "the session's call and a stranger's words");
        assert!(!m.items.iter().any(|i| matches!(i, ChatItem::Agent { .. } | ChatItem::Thought { .. })));
    }
    ```
    `subagent_plan_and_stop_right_follow_the_agent`: a `SubagentPlan` for `c` sets `subagents[0].plan` and leaves `m.plan` empty; `can_stop` is false until a patch with `can_stop: Some(true)`.
    `stoppable_needs_a_live_subagent_that_allows_it` (Review Focus 2): Running with `can_stop: None` → false; NeedsYou with `Some(true)` → true; Done → false; `stoppable("nobody")` → false.
  - `view.rs`: `restart_clears_the_subagents` (Review Focus 3): `fake_settings("--subagent")`, `send("hi")`, `wait_until` Idle with a subagent; `s.load(false, window, cx)`; `wait_until` Idle → `model.subagents.is_empty()`.
- [ ] **Step 3: Implement `wai-agent`** (Interfaces above). Then the fake:
  - Announce gains `.capabilities(acp::SubagentSessionCapabilities::new().cancel(acp::SessionCancelCapabilities::new()))`.
  - `subagent_start`: after the plan, an `AgentThoughtChunk` `sub thought` on the child, then today's `sub text`.
  - `subagent_finish`: the child was stopped when `is_cancelled(&sub.child)`, checked after the answer or the background wait; after an answer ending in `:cancelled`, also if that turns true within 1 s (checked every 10 ms: the cancel notification and the cancelled answer race). A stopped child completes `sub-read` as `Failed`, sends `idle` with `IdleStopReason::Cancelled` on the parent, and sends no reply. Otherwise as today.
- [ ] **Step 4: Implement `wai`** (Interfaces above).
- [ ] **Step 5:** `cargo check --workspace --all-targets`. Expected: no errors.
- [ ] **Step 6: Commit.** `git commit -m "feat: route subagent words, checklists and stop rights to the session"`

---

### Task 3: 3.2, the drawer shows the subagent's text, thinking and checklist

**Files:**
- Modify: `crates/wai/src/chat/view.rs` (field `sub_texts`; `sync_markdown`; `sync_row`'s subagent-card branch ~915; `handle_event` ~602; `schedule_flush` ~880; `load`; tests)
- Modify: `crates/wai/src/chat/agents.rs` (`render_drawer`)
- Modify: `crates/wai/src/chat/render.rs` (`output_box` ~265 becomes `pub(super)`), `crates/wai/src/agents_view.rs` (`plan_row` ~310 becomes `pub(crate)`)
- Modify: `docs/user-guide.md` (lines 66, 71)

**Interfaces:**
- Consumes: `Said`, `Subagent::{said, plan}`, `SessionEvent::SubagentSaid` (Task 2); `agents_view::{plan_row, MAX_PLAN_ROWS}`.
- Produces:
  - `pub(super) sub_texts: HashMap<(String, usize), Markdown>` on `ChatSession`: "per subagent and `said` entry, its Markdown"; cleared in `load` next to `texts`.
  - `fn sync_markdown<K: Eq + Hash>(map: &mut HashMap<K, Markdown>, key: K, text: &str, cx: &mut App)` in `view.rs`: today's subagent-card branch of `sync_row` (create; push the new tail; start over when the text shrank or the cut is no char boundary), which that branch now calls.
  - Ids: `subagent-plan` (the pinned checklist), `("subagent-text", k)`, `("subagent-thought", k)` (`k` = index in `said`); `subagent-answer` kept.

- [ ] **Step 1: Write the tests** (`view.rs`):
  - Update `subagent_shows_in_the_chat` (fake `--subagent --plan`): after the drawer opens, `said == [Call(read_ix), Thought("sub thought"), Text("sub text")]`; inside `subagent-drawer` find `("tool", read_ix)`, `subagent-plan`, `("subagent-thought", 1)` and `("subagent-text", 2)`; its last assert becomes `drawer.try_find("subagent-answer").is_none()` ("it spoke: its text holds the answer", D6). The card's `text == "found it"` assert stays.
  - `drawer_shows_the_answer_only_for_a_silent_subagent` (Review Focus 1; `fake_settings("")`, events through `s.handle_event`): a `Subagent` patch `c` (Running) then `SubagentReply { id: c, text: "found it" }`; `open_drawer(Some("c"))` → `subagent-answer` shows; then `SubagentSaid { id: c, thought: false, text: "found it" }` → `wait_until` `("subagent-text", 0)` shows and `subagent-answer` does not.
  - `subagent_words_wake_only_an_open_drawer`: same setup without the reply; `SubagentSaid "hi"` with the drawer closed → `s.flush.is_none()`; `open_drawer(Some("c"))`, `SubagentSaid " there"` → `s.flush.is_some()`; `wait_until` `sub_texts[&("c".to_string(), 0)]`'s `rendered_text()` trims to `hi there`.
- [ ] **Step 2: Implement.**
  - `handle_event`: a `SubagentSaid` goes through `update_model(.. apply ..)`, then calls `schedule_flush` only when `drawer` names its id, and returns without `cx.notify()`.
  - `schedule_flush`'s closure ends with `if this.drawer.is_some() { cx.notify() }`.
  - `render_drawer`: no per-frame clone of the subagent (index into `model.subagents`); first bring each `Text`/`Thought` entry's `sub_texts` up to date with `sync_markdown`, then build: header as today; `subagent-plan` (a `v_flex` of `plan_row` for the first `MAX_PLAN_ROWS`, then a muted `+N more`), only when `plan` is non-empty, outside the scrolling body; the body walks `said`: `Call(ix)` → `render_tool(ix, ..)` when `items[ix]` is a `Tool`, `Text` → its `TextView` in `("subagent-text", k)`, `Thought` → `output_box(cx)` with its `TextView`, muted, in `("subagent-thought", k)`; then `subagent-answer` only when no entry is `Said::Text`.
- [ ] **Step 3: User guide.** Line 66: "Its own text and checklist are not shown." becomes "Its own text, thinking and checklist show in its drawer." Line 71: "The body lists its tool calls one per line (a click unfolds the output), then its answer as Markdown." becomes "Under the header, its checklist (the first 8 items, then `+N more`). The body follows what it does, in order: its tool calls one per line (a click unfolds the output), its thinking in a muted box and its text as Markdown. A subagent that wrote no text of its own shows its answer at the end."
- [ ] **Step 4:** `cargo check --workspace --all-targets`. Expected: no errors.
- [ ] **Step 5: Commit.** `git commit -m "feat: show a subagent's text, thinking and checklist in its drawer"`

---

### Task 4: 3.1, stop one subagent from the agent list or its drawer

**Files:**
- Modify: `crates/wai/src/chat/view.rs` (`stop_subagent` beside `stop` ~596; tests)
- Modify: `crates/wai/src/chat/agents.rs` (`render_agents`' subagent rows, `render_drawer`'s header)
- Modify: `docs/user-guide.md` (lines 68, 71)

**Interfaces:**
- Consumes: `ChatModel::stoppable` (Task 2); `AgentHandle::cancel(SessionId)` (existing).
- Produces: `pub fn stop_subagent(&self, id: &str)` on `ChatSession`: `self.handle.cancel(id.into())` only when `self.model.stoppable(id)`; nothing optimistic (D5). Ids `("agent-stop", ix)` (same `ix` as `("agent-row", ix)`) and `subagent-stop`: `Button` ghost xsmall, `IconName::Square`, tooltip and accessibility label `Stop subagent`; each `on_click` calls `cx.stop_propagation()` first (the row's own click opens the drawer), then `stop_subagent`.

- [ ] **Step 1: Write the tests** (`view.rs`; both `fake_settings("--subagent --background --ask-permission")`, `send("edit")`, `wait_until` `s.model.done && s.model.stoppable(&sub)`, `sub = format!("{}-sub", s.id)`):
  - `a_subagent_stops_from_the_agent_list` (Review Focus 2): `click_edits(None, ("agent-stop", 0usize))` → `wait_until` `sub` is `SubagentState::Stopped`; then `model.waiting.is_empty()`, state `Idle`, `drawer.is_none()` (the click opened nothing), `!shows(("agent-stop", 0usize))`.
  - `a_subagent_stops_from_its_drawer`: `click_edits(None, ("agent-row", 0usize))`, then `subagent-stop` → `wait_until` Stopped; the drawer is still `Some(sub)` (it follows the state) and `!shows("subagent-stop")`.
- [ ] **Step 2: Implement.** Row: the Stop button after the stat, only when `stoppable`. Drawer header: `subagent-stop` before `subagent-drawer-close`, only when `stoppable`.
- [ ] **Step 3: User guide.** Line 68, after "…and its tokens when the agent reports them.": "A subagent that runs or waits on you, when the agent lets wai stop it (Claude Code does), has a square at the row's right end: **Stop subagent** stops that subagent only. Its open cards close as `Cancelled`, its mark turns `✗` once the agent confirms, and the session's turn goes on." Line 71: "The header has its mark, kind, task, time and tokens, and **×**." becomes "The header has its mark, kind, task, time and tokens, **Stop subagent** (as on its row) and **×**."
- [ ] **Step 4:** `cargo check --workspace --all-targets`. Expected: no errors.
- [ ] **Step 5: Commit.** `git commit -m "feat: stop one subagent from the agent list or its drawer"`

---

### Task 5: 2.1b, History rows mark a conversation open elsewhere

**Files:**
- Modify: `crates/wai/src/workspace.rs` (one field beside `history` ~216, its initializer in `new` ~430)
- Modify: `crates/wai/src/sidebar.rs` (`refresh_history` ~161, `history_row`)
- Test: `crates/wai/src/elsewhere.rs` `mod tests`
- Modify: `docs/user-guide.md` (line 129)

**Interfaces:**
- Consumes: `elsewhere::open_elsewhere(home, ids, quiet)`, `Workspace::{quiet, session, claude_home, default_agent}` (existing).
- Produces:
  - `pub(crate) elsewhere: HashSet<String>` on `Workspace`: "History ids whose Claude conversation was open in another process at the last refresh".
  - `refresh_history`: before the spawn, when `default_agent() == Some("claude")`, take `home = claude_home`, `quiet = self.quiet()` and the ids of open `claude` sessions; the background step returns the rows as today **and** `open_elsewhere(&home, <listed ids not open here>, &quiet)` (empty, with no `ps`, otherwise); the update stores both. Comment: `// ponytail: stats ids × project dirs and runs ps once per refresh; read each projects dir once if it shows.`
  - `history_row`: when `self.elsewhere.contains(&summary.id)` and `self.session("claude", &summary.id, cx).is_none()`, a `div().id(("history-elsewhere", ix))` before the date: `Icon::new(Lucide::Monitor)` 11 px, amber (`aurora::ambar()` dark, `theme.warning` light), tooltip and `aria_label` `Open elsewhere`, `.test_support()`.

- [ ] **Step 1: Write the test** `history_marks_conversations_open_elsewhere` (elsewhere tests, `setup(cx, 2)`; Review Focus 5): `click(("history", 1usize))` → `wait_until` `ids == ["session-1"]`; `fresh(home, "session-0")`, `fresh(home, "session-1")`; `workspace.update(cx, |w, cx| w.refresh_history(cx))`; `wait_until` `w.elsewhere.contains("session-0")`; then `!w.elsewhere.contains("session-1")`, `("history-elsewhere", 0usize)` shows and `("history-elsewhere", 1usize)` does not.
- [ ] **Step 2: Implement** the interfaces above.
- [ ] **Step 3: User guide**, line 129, append: "Such a row shows an amber screen icon (**Open elsewhere**); the check runs when the History list refreshes (when the agent starts, after each turn and each new session)."
- [ ] **Step 4:** `cargo check --workspace --all-targets`. Expected: no errors.
- [ ] **Step 5: Commit.** `git commit -m "feat: mark history rows open elsewhere"`

---

### Task 6: Gate, final review, one fix wave

- [ ] **Step 1: Merge `development`** in if anything landed since the branch started (`git log --oneline feat/sidebar-followups..development`; expect `sidebar.rs` from the work-areas track); resolve in the worktree; `cargo check --workspace --all-targets`.
- [ ] **Step 2: Full gate** (Global Constraints), in the background. Record pass/fail counts and each failure's test name and first error line.
- [ ] **Step 3: One whole-branch review** (opus) against this plan: Decisions, Review Focus, copy, the "not ours" files untouched, the Sessions-panel functions limited to the three named, no timers or polling, a closed drawer woken by no subagent word.
- [ ] **Step 4: One fix wave** for gate failures and the review's Critical/Important findings (Minors listed in the report). Commit `fix: apply the sidebar follow-ups review`. Re-run the gate. Expected: fmt and clippy clean, `cargo test --workspace --no-fail-fast` 0 failed.
- [ ] **Step 5: Report** to the controller: commits, gate counts, deferred minors, anything Task 1 Step 1 found moved, and what the owner checks in the app with real Claude: a background `general-purpose` subagent stopped from its row and from its drawer; a drawer following a subagent's thinking, text and checklist (the answer once); `claude --resume <id>` in the wai terminal, then a turn, and the History row's mark; a double click under History. Then `cargo clean`.

---

## Coverage

| Item | Task |
|---|---|
| 1.11 double click on the empty Sessions panel opens a draft; rows keep their clicks | 1 |
| 3.2 the subagent's text, thinking and checklist reach wai and its model, in order | 2 |
| 3.2 the drawer shows them; the answer once; words wake only an open drawer | 3 |
| 3.1 stop right read from the agent; one subagent stops alone (protocol and fake) | 2 |
| 3.1 Stop on the agent-list row and in the drawer header | 4 |
| 3.2 finished subagents listed | already in the redesign (`shown_subagents`) |
| 2.1b "open elsewhere" mark on History rows, computed in the refresh's background step | 5 |
| Restart/restore does not double subagents (D8) | 2 |
| Gate, review, fix wave | 6 |
