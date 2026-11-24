# Extensions page (list and detail) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the single-column Extensions page in the Settings modal with a VS Code-style list pane and detail pane.

**Architecture:** `ExtensionsView` keeps its install/consent/update logic in `extensions_view.rs`; the rendering moves to a child module `extensions_view/page.rs` (a child module sees the view's private fields). Pure helpers (status, update lookup, features, links, filter) live in that module with unit tests. `SettingsView` gives the Extensions section its whole content area and a nav dot for updates.

**Tech Stack:** Rust, GPUI via `gpui-kit` 0.9 (`Button`, `Input`, `DropdownMenu`, `PopupMenuItem`, `IconName`), wai's `aurora`/`campo`/`fonts` helpers.

**Spec:** `docs/superpowers/specs/2026-11-24-extensions-page-design.md` (mockup: `.superpowers/brainstorm/12078-1795522903/content/extensions-vscode-v3.html`).

## Global Constraints

- Every install and update still goes through `consent`; nothing installs from a click alone.
- Every manifest or index string shown goes through `clamp` (`NAME_CHARS` for names/ids/versions, `DESCRIPTION_CHARS` for descriptions and error text), and single-line spots `truncate()`.
- UI copy is English. Commits: `feat:`/`fix:`/`docs:`, no scope, no Co-Authored-By.
- Build and test with `export PATH="$HOME/.cargo/bin:$PATH"; CARGO_TARGET_DIR=/Users/guilherme.brito/Project/Guilherme/wai/target` (shared target; the disk is nearly full).
- Work in the worktree `.claude/worktrees/extensions-page` on branch `feat/extensions-page` from `development`.

## Review Focus

1. The selected extension is uninstalled → the detail falls back to the first installed one, else the first listing, else an empty state; no stale detail, no panic. (Task 3 test `uninstall_moves_the_selection`.)
2. A name or description with newlines or 10 000 characters → one clamped, truncated line in the row; the hero clamps it too. (Task 2 test `long_name_stays_one_row`.)
3. Nothing installed and the index failing → INSTALLED says "No extensions installed", AVAILABLE shows the error box, detail shows "Select an extension". (Task 3 test `empty_page_has_an_empty_detail`.)
4. A search that matches nothing → "No extensions match" under each open section; ids stay the store/index position so other rows keep their ids. (Task 2 test `search_filters_rows`.)
5. An id both installed and listed → only the INSTALLED row; same version shows no Update, another version shows Update. (Task 2 test `update_shows_on_the_installed_row`.)

---

### Task 1: Pure helpers

**Files:**
- Modify: `crates/wai/src/extensions_view.rs` (add `mod page;` near the top)
- Create: `crates/wai/src/extensions_view/page.rs`

**Interfaces:**
- Produces (all `pub(super)` in `page.rs`):
  - `enum RowStatus { Running, Starting, Enabled, Crashed(String), Failed(String), Disabled }` with `fn label(&self) -> &'static str` → `"Running" | "Starting" | "Enabled" | "Crashed" | "Failed" | "Disabled"`.
  - `fn row_status(enabled: bool, status: Option<&ExtStatus>) -> RowStatus` — `!enabled` → Disabled first; `None` → Enabled.
  - `fn listed_update<'a>(ext: &Installed, entries: &'a [Entry]) -> Option<&'a Listing>` — same id, `package().is_some()`, version `!=` installed.
  - `fn listing_for<'a>(id: &ExtId, entries: &'a [Entry]) -> Option<&'a Entry>`.
  - `fn features(m: &Manifest) -> Vec<(&'static str, Vec<String>)>` — kinds in this order, only non-empty: `"Panels"` (panel title), `"Bar items"` (item title), `"Commands"` (title), `"Agents"` (name), `"MCP servers"` (name), `"Templates"` (name), `"Themes"` (the string). Each string clamped to `NAME_CHARS`.
  - `fn releases_url(repository: &str) -> Option<String>` — `https://github.com/<o>/<r>` (trailing `/` trimmed) → `…/releases`; anything else `None`.
  - `fn tint(id: &str) -> usize` — sum of bytes `% 6` (index into lilas, boreal, brasa, ceu, magenta, ok).
  - `fn matches(query: &str, fields: &[&str]) -> bool` — trimmed, case-insensitive `contains` on any field; empty query → true.
  - `fn platform_label(key: &str) -> String` — `darwin-arm64` → `macOS arm64`, `darwin-x64` → `macOS x64`, `linux-*` → `Linux *`, `windows-*` → `Windows *`, `any` → `Any`.

- [ ] **Step 1: Write the failing tests** in `page.rs`'s `#[cfg(test)] mod tests`:

```rust
#[test]
fn status_labels() {
    use crate::ext_host::ExtStatus::*;
    assert_eq!(row_status(false, Some(&Ready)).label(), "Disabled");
    assert_eq!(row_status(true, Some(&Ready)).label(), "Running");
    assert_eq!(row_status(true, Some(&Starting)).label(), "Starting");
    assert_eq!(row_status(true, None).label(), "Enabled");
    assert_eq!(row_status(true, Some(&Crashed("boom".into()))), RowStatus::Crashed("boom".into()));
    assert_eq!(row_status(true, Some(&Failed("nope".into()))), RowStatus::Failed("nope".into()));
}

#[test]
fn links_and_labels() {
    assert_eq!(releases_url("https://github.com/o/r/").as_deref(), Some("https://github.com/o/r/releases"));
    assert_eq!(releases_url("https://gitlab.com/o/r"), None);
    assert_eq!(platform_label("darwin-arm64"), "macOS arm64");
    assert_eq!(platform_label("windows-x64"), "Windows x64");
    assert_eq!(platform_label("any"), "Any");
    assert!(matches("", &["x"]) && matches(" USa ", &["Usage"]) && !matches("zzz", &["Usage", "wai-usage"]));
    assert_eq!(tint("wai-usage"), tint("wai-usage"));
    assert!(tint("wai-usage") < 6);
}

#[test]
fn features_group_by_kind() {
    let m = manifest("id = \"s\"\nname = \"S\"\nversion = \"1\"\npermissions = [\"panels\", \"process\"]\n\
        [process]\ncommand = \"bin/run\"\n\
        [[contributes.panels]]\nid = \"p\"\ntitle = \"Pet\"\n\
        [[contributes.mcp_servers]]\nname = \"playwright\"\ncommand = \"npx\"\n");
    assert_eq!(features(&m), vec![("Panels", vec!["Pet".to_string()]), ("MCP servers", vec!["playwright".to_string()])]);
}

#[test]
fn update_needs_another_listed_version() {
    // installed 1.0.0; listed 1.0.0 → None; listed 1.1.0 with a package for any platform → Some
}
```

`manifest(text)` parses with the same function `wai_ext` tests use (`Manifest::parse` or the crate's equivalent; check `crates/wai-ext/tests/manifest.rs`). Build the `Installed`/`Entry` values for the last test from `crate::marketplace::tests::{entry, index}` or directly. Adjust the TOML to whatever keys `PanelSpec`/`McpServerSpec` require.

- [ ] **Step 2: Run, expect compile failure**

Run: `cargo test -p wai --lib extensions_view::page`
Expected: FAIL (functions not defined).

- [ ] **Step 3: Implement the helpers** with the signatures above.

- [ ] **Step 4: Run, expect PASS**

Run: `cargo test -p wai --lib extensions_view::page`

- [ ] **Step 5: Commit**

```bash
git add crates/wai/src/extensions_view.rs crates/wai/src/extensions_view/page.rs docs/superpowers/specs/2026-11-24-extensions-page-design.md docs/superpowers/plans/2026-11-24-extensions-page.md
git commit -m "feat: add the Extensions page's status, update and feature helpers"
```

---

### Task 2: Two panes, the list, and the Settings page

**Files:**
- Modify: `crates/wai/src/extensions_view.rs` (state, dialogs, tests; delete `row`, `available_row` and the old `Render` body)
- Modify: `crates/wai/src/extensions_view/page.rs` (list pane, detail hero, Details tab)
- Modify: `crates/wai/src/settings_view.rs` (Extensions fills the content area; nav dot)
- Modify: `crates/wai/src/ext_host.rs:776` only if its `("ext-enabled", 0usize)` click stops working

**Interfaces:**
- Consumes: Task 1 helpers.
- Produces:
  - New `ExtensionsView` fields: `search: Entity<InputState>` (placeholder `"Search extensions"`, a `Subscription` on its change event that calls `cx.notify()`), `selected: Option<ExtId>`, `tab: DetailTab`, `closed: [bool; 2]` (INSTALLED, AVAILABLE).
  - `pub(crate) enum DetailTab { Details, Features, Permissions }` (Task 3 renders the last two).
  - `pub(crate) fn updates(&self) -> usize` — installed extensions with `listed_update(..).is_some()` (0 before the index loads).
  - `fn current(&self, installed: &[Installed], entries: &[Entry]) -> Option<ExtId>` — `selected` if it is installed or listed-not-installed, else the first installed, else the first listing not installed.
  - `fn install_url_dialog(&mut self, window, cx)` — like `add_index_dialog`, with `self.url`, title `"Install from a git URL"`, buttons ids `"git-cancel"` / `"git-install"` (calls `install_url`).
  - `SettingsView::nav_dot(&self, section: Section, cx: &App) -> bool` — `Extensions` → `updates() > 0`.
  - Element ids: `("ext-row", i)` installed row (i = store list index), `("ext-listing", i)` listing row (i = `entries` index), `("ext-available", i)` a listing row's Install button, `("ext-update", i)` an installed row's Update button, `("ext-manage", i)` the gear dropdown, `"ext-more"` the `⋯` dropdown, `"ext-refresh"`, `"ext-search"`, `"ext-section-installed"`, `"ext-section-available"`, `("ext-enabled", i)` and `("ext-uninstall", i)` the detail's Enable/Disable and Uninstall buttons (i = store index of the selected one), `"ext-install"` / `"ext-update-detail"` the detail's Install / Update to x buttons. Debug selectors: `"ext-detail: <id>"` on the detail root, `"settings-nav-dot-extensions"` on the dot.

Layout (values from the mockup): root `h_flex().size_full()`; list pane `w(px(340.))`, right border, header (title "Extensions" in `DISPLAY_FONT` 15, Refresh `IconName::RefreshCw` ghost xsmall, `⋯` `IconName::Ellipsis` ghost xsmall with `dropdown_menu` items "Install from folder…" → `pick_folder`, "Install from git URL…" → `install_url_dialog`, "Add index…" → `add_index_dialog`), search `campo::field(Input::new(&self.search).small(), false, cx)`, then a scrolling body. Section header: chevron (`IconName::ChevronDown`/`ChevronRight`), `INSTALLED`/`AVAILABLE` in 11 px `nevoa_2`, count; click toggles `closed`. Row: 38 px monogram (first char of the name, uppercase, `DISPLAY_FONT` bold 17, `rounded(px(10.))`, bg = tint color, text `noite`), name semibold + version 11.5 `nevoa_2`, description 12 px `nevoa` truncated, then status (dot + label, colors: Running/Enabled `ok`, Starting `boreal`, Crashed/Failed `erro`, Disabled `nevoa_2`) or the listing's index (`entry.from`, else `"Official index"`), and at the right Install (primary xsmall) / Update (primary xsmall) / gear (`IconName::Settings`, ghost xsmall, dropdown "Disable"|"Enable", "Uninstall"). Selected row: `lilas` 13% fill and a 2 px `lilas` left border; disabled rows dim the monogram and name. Under AVAILABLE, each `listings.errors` line in one `ambar` box (border `ambar` 30%, fill 7%). "Loading extensions…" while not loaded; "No extensions installed"; "No extensions match".

Detail (this task: hero + Details tab + tab bar; Features/Permissions bodies are Task 3): `overflow_y_scroll`; hero with 76 px monogram (`rounded(px(18.))`, 34 px letter), name `DISPLAY_FONT` 24, `aurora::badge(version)`, and when an update is listed a `boreal` badge `"<v> available"`; sub line `id · index · repository` (repository as a link opening the URL with `cx.open_url`); description `tinta_2`; actions: installed → [`Update to <v>` primary, only with an update] [`Disable`|`Enable`] [`Uninstall` ghost, `erro` text] + status; listing → [`Install` primary] or the text "Not available for this platform". Tab bar: Details, Features (count), Permissions (count) for installed; Details only for a listing; active tab underlined in `lilas`. Details tab: the Crashed/Failed text in an `erro` box (title "Crashed"/"Failed", text in the mono font, clamped to `DESCRIPTION_CHARS`), "ABOUT" + description, "CLAUDE CODE PLUGINS" with the required then suggested plugins exactly as `row` built them today (same `ext-plugin:<id>:<p>` ids, same `InstallPlugin` emit, same "Invalid name" for a bad one, only when enabled, `PLUGINS_SHOWN` cap); for a listing, the note "Permissions and features are listed when you install: wai downloads the package and shows what it asks for before anything runs."

`settings_view.rs`: when `section == Section::Extensions`, the right side is `v_flex().flex_1().min_w_0().min_h_0().children(self.data.extensions.clone())` — no `header`, no `column`, no `settings-content` scroll. In the nav item, before the count: when `self.nav_dot(s, cx)`, a 6 px round `boreal` dot with the debug selector.

- [ ] **Step 1: Write the failing tests** in `extensions_view.rs`'s tests (helpers: install two folder extensions `sample` and `other` with `store.stage`/`commit` like `extensions_page_shows_installed`):

```rust
#[gpui_kit::test]
fn selecting_a_row_shows_its_detail(cx: &mut TestAppContext) {
    // two installed; show_extensions; render
    assert!(cx.debug_bounds("ext-detail: sample").is_some(), "the first one by default");
    click(("ext-row", 1usize), cx);
    assert!(cx.debug_bounds("ext-detail: other").is_some());
    click(("ext-enabled", 1usize), cx);
    assert!(!installed(&view, cx)[1].enabled);
}

#[gpui_kit::test]
fn search_filters_rows(cx: &mut TestAppContext) {
    // type "oth" into view.search
    assert!(try_find(("ext-row", 0usize)).is_none() && try_find(("ext-row", 1usize)).is_some());
    // type "zzz": neither row; the text "No extensions match" (debug selector "ext-no-match")
}

#[gpui_kit::test]
fn sections_collapse(cx: &mut TestAppContext) {
    click("ext-section-installed", cx);
    assert!(try_find(("ext-row", 0usize)).is_none());
    click("ext-section-installed", cx);
    assert!(try_find(("ext-row", 0usize)).is_some());
}

#[gpui_kit::test]
fn update_shows_on_the_installed_row(cx: &mut TestAppContext) {
    // index lists sample 1.2.0 (installed 1.2.0) → no ("ext-update", 0), no ("ext-listing", 0), dot absent
    // index lists sample 1.3.0 + Refresh → ("ext-update", 0) present,
    // settings view nav_dot(Section::Extensions) == true, debug selector "settings-nav-dot-extensions" present
}

#[gpui_kit::test]
fn long_name_stays_one_row(cx: &mut TestAppContext) {
    // name = "Sample\\n" + "Totally Safe " * 20, description 10 000 chars (as in long_description_cannot_hide_permissions)
    // the row's bounds ("ext-row", 0) height < 100 px
}
```

Update existing tests to the new ids:
- `extensions_page_shows_installed`: assert `("ext-enabled", 0usize)` and `"ext-more"` exist (not `"ext-install-folder"`).
- `adds_an_index`: open the dialog with `view.update_in(cx, |v, window, cx| v.add_index_dialog(window, cx))` instead of clicking `"ext-add-index"`.
- `installs_and_updates_from_an_index`: after installing `sample`, `("ext-available", 0usize)` no longer exists (installed listings leave AVAILABLE); `("ext-available", 1usize)` stays the refused one; after the 1.3.0 Refresh click `("ext-update", 0usize)` instead of `("ext-available", 0usize)`.

- [ ] **Step 2: Run, expect FAIL**

Run: `cargo test -p wai --lib extensions_view`
Expected: the new tests fail (no `ext-row`, no `ext-detail:` selectors).

- [ ] **Step 3: Implement** the state, `current`, `updates`, `install_url_dialog`, the list pane, the hero, the tab bar, the Details tab, and the `settings_view` changes. `Render for ExtensionsView` keeps the "fetch on first render" line and returns the two panes from `page.rs`.

- [ ] **Step 4: Run the view's and the settings' tests, expect PASS**

Run: `cargo test -p wai --lib extensions_view settings_view ext_host`

- [ ] **Step 5: Commit**

```bash
git add crates/wai/src/extensions_view.rs crates/wai/src/extensions_view/page.rs crates/wai/src/settings_view.rs crates/wai/src/ext_host.rs
git commit -m "feat: show extensions as a list and a detail, like VS Code"
```

---

### Task 3: Features, Permissions, the meta column, and the empty state

**Files:**
- Modify: `crates/wai/src/extensions_view/page.rs`
- Modify: `crates/wai/src/extensions_view.rs` (tests)

**Interfaces:**
- Consumes: `features`, `releases_url`, `platform_label`, `listing_for` (Task 1); `DetailTab`, `current` (Task 2); `permission_text`, `LIMITATION` (existing).
- Produces: tab ids `"ext-tab-details"`, `"ext-tab-features"`, `"ext-tab-permissions"`; debug selectors `"ext-feature: <text>"`, `"ext-permission: <text>"`, `"ext-empty-detail"`.

Body below the tabs: a two-column `h_flex` — main (`flex_1`) and a 210 px meta column, gap 28, padding 20/28.
- Features: per kind from `features(&manifest)`, an 11 px uppercase label and a card (`aurora::card_fill`, `borda`, radius 12) with one row per string; none → note "This extension adds no panels, commands, agents, MCP servers or themes."
- Permissions: "IT ASKS TO" + a card with one row per `ext.granted` (`permission_text`), "No permissions" when empty, then `LIMITATION` in `ambar`.
- Meta, installed: INSTALLATION — Identifier (mono), Version (+ ` → <v>` in `boreal` with an update), Folder (`ext.dir`, mono). Listing: LISTING — Identifier, Version, Index (`from` or "Official index"), PLATFORMS chips from `listing.packages` keys via `platform_label`. RESOURCES when a repository is known (listing's, or the matching listing's for an installed one): "Repository ↗" and, with `releases_url`, "Releases ↗", each opening its URL.
- Switching selection resets `tab` to Details; a listing always shows Details.
- No current extension → the detail shows "Select an extension" centered (debug selector `"ext-empty-detail"`).

- [ ] **Step 1: Write the failing tests**

```rust
#[gpui_kit::test]
fn tabs_show_features_and_permissions(cx: &mut TestAppContext) {
    // MANIFEST + "[[contributes.commands]]\nid = \"go\"\ntitle = \"Go now\"\nprompt = \"go\"\n" installed
    click("ext-tab-features", cx);
    assert!(cx.debug_bounds("ext-feature: Go now").is_some());
    click("ext-tab-permissions", cx);
    for line in PERMISSION_LINES { assert!(cx.debug_bounds(format!("ext-permission: {line}").leak()).is_some()); }
}

#[gpui_kit::test]
fn uninstall_moves_the_selection(cx: &mut TestAppContext) {
    // sample and other installed; click ("ext-row", 1) then ("ext-uninstall", 1)
    assert!(cx.debug_bounds("ext-detail: sample").is_some());
}

#[gpui_kit::test]
fn empty_page_has_an_empty_detail(cx: &mut TestAppContext) {
    // nothing installed, the official index offline in tests
    assert!(cx.debug_bounds("ext-empty-detail").is_some());
}
```

- [ ] **Step 2: Run, expect FAIL**

Run: `cargo test -p wai --lib extensions_view`

- [ ] **Step 3: Implement** the bodies, the meta column and the empty state.

- [ ] **Step 4: Run the whole crate's tests, clippy and fmt**

Run: `cargo test -p wai && cargo clippy -p wai --all-targets -- -D warnings && cargo fmt --check`
Expected: all pass, no warnings, no diff.

- [ ] **Step 5: Commit**

```bash
git add crates/wai/src/extensions_view.rs crates/wai/src/extensions_view/page.rs
git commit -m "feat: list an extension's features, permissions and links"
```
