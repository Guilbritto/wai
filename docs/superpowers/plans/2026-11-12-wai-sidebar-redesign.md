# wai sidebar redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the sidebar's text tabs with a VS Code-style activity bar, give session rows a second line saying what each session does, fold History into the Sessions panel, list a session's agents under its prompt box and open each subagent in a drawer above it.

**Architecture:** The data the UI needs is added first, in the model (`ChatModel::subagents` gains kind, timing, tokens and turn; `ChatSession` gains the time its state changed), with pure functions for the row's second line and short ages. Then the bar (`sidebar.rs` + `workspace.rs` layout + `topbar.rs`), the Sessions panel (`sidebar.rs`), the agent list and drawer (new `chat/agents.rs`, wired from `chat/render.rs` and `chat/input.rs`), and the visual touches of Files, Search and Agents in their own files.

**Tech Stack:** Rust workspace, GPUI + gpui-kit 0.9.0 (Lucide icons through `icon_assets!`), ACP (`agent-client-protocol-schema` 1.16, `claude-agent-acp` 0.103.0), the fake agent `wai-fake-agent` for tests.

**Spec:** `docs/superpowers/specs/2026-11-12-wai-sidebar-redesign-design.md`, with its mockups in `docs/superpowers/specs/2026-11-12-wai-sidebar-redesign-mockups/` (`sidebar-v2.html`: window + drawer; `sidebar-tabs.html`: every bar icon). Read both before Task 1. The mockups are the reference for every size, colour and spacing this plan does not repeat.

## Global Constraints

- Worktree `/Users/guilherme.brito/Project/Guilherme/wai/.claude/worktrees/sidebar-redesign`, branch `feat/sidebar-redesign`, base `development` at `a818cc8` (0.6.2). Another session commits to `development` and runs the Onda 2 tracks, whose sessions track also edits `sidebar.rs` (`refresh_history`, `history_row`, Rename prefill). Before handing the branch over, `git merge development` into it and resolve here. Never merge into `development`, never push.
- Commits: `feat:` / `fix:` / `test:` / `docs:` / `refactor:`, no scope, no Co-Authored-By, no Jira ref, imperative, first line < 72 chars.
- **Owner's pace** (as in the Onda 2 plans): per task, write the task's tests and code, run `cargo check --workspace --all-targets` (expected: no errors) and the task's own tests (`cargo test -p wai <name>`), then commit. The full gate runs once, in Task 10.
- Every shell: `export PATH="$HOME/.cargo/bin:$PATH"`. Before a build, `df -h /System/Volumes/Data`; under 8 GB free, stop and report. `cargo clean` in the worktree when done.
- Gate (Task 10): `cargo build -p wai-agent --bin wai-fake-agent && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.
- UI copy in English, exact as quoted here; the mockups' Portuguese maps to: `Sessions`, `Open`, `History`, `this project`, `All`, `Filter sessions`, `Draft`, `Permission:`, `Question:`, `✓ Done`, `Idle`, `Agent exited`, `Restart`, `N subagents` / `1 subagent`, `↑↓ navigate · Enter open · Esc back to main`.
- No new timers. Row times and the agent list repaint from the sessions' own ticks (`tick_period`) and notifications; nothing ticks while every session is idle (idle CPU < 0.5 %).
- Keep existing test ids, or rename them only together with their tests. Ids that must survive: `tab-sessions`, `tab-files`, `tab-search`, `tab-agents`, `("tab-panel", ix)`, `open-row`, `open`, `open-side`, `open-close`, `draft-row`, `draft`, `draft-close`, `new-group`, `group`, `history`, `("history-side", ix)`, `all-projects`, `history-hint`, `sidebar-new`, `sidebar-templates`, `sidebar-extensions`, `topbar-settings`, `turn-status`, `transcript`. `tab-history` goes (History is no longer a tab).
- Not ours: never touch `crates/wai/src/update.rs`, the Updates section of `settings_view.rs`, the quit/close-confirmation path of `workspace.rs`, `.github/workflows/*`, `CLAUDE.md`.
- Tests never read or write the real `~/.claude`, `~/.claude.json` or `~/.config/wai`.
- Follow existing patterns (`aurora::*` colours and cards, `sidebar::row`, `Button` ghost/xsmall, `.test_support()` to register ids). Comments only for a non-obvious why; `ponytail:` comments for deliberate ceilings.

## Review Focus

1. **A subagent drawer outliving its subject:** the session is ended, the pane shows another session, or the subagent id is no longer in `subagents`. Expected: the drawer closes, nothing panics. Test in Task 6.
2. **Many subagents at once** (a turn that fans out 8+). Expected: the agent list shows at most 6 rows and scrolls; the prompt box never leaves the pane. Test in Task 5.
3. **Filter with no match / clearing it.** Expected: no rows, a muted `No sessions match` line; clearing restores every group's own folded state (the filter never writes `collapsed`). Test in Task 4.
4. **Extension panel titles** that are one word, non-ASCII or empty (`Routines`, `Rotinas`, `日本語`, ``). Expected: initials by characters, never by bytes; empty gives `?`. Test in Task 3.
5. **Two-line rows still behave like rows:** drag onto a group header, a pane, another row; double click renames; the hover chip's buttons do not swallow drops. Expected: today's tests for these pass unchanged. Covered by the existing `rows_drag_into_and_out_of_groups`, `rows_reorder_by_dragging`, `rename_session`, kept green in Task 4.

---

### Task 1: Subagent and session data

**Files:**
- Modify: `crates/wai-agent/src/types.rs` (`SubagentPatch`)
- Modify: `crates/wai-agent/src/convert.rs` (`subagent_patch`)
- Modify: `crates/wai-agent/src/subagents.rs` (`route`: a child's usage)
- Modify: `crates/wai-agent/src/bin/wai-fake-agent.rs` (`--subagent` sends a description)
- Modify: `crates/wai/src/chat/model.rs` (`Subagent`, `ChatModel::turn`, `patch_subagent`, `push_user`, `shown_subagents`)
- Modify: `crates/wai/src/chat/view.rs` (`ChatSession::since`, set in `update_model`)

**Interfaces:**
- Produces:
  - `SubagentPatch { id, title: Option<String>, description: Option<String>, state: Option<SubagentState>, used: Option<u64> }`.
  - `model::Subagent { id: String, title: String, kind: Option<String>, state: SubagentState, used: Option<u64>, started: Instant, ended: Option<Instant>, turn: u32 }`. `kind` is ACP's `description` (Claude: the subagent type, e.g. `general-purpose`); `ended` is set the first time the state becomes Done/Ended/Stopped/Failed.
  - `ChatModel::turn: u32`, incremented by `push_user` (not by `push_steered`).
  - `ChatModel::shown_subagents(&self) -> Vec<&Subagent>`: those with `turn == self.turn`, plus any still `Running`/`NeedsYou`, in announced order.
  - `pub since: Option<Instant>` on `ChatSession`: set to `Instant::now()` whenever `update_model` sees the state change; `None` until the first change (restored sessions show no time).
  - `pub fn turn_started(&self) -> Option<Instant>` on `ChatSession` (the start held in the `pub(super)` field `turn`), for the sidebar, which lives outside `chat`.

- [ ] **Step 1: Write the failing tests**

In `convert.rs` tests: `subagent_patch_reads_the_description` builds an `acp::SubagentUpdate` with title `List my open GitHub PRs` and description `general-purpose` and asserts `patch.description.as_deref() == Some("general-purpose")`.

In `subagents.rs` tests: `child_usage_goes_to_its_subagent`: after a `SubagentUpdate` for child `c` of root `r`, a `UsageUpdate { used: 46_900, size: 200_000 }` on `c` routes to `(r, SessionEvent::Subagent(SubagentPatch { id: "c", used: Some(46_900), title: None, description: None, state: None }))`.

In `model.rs` tests:
```rust
#[test]
fn subagents_of_earlier_turns_leave_unless_live() {
    let mut m = ChatModel::default();
    m.push_user("one");
    m.apply(&sub_patch("a", "Explore the code", Some(SubagentState::Done)));
    m.apply(&sub_patch("b", "Watch the build", Some(SubagentState::Running)));
    assert_eq!(ids(m.shown_subagents()), ["a", "b"]);
    m.push_user("two");
    assert_eq!(ids(m.shown_subagents()), ["b"], "a finished in an earlier turn");
}

#[test]
fn a_subagent_keeps_its_kind_and_end() {
    let mut m = ChatModel::default();
    m.apply(&SessionEvent::Subagent(SubagentPatch { description: Some("Explore".into()), ..running("a") }));
    let s = &m.subagents[0];
    assert_eq!(s.kind.as_deref(), Some("Explore"));
    assert!(s.ended.is_none());
    m.apply(&sub_patch("a", "x", Some(SubagentState::Done)));
    assert!(m.subagents[0].ended.is_some());
}
```
(`sub_patch`, `running`, `ids` are small test helpers in the same module.)

In `view.rs` tests: `since_follows_the_state` (fake agent `--steps 1`): `since` is `None` before the first prompt, `Some` once the turn runs, and a later value after the turn ends.

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test -p wai-agent subagent_patch_reads_the_description child_usage_goes_to_its_subagent; cargo test -p wai subagents_of_earlier_turns a_subagent_keeps since_follows_the_state`
Expected: compile errors on the new fields.

- [ ] **Step 3: Implement**

- `subagent_patch` reads `u.description` the same way it reads `title`.
- `route`: for a child (`Some(root)`), a `SessionEvent::Usage { used, .. }` becomes `SessionEvent::Subagent(SubagentPatch { id: child, used: Some(used), ..Default })` to the root. (`claude-agent-acp` 0.103.0 sends no usage for children, so this stays dormant with Claude; derive `Default` on `SubagentPatch`.)
- `patch_subagent` fills `kind` from `description`, `used`, `started` at first sight, `ended` on the first terminal state, `turn` from `self.turn` at first sight.
- Fake agent: the `--subagent` child's `SubagentUpdate` carries `description: "Explore"` beside its title `Explore the code`.

- [ ] **Step 4: Run the tests and see them pass**

Same commands as Step 2. Expected: PASS. Then `cargo check --workspace --all-targets`.

- [ ] **Step 5: Commit**

`git commit -m "feat: keep each subagent's kind, timing, tokens and turn"`

---

### Task 2: What a row says, and short ages

**Files:**
- Modify: `crates/wai/src/sidebar.rs` (new pure functions near `status_color`)

**Interfaces:**
- Consumes: `ChatModel::{state, waiting, done, subagents, shown_subagents, activity}`, `Subagent::state` (Task 1).
- Produces:
  - `pub(crate) enum Detail { Doing(String), Permission(String), Question(String), Done, Idle, Exited, Draft }`.
  - `pub(crate) fn detail(m: &ChatModel) -> Detail`: Working → `Doing(m.activity())` (the tool line or `Thinking…`); NeedsYou → the first `waiting` card's title as `Permission` or `Question`; Idle → `Done` if `m.done` else `Idle`; Error → `Exited`.
  - `pub(crate) fn live_subagents(m: &ChatModel) -> usize` (Running or NeedsYou).
  - `pub(crate) fn short_age(d: Duration) -> String`: `< 60 s` → `"{s}s"`, `< 60 min` → `"{m}m"`, `< 24 h` → `"{h}h"`, else `"{d}d"`; whole units, rounded down.

- [ ] **Step 1: Write the failing tests** (in `sidebar.rs` tests)

```rust
#[test]
fn short_ages_round_down() {
    let s = Duration::from_secs;
    assert_eq!([s(0), s(59), s(60), s(3599), s(3600), s(86_399), s(86_400), s(200_000)].map(short_age),
               ["0s", "59s", "1m", "59m", "1h", "23h", "1d", "2d"]);
}

#[test]
fn detail_says_what_the_session_does() {
    let mut m = ChatModel::default();
    assert_eq!(detail(&m), Detail::Idle);
    m.done = true;
    assert_eq!(detail(&m), Detail::Done);
    m.state = TurnState::Working;
    assert_eq!(detail(&m), Detail::Doing("Thinking…".into()));
    m.on_permission(permission("Write notes.md")); // sets NeedsYou; `permission` builds a PermissionRequest with that title, like the model tests' own helpers
    assert_eq!(detail(&m), Detail::Permission("Write notes.md".into()));
    m.state = TurnState::Error("exit 1".into());
    assert_eq!(detail(&m), Detail::Exited);
}
```

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test -p wai short_ages_round_down detail_says_what_the_session_does`
Expected: FAIL (functions not defined).

- [ ] **Step 3: Implement the three functions** as specified in Interfaces.

- [ ] **Step 4: Run the tests and see them pass**, then `cargo check --workspace --all-targets`.

- [ ] **Step 5: Commit**

`git commit -m "feat: say what each session is doing, in a word"`

---

### Task 3: The activity bar

**Files:**
- Modify: `crates/wai/src/app.rs:14` (`ExtraIcons`: add `MessageSquare`, `Sparkles`, `Puzzle`)
- Modify: `crates/wai/src/sidebar.rs` (`SidebarTab` loses `History`; `TABS`; new `render_rail`, `initials`, `panel_title`; `render_sidebar` renders only the panel)
- Modify: `crates/wai/src/workspace.rs` (`render`: the rail as a column left of the resizable row, outside the cards)
- Modify: `crates/wai/src/topbar.rs` (drop `sidebar-extensions` and `topbar-settings` from the top bar)
- Modify: `crates/wai/src/agents_view.rs` (`render_agents` header uses `panel_title`)

**Interfaces:**
- Produces:
  - `pub(crate) fn render_rail(&self, cx: &Context<Self>) -> impl IntoElement` on `Workspace`.
  - `pub(crate) fn initials(title: &str) -> String`: first char of the first two whitespace-separated words, else the first two chars of the one word, uppercased; empty → `"?"`.
  - `pub(crate) fn panel_title(label: &str, cx: &App) -> Div`: the title row (11 px, bold, 0.07 em tracking, muted, uppercase); callers append their action buttons.
  - Rail item ids: `tab-sessions`, `tab-files`, `tab-search`, `tab-agents`, `("tab-panel", ix)` (top); `sidebar-extensions`, `topbar-settings` (foot, same handlers as today's top-bar buttons). Badge id `sessions-badge`.

- [ ] **Step 1: Write the failing tests**

- `initials_by_characters`: `["My cards", "Routines", "Rotinas", "日本語", "", "a b c"]` → `["MC", "RO", "RO", "日本", "?", "AB"]`.
- Rewrite `history_has_its_own_tab` as `history_is_a_section_of_sessions`: `tab-history` is not found; on Sessions, `all-projects` and `("history-side", 0)` are shown.
- `rail_badge_counts_needs_you` (fake agent `--ask-permission`): `sessions-badge` absent while idle, shows `1` while the card waits.
- `rail_foot_opens_settings`: clicking `topbar-settings` and `sidebar-extensions` on the rail opens Settings (assert the same way today's top-bar tests do; move those tests to the rail ids).
- Keep `double_click_on_sessions_tab_opens_a_draft` passing against the rail icon.

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test -p wai initials_by_characters history_is_a_section rail_badge rail_foot double_click_on_sessions`
Expected: FAIL.

- [ ] **Step 3: Implement**

- Rail: 44 px wide column, icons 32 px with 9 px radius, 6 px gap; selected: `list.active.background` + `list.active.border` + a 2 px violet bar on its left edge; tooltip and aria label = the label; role tab, `tab_index(0)`, focus ring as today. Icons: `MessageSquare` Sessions, `Folder` Files, `Search` Search, `Sparkles` Agents (hidden while `agents_right`), extension panels as `initials` in a 20 px rounded square; a flexible gap; `Puzzle` Extensions, `Settings` Settings. Badge: amber pill, count of sessions with `TurnState::NeedsYou`, hidden at 0.
- Click handlers move from the tab pills unchanged (Search focuses its query; double click on Sessions starts a session).
- `render_sidebar` drops the pill strip and renders only `content`. `SidebarTab::History` and `render_history`'s tab go; History's body is reused in Task 4.
- Agents header: `panel_title("Agents")` + today's move/collapse buttons.

- [ ] **Step 4: Run the tests and see them pass**, then `cargo check --workspace --all-targets`.

- [ ] **Step 5: Commit**

`git commit -m "feat: put the sidebar's tabs on an activity bar"`

---

### Task 4: The Sessions panel

**Files:**
- Modify: `crates/wai-settings/src/lib.rs` (`SessionGroup::color: u8` with `#[serde(default)]`; `AppState::{open_folded, history_folded}: bool`)
- Modify: `crates/wai/src/sidebar.rs` (`render_sessions`, `open_row`, `draft_row`, `group_header`, `new_group`, history section, filter)
- Modify: `crates/wai/src/workspace.rs` (fields `sessions_filter: Entity<InputState>`, `open_folded`, `history_folded`; save/restore them)

**Interfaces:**
- Consumes: `detail`, `live_subagents`, `short_age` (Task 2); `ChatSession::since`, `ChatSession::turn_started()` (Task 1); `panel_title` (Task 3).
- Produces:
  - `pub(crate) const GROUP_COLORS: [fn() -> Hsla; 6]` (violet, cyan, blue, magenta, amber, green from `aurora`/theme); `new_group` sets `color = (self.groups.len() % 6) as u8`.
  - Ids: `sessions-new`, `new-group` (kept), `groups-collapse`, `sessions-filter`, `open-section`, `history-section`, `all-projects` (kept, now a toggle on the History header), `("open-age", ix)`, `("open-detail", ix)`, `("group-needs", ix)`, `sessions-no-match`.

- [ ] **Step 1: Write the failing tests**

- `rows_say_what_sessions_do` (fake agent `--steps 1 --chunk-delay-ms 300`): while the turn runs, `("open-detail", 0)`'s text starts with the status line's tool text or is `Thinking…`; after it ends, `✓ Done`; `("open-age", 0)` is present after the first state change and absent on a freshly restored session.
- `collapsed_group_says_who_needs_you` (`--ask-permission`, session moved into a group, group collapsed): `("group-needs", 0)` reads `● 1`; expanded, it is absent.
- `filter_narrows_open_and_history`: three resumed sessions titled from `history_settings(3)`; typing a substring of one title leaves one Open row and the matching History rows; typing `zzz` shows `sessions-no-match` and no rows; clearing restores rows and every group's `collapsed` exactly as before (Review Focus 3).
- `sections_fold_and_survive_a_relaunch`: clicking `open-section` and `history-section` folds them; the relaunch (same pattern as `names_and_groups_survive_a_relaunch`) restores both, and group colours.
- `new_groups_take_the_next_colour`: three new groups get colours `0, 1, 2`.
- Existing: `rename_session`, `groups_are_created_renamed_collapsed_and_deleted`, `row_menu_groups_sessions`, `rows_drag_into_and_out_of_groups`, `rows_reorder_by_dragging`, `names_and_groups_survive_a_relaunch`, `draft_row_shows_and_discards` stay green (Review Focus 5).

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test -p wai rows_say_what collapsed_group_says filter_narrows sections_fold new_groups_take`
Expected: FAIL.

- [ ] **Step 3: Implement**

- Panel order: `panel_title("Sessions")` + `sessions-new` (New session), `new-group`, `groups-collapse` (sets every group's `collapsed = true`); `sessions-filter` (placeholder `Filter sessions`, `⌘K` hint; bind `secondary-k` in the `Workspace` context to focus it and select Sessions; Esc in it clears); `open-section` header (`Open`, count of drafts + sessions); the rows and groups; `history-section` header (`History · this project` / `History · all projects`, `all-projects` toggle labelled `All` at its right); the history list.
- Session row (two lines): line 1 dot, title, `open-age` = `short_age(since.elapsed())` (Working: from `turn_started()`); line 2 per `detail` with the spec's colours, then ` · N subagents` when `live_subagents > 0`. Hover chip (glass, top-right) holds `open-side` and `open-close`, drawn only while hovered (`group_hover`). Active row as today.
- Draft row: line 2 `Draft`. Group header: chevron, 8 px swatch `GROUP_COLORS[color]`, name, count, and `group-needs` when collapsed and any member needs you; rows of a group indented 10 px.
- Filter: case-insensitive `contains` on the shown title; while non-empty, groups without a match hide and groups with one draw expanded without touching `collapsed`.
- History section: today's `render_history` body (lazy `uniform_list`, hint, `history-side` rows) as one-line rows with ↺, title, and `updated_at` as a relative date (`yesterday`, `2d`, else `short_age`). Open sizes to its content up to 60 % of the panel and scrolls beyond; History takes the rest.
- Persist `open_folded`, `history_folded` and `SessionGroup::color` in `state.json`.
- Line 2 must follow the running tool. If `rows_say_what_sessions_do` shows it stale (the workspace not repainting on a session's notify), observe each session where the workspace subscribes to it (`workspace.rs:1031`): `cx.observe(&session, |_, _, cx| cx.notify())`.

- [ ] **Step 4: Run the tests and see them pass**, then `cargo check --workspace --all-targets`.

- [ ] **Step 5: Commit**

`git commit -m "feat: two-line session rows, history and a filter in one panel"`

---

### Task 5: The agent list under the prompt

**Files:**
- Create: `crates/wai/src/chat/agents.rs` (the list and, in Task 6, the drawer)
- Modify: `crates/wai/src/chat/mod.rs` (`mod agents;`)
- Modify: `crates/wai/src/chat/view.rs` (fields `drawer: Option<String>`, `agent_cursor: Option<usize>`)
- Modify: `crates/wai/src/chat/render.rs:825` (`render`: the list after `render_prompt`)
- Modify: `crates/wai/src/chat/input.rs` (`answer_key`: ↓ from an empty prompt; `cancel`: Esc closes the drawer first)

**Interfaces:**
- Consumes: `ChatModel::shown_subagents`, `Subagent::{kind, title, state, started, ended, used}` (Task 1); `short_age`-style elapsed: reuse `model::elapsed(secs)` for the list's times; `render::tokens(n)` for `46.9k`.
- Produces:
  - `impl ChatSession { pub(super) fn render_agents(&mut self, cx: &mut Context<Self>) -> Option<AnyElement>; pub(super) fn open_drawer(&mut self, id: Option<String>, cx: &mut Context<Self>) }` (`None` = main).
  - Ids: `agent-list`, `agent-main`, `("agent-row", ix)`, `agent-hint`.

- [ ] **Step 1: Write the failing tests** (in `view.rs` tests, fake agent)

- `agent_list_shows_main_and_subagents` (`--subagent`): before any subagent, `agent-list` is absent; after, `agent-main` and `("agent-row", 0)` show, the row's text holds `Explore` and `Explore the code`.
- `agent_list_caps_at_six_rows` (model-level: push 8 running subagents into `model.subagents`, render): the list's height equals six rows' height and it scrolls (Review Focus 2).
- `arrow_down_enters_the_list`: with the prompt empty, `down` selects `agent-main` (`agent_cursor == Some(0)`), `down` again selects row 0, `enter` sets `drawer == Some(<sub id>)`; with text in the prompt, `down` does not leave it.

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test -p wai agent_list_shows agent_list_caps arrow_down_enters`
Expected: FAIL.

- [ ] **Step 3: Implement**

- List (mono font, 12 px), drawn only when `shown_subagents()` is non-empty: `agent-main` row (`●`, bold `main`, right: turn elapsed while a turn runs and `↓ {tokens(usage.used)} tokens` when usage is known); one row per shown subagent: mark (`○` cyan running, amber needs you, `✓` green done/ended, `✗` red stopped/failed), name = `kind` else `title`, task = `title` when `kind` is set, right: `elapsed(started→now)` while live, `done · {elapsed(started→ended)}` after, ` · ↓ {tokens} tokens` when `used` is set. Selected row (drawer open, or cursor) has the active background and border; with no drawer, `main` is selected. Max 6 rows visible, then it scrolls.
- Clicks: a subagent row → `open_drawer(Some(id))`; `main` or the selected row again → `open_drawer(None)`.
- Keyboard in `answer_key`: prompt empty and list shown → `down` moves `agent_cursor` (from `None` to `Some(0)` = main, then rows), `up` back (from 0 to `None` returns to the prompt), `enter` opens the cursor's row (main closes). `agent-hint` (`↑↓ navigate · Enter open · Esc back to main`) shows while `agent_cursor.is_some()`.
- `cancel`: if `drawer.is_some()` or `agent_cursor.is_some()`, clear both, focus the prompt, and return without stopping the turn; else today's behaviour.

- [ ] **Step 4: Run the tests and see them pass**, then `cargo check --workspace --all-targets`.

- [ ] **Step 5: Commit**

`git commit -m "feat: list the session's agents under the prompt"`

---

### Task 6: The subagent drawer, and subagents in the transcript

**Files:**
- Modify: `crates/wai/src/chat/agents.rs` (`render_drawer`)
- Modify: `crates/wai/src/chat/render.rs` (`render`: drawer over the list's bottom like `render_waiting`; `shown`: a tool row with `subagent.is_some()` is not shown; the subagent card's click opens its drawer and shows the `Running`/`Done`/`Failed` pill)
- Modify: `crates/wai/src/workspace.rs` (switching the session shown in a pane closes that session's drawer)
- Modify: `crates/wai/src/chat/view.rs` tests (`subagent_shows_in_the_chat`)

**Interfaces:**
- Consumes: `drawer`, `open_drawer` (Task 5); `render_tool(ix, …)` for each row whose `subagent == Some(id)`; the subagent card's reply text (the `SubagentReply` already stored on the card).
- Produces: ids `subagent-drawer`, `subagent-drawer-close`, `("subagent-card", ix)`.

- [ ] **Step 1: Write the failing tests**

- Update `subagent_shows_in_the_chat`: the subagent's call `sub-read` is no longer drawn in the transcript (`!shows(("tool", <its row>))`); clicking its card opens `subagent-drawer`, where the call's line shows; the answer shows in the drawer once the subagent finishes.
- `esc_closes_the_drawer_not_the_turn` (`--subagent --background --chunk-delay-ms 300`): drawer open during the turn; `escape` closes `subagent-drawer` and the session is still `Working`; a second `escape` stops the turn.
- `drawer_closes_when_its_subject_goes` (Review Focus 1): open the drawer; (a) show another session in the pane → closed; (b) clear `model.subagents` → `render` closes it without panicking; (c) end the session → no panic.
- `waiting_cards_stay_above_the_drawer` (`--subagent --ask-permission`): with the drawer open, the permission card's choice `1` is still clickable and answers.

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test -p wai subagent_shows_in_the_chat esc_closes_the_drawer drawer_closes_when waiting_cards_stay_above`
Expected: FAIL.

- [ ] **Step 3: Implement**

- Drawer: absolutely positioned over the transcript's bottom, above the prompt, ~46 % of the pane height, glass background, `list.active.border` border, shadow; transcript dims to 35 % opacity behind it. Header: mark, bold name, muted task, right: time · tokens and `subagent-drawer-close` (×). Body (mono, scrolls, follows the bottom while at the bottom): `render_tool` for each of the subagent's calls in order (a click unfolds the output, as in the transcript), then the answer as Markdown when it has one. Waiting cards render after (on top of) the drawer.
- `shown(ix)`: `false` for `ChatItem::Tool(t)` with `t.subagent.is_some()`, whatever the processing mode. `card_edges` uses `shown`, so cards close up by themselves.
- Subagent card in the transcript: one line with mark, kind (violet), task, and a right-aligned pill `Running` / `Done` / `Failed`; click → `open_drawer(Some(id))`.
- In `render`, if `drawer` names an id not in `model.subagents`, set it to `None`.

- [ ] **Step 4: Run the tests and see them pass**, then `cargo check --workspace --all-targets`.

- [ ] **Step 5: Commit**

`git commit -m "feat: open a subagent in a drawer above the prompt"`

---

### Task 7: The Files panel

**Files:**
- Modify: `crates/wai/src/files.rs` (`row`, `render`, new `collapse_all`, `file_mark`)
- Modify: `crates/wai/src/workspace.rs` (pass the editor's active file to the tree on tab changes: `FileTree::set_active(Option<PathBuf>)`)

**Interfaces:**
- Consumes: `panel_title` (Task 3).
- Produces: `FileTree::collapse_all(&mut self, cx)`, `FileTree::set_active(&mut self, path: Option<PathBuf>, cx)`, `fn file_mark(name: &str) -> (&'static str, Mark)` where `Mark` picks the colour: `rs` orange (`#ff9a5a`), `md` → `M↓` blue (`base.blue`), `ts`/`tsx` `ts` blue, `js` `js` amber, `go` `go` cyan, `toml`/`json`/`yaml`/`yml`/`lock`/other → `≡` muted. Ids: `files-collapse`, `files-root`.

- [ ] **Step 1: Write the failing tests**

- `file_marks_by_extension`: `["main.rs", "README.md", "Cargo.toml", "app.tsx", "x"]` → marks `["rs", "M↓", "≡", "ts", "≡"]`.
- `collapse_all_folds_every_folder`: expand two folders (as `files_tab_expands_and_emits_open` does), click `files-collapse`, only the root's entries remain.
- `the_open_file_is_marked`: open `src/limits.rs` in the editor; its row is drawn active.

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test -p wai file_marks_by_extension collapse_all_folds the_open_file_is_marked`

- [ ] **Step 3: Implement** the title row (`panel_title("Files")` + `files-collapse`), the root header `files-root` (chevron + the project folder's name, folds the whole tree), the mark before each file (folder icon for folders), and the active row.

- [ ] **Step 4: Run the tests and see them pass**, then `cargo check --workspace --all-targets`.

- [ ] **Step 5: Commit**

`git commit -m "feat: file marks, the open file and collapse all in Files"`

---

### Task 8: The Search panel

**Files:**
- Modify: `crates/wai/src/search_view.rs` (`Row::File` gains `folded: bool`; `row`, `render`, `collapse_all`, `highlights`, last clicked)

**Interfaces:**
- Consumes: `panel_title` (Task 3).
- Produces: `fn highlights(text: &str, query: &str, regex: bool, case: bool) -> Vec<Range<usize>>` (byte ranges on char boundaries; an invalid regex gives none); `SearchView::collapse_all`. Ids: `search-collapse`, `("search-file", ix)`, `("match", ix)` (kept).

- [ ] **Step 1: Write the failing tests**

- `highlights_find_every_match`: `highlights("let per_route = per_route;", "per_route", false, true) == [4..13, 16..25]`; case-insensitive `"PER"` finds the same starts; regex `r"per_\w+"` finds both; `"ção"` in `"ação ação"` gives char-boundary ranges; invalid regex `"("` gives `[]`.
- `files_fold_in_results`: clicking `("search-file", 0)` hides that file's matches; `search-collapse` folds every file; a new query starts unfolded.
- `clicked_match_is_marked`: after clicking `("match", 1)`, that row draws active.

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test -p wai highlights_find_every_match files_fold_in_results clicked_match_is_marked`

- [ ] **Step 3: Implement** the title row (`panel_title("Search")` + `search-collapse`), the query field with `Aa` and `.*` inside it at its right, the summary line (existing `summary`), file headers (chevron, mark from `files::file_mark`, name, folder muted, count pill), match lines in mono (line number muted, highlights amber at 25 % behind the text), and the last clicked match active.

- [ ] **Step 4: Run the tests and see them pass**, then `cargo check --workspace --all-targets`.

- [ ] **Step 5: Commit**

`git commit -m "feat: foldable files and highlighted matches in Search"`

---

### Task 9: The Agents cards' context bar

**Files:**
- Modify: `crates/wai/src/agents_view.rs` (`card`)

**Interfaces:**
- Consumes: `UsageInfo { used, size }`.
- Produces: `pub fn context_fraction(used: u64, size: u64) -> f32` (clamped 0..=1; `size == 0` → 0). Id: `("agent-context", ix)`.

- [ ] **Step 1: Write the failing test**: `context_fraction_clamps`: `(82_000, 200_000) → 0.41`, `(300, 200) → 1.0`, `(5, 0) → 0.0`.

- [ ] **Step 2: Run it and see it fail**: `cargo test -p wai context_fraction_clamps`.

- [ ] **Step 3: Implement** the 3 px bar (track `#ffffff14`, fill cyan→violet gradient, width = fraction) above the existing `used / size (pct%)` line, only when usage is known.

- [ ] **Step 4: Run it and see it pass**, then `cargo check --workspace --all-targets`.

- [ ] **Step 5: Commit**

`git commit -m "feat: a context bar on each Agents card"`

---

### Task 10: User guide, gate and hand-over

**Files:**
- Modify: `docs/user-guide.md` (The window → Sidebar; Sessions → Status, History, side by side, and forks; Search; The Agents tab; Chatting → a new bullet on the agent list and drawer; Keyboard shortcuts: `⌘K`, `↓` from an empty prompt, Esc closes the drawer first)

- [ ] **Step 1: Update the user guide** to describe the bar, the Sessions panel (two-line rows, History section, `All`, filter, group colours, `● N` on folded groups), the agent list and drawer, Esc's new first press, and the Files/Search/Agents touches. Keep the guide's style: short bullets, exact UI strings in bold or code.

- [ ] **Step 2: Run the gate**

Run: `cargo build -p wai-agent --bin wai-fake-agent && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all pass. Fix what fails, one fix commit per cause (`fix: …`).

- [ ] **Step 3: Merge `development` in**

Run: `git log --oneline -5 development; git merge development`
Resolve conflicts here (expect `sidebar.rs` from the Onda 2 sessions track), re-run the gate.

- [ ] **Step 4: Commit and clean**

`git commit -m "docs: describe the activity bar, sessions panel and agent drawer"`, then `cargo clean`.

- [ ] **Step 5: Hand over** the branch for the owner to run and compare against the mockups. Do not merge into `development`.
