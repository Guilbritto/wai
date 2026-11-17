# WayCarbon full-width panel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `wide` sidebar panel that hides the sessions area while shown, three layout primitives, and
wai-waycarbon 0.6.0 rebuilt as list/board + detail on top of them, without the Help tab.

**Architecture:** wai derives "wide shown" from the selected sidebar tab (nothing saved) and folds it
into `main_shown`. The panel schema gains `row.fill`, `column.width` and `card.selected`; the renderer
gives a fill row the remaining height and each of its columns its own scroll with stable ids. The Go
extension keeps one server; each tab paints `toolbar / alerts / row fill [list, detail]` from server
state (`sel` per tab, detail caches filled off the lock).

**Tech Stack:** Rust + gpui (`crates/wai`, `crates/wai-ext`), Go 1.x stdlib (`extensions/wai-waycarbon`),
`gh` and `jira` CLIs.

**Spec:** `docs/superpowers/specs/2026-11-16-waycarbon-full-width-design.md`

## Global Constraints

- wai commits `feat:`/`fix:`; extension commits `chore:`; no scope, no Co-Authored-By.
- `wai_ext::manifest::API` becomes `"1.6"`; the extension manifest `api = "1.6"`, `version = "0.6.0"`.
- Trees: at most 2,000 nodes, depth 32; button/section/tab/field ids unique across the whole tree.
- `column.width` clamped to 24–2,000 px.
- Board order: Tech Refinement, To Do, Ready For Dev, Development, Development Done, Code Review,
  Code Review Done, Ready For Test, Test, Test Done; unknown statuses after, in first-seen order.
- Widths: Work list 380, PRs list 420, Jira detail 420, collapsed column 28.
- Trims: review body 1,000 chars, description 4,000 chars.
- The extension never reads tokens; detail fetches happen only on selection, never in `refreshWork`.

## Review Focus

- A refresh while an item is selected: the selection, the Jira scope and both searches survive
  (test `TestSelectionSurvivesRefresh`).
- Clicking a detail action in one tab while another tab has the same item: ids stay unique
  (`TestPanelIdsAreUnique` over a tree with every tab showing a detail).
- A session opened from the wide panel must land in a visible pane (Rust e2e in `waycarbon.rs`
  asserts the sidebar tab is Sessions after the click).
- A detail fetch that answers after the selection moved, or after a refresh, must not paint stale
  data over the new item (cache keyed by URL/key; test `TestStaleDetailDropped`).
- An ADF description with unknown node types or a plain string description renders text, never
  JSON (test `TestADFText`).

---

### Task 1: wai — `wide` panels

**Files:**
- Modify: `crates/wai-ext/src/manifest.rs` (`PanelSpec`, validation, `API`), `crates/wai/src/ext_host.rs`
  (`Panel.wide`), `crates/wai/src/workspace.rs` (`main_shown`, `start_session`, `open_panel`),
  `crates/wai/src/sidebar.rs` (rail click)
- Test: `crates/wai-ext/tests/manifest.rs`, `crates/wai/src/ext_host.rs` tests

**Interfaces:**
- Produces: `PanelSpec.wide: bool` (`#[serde(default)]`); `Panel.wide: bool`;
  `Workspace::wide_shown(&self, cx: &App) -> bool`; `Workspace::main_shown(&self, cx: &App) -> bool`;
  `Workspace::leave_wide(&mut self, cx: &App)` (sets `sidebar_tab = Sessions` when `wide_shown`).

- [ ] **Step 1: failing tests**
  - manifest: `wide = true` on a sidebar panel parses to `wide: true`; `wide = true` with
    `location = "main"` is an error naming `wide`.
  - ext_host (`start_panels` with a wide `side` panel, a `WIDE` manifest constant):
    `wide_panel_hides_the_main_area` — click `("tab-panel", 1)`: `drawn("pane-0")` false, `drawn("sidebar")`
    true; click it again: tab is Sessions and `pane-0` drawn; click it, then click `tab-sessions`: pane drawn.
  - `start_session` while wide: tab becomes Sessions.
- [ ] **Step 2:** run `cargo test -p wai-ext manifest` and `cargo test -p wai wide` → FAIL.
- [ ] **Step 3: implement.** `main_shown = !(closed && empty) && !wide_shown`. Rail closure: when the
  clicked tab is selected, the sidebar shown, `click_count() == 1` and the tab is wide → `show_tab(Sessions)`;
  otherwise today's branch. `leave_wide` at the top of `start_session` and `open_panel`. `API = "1.6"`.
- [ ] **Step 4:** tests pass; `cargo test -p wai -p wai-ext` green.
- [ ] **Step 5:** commit `feat: show a wide extension panel in place of the sessions area`.

### Task 2: wai — layout primitives

**Files:**
- Modify: `crates/wai-ext/src/panel.rs` (`Node::Column { children, width }`, `Node::Row { children, fill }`,
  `Node::Card { .., selected }`), `crates/wai/src/ext_panel.rs` (render), `crates/wai/src/waycarbon.rs`
  (pattern), `docs/extensions.md`
- Test: `crates/wai-ext/src/panel.rs` tests, `crates/wai/src/ext_panel.rs` tests

**Interfaces:**
- Produces: `Node::Column { children: Vec<Node>, width: Option<f32> }`,
  `Node::Row { children: Vec<Node>, fill: bool }`, `Node::Card { .., selected: bool }`.

- [ ] **Step 1: failing tests**
  - parse: `{"type":"row","fill":true,...}` → `fill: true`; `{"type":"column","width":10}` → `Some(24.0)`,
    `5000` → `Some(2000.0)`, a string → `None`; card `"selected": true` → `selected: true`.
  - render (`panel_window`, `show`, `shown`): `fill_row_splits_and_scrolls` — a column with a text and a
    `row fill [column width=200 [60 texts], column [text]]`: the panel root does not scroll, element
    `("panel-fill", 0)` and `("panel-fill", 1)` are drawn, the first is 200 px wide and its bounds end at
    the panel's bottom. A fill row under a `row` is drawn as a plain row (no `panel-fill`).
  - `text` with `"a\nb"` draws two lines (height ≈ 2× one line).
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3: implement.** In `At` add `fill_rows: usize` (count of fill rows seen in this tab).
  A fill row (all ancestors columns, `At.row == false`) → `h_flex().flex_1().min_h_0().items_stretch()`,
  no wrap, and each child column gets `.id(("panel-fill", scope, ix)).h_full().overflow_y_scroll()` where
  `scope` is the tabs' current id; it marks the root as filling so the root does not scroll. Column
  `width` → `.w(px(w)).flex_none()`. Card `selected` → the theme's selected border/background.
  Text with `\n`: split into a `v_flex` of lines if gpui does not break.
- [ ] **Step 4:** `cargo test -p wai -p wai-ext` green; update `docs/extensions.md` (manifest `wide`,
  `row.fill`, `column.width`, `card.selected`, API 1.6).
- [ ] **Step 5:** commit `feat: let extension panels split into scrolling columns`.

### Task 3: extension — remove Help

**Files:** delete `help.go`, `help_test.go`, `helpview.go`, `helpapi/`, `relay/`; modify `main.go`,
`view.go`, `README.md`, `main_test.go`, `crates/wai/src/waycarbon.rs` (no `helpapi` copy).

- [ ] **Step 1:** delete, then remove `help`, `helpInit`, `helpStart`, `helpAction`, `helpCounts`,
  `sessionState` (if only Help uses it), `counts.Help/Replies`.
- [ ] **Step 2:** `go vet ./... && go test ./...` green; `cargo test -p wai waycarbon` green.
- [ ] **Step 3:** commit `chore: drop the WayCarbon help requests`.

### Task 4: extension — data for details

**Files:** modify `jira.go`, `github.go`; test `jira_test.go` (new), `github_test.go` (new) or `work_test.go`.

**Interfaces:**
- `sprintCards(ctx) ([]Card, error)` — `jira issue list -a <me> -s~Done -q "sprint in openSprints()" --plain --no-headers --columns key,summary,status`.
- `type CardDetail struct{ Key, Summary, Status, Type, Priority, Assignee, ParentKey, ParentSummary, Description, Site string; Subtasks []Card }`;
  `viewCard(ctx, key) (CardDetail, error)` from `jira issue view <key> --raw`; `Site` from the host of `self`.
- `adfText(raw json.RawMessage) string` — string as is; ADF: paragraph/heading/listItem/codeBlock → lines, text joined, max 4,000 runes.
- `moveTo(ctx, key, status string) error` — `jira issue move <key> <status>`; `moveCard` calls it with `WAI_WAYCARBON_DONE`.
- `type Check struct{ Name, State string; URL string }`, `type Thread struct{ Author, Body, Path string; Line int }`,
  `type PRDetail struct{ Additions, Deletions, Files int; Author string; Checks []Check; Reviews []Review; Threads []Thread }`;
  `prDetail(ctx, url) (PRDetail, error)` — the verified `resource(url:)` query; failed checks first.
- `searchPRs(ctx, org, q string) ([]PR, error)` — `search(query: "<q> in:title is:pr org:<org>", type: ISSUE, first: 50)`, `PR.State` = OPEN/MERGED/CLOSED, newest first.

- [ ] **Step 1:** tests with fake CLIs (as `fakeJira`/`fakeGh`): `TestADFText` (doc with heading, paragraph,
  bullet list, code block, unknown node; and a plain string), `TestViewCard` (fixture `testdata/issue.json`),
  `TestPRDetailFailedFirst` (fixture `testdata/prdetail.json`), `TestSearchPRs` (fixture `testdata/search.json`).
- [ ] **Step 2:** FAIL → implement → PASS.
- [ ] **Step 3:** commit `chore: load card and PR details for the WayCarbon panel`.

### Task 5: extension — wide views, selection and routing

**Files:** modify `view.go` (shared layout, PR detail), `work.go` (Work/PRs lists), new `board.go`
(Jira board + card detail), `main.go` (state, routing), `wai-extension.toml`, `README.md`;
tests `work_test.go`, `board_test.go`, `main_test.go`, `crates/wai/src/waycarbon.rs`.

**Interfaces:**
- Server state: `sel map[string]string` (tab → item id), `scope string` ("sprint"|"mine"), `sprint []Card`,
  `prSearch *PRSearch{Query, Err string; Busy bool; PRs []PR}`, `prDetails map[string]*detail[PRDetail]`,
  `cardDetails map[string]*detail[CardDetail]` (`detail[T]{Busy bool; Err string; Data T}`), cleared by `refreshWork`.
- `split(listWidth float64, list, detail node) node` → `row fill [column width, column]`.
- Item ids: Work `pr:<url>` / `card:<key>`; Jira `card:<key>`; PRs `pr:<url>` / `group:<key>`. Clicks send `sel:<tab>:<id>`.
- Detail buttons: `act:<tab>:<inner>` where `<inner>` is today's `clickPrompt` id; the server strips `act:<tab>:`.
- Moves: `move:<key>:<status>` (menu items, confirm). `jira:scope:sprint`, `jira:scope:mine`. `prs:search`
  (`values["prs:q"]`), `prs:clear`.
- `board(cards []Card, prs map[string]int) node` — fixed order, unknown after, empty column `width=28`
  with count and the name one letter per line.

- [ ] **Step 1: failing tests**
  - `TestBoardOrder` (statuses Test, Weird, To Do → columns in fixed order, Weird after Test Done; empty ones 28 wide).
  - `TestSelectionSurvivesRefresh` (select `pr:<url>` on Work; refresh; still selected; item gone → first item).
  - `TestSelectThenAct` (main_test: `sel:work:pr:<url>` repaints with `selected`; `act:work:pr:<url>` → `sessions/open`).
  - `TestMoveTo` (`move:EC-7:Code Review` → `jira issue move EC-7 Code Review`).
  - `TestPRSearch` (`prs:search` with `prs:q`=`EC-1240` → gh search argv; list shows MERGED badge).
  - `TestStaleDetailDropped`, `TestPanelIdsAreUnique` (every tab with a detail shown).
  - Rust `waycarbon.rs`: sprint argv in `FAKE_JIRA`; select the second card, click its Solve card → session
    pre-filled `/waycarbon:solve-card EC-102`, sidebar tab Sessions.
- [ ] **Step 2:** FAIL → implement → PASS (`go vet ./... && go test ./...`, `cargo test -p wai waycarbon`).
- [ ] **Step 3:** manifest 0.6.0 / api 1.6 / `wide = true`, description without help; README three tabs.
- [ ] **Step 4:** commit `chore: lay the WayCarbon panel out full width`.

### Task 6: verify

- [ ] `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `go test ./...`.
- [ ] Local build in the background and install per CLAUDE.md; `cargo clean` after.
- [ ] One sonnet reviewer over the branch diff; fix what holds.
