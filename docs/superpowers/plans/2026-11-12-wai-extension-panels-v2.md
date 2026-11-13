# Extension panels v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give extension panels the components DevHub-like screens need (card, section, table, log, alert, link, icon, spinner, divider, spacer, kv, menu, confirm), panels in the main area, panel icons, and API 1.2.

**Architecture:** The parser in `wai-ext` (`panel.rs`) grows new `Node` variants under the same caps; the renderer in `wai` (`ext_panel.rs`) draws them with gpui-component. A `main` panel is shown as `Pane::Extension` holding the `ExtPanel` entity `ExtHost` already owns. `panel/open` is a deferred host call routed to the workspace like `sessions/open`.

**Tech Stack:** Rust, GPUI via gpui-kit 0.9 / gpui-component 0.9, serde_json, toml.

**Spec:** `docs/superpowers/specs/2026-11-12-wai-extension-panels-v2-design.md`

## Global Constraints

- `initialize` sends `"api": "1.2"`; manifest `api = "major.minor"`; refusal text: `<name> needs a newer wai (extension API <api>; this wai has 1.2)`.
- Parsing never panics; `MAX_DEPTH` 32, `MAX_NODES` 2,000 unchanged; one-line strings ≤ `LINE_CHARS` 500; `text`/`markdown`/`alert` ≤ `TEXT_CHARS` 10,000.
- Caps: table 20 columns, 1,000 rows; log keeps the last 100,000 chars, ANSI stripped; kv 200 items; menu 50 items; multiline input rows default 4, max 20; table/log show 12 rows/lines without `fill`.
- `panel/open`: needs `panels`; one per 2 s per extension, else `Too many panel opens; try again later`; never takes keyboard focus.
- Links: only `http://` / `https://`, else `Unsupported`; no action sent.
- API 1.1 trees render as before (existing tests keep passing unchanged in meaning).
- No new hard-coded colours outside `aurora.rs`; theme tokens for tones.
- Commits: `feat:`/`fix:`/`test:`/`docs:`, no scope, no Co-Authored-By.

## Review Focus

- A `confirm` that is present but malformed must not degrade to "no confirmation": the node becomes `Unsupported` (test in Task 1).
- A table cell holding a form field or container: `Unsupported` cell, no id taken (Task 1).
- A click on a button/menu/link inside a clickable card or table row sends only the control's action (Task 3).
- A `main` panel's pane closes when the extension crashes or is disabled, and the workspace focus lands on a live pane (Task 4).
- A log that grows while the user scrolled up does not jump to the bottom (Task 3, follow only when at the end).

## Decisions taken while planning (spec gaps)

- `fill` keeps API 1.1's rule (honoured through columns only), applied the same way to `table` and `log`: the spec's "behaves as list's fill does today" wins over "direct child of the root".
- Icons: the app bundles the full Lucide catalog (`AllAssets`, ~750 KB) so any Lucide name works; the parser checks the name's shape (`[a-z0-9-]{1,64}`), the renderer resolves it (`IconName::ALL` by path) and shows `Unsupported` for an unknown `icon` node, nothing (or initials) for an unknown optional `icon`.
- `log` is virtualised by line (`uniform_list`, no wrapping) and has a Copy button instead of text selection.
- An extension pane has a slim header: icon, title, extension name, close ×.

---

### Task 1: Parser nodes (`crates/wai-ext/src/panel.rs`)

**Interfaces — Produces:**
- `pub enum Tone { Neutral, Info, Success, Warning, Danger }` (`Default` = Neutral).
- `pub struct Confirm { pub title: Arc<str>, pub text: Option<Arc<str>>, pub label: Option<Arc<str>> }`.
- `Node` changes: `Text { text, muted, mono }`, `Button { id, label, style, icon: Option<Arc<str>>, disabled: bool, confirm: Option<Confirm> }`, `Input { id, placeholder, value, rows: Option<u8> }` (`Some` = multiline), `Badge { text, tone }`.
- New: `Card { id: Option<String>, title, subtitle, badge: Option<Arc<str>>, icon, children: Vec<Node> }`, `Section { id: String, title, open: bool, badge, children }`, `Table { columns: Vec<Column>, rows: Arc<[Option<TableRow>]>, fill }`, `Log { text, follow, fill }`, `Alert { tone, title, text }`, `Link { text, url }`, `Icon { name, tone }`, `Spinner(Option<Arc<str>>)`, `Divider`, `Spacer`, `Kv(Arc<[(Arc<str>, Arc<str>)]>)`, `Menu { id: Option<String>, label, icon: Option<Arc<str>>, items: Vec<MenuItem> }`.
- `pub struct Column { pub title: Arc<str>, pub width: Option<f32> }`, `pub struct TableRow { pub id: Option<String>, pub cells: Vec<Node> }`, `pub struct MenuItem { pub id: String, pub label: Arc<str>, pub danger: bool, pub confirm: Option<Confirm> }`.
- `walk` visits card/section children and table cells. Constants `MAX_COLUMNS` 20, `MAX_ROWS` 1,000, `LOG_CHARS` 100,000, `MAX_KV` 200, `MAX_MENU` 50, `MAX_ROWS_INPUT` 20, `MAX_URL` 2,048.

- [ ] Tests: `new_nodes_parse` (one of each, exact `Node`), `new_nodes_break_on_bad_required_fields` (each required field missing/mistyped → `Unsupported`; `alert` tone `neutral`/unknown → `Unsupported`; malformed `confirm` → `Unsupported`; link `javascript:`/`file:`/whitespace → `Unsupported`), `table_caps_and_cells` (21 columns → 20, 1,001 rows → 1,000, extra cells dropped, a `column`/`input` cell → `Unsupported` without taking its id, node cells count toward `MAX_NODES`, string cells do not), `log_keeps_the_tail_without_ansi`, `section_ids_are_unique`, existing tests updated for the new fields.
- [ ] Implement; `cargo test -p wai-ext panel` passes.
- [ ] Commit `feat: parse the panel v2 nodes`.

### Task 2: Manifest and host protocol (`manifest.rs`, `host.rs`)

**Interfaces — Produces:** `PanelSpec { id, title, icon: Option<String>, location: PanelLocation }`, `pub enum PanelLocation { Sidebar, Main }` (serde lowercase, default Sidebar); `pub const API: (u32, u32) = (1, 2)`; `ManifestError::TooOld { name, api }`; `ExtEvent::OpenPanel { panel: String, reply: Reply }`.

- [ ] Tests: manifest `icon`/`location` parse, `location = "top"` refused, `api = "1.2"` ok, `api = "1.3"` → `"<name> needs a newer wai (extension API 1.3; this wai has 1.2)"` even with an unknown field beside it, `api = "1"`/`"x.y"` → invalid manifest; host `panel/open` without `panels` → permission denied, unknown panel → invalid params.
- [ ] Implement: `api` read from a loose `toml::Value` before the strict parse; `panel/open` → `Call::Deferred(ExtEvent::OpenPanel)`.
- [ ] Commit `feat: declare panel icons, main-area panels and the extension API version`.

### Task 3: Renderer (`crates/wai/src/ext_panel.rs`, `aurora.rs`, `app.rs`)

**Interfaces — Consumes:** Task 1 nodes. **Produces:** `ExtPanel::new(ext_name: SharedString)`; `pub(crate) fn lucide(name: &str) -> Option<IconName>`; `aurora::tile(cx) -> Div`.

- [ ] Tests: `every_node_renders` (all v2 nodes, no `panel-unsupported`), `unknown_icon_is_unsupported`, `card_row_and_menu_send_actions`, `button_in_a_row_sends_only_its_own`, `confirm_cancel_sends_nothing_confirm_sends_once`, `section_state_survives_rerender`, `log_follows_only_at_the_end`, `docs_samples_render` (Task 5 trees).
- [ ] Implement: section state in `values`/`initials` like a checkbox; confirm via `window.open_dialog` capturing the action id; nested controls wrapped with a mouse-down `stop_propagation` when inside a clickable card or row; log with `UniformListScrollHandle` per log; `AllAssets` replaces `ExtraIcons`.
- [ ] Commit `feat: draw the panel v2 nodes`.

### Task 4: Main-area panes, rail icons, `panel/open` (`workspace.rs`, `ext_host.rs`, `sidebar.rs`)

**Interfaces — Produces:** `Pane::Extension(Entity<ExtPanel>)`; `Panel { …, pub icon: Option<IconName>, pub main: bool, pub ext_name }`; `Workspace::open_panel(view, focus: bool, window, cx)`; `ExtHost` emits `SessionCall` for `OpenPanel` after its own rate check.

- [ ] Tests: `main_panel_opens_as_a_pane` (rail click opens once, second click focuses, not a sidebar tab), `main_pane_closes_with_its_panel`, `panel_open_shows_and_rate_limits`, `panel_icon_replaces_initials`.
- [ ] Implement.
- [ ] Commit `feat: show extension panels in the main area`.

### Task 5: Docs and sample trees

- [ ] `docs/extensions.md` at API 1.2: every node, the panel fields, `panel/open`, `confirm`, the two DevHub samples as JSON; the samples also live in `crates/wai/src/ext_panel.rs` tests via `include_str!` of `docs/samples/*.json`.
- [ ] Commit `docs: extension API 1.2`.
