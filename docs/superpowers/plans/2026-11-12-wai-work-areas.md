# wai work areas: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Named work areas: sets of up to four sessions with their pane arrangement, listed in a "Work areas" section of the Sessions panel. Showing one puts its panes back exactly as they were left; clicking a session outside it, or another area, hides it intact.

**Architecture:** `wai-settings` gains `WorkArea { name, collapsed, panes: Vec<String>, focused }` beside `SessionGroup`, kept in `AppState.work_areas` with `AppState.shown_area`. At runtime `Workspace.work_areas` holds ids only: a hidden area is plain data, with no view, no entity and no task. The shown area's truth is the live `panes`: the pure `areas::rearranged` derives its order and focus on demand (for the sidebar and `save_state`), and the result is stored back only when it hides, so no pane code path needs a hook. A new `crates/wai/src/areas.rs` (`impl Workspace`, as `elsewhere.rs` and `start.rs` do) holds showing, hiding and membership; `workspace.rs` changes only in `show`, `close_pane`, `drop_session`, `save_state`, `restore`, `Renamed` and two fields; the panel's new section sits in `sidebar.rs` next to the groups it mirrors.

**Tech Stack:** Rust workspace, GPUI + gpui-kit/gpui-component, serde (`state.json`), the fake agent `wai-fake-agent` for tests.

**Spec:** `docs/superpowers/specs/2026-11-12-wai-work-areas-design.md` (binding). Mockup A, local only (git-ignored): `/Users/guilherme.brito/Project/Guilherme/wai/.superpowers/brainstorm/85564-1794503108/content/work-areas-v1.html`.

**Code read for this plan:** `development` at `0fc0a5a`; the sidebar redesign `feat/sidebar-redesign` at **`85a1bf1`** (worktree `.claude/worktrees/sidebar-redesign`, not merged when this was written); the workspace track `feat/onda2-workspace` at `ebbb353` plus its uncommitted Task 5 (`AppState.open_files`/`active_file`, `restore_files`). Names below are the redesign's; there are no line numbers on purpose.

## Global Constraints

- Repo `/Users/guilherme.brito/Project/Guilherme/wai`. Start only after `feat/sidebar-redesign` and `feat/onda2-workspace` are merged into `development`. Work in the worktree `.claude/worktrees/work-areas` on branch `feat/work-areas`, made from `development`. Run `git log --oneline -5 development` first: another session commits here.
- **Owner's pace.** Per task: write the task's tests and code, run only `cargo check --workspace --all-targets` (expected: no errors), then commit. No per-task `cargo test`, clippy, reviewer or fix loop. Failures surface in Task 5: the full gate once (`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build -p wai-agent --bin wai-fake-agent && cargo build -p wai-ext --bin wai-fake-ext`, `cargo test --workspace --no-fail-fast`), then one opus final review and one fix wave. One test: `cargo test -p wai <name>`.
- Every shell: `export PATH="$HOME/.cargo/bin:$PATH"`. Before a build, `df -h /System/Volumes/Data`; under 8 GB free, stop and report. `cargo clean` in the worktree when the track is done.
- **Idle CPU < 0.5 %.** A hidden area is ids in `work_areas`; its sessions are in no pane, so their views do not render. No timer, polling loop or background task is added.
- UI copy in English; strings in quotes in this plan are exact. Never break existing test ids. Follow existing patterns; comments only for a non-obvious why.
- Tests never read or write the real `~/.config/wai`, `~/.claude` or `~/.claude.json`: state files in temp dirs (`open_at`), `open` already points `user_dir`/`claude_home` at temp dirs.
- Commits: `feat:` / `fix:` / `test:` / `docs:` with no scope, no Co-Authored-By, no Jira ref; imperative, first line < 72 chars. **No push, no merge into `development`.**
- Not ours: never touch `crates/wai/src/update.rs`, the Updates section of `settings_view.rs`, the quit/close-confirmation path of `workspace.rs` (`leave`, `proceed`, `on_window_should_close`, `dirty_tabs`, `busy_sessions`, `quit_asks_while_a_session_works`), `.github/workflows/*`, `CLAUDE.md`. Keep `workspace.rs` edits to the functions this plan names; new `impl Workspace` code goes in `areas.rs` (model) or `sidebar.rs` (panel).

## Decisions to confirm

D1 to D10 are the spec's "My calls", planned as written. D11 onwards are the planner's calls on what the spec left open.

| # | Decision | Cost if wrong |
|---|---|---|
| D1 | **+** creates an empty area named "New work area" at the end of the list, with its name being typed inline like New group. Creating does not show it. An empty area shows one empty pane. | Showing it on create: one call to `show_area` in `new_work_area`. |
| D2 | A session dropped on an area becomes a new pane at the right end of the panes (after an editor pane, if that is last). A 5th is refused with a warning notification `A work area holds up to 4 sessions`. | Insert before the editor: one `position` in `add_to_area`. |
| D3 | A session is in **one area at most**, and **in an area or a group, never both**: joining one leaves the other (`ungroup` clears both). Its row moves from Open to under its area's row, which folds like a group. | Allowing both means a second place to draw the row and a rule for which header shows it. |
| D4 | A pane's × (or ⌘W) on a session of the **shown** area takes it out of the area. So does dragging its row onto Open, onto a loose row or onto a group header, and ending it (Open row ×). The session is never ended by leaving. | ⌘W keeping it in the area: the check moves from `close_pane` to the `ChatEvent::Hide` arm. |
| D5 | Delete (row menu) removes the area only; its sessions go back to Open, nothing ends. Deleting the shown area leaves the panes as they are, now loose. | None expected. |
| D6 | A click on a session of another area shows that area with the focus on that session's pane. A click on a session of the shown area that a draft or file pushed off screen lays the area out again. | None expected. |
| D7 | A **hidden** area's row shows an amber `● N` (N = its sessions that need you), folded or not; the shown area never does (its panes show it). | Showing it only when folded: one more condition. |
| D8 | `AppState.work_areas` (name, collapsed, sessions in pane order, focused index) and `AppState.shown_area` are saved on every `save_state`. On relaunch the shown area is laid out again, without following its session's folder (as `restore` never follows). | None expected. |
| D9 | **Changes today's click.** With no area shown, a click on a session that is in no pane shows it **alone** (the loose split goes; it is not saved anywhere). A session already in a pane only takes the focus, in both views. Open to the side splits as today. | Today's "replace the focused pane" in the loose view: drop one branch in `show`. |
| D10 | While an area is shown, a file, a draft (⌘N, a template), an extension's session or a start error uses a pane as today and is not saved in the area. A session whose pane it took stays in the area and comes back the next time the area shows. Settings is a modal, so it never takes a pane. | None expected. |
| D11 | Data model: `WorkArea { name, collapsed, panes: Vec<String>, focused: usize }` in `wai-settings`, keyed by session id alone like `SessionGroup` (`ponytail` note). `shown_area: Option<usize>` is an index into `work_areas`. | Two agents sharing an id put both sessions in one slot. Upgrade: `(agent, id)`. |
| D12 | Runtime: hidden areas are ids only, resolved to sessions when shown; ids not open (an agent missing at launch) are kept and skipped. The shown area's order and focus are derived from `panes` (`rearranged`) and written back when it hides. The row's count is the area's open sessions. | None expected; it removes every "keep the area in sync" hook. |
| D13 | **The editor pane is carried** across area switches and show-alone: it stays beside the new panes (a dirty tab must never be dropped). With four sessions it takes the last pane; that session is pushed off screen and comes back later (D10). | The owner sees the editor in a "show alone" view. Alternative: carry it only when dirty. |
| D14 | While an area is shown, a session in no area that the user opens to the side (Open row, History side button, Fork) or drops on a pane **joins the area**, while it holds fewer than 4. A click, a draft that sends or an extension's session never joins. | Without it, "opens panes, the area keeps the result" is only true for drops on the area's row. |
| D15 | A click on the shown area's row lays it out again: sessions pushed off screen return, other panes go (the editor stays, D13). | A no-op instead: `show_area` returns early when `shown_area == Some(ix)`. |
| D16 | Switching areas closes no subagent drawer of the area's sessions (shown "exactly as left"). A session in no area that leaves the screen closes its drawer, as `show` does today. | Cosmetic. |
| D17 | Not built: folding the "Work areas" heading (it has no chevron, as in mockup A), an "Add to work area" menu item, reordering areas, dropping a History row on an area, pane widths per area (the split is keyed by pane count, as today). | Each is a small follow-up. |
| D18 | The filter treats areas as groups: an area with a matching session shows expanded with only those rows; one with none hides. **Collapse all** folds areas too. Open's count leaves out area sessions. | Cosmetic. |
| D19 | Test ids: `new-work-area` (+), `("work-area-row", ix)` (row container), `("work-area", ix)` (click, drop and menu target; aria label `"{name}, {count}"`, `aria_selected` while shown), `("work-area-toggle", ix)` (chevron), `("work-area-needs", ix)` (badge). | Renames only. |

## Review Focus

1. **Unsaved editor edits when switching areas or showing a session alone:** the editor pane, dirty tab and all, stays on screen. Test `switching_keeps_the_editor_pane` (Task 2).
2. **A hidden area's session asks for permission:** it keeps running, the card is there and answerable when the area shows, and the hidden row says `● 1`. Tests `hidden_area_keeps_a_waiting_permission` (Task 2), `a_hidden_work_area_says_who_needs_you` (Task 4).
3. **A draft takes an area session's pane:** the session stays in the area and returns on the next show. Test `a_draft_does_not_push_a_session_out` (Task 3).
4. **Index shifts:** deleting an area before the shown one keeps the right one shown; ending a session of a hidden area removes only it. Tests `a_work_area_folds_and_deletes` (Task 4), `closing_a_pane_takes_it_out_of_its_area` (Task 3).
5. **Relaunch with a shown area whose session did not come back:** the rest show, the focus lands on the saved session, the missing id is kept. Test `a_shown_area_comes_back_on_relaunch` (Task 2).

## File map

| File | Responsibility | Tasks |
|---|---|---|
| `crates/wai-settings/src/lib.rs` | `WorkArea`; `AppState.work_areas`, `shown_area`; round-trip test | 1 |
| `crates/wai/src/areas.rs` (new) | `rearranged`, `area_of`, `area_now`; show/hide/alone; join, adopt, `add_to_area`; tests | 1, 2, 3 |
| `crates/wai/src/main.rs` | `mod areas;` | 1 |
| `crates/wai/src/workspace.rs` | fields, `new`, `save_state`, `restore`; `show`; `close_pane`, `drop_session`; `Renamed::Area` | 1, 2, 3, 4 |
| `crates/wai/src/sidebar.rs` | `ungroup`; the Work areas section, area rows, rename/delete, `reorder`, filter, count; tests; test helpers made `pub(crate)` | 2, 3, 4 |
| `docs/user-guide.md` | Work areas bullet, Sessions panel and click bullets | 4 |

---

### Task 1: Work areas in the saved state

**Files:**
- Modify: `crates/wai-settings/src/lib.rs` (`WorkArea` beside `SessionGroup`; `AppState` and its `Default`; test `state_roundtrip_and_corrupt_falls_back`)
- Create: `crates/wai/src/areas.rs`; Modify: `crates/wai/src/main.rs` (`mod areas;`)
- Modify: `crates/wai/src/workspace.rs` (fields and `new`; `save_state`; `restore` reads `work_areas`)

**Interfaces:**
- Produces:
  ```rust
  // wai-settings
  /// A named set of open sessions with their screen arrangement.
  #[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
  #[serde(default)]
  pub struct WorkArea {
      pub name: String,
      pub collapsed: bool,
      /// Its sessions' ids, one per pane, left to right; four at most.
      // ponytail: by session id alone, as SessionGroup.
      pub panes: Vec<String>,
      /// The index in `panes` of the session that had the focus.
      pub focused: usize,
  }
  // AppState, after the last field: "The work areas, in their order." / "The one on screen."
  pub work_areas: Vec<WorkArea>,
  pub shown_area: Option<usize>,

  // workspace.rs, Workspace fields (`new`: empty, None)
  /// The work areas, each with its sessions' ids; kept in `state.json`.
  pub work_areas: Vec<WorkArea>,
  /// The area on screen: its truth is `panes` (see `area_now`).
  pub(crate) shown_area: Option<usize>,

  // areas.rs
  /// `area` as the panes now show it: its sessions on screen (`shown`, the panes' session ids
  /// left to right) take, in pane order, the slots they held; the others keep theirs. The focus
  /// moves to `focused` (the focused pane's session id) when it is one of them.
  pub(crate) fn rearranged(area: &WorkArea, shown: &[&str], focused: Option<&str>) -> WorkArea;
  impl Workspace {
      /// The area holding session `id`.
      pub(crate) fn area_of(&self, id: &str) -> Option<usize>;
      /// Area `ix` as it stands: `rearranged` from the panes while it is shown, else as stored.
      pub(crate) fn area_now(&self, ix: usize, cx: &App) -> WorkArea;
  }
  ```
- `rearranged` algorithm: `slots` = the indices `i` of `area.panes` whose id is in `shown`; `order` = the ids of `shown` that are in `area.panes`, in `shown` order; write `order[k]` into `panes[slots[k]]`. `focused` = the index of `focused` in the result when it is a member, else `area.focused`.
- `save_state` writes `work_areas: (0..len).map(|ix| self.area_now(ix, cx))` and `shown_area`. `restore` sets `self.work_areas = state.work_areas` next to `self.groups = state.groups` (the shown area is Task 2).

- [ ] **Step 1: Re-verify the named functions after the redesign merges.** `git branch --merged development` must list `feat/sidebar-redesign` and `feat/onda2-workspace`; if not, stop and report. Then check that each name this plan uses still exists with the role described here: `workspace.rs` `Pane`, `Renamed`, `Dragged`, `DraggedSession`, `show`, `session_pane`, `focus_pane`, `follow_session`, `close_drawer_at`, `drop_session`, `close_pane`, `drop_pane`, `end_session`, `add_session` (its `RestoreFailed` arm), `new_session`, `save_state`, `restore` (and the `focus_pane` call that ends it); `sidebar.rs` `render_sessions`, `ungroup`, `move_to_group`, `group_session`, `reorder`, `new_group`, `delete_group`, `rename`, `end_rename`, `commit_rename`, `renaming`, `group_header`, `open_row`, `two_line_row`, `section_header`, `title_button`, `light`, `faint`, `fg2`, `item`, `shows`, `session_title`, and the tests' `with_sessions`, `press`, `label`, `drag`, `type_name`, `open_row`, `menu_item`; `workspace::tests` `open`, `open_at`, `fake_settings`, `history_settings`, `load_history`, `launch`, `start_now`, `wait_until`, `click`, `drag_to_pane`, `question`; `wai-settings` `AppState`, `SessionGroup`, `state_roundtrip_and_corrupt_falls_back`. Renamed or moved: use the new name everywhere below. A changed role: stop and report.
- [ ] **Step 2: Write the tests:**
  - `state_roundtrip_and_corrupt_falls_back` (wai-settings): its exhaustive `AppState` literal gains `work_areas: vec![WorkArea { name: "Evolução BB".into(), collapsed: true, panes: vec!["s1".into(), "s2".into()], focused: 1 }]` and `shown_area: Some(0)` (and the workspace track's `open_files`/`active_file` if it left them out); the "state from before names and groups" block asserts `state.work_areas == []` and `state.shown_area == None`; `{"work_areas":[{"name":"A"}]}` loads as `WorkArea { name: "A".into(), ..Default::default() }`.
  - `areas::tests::rearranged_follows_the_panes` (`#[test]`): area `["a","b","c","d"]`, `focused: 0`. `rearranged(&area, &["c","x","a"], Some("a"))` gives panes `["c","b","a","d"]`, focused `2`. `rearranged(&area, &["d","a"], Some("x"))` gives `["d","b","c","a"]`, focused `0`. `rearranged(&area, &[], None) == area`.
  - `areas::tests::work_areas_are_saved_and_read_back` (`cx`, `relaunch`): `open_at(cx, Settings::default(), Some(<tmp>/state.json))`; set `w.work_areas` to the area above with ids `"s1"`, `"s2"` (`cx.notify()`), `w.save_state(cx)`; `AppState::load(&path).work_areas` equals it. `open_at(relaunch, …)` on the same path, `launch` → `w.work_areas` equals it.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4:** `cargo check --workspace --all-targets`. Expected: no errors.
- [ ] **Step 5: Commit.** `git commit -m "feat: keep work areas in the saved state"`

---

### Task 2: Show an area, hide the others, show a session alone

**Files:**
- Modify: `crates/wai/src/areas.rs` (functions below, tests)
- Modify: `crates/wai/src/workspace.rs` (`show` and its doc; `restore`)
- Modify: `crates/wai/src/sidebar.rs` (test visibility only: `mod tests` → `pub(crate) mod tests`, `with_sessions` → `pub(crate)`)

**Interfaces:**
- Consumes: `area_of`, `area_now`, `work_areas`, `shown_area` (Task 1).
- Produces in `areas.rs`:
  ```rust
  /// Shows area `ix` (`lay_out_area`), focuses that pane and follows its session.
  pub(crate) fn show_area(&mut self, ix: usize, focus: Option<&Entity<ChatSession>>, window: &mut Window, cx: &mut Context<Self>);
  /// Shows `session`, in no area, alone in one pane (beside the editor pane, see `replace_panes`);
  /// the shown area, if any, hides as its panes left it. Focuses it and follows it.
  pub(crate) fn show_alone(&mut self, session: Entity<ChatSession>, window: &mut Window, cx: &mut Context<Self>);
  /// Hides the shown area, storing it as `area_now` gives it.
  fn hide_area(&mut self, cx: &App);
  /// `hide_area`, then area `ix`'s open sessions (`take(MAX_PANES)`; one empty pane if none) go
  /// on screen through `replace_panes`, and it is the shown area. Returns the pane to focus:
  /// `focus`'s, else the one showing the area's `panes[focused]`, else 0.
  fn lay_out_area(&mut self, ix: usize, focus: Option<&Entity<ChatSession>>, cx: &mut Context<Self>) -> usize;
  /// Puts `panes` on screen with the editor pane, if there is one, kept after them (at
  /// `MAX_PANES` it takes the last pane). A session leaving the screen that is in no area closes
  /// its subagent drawer (`close_drawer_at`), as `show` does; an area's sessions keep theirs.
  fn replace_panes(&mut self, panes: Vec<Pane>, cx: &mut Context<Self>);
  ```
  `show_area` and `show_alone` end with `focus_pane`, `follow_session`, `save_state`, like `show`.
- `show` in `workspace.rs`, before today's body (`on_screen` = a pane shows `session`):
  - in an area, and not (`shown_area == Some(area)` and `on_screen`) → `show_area(area, Some(&session))`;
  - else `!side && !on_screen` → `show_alone(session)`;
  - else today's body (focus where it is, or the side split).
- `restore`: after the saved panes are placed, when `state.shown_area` is a valid index, `self.focused_pane = self.lay_out_area(ix, None, cx)`; the `focus_pane(0, …)` that ends `restore` becomes `focus_pane(self.focused_pane, …)` (if the workspace track already changed that call, keep its pane when no area is shown). No `follow_session`.

- [ ] **Step 1: Write the tests** in `areas::tests`. Helpers: `with_areas(n, members: &[&[usize]], cx) -> (Entity<Workspace>, &mut VisualTestContext, Vec<Entity<ChatSession>>)` (`sidebar::tests::with_sessions(n)`, then `work_areas` = one `WorkArea` per entry, named `"Area {i}"`, holding those sessions' ids), `on_screen(&workspace, cx) -> Vec<Option<Entity<ChatSession>>>`, and `show_area(&workspace, ix, cx)` (a `cx.update` around `Workspace::show_area(ix, None, …)`).
  - `showing_an_area_puts_back_its_panes`: `with_areas(4, [[0,1,2],[3]])`. Show area 0 → `on_screen == [s0,s1,s2]`, `shown_area == Some(0)`. `focus_pane(2)`. Show area 1 → `[s3]`. Show area 0 → `[s0,s1,s2]`, `focused_pane == 2`. All 4 sessions still in `w.sessions`, none `TurnState::Error`.
  - `an_outside_session_shows_alone`: `with_areas(4, [[0,1]])`. Show area 0; `show(s2, false)` → `[s2]`, `shown_area == None`, `work_areas[0].panes == [id0, id1]`. `show(s3, true)` → `[s2, s3]`, area unchanged. `show(s2, false)` → panes unchanged, `focused_pane == 0`. Show area 0, then `show(s2, false)` → `[s2]` (the loose split was not kept).
  - `clicking_a_session_of_another_area_shows_that_area`: `with_areas(4, [[0,1],[2,3]])`. Show area 0; `show(s3, false)` → `[s2, s3]`, `focused_pane == 1`, `shown_area == Some(1)`.
  - `switching_keeps_the_editor_pane`: `with_areas(6, [[0],[1,2,3,4]])`. Show area 0; `open_file(<tmp>/a.txt)` → `[s0, Editor]`; mark its tab `dirty = true`. Show area 1 → panes `[s1,s2,s3, Editor]`, the same editor entity, its tab still dirty, `work_areas[1].panes.len() == 4`. `show(s5, false)` → `[s5, Editor]`. Show area 0 → `[s0, Editor]`.
  - `hidden_area_keeps_a_waiting_permission`: `open(cx, fake_settings("--ask-permission"))`, `start_now` twice (s0, s1), areas `[[s0],[s1]]`. Show area 0; `simulate_input("edit")`, `enter`; show area 1 at once. `wait_until` s0 is `NeedsYou` → `s0.model.waiting` is non-empty, s0 is in no pane. Show area 0 → `within("waiting").click("perm-allow_once")` → `wait_until` s0 is Idle and `done`.
  - `a_shown_area_comes_back_on_relaunch`: state file with `open_sessions` `a` (pane 0) and `b` (pane 1), both agent `fake`, cwd the temp dir; `work_areas: [WorkArea { name: "A", panes: ["a","gone","b"], focused: 2, .. }]`, `shown_area: Some(0)`. `open_at(fake_settings(""))`, `launch` → panes show `a`, `b`; `focused_pane == 1`; `shown_area == Some(0)`; `work_areas[0].panes == ["a","gone","b"]`; `w.project` unchanged.
- [ ] **Step 2: Implement** the interfaces above.
- [ ] **Step 3:** `cargo check --workspace --all-targets`. Expected: no errors.
- [ ] **Step 4: Commit.** `git commit -m "feat: show a work area's panes and hide the others"`

---

### Task 3: Sessions join and leave an area

**Files:**
- Modify: `crates/wai/src/areas.rs` (functions below, tests)
- Modify: `crates/wai/src/workspace.rs` (`close_pane`, `drop_session`, `show`'s side path)
- Modify: `crates/wai/src/sidebar.rs` (`ungroup`)

**Interfaces:**
- Consumes: `area_of`, `area_now`, `show_area` (Tasks 1, 2).
- Produces in `areas.rs`:
  ```rust
  pub(crate) const AREA_FULL: &str = "A work area holds up to 4 sessions";
  /// A session dropped on area `ix`'s row joins it (D2, D3): leaving its group or other area, as a
  /// new pane at the right while `ix` is shown and has room. A full area refuses with a warning
  /// notification `AREA_FULL` and returns false.
  pub(crate) fn add_to_area(&mut self, ix: usize, session: &Entity<ChatSession>, window: &mut Window, cx: &mut Context<Self>) -> bool;
  /// Session `id` leaves its group and area (`ungroup`), then joins area `ix` at the end.
  fn join(&mut self, ix: usize, id: &str);
  /// D14: while an area is shown, `session`, in no area, which the user just put in a pane,
  /// joins it when it holds fewer than `MAX_PANES`.
  fn adopt(&mut self, session: &Entity<ChatSession>, cx: &App);
  ```
  `add_to_area` does nothing (returns true) for a session already in `ix`; it pushes `Pane::Session` only when the session is in no pane and `panes.len() < MAX_PANES`; it saves and notifies.
- `sidebar.rs` `ungroup`: "Takes session `id` out of its group and its work area." Also `retain`s every area's `panes`. Its callers (`end_session`, `RestoreFailed`, `move_to_group`, `new_group`) need no change.
- `workspace.rs`:
  - `close_pane`, after the editor early return: when `panes[ix]` shows a session of the shown area, `self.ungroup(&id)` before `drop_pane`. (The pane's ×, `ChatEvent::Hide`, and ⌘W both come here.)
  - `drop_session`: inside the `layout::place` branch, after the panes are rebuilt and before its `save_state`, `self.adopt(session, cx)` for a dropped session.
  - `show`: in today's body, `if side { self.adopt(&session, cx) }` before its `save_state`.

- [ ] **Step 1: Write the tests** in `areas::tests` (helpers from Task 2; `add(&workspace, ix, &session, cx) -> bool` wraps `add_to_area`):
  - `a_dropped_session_joins_at_the_right`: `with_areas(3, [[0], []])`. Show area 0; `add(0, s1)` → `[s0, s1]`, area 0 `== [id0, id1]`. `add(1, s2)` (hidden) → area 1 `== [id2]`, panes unchanged.
  - `a_full_area_refuses`: `with_areas(5, [[0,1,2,3]])` → `add(0, s4) == false`, area unchanged, `window.notifications(cx)` non-empty.
  - `a_session_is_in_one_place_at_most`: `with_areas(2, [[0], []])`, `w.groups = vec![SessionGroup { name: "G".into(), collapsed: false, sessions: vec![id1], color: 0 }]`. `add(1, s0)` → area 0 `== []`, area 1 `== [id0]`. `add(0, s1)` → area 0 `== [id1]`, `groups[0].sessions == []`.
  - `closing_a_pane_takes_it_out_of_its_area`: `with_areas(3, [[0,1],[2]])`. Show area 0; `focus_pane(1)`; `secondary-w` → `[s0]`, area 0 `== [id0]`, s1 still in `w.sessions`. `end_session(s2)` (area 1 hidden) → area 1 `== []`, area 0 unchanged.
  - `a_draft_does_not_push_a_session_out`: `with_areas(3, [[0,1],[2]])`. Show area 0; `focus_pane(1)`; `secondary-n` → `panes[1]` is a draft; `area_now(0).panes == [id0, id1]`. Show area 1, then area 0 → `[s0, s1]`.
  - `an_area_keeps_its_panes_as_moved`: `with_areas(3, [[0,1],[2]])`. Show area 0; drag s1's `session-title` (`within(View(s1))`) with `drag_to_pane(from, 0, 0.5)` → `[s1, s0]`. Show area 1, then area 0 → `[s1, s0]`.
  - `open_to_the_side_joins_the_shown_area`: `with_areas(3, [[0]])`. Show area 0; `show(s1, true)` → `[s0, s1]`, area 0 `== [id0, id1]`. `show(s2, false)` → `[s2]`, area 0 still `[id0, id1]`.
- [ ] **Step 2: Implement** the interfaces above.
- [ ] **Step 3:** `cargo check --workspace --all-targets`. Expected: no errors.
- [ ] **Step 4: Commit.** `git commit -m "feat: add and remove sessions in a work area"`

---

### Task 4: The Work areas section of the Sessions panel

**Files:**
- Modify: `crates/wai/src/sidebar.rs` (`render_sessions`, new section and row functions, `new_work_area`, `delete_work_area`, `rename`, `commit_rename`, `reorder`, the `groups-collapse` handler, tests)
- Modify: `crates/wai/src/workspace.rs` (`Renamed` gains `Area(usize)`)
- Modify: `docs/user-guide.md` (Sessions panel bullets)

**Interfaces:**
- Consumes: `show_area`, `add_to_area`, `area_now`, `area_of`, `shown_area` (Tasks 1–3).
- Produces in `sidebar.rs` (`impl Workspace`):
  - `fn new_work_area(&mut self, window, cx)`: pushes `WorkArea { name: "New work area", ..Default }`, saves, `rename(Renamed::Area(last))`. Does not show it (D1).
  - `fn delete_work_area(&mut self, ix: usize, cx)`: `renaming = None`; `shown_area` becomes `None` when it was `ix`, one less when it was after `ix`; removes the area; saves, notifies.
  - `rename`/`commit_rename`/`renaming` handle `Renamed::Area(ix)` as `Renamed::Group(ix)` (an empty name keeps the old one).
  - `reorder(&mut self, dragged, target, window, cx)`: gains `window` (its `open_row` call site passes it); when `target` is in area `a`, `add_to_area(a, dragged session)` and return.
  - The section, drawn first inside the `open-list` scroll, above `open-ungrouped`:
    - Heading in `section_header`'s text style, with no chevron and no click: "Work areas" (drawn uppercase), then `title_button("new-work-area", IconName::Plus, "New work area")` → `new_work_area`. Always shown.
    - Per area, a row built like `group_header`: `Button ("work-area-toggle", ix)` chevron (aria "Expand work area" / "Collapse work area") flips `collapsed` and saves; then `("work-area", ix)` on `aurora::tab(shown, cx)` with `aria_label("{name}, {count}")`, `aria_selected(shown)`, `drag_over(light)`, `on_drop` → `add_to_area(ix, session)` for `Dragged::Open`, `Escape` → `end_rename(false)`; children: `▥` (faint), the name (truncates) or the rename input, the count (faint), a spacer, and when hidden with `needs > 0` an amber `● N` with id `("work-area-needs", ix)`. Click: one → `show_area(ix, None)`; two → rename. Menu: "Rename work area", "Delete work area". Row container id `("work-area-row", ix)`.
    - Under an expanded area (or any area with a filter hit), its open sessions in `area_now(ix).panes` order, as `open_row(session_ix, session, true, cx)`.
  - `render_sessions`: the ungrouped rows and Open's count skip sessions in an area; with a filter, an area with no hit hides and one with hits shows expanded with only those rows (its `collapsed` unchanged); area hits count for `matched` (so "No sessions match" stays right). The `groups-collapse` button also sets every area's `collapsed`.

- [ ] **Step 1: Write the tests** in `sidebar::tests` (helper `work_area(ix) -> ElementId`; areas set directly with `workspace.update(cx, |w, cx| { w.work_areas = …; cx.notify() })`):
  - `work_areas_are_created_and_renamed`: `press("new-work-area")` → `work_areas == [WorkArea { name: "New work area", .. }]`, `renaming.is_some()`, `shown_area == None`, panes unchanged. `type_name("Evolution BB", "enter")` → `label(work_area(0)) == "Evolution BB, 0"`. `menu_item(work_area(0), "Rename work area")` + `type_name("", "enter")` keeps the name; a double click then `type_name("Later", "escape")` keeps it too.
  - `rows_drag_onto_a_work_area_and_back`: `with_sessions(3)`; create area "A". `drag(open_row(0), work_area(0))` → `work_areas[0].panes == [id0]`, `label(work_area(0)) == "A, 1"`, and `open_row(0)`'s centre lies below `work_area(0)`'s and above `"open-section"`'s. `drag(open_row(1), open_row(0))` → `== [id0, id1]`. `drag(open_row(0), "open-ungrouped")` → `== [id1]`. `label("open-section") == Some("Open 2")`.
  - `clicking_a_work_area_shows_it`: `with_sessions(3)`; areas `A = [0,1]`, `B = [2]`. `press(work_area(0))` → panes show s0, s1; `find(work_area(0)).selected() == Some(true)`, `work_area(1)` `Some(false)`. `press(open_row(2))` → panes `[s2]`, `shown_area == Some(1)`, area 1 selected.
  - `a_hidden_work_area_says_who_needs_you`: `fake_settings("--ask-permission")`, `start_now` twice; areas `[s0]`, `[s1]`. `press(work_area(0))`; `simulate_input("edit")`, `enter`; `press(work_area(1))`; `wait_until` s0 is `NeedsYou` → `label(("work-area-needs", 0usize)) == Some("● 1")`, `("work-area-needs", 1usize)` is `None`. `press(work_area(0))` → `("work-area-needs", 0usize)` is `None`.
  - `a_work_area_folds_and_deletes`: `with_sessions(3)`; areas `A = [0]`, `B = [1]`. `press(("work-area-toggle", 0usize))` → `collapsed`, `label(open_row(0)) == None`; press again → shown. `press(work_area(1))`. `menu_item(work_area(0), "Delete work area")` → `work_areas` is `[B]`, `shown_area == Some(0)`, 3 sessions open, `label(open_row(0))` is some (back in Open), panes still `[s1]`.
  - `filter_finds_sessions_in_work_areas`: `with_sessions(3)`; area `A = [0]`, collapsed. `secondary-k`, `"SESSION 0"` → `label(open_row(0))` is some and `label(work_area(0))` is some. `escape`, `"zzz"` → `label(work_area(0)) == None`. `escape` → `work_areas[0].collapsed`, `label(open_row(0)) == None`.
- [ ] **Step 2: Implement** the interfaces above.
- [ ] **Step 3: User guide.** In the Sessions panel bullet, the sections from the top become "… a **Filter sessions** field (…); the **Work areas** section; the **Open** section; and the **History** section." Replace the "**Opening a row:**" bullet with the first block, and add the second after the "**Groups:**" bullet:
  ```markdown
  - **Opening a row:** a click shows the session alone in one pane (an open editor stays beside it) or, for a session in a work area, shows that area. A session already in a pane takes the focus there. **Open to the side** adds a pane, while there are fewer than four.
  - **Work areas:** a work area keeps a few sessions side by side, the way you arranged them. The **+** on the **Work areas** heading adds one, ready for its name. Drag an **Open** row onto it to add that session as a new pane on the right; an area holds four (`A work area holds up to 4 sessions`). Its row moves under the area's row, which folds like a group and shows how many sessions it holds. A click on the area shows its panes as you left them, focus included; a click on a session outside it, or on another area, hides it as it is, and its sessions keep running. A hidden area whose sessions wait for you shows an amber `● N`. While an area shows, move panes, close them and open sessions to the side: the area keeps the result. A pane's **×**, dragging the row back to **Open** or ending the session takes it out of the area; it stays open. A file or a draft may take one of its panes for a while: the session there comes back the next time the area shows. A session is in one area or one group, not both. The menu has **Rename work area** and **Delete work area** (its sessions go back to **Open**). Areas, and the one showing, are kept on relaunch.
  ```
- [ ] **Step 4:** `cargo check --workspace --all-targets`. Expected: no errors.
- [ ] **Step 5: Commit.** `git commit -m "feat: list work areas in the Sessions panel"`

---

### Task 5: Gate, final review, one fix wave

- [ ] **Step 1: Sync.** If `development` moved (`git log --oneline feat/work-areas..development`), merge it into `feat/work-areas` in the worktree and resolve there; `cargo check --workspace --all-targets`.
- [ ] **Step 2: Full gate** (Global Constraints). Record pass/fail counts, and each failure's test name and first error line. Expected regressions to look at first: a test that clicked an off-screen session in a loose split (D9; the planner found none), and the `AppState` literal in `state_roundtrip_and_corrupt_falls_back`.
- [ ] **Step 3: One whole-branch review** (opus) against this plan and the spec: Decisions, Review Focus, the anti-goals (no session ended, paused or dropped from an area by a plain click; no lost sidebar feature; no timer), copy, the "not ours" files untouched.
- [ ] **Step 4: One fix wave** for gate failures and the review's Critical/Important findings (Minors listed in the report, not fixed). Commit `fix: apply the work areas review`. Re-run the gate. Expected: fmt and clippy clean, `cargo test --workspace --no-fail-fast` 0 failed.
- [ ] **Step 5: Report** to the controller: commits, gate counts, deferred minors, and what the owner checks in the app (create "Evolução BB" with three sessions, rearrange its panes, click a session outside it and come back, let a hidden area's session ask for permission, relaunch). `cargo clean` in the worktree.

---

## Coverage

| Spec item | Task |
|---|---|
| Goal 1: named set, up to `MAX_PANES`, focus kept | 1, 3 |
| Goal 2: showing restores the panes exactly | 1 (`rearranged`), 2 |
| Goal 3: outside session or other area hides the area intact; outside session alone | 2 |
| Goal 4, Layout A: section above Open, ▥ row, count, highlight, + | 4 |
| Fill by drag; Organize (move, open, close); Switch | 3, 4 |
| My calls 1–10 | D1–D10 (Tasks 1–4) |
| Anti-goals: no silent drop, no pause, needs-you sign, idle CPU, no lost feature | 2, 3, 4 (Review Focus 1–3), Global Constraints |
| Left to the builder: data model, runtime home of hidden layouts, pane sizes, test ids | D11, D12, D17, D19 |
