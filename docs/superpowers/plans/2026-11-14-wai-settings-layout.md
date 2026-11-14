# Settings layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Settings modal's nav and content scroll on their own, and every row lines up on one grid.

**Architecture:** All in `crates/wai/src/settings_view.rs` and `settings_view/claude.rs`. The root gets a zero minimum height so the dialog's own scroll area never scrolls; the content splits into a fixed header and a scrolling body, both in a centered 760 px column; one row helper and one card helper lay out every row.

**Tech Stack:** Rust, gpui-kit 0.9 (gpui-component).

**Spec:** `docs/superpowers/specs/2026-11-14-wai-settings-layout-design.md`

## Global Constraints

- Column: `max_w(px(760.))`, horizontal padding 32 px, centered (`h_flex().justify_center()`).
- Control column: `w(px(240.))`, `flex_none`, content aligned to its right edge.
- Every existing `debug_selector`, element id and aria label stays (the tests find them).
- Colors from `aurora` in dark mode, theme tokens in light mode.
- Commits `feat:`/`fix:`, no scope, no Co-Authored-By.

## Review Focus

- A page taller than the modal (AI with 5 agents): the modal keeps its height, the body scrolls, the nav does not move.
- A wide or maximized modal: rows stay inside the 760 px column.
- A failed update check: the status line still shows in danger color under Version.
- The broken-file banner: still shows, with its Open button, above the search.
- Light theme: cards use the theme's border and background.

---

### Task 1: Nav and content scroll on their own

**Files:**
- Modify: `crates/wai/src/settings_view.rs` (root of `Render for SettingsView`)
- Test: same file, `mod tests`

- [ ] **Step 1: Write the failing test** `tall_page_keeps_the_modal_height`: `fake_ws(cx, "")`, open `Section::Ai`, render a frame; assert `cx.debug_bounds("settings-modal").unwrap().size.height <= modal_size(viewport, false).height`.
- [ ] **Step 2: Run** `cargo test -p wai tall_page_keeps_the_modal_height` — expect FAIL (the modal is as tall as its content).
- [ ] **Step 3:** add `.overflow_hidden()` to the root `v_flex()`.
- [ ] **Step 4: Run** the test — expect PASS.
- [ ] **Step 5: Commit** `fix: scroll the settings nav and content on their own`.

### Task 2: One grid for every row

**Files:**
- Modify: `crates/wai/src/settings_view.rs` (`row`, `agent_row`, `default_model`, `advanced`, `version_row`, `rows`, `render`)
- Modify: `crates/wai/src/settings_view/claude.rs` (`group`, `claude_row`)
- Test: `crates/wai/src/settings_view.rs`, `mod tests`

**Interfaces:**
- Produces: `fn row(&self, label, hint: Option<&'static str>, selector, control, cx) -> Div` (label and hint left, 240 px control column right); `pub(super) fn settings_card(cx: &App) -> Div` (bordered glass card, rows divided by a top border); `pub(super) fn divided(rows: impl IntoIterator<Item = Div>, cx) -> Div`.

- [ ] **Step 1: Write the failing test** `rows_share_one_column`: open `Section::Updates` with the modal maximized; assert the bounds of `settings-updates-check`, `settings-update`, `settings-diagnostics` and `settings-updates-repo` have the same right edge and width, and that width `<= 696.`.
- [ ] **Step 2: Run** it — expect FAIL (rows span the whole content width).
- [ ] **Step 3:** implement the spec's layout: header outside the scroll (title 24 px, description, actions, banner, search); body `overflow_y_scroll` with the column; rows in cards; agents as cards with indented sub-rows; Version's status in its description; Advanced (settings.toml) as a card under an "ADVANCED" label; nav right border, faint fill, amber "Soon"; Claude groups' rows in one card.
- [ ] **Step 4: Run** `cargo test -p wai settings` — expect all PASS.
- [ ] **Step 5: Commit** `feat: align the settings rows on one grid`.
