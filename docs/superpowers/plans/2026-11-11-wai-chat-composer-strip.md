# Conversation composer like the draft Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In a running conversation, the session's information sits on the prompt box as in the mockup, not in the header: a strip of chips (folder · branch · ✳ agent) on the box's top edge, and the mode and model pickers in the box's bottom bar. This is the same composition as the draft screen.

**Architecture:** Move the chip-strip and bar-chip styling out of `chat/draft.rs` into one shared module, `chat/composer.rs`, used by the draft and by the conversation's `render_prompt` (`chat/input.rs`). The header (`render_header`, `chat/render.rs`) keeps only the title, the context-usage pill and the close button.

**Tech Stack:** Rust, GPUI via `gpui-kit` 0.9.0.

**Spec:** owner, 2026-11-11, after installing 0.6.1: "as informações foram parar no topo sendo que no mockup isso fica em cima do input". Visual reference: the draft state of `.superpowers/brainstorm/44632-1794399351/content/new-session-draft-v2.html` (classes `.strip`, `.chip`, `.composer`, `.cbar`). The draft screen already implements it, in `chat/draft.rs` ~lines 930–1100.

## Global Constraints

- Strip (`.strip`): sits directly on the prompt box's top edge (no gap), 18 px in from the box's left edge. Padding 5/6 px, gap 2. Fill white α 0.035. Border `aurora::borda()` on top, left and right, none at the bottom. Radius 14 14 0 0. Never wider than the box minus 18 px (chips shrink and truncate first, as in the draft).
- Strip chips in a live session are read-only labels, because the folder, branch and agent are fixed once the session runs:
  - Folder: Folder icon + folder name.
  - Branch: GitBranch icon, or GitFork for a worktree, + branch name. Shown only when known.
  - Agent: `✳` in `aurora::brasa()` for Claude (id `"claude"`; other agents get no mark), then `wai_settings::agent_label`.
  - Chip text is 13.5 px `tinta_2`, height 30, horizontal padding 10. No hover effect, no chevron.
- Bottom bar (`.cbar`), inside the box under the textarea, in this order:
  - `+` attach;
  - the mode chip (shield icon + current mode ▾), from the `mode` config option or the legacy mode picker, whichever the session shows today;
  - a flexible spacer;
  - the model chip, plus the thought-level chip when offered (the `self.pickers` selects);
  - Stop while busy, then Send.
  - Bar chips look like the draft's bar chips: Select with `appearance(false)`, 28 px, 13 px `nevoa`, hover fill `vidro()`.
- The prompt box keeps its behaviour: the textarea grows as today, the focus ring, paste, the `/` menu and the attachment chips. The box takes the draft composer's look (radius, fill, border, focus ring). Attachment chips go inside the box above the textarea, so the strip still touches the box.
- The status line stays above the strip. The pinned stack (1.6/1.7) and its list inset are unchanged.
- The header keeps the title (drag to move the pane), the usage pill and the close button. The pills and pickers leave it.
- Light theme: use the draft's light branches; no new colors.
- The draft screen must look exactly as it does now, after it switches to the shared helpers.
- Commits: `feat:`/`fix:`/`test:`/`docs:`, no scope, no Co-Authored-By. Tests never touch the real `~/.claude`, `~/.claude.json` or `~/.config/wai`.

## Review Focus

- A narrow pane, with a long branch name and a long folder name: the strip stays inside the box.
- An agent with no mode and no model options: the bar shows only `+`, the spacer and Send/Stop, with no empty chips.
- A turn running while the session has a pending permission: the status line, the strip and the pinned stack all show, and none overlaps another.
- Changing the model or mode from the bar applies to the session, as the header pickers did. The existing tests for the pickers must still pass, retargeted to the new place.
- The draft → conversation hand-off: right after the first send, the strip and the bar sit where they sat in the draft.

---

### Task 1: The composer strip and bar in the conversation

**Files:**
- Create: `crates/wai/src/chat/composer.rs` (shared `strip(cx) -> Div`, `strip_label(...)` and `bar_chip(...)`, moved out of `draft.rs`). Add it to `chat/mod.rs`.
- Modify: `crates/wai/src/chat/draft.rs` (use the shared helpers; no visual change).
- Modify: `crates/wai/src/chat/input.rs` (`render_prompt`: strip on the box, the box as a v_flex of textarea + bar).
- Modify: `crates/wai/src/chat/render.rs` (`render_header`: drop the pills and pickers; `pill`/`agent_model` go if unused).
- Modify: `crates/wai/src/chat/view.rs` tests (retarget the header-pill and picker tests).
- Modify: `docs/user-guide.md`. In the session-header section (~line 45), the header becomes title + context + close. Describe the strip and the bar under the prompt box.

- [ ] **Step 1:** Move the draft's strip and chip styling into `composer.rs`, and switch `draft.rs` to it (`cargo check`).
- [ ] **Step 2:** Implement the conversation strip and bar per Global Constraints. Keep the ids `pill-folder`, `pill-branch` and `pill-agent` on the strip chips. `pill-agent` now reads just the agent label; the model is in the bar.
- [ ] **Step 3:** Add the view test `composer_strip_sits_on_the_prompt_box` (fake agent, after a turn):
  - `(bounds("pill-folder").bottom() <= field.top() + px(0.5))`;
  - the strip's bottom touches the box's top: `|strip.bottom() - field.top()| < px(0.5)`;
  - `strip.left() - field.left()` is 18 ± 0.5 px;
  - `pill-folder.top() > transcript.bottom() - px(0.5)`, so the strip is not in the header.

  Give the strip id `"composer-strip"` and the box `"prompt-box"`, both via `.test_support()`. Add a narrow-pane variant like the draft's `chips_stay_inside_the_composer_in_a_narrow_pane`.
- [ ] **Step 4:** Retarget the tests that read the header pills or pickers (`view.rs`: the `pill(id, cx)` helper and the model/mode picker tests) to the new places. Their assertions on text and behaviour stay.
- [ ] **Step 5:** Guide.
- [ ] **Step 6:** Gate, once:
  - `cargo fmt --all --check`;
  - `cargo clippy --workspace --all-targets -- -D warnings`;
  - `cargo test --workspace` (in the background, logged to a file).

  Fix until green.
- [ ] **Step 7:** Screenshots, same offscreen path and scratch setup as last time (`WAI_SHOT`/`WAI_SHOT_DRAFT`, scratch git repo with a dirty file, approved command, no terminal split):
  - a finished conversation turn;
  - the draft.

  Both must show the strip on the box, with mode and model in the bar. Compare them with the mockup's draft composer, then fix and re-shoot until the clear differences are gone.
- [ ] **Step 8:** Commit `feat: put the session's chips on the prompt box`. Fixes go in `fix: …` commits.
