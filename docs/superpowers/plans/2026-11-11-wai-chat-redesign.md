# Chat redesign (mockup v2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The conversation looks like the chat state of the approved mockup `new-session-draft-v2.html`: each turn of the agent is one card headed "✦ <agent>" holding its steps and its text, steps are bare lines with the status mark on the left, and the status line has a turning ring.

**Architecture:** Rows stay one virtual-list row per `ChatItem`. A turn's card is drawn across consecutive rows as segments (first segment: top border, corners and header; last: bottom border and corners; middle: side borders only), so the list, its measuring and the processing modes stay as they are. Which segment a row is comes from one pure function over the items and a "shown" predicate.

**Tech Stack:** Rust, GPUI via `gpui-kit` 0.9.0 (`gpui_kit::component::*`, full Lucide catalog in `gpui_kit::assets::IconName`).

**Spec:** the chat state of `.superpowers/brainstorm/44632-1794399351/content/new-session-draft-v2.html` (git-ignored; open it, type a message, press ⏎ to see it). Owner's decision (2026-11-11): where it disagrees with the older `.superpowers/brainstorm/38118-1794303719/content/processamento.html`, v2 wins. From the older mockup only behaviours stay: a click on a step opens its output under it, and the Step by step / Grouped / Hidden modes.

## Global Constraints

- Values from the mockup's CSS (dark theme, Aurora tokens in `crates/wai/src/aurora.rs`):
  - Rows: 12 px apart (`.msgs gap:12px`).
  - Your message (`.me`): right-aligned, max width 70 %, fill `lilas` α 0.22 (flat, no gradient), border `lilas` α 0.5, radius 16 on all four corners, padding 10 px vertical / 14 px horizontal.
  - Turn card (`.agent`): fill white α 0.035, border `borda()`, radius 14, padding 10 px vertical / 14 px horizontal.
  - Card header (`.agent-h`): `✦ <agent name>`, all `brasa`, 12.5 px, semibold, 6 px under it.
  - Agent text inside the card (`.say`): `tinta`, 4 px above and below.
  - Step line (`.step`): no box, no border, no fill; gap 8 px; text `nevoa`, 13 px; 3 px above and below; status mark on the left, 14 px; done = `ok` `#46e39a`.
  - Code in a step title (`.step code`): mono where the text system allows it, 12.5 px, fill `vidro()`, radius 5, padding 1/5 px, `tinta_2`.
  - Status line (`.loading`): gap 10 px, 12.5 px, text `nevoa`; the turning mark is 12 px `boreal`; time and `Esc to stop` at its right end.
- Light theme: keep each element's current light-mode branch (`theme.border`, `theme.muted_foreground`, `theme.success`, …); no new light colors.
- The prompt box, the pinned stack (1.6/1.7), the header pills, permission/question cards, notices and the Edits block keep their current look. These rows stay outside any card, so a closed permission in mid-turn splits the turn into two cards, and only the first gets the header.
- CPU: no continuous animation. The turning mark moves on the turn clock's tick: every 125 ms while the agent works, every 1 s while a card waits for you, no timer otherwise (master plan: idle < 0.5 %, streaming already redraws every 100 ms).
- Copy buttons (1.2) stay: one per agent text row, at its right, and one per code block.
- UI copy in English. Commits: `feat:`/`fix:`/`test:`/`docs:` with no scope, no Co-Authored-By, no Jira reference.
- Pace (owner): per task only `cargo check --workspace --all-targets`, then commit. Run the tests once, after Task 3.
- Tests never read or write the real `~/.claude`, `~/.claude.json` or `~/.config/wai`.

## Review Focus

- Hidden mode with a turn that is only steps, then text: the text row must be the card's first segment and carry the header (the hidden rows draw nothing). Pinned by `card_edges_skip_hidden_rows` (Task 1).
- A row that stops being the card's last when a new row streams in: its bottom border and padding must go, with no stale height (no gap and no overlap). Pinned by `turn_is_one_card` (Task 1), which checks the segments touch after the turn ended.
- Grouped mode: the folded head is the card's only step segment. Opening it must keep the segments contiguous. Pinned by the Grouped part of `turn_is_one_card` (Task 1).
- A step title with an unclosed backtick or with only code. Pinned by `code_ranges_split_titles` (Task 2).
- A card waiting for a long time must not redraw 8 times a second. Pinned by `tick_period_follows_the_state` (Task 3).

---

### Task 1: The turn is one card

**Files:**
- Modify: `crates/wai/src/chat/model.rs` (`impl ChatItem`, near `is_step` ~line 82)
- Modify: `crates/wai/src/chat/render.rs` (`user_bubble` ~23, `starts_reply` ~93 and its test, `glass`, `render_item` ~307, `render_row` Agent arm ~383, `impl Render` row processor ~790)
- Modify: `crates/wai/src/chat/view.rs` (`sync_rows` ~647; new test next to `step_lines_are_compact_and_open_below` ~1883)
- Modify: `docs/user-guide.md` (The transcript, ~line 57)

**Interfaces:**
- Produces: `ChatItem::in_card(&self) -> bool` (Agent, Thought, Tool).
- Produces: `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub(crate) struct CardEdges { pub first: bool, pub last: bool, pub header: bool }` and `pub(crate) fn card_edges(items: &[ChatItem], ix: usize, shown: impl Fn(usize) -> bool) -> Option<CardEdges>` in `render.rs`.
- Produces: `ChatSession::shown(&self, ix: usize) -> bool`, the one place that says whether a row draws anything under the processing mode (Steps: all; Hidden: no step; Grouped: a step only when it heads its run or its group is open). `render_item` returns `None` exactly when `!shown(ix)`.
- Produces: each card segment carries the id `("card", ix)` and the header `("reply-head", ix)`, both via `.test_support()`.

- [ ] **Step 1: Unit test `card_edges`** in `render.rs` tests, replacing `reply_header_once_per_turn` (U = User, S = Steered, A = Agent, T = Tool not an edit, N = Notice):

```rust
#[test]
fn card_edges_frame_each_run() {
    let all = |_| true;
    let e = |first, last, header| Some(CardEdges { first, last, header });
    let items = [u(), t(), t(), a()];
    assert_eq!(card_edges(&items, 0, all), None);
    assert_eq!(card_edges(&items, 1, all), e(true, false, true));
    assert_eq!(card_edges(&items, 2, all), e(false, false, false));
    assert_eq!(card_edges(&items, 3, all), e(false, true, false));
    let items = [u(), a(), n(), a()];
    assert_eq!(card_edges(&items, 1, all), e(true, true, true));
    assert_eq!(card_edges(&items, 3, all), e(true, true, false), "one header per turn");
    assert_eq!(card_edges(&[u(), a(), u(), a()], 3, all), e(true, true, true));
    assert_eq!(card_edges(&[u(), a(), s(), a()], 3, all), e(true, true, true));
}

#[test]
fn card_edges_skip_hidden_rows() {
    let shown = |items: &[ChatItem]| {
        let steps: Vec<bool> = items.iter().map(|i| !matches!(i, ChatItem::Tool(_))).collect();
        move |j: usize| steps[j]
    };
    let items = [u(), t(), t(), a()];
    assert_eq!(card_edges(&items, 1, shown(&items)), None);
    assert_eq!(card_edges(&items, 3, shown(&items)), Some(CardEdges { first: true, last: true, header: true }));
    let items = [u(), a(), t()];
    assert_eq!(card_edges(&items, 1, shown(&items)), Some(CardEdges { first: true, last: true, header: true }));
    let items = [u(), t(), n(), a()];
    assert_eq!(card_edges(&items, 3, shown(&items)).map(|e| e.header), Some(true), "an invisible card gives no header");
}
```

(`u`, `s`, `a`, `t`, `n` are small constructors in the test module; `t()` builds the `ToolItem` the old test built, status `Completed`, no diffs.)

- [ ] **Step 2: Implement `ChatItem::in_card` and `card_edges`.** With `run = items run of in_card around ix`: `None` unless `items[ix].in_card() && shown(ix)`; `first` = no shown row in `run.start..ix`; `last` = no shown row in `ix + 1..run.end`; `header` = `first` and no shown `in_card` row between the last `User`/`Steered` before `ix` and `run.start`. Delete `starts_reply`.

- [ ] **Step 3: Draw the segments.** Add `ChatSession::shown` and make `render_item` use it. In the `impl Render` row processor, a row with `Some(edges)` is wrapped in `fn card_segment(edges: CardEdges, cx: &App) -> Div` (render.rs): side borders always; top border, `rounded_t(px(14.))` and 10 px top padding when `first`; bottom border, `rounded_b(px(14.))` and 10 px bottom padding when `last`; 14 px horizontal padding; the header child when `header`. The row gets 12 px under it (outside the segment) when it is a card's last segment or not in a card, and is not the list's last row; none inside a card. Dark: fill white α 0.035 and `aurora::borda()`; light: `theme.border`, no fill. The Agent arm drops its own glass card and becomes `h_flex [text (tinta, 4 px above/below, flex_1, min_w_0), copy]`. `user_bubble` takes the `.me` values from Global Constraints (`pinned_message` keeps using it).

- [ ] **Step 4: Remeasure a card's rows when its edges can change.** In `sync_rows`: after an append, when the row before the appended ones (`known - 1`) is `in_card`, remeasure `model.run(known - 1, ChatItem::in_card)` clipped to `..known`; for `changed`, remeasure its run when it is `in_card`. (Existing paths already remeasure on mode change and group toggles.)

- [ ] **Step 5: View test `turn_is_one_card`** (`view.rs`, fake agent `--steps 2`: rows 0 = your prompt, 1 = Thought, 2 and 3 = Read calls, last = the reply). After `turn(&session, cx)`:
  - `shows(("reply-head", 1usize))` and `!shows(("reply-head", last))`;
  - for each `k` in `1..last`, `(bounds(("card", k + 1)).top() - bounds(("card", k)).bottom()).abs() < px(0.5)` (the segments touch);
  - with `processing_settings("--steps 2", Processing::Hidden)`: `shows(("reply-head", last))`, `!shows(("card", 2usize))`;
  - with `Processing::Grouped`: `shows(("reply-head", 1usize))`; after `click_edits(None, ("group", 1usize), cx)` the segments `1..=last` still touch.

- [ ] **Step 6: Guide.** In `docs/user-guide.md` → The transcript, add one bullet: each turn of the agent is one card headed `✦ <agent>`, holding its steps and its replies in order; your prompts are violet balloons on the right; a closed permission card or a notice between them splits the card.

- [ ] **Step 7: `cargo check --workspace --all-targets`, then commit** `feat: draw each agent turn as one card`.

### Task 2: Steps are bare lines inside the card

**Files:**
- Modify: `crates/wai/src/aurora.rs` (add `ok()`)
- Modify: `crates/wai/src/chat/render.rs` (`status_icon` ~110, `icon_color`, `chevron`, `kind_icon`, `step_line` ~160, `render_group`, Thought arm, `render_tool`)
- Modify: `crates/wai/src/chat/view.rs` (`step_lines_are_compact_and_open_below` ~1883)
- Modify: `docs/user-guide.md` (Processing table, ~line 62)

**Interfaces:**
- Consumes: `card_segment` from Task 1 (the step lines are its children).
- Produces: `pub(crate) fn ok() -> Hsla` (`#46e39a`) in `aurora.rs`; `pub(crate) fn code_ranges(title: &str) -> (String, Vec<std::ops::Range<usize>>)` in `render.rs`.

- [ ] **Step 1: Unit test `code_ranges_split_titles`:**

```rust
assert_eq!(code_ranges("Read README.md"), ("Read README.md".into(), vec![]));
assert_eq!(code_ranges("`git status`"), ("git status".into(), vec![0..10]));
assert_eq!(code_ranges("Run `cargo test` now"), ("Run cargo test now".into(), vec![4..14]));
assert_eq!(code_ranges("a `b"), ("a `b".into(), vec![]), "an unclosed backtick stays text");
assert_eq!(code_ranges(""), (String::new(), vec![]));
```

- [ ] **Step 2: Implement `code_ranges`** (byte ranges into the returned text, backticks removed).

- [ ] **Step 3: Restyle the step line** to the `.step` values: `step_line` has no border, fill or radius, and turns `tinta` on hover. Children are `status_icon` (left, 14 px; Completed `aurora::ok()`, Failed `aurora::erro()`, InProgress `Loader` in `nevoa`, Pending `Ellipsis` in `nevoa_2`; light theme as today), then the title as one `StyledText` whose `code_ranges` get the code highlight (fill `vidro()`, `tinta_2`), single line, truncated. Remove the chevron and the kind icon (`chevron`, `kind_icon`, `icon_color` go if nothing else uses them). The Thought line becomes `Asterisk` (nevoa_2) + `Thought`; the grouped head becomes the run's status mark + `run_label`. Output boxes, Show all, Open full output and diffs keep opening under the line, unchanged.

- [ ] **Step 4: Update `step_lines_are_compact_and_open_below`:** consecutive step lines now touch: `(next.top() - line.bottom()).abs() < px(0.5)`. Keep the `tool`/`step` top check and the output-under-line check.

- [ ] **Step 5: Guide.** Processing table, Step by step row: "Each step is one line in the reply's card: ✓ or ✗ (or a mark while it runs), then its title, commands in code style. A click opens …" (keep the rest of the row).

- [ ] **Step 6: `cargo check --workspace --all-targets`, then commit** `feat: draw steps as bare lines in the turn card`.

### Task 3: The status line turns a ring

**Files:**
- Modify: `crates/wai/src/chat/input.rs` (status line ~510, `mark` ~578 and its test)
- Modify: `crates/wai/src/chat/view.rs` (`sync_clock` ~630, the `turn` field)
- Modify: `docs/user-guide.md` (The status line, line 44)

**Interfaces:**
- Produces: `pub(super) fn tick_period(state: &TurnState) -> Option<Duration>` (view.rs) and `fn spin_angle(elapsed: Duration) -> Radians` (input.rs), replacing `mark`.

- [ ] **Step 1: Unit tests** replacing `status_mark_turns_once_a_second`:

```rust
#[test]
fn tick_period_follows_the_state() {
    assert_eq!(tick_period(&TurnState::Working), Some(Duration::from_millis(125)));
    assert_eq!(tick_period(&TurnState::NeedsYou), Some(Duration::from_secs(1)));
    assert_eq!(tick_period(&TurnState::Idle), None);
    assert_eq!(tick_period(&TurnState::Error(String::new())), None);
}

#[test]
fn ring_turns_an_eighth_per_tick() {
    let deg = |ms| spin_angle(Duration::from_millis(ms)).0.to_degrees().round();
    assert_eq!([0, 125, 250, 1000, 1125].map(deg), [0., 45., 90., 0., 45.]);
}
```

- [ ] **Step 2: Implement.** `sync_clock` keeps the turn's start `Instant` and restarts its timer only when `tick_period` changes (Working ↔ NeedsYou); the timer just notifies. The status line: mark = `Icon::new(gpui_kit::assets::IconName::LoaderCircle)` 12 px, `boreal`, rotated `spin_angle(elapsed)` while Working; while a card waits it stands still and is `ambar`, and so is the text. Text `nevoa` while working (light: `muted_foreground`), 12.5 px, gap 10 px; time and `Esc to stop` stay at the right end in whole seconds.

- [ ] **Step 3: Guide**, line 44: replace "It is green while the agent works … turns once a second" with: the text is grey while the agent works and amber while a card waits for you; a green ring turns while the agent works and stands still while it waits.

- [ ] **Step 4: `cargo check --workspace --all-targets`, then commit** `feat: turn a ring in the status line`.

### Task 4: Gate and look

- [ ] **Step 1:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (run in the background, log to a file). Fix every failure, then commit `fix: …` or `test: …`.
- [ ] **Step 2:** Screenshots of the finished branch with the offscreen path in `crates/wai/Cargo.toml:66` (`--features screenshot`, fake agent `--steps 3`, Step by step and Grouped), with a scratch `HOME` whose `.config/wai/state.json` has `"approved": ["<exact fake-agent command>"]`. Never run against the real `HOME`. Kill only a PID you started. Compare each shot with the v2 mockup's chat state and list what still differs.
