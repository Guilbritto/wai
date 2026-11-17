# The pet speaks — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The bichinho speaks short, in-character lines in an animated speech bubble above it, written ahead of time by Claude from its stats and its diary.

**Architecture:**
- wai gains a generic `bubble` panel node (API 1.8): it parses it, runs its timeline (pop, typewriter, hold, leave), pauses it on hover and while the prompt box has text, and reports the end with `panel/action`.
- The Go extension keeps a speech queue with priorities. It puts the current line in a `bubble` on top of its prompt tree, and refills per-event line pools in the background through the `claude` CLI.

**Tech Stack:**
- Rust: `wai-ext` parser and `wai` with gpui-fast 0.1.13 / gpui-kit 0.9.0; tests are `#[gpui_kit::test]` with a fake clock.
- Go 1.x: the stdlib only, in `extensions/wai-bichinho`.

**Spec:** `docs/superpowers/specs/2026-11-17-pet-speech-design.md`

## Global Constraints

- API: the `bubble` node is API 1.8.
  - Before Task 1, run `git tag --contains c7392b2 --list 'v*'`. If it prints nothing, 1.8 is unreleased and the bubble joins it.
  - If it prints a tag, bump `API` in `crates/wai-ext/src/manifest.rs` to `"1.9"`, and write 1.9 everywhere this plan says 1.8.
- Commits:
  - wai code is `feat:`/`fix:`; extension code and `extensions/index.toml` are `chore:`.
  - No scope and no `Co-Authored-By`.
  - Branch `feat/pet-speech` from `development`.
- Bubble timing:
  - Pop 200 ms, one character per 30 ms, leave 150 ms.
  - Hold is 2.5 s + 50 ms per character, at most 7 s; a sticky bubble shrinks after 6 s of hold.
  - Dim opacity is 0.25; the ticker runs every 30 ms; the text is at most 140 characters.
- Speech:
  - The gap between lines is 250 ms, and the queue holds at most 3 lines.
  - Chatter comes at most once every 15 min.
  - A non-sticky line is dropped as a safety net after 30 s.
  - The mouth talks for 30 ms × characters + 200 ms.
- Agent:
  - Model `claude-opus-5-5`, effort `low`, timeout 90 s, `--max-budget-usd 0.10`.
  - Pools refresh after 6 h, with a 30 min backoff after a failure.
  - The model writes 6 lines per kind; lines over 80 characters are dropped.
  - The diary keeps the last 30 notes, and the prompt sends the last 15.
- Lines are Brazilian Portuguese. Claude never receives session titles, prompts, file names or PR URLs.
- Tests:
  - Rust: `cargo test -p wai-ext` and `cargo test -p wai`, with `export PATH="$HOME/.cargo/bin:$PATH"` first.
  - Go: `cd extensions/wai-bichinho && go test ./...`.
- Formatting: `cargo fmt --all` and `gofmt -l .` must be clean before each commit.

## Review Focus

1. **The slot is hidden** (narrow pane, or a non-session pane focused) while a line is up. The extension's 30 s safety net drops it, and the queue moves on. Test: `TestStuckLineIsDropped` (Task 5).
2. **Another line arrives while one is leaving.** A new text for the same id must pop fresh, not inherit the old one's leave or a `Done` state. Test: `new_text_restarts_the_bubble` (Task 4).
3. **The user types while a sticky "!" sits there.** No ticker runs then, yet the dim must still show, through the global observer. Test: `typing_dims_a_collapsed_bubble` (Task 4).
4. **`claude` prints something other than the expected JSON** (a login prompt, an error object, a budget stop). There must be no crash and no empty pools: the fixed lines are kept and the 30 min backoff starts. Test: `TestBadClaudeOutputKeepsFallback` (Task 7).
5. **An old `pet.json` without the new fields, or a newer one with unknown kinds.** It must load, and speaking falls back per kind. Test: `TestOldStateLoads` (Task 7).

---

### Task 1: Parse the `bubble` node

**Files:**
- Modify: `crates/wai-ext/src/panel.rs`. Change the `Node` enum (~line 57), add a `"bubble"` arm in `Parser::leaf` (~line 704), add `Node::Bubble` to the id-taking match (~line 851), and add tests in `mod tests`.

**Interfaces:**
- Produces:
  - `pub const BUBBLE_CHARS: usize = 140;`
  - `Node::Bubble { id: String, text: Arc<str>, tone: Tone, sticky: bool }`
  - The bubble takes its id into the tree's id set, like `Button`.

- [ ] **Step 1: Write the failing tests** in `panel.rs` `mod tests`:

```rust
#[test]
fn bubble_parses() {
    let tree = parse(&json!({"type": "bubble", "id": "say", "text": "Oi", "tone": "success", "sticky": true}));
    assert_eq!(tree, Ok(Node::Bubble { id: "say".into(), text: "Oi".into(), tone: Tone::Success, sticky: true }));
    let plain = parse(&json!({"type": "bubble", "id": "say", "text": "Oi"}));
    assert_eq!(plain, Ok(Node::Bubble { id: "say".into(), text: "Oi".into(), tone: Tone::Neutral, sticky: false }));
}

#[test]
fn bubble_needs_id_and_text_and_is_cut() {
    assert_eq!(parse(&json!({"type": "bubble", "text": "Oi"})), Ok(Node::Unsupported));
    assert_eq!(parse(&json!({"type": "bubble", "id": "say"})), Ok(Node::Unsupported));
    let Ok(Node::Bubble { text, .. }) = parse(&json!({"type": "bubble", "id": "b", "text": "x".repeat(300)})) else { panic!() };
    assert_eq!(text.chars().count(), BUBBLE_CHARS);
}

#[test]
fn bubble_id_joins_the_id_set() {
    let tree = parse(&json!({"type": "column", "children": [
        {"type": "button", "id": "x", "label": "B"},
        {"type": "bubble", "id": "x", "text": "Oi"},
    ]}));
    let Ok(Node::Column { children, .. }) = tree else { panic!() };
    assert_eq!(children[1], Node::Unsupported);
}
```

Use whatever `parse` returns today: if it is `Result<Node, TooLarge>`, compare as above; adapt the `Ok(...)` wrapping if not. A bubble in a hover `card` is not a display node, so `card()` makes it `Unsupported`; no change is needed there.

- [ ] **Step 2: Run them to make sure they fail.**
  Run `cargo test -p wai-ext bubble`. Expected: FAIL to compile (`no variant Bubble`).

- [ ] **Step 3: Implement.**
  - Add the variant, with a doc line: `/// A speech bubble (API 1.8): wai animates it and sends `panel/action` with its id when it ends.`
  - In `leaf`, add the `"bubble"` arm. It reads `id: id(o)?`, and `text: line(o, "text")` cut with `cut(&t, BUBBLE_CHARS)`. `cut` already ends in `…`, the way `ITEM_TEXT_CHARS` is handled. The arm also reads `tone(o).unwrap_or_default()` and `flag(o, "sticky").unwrap_or(false)`.
  - Add `Node::Bubble { id, .. }` to the id-taking match.
  - Update the module doc's id sentence: "Buttons, form fields, sections, tabs and bubbles share one set of ids".

- [ ] **Step 4: Run the tests.**
  Run `cargo test -p wai-ext`. Expected: PASS. The exhaustive matches in `crates/wai` will not compile until Task 4, so check only this crate now.

- [ ] **Step 5: Commit.**
  To keep `cargo build` green at each commit, add a `Node::Bubble { .. } => Node::Unsupported`-style placeholder arm in `crates/wai/src/ext_panel.rs` where the compiler asks. Task 4 replaces it. Run `cargo build -p wai`, then:

```bash
git add crates/wai-ext/src/panel.rs crates/wai/src/ext_panel.rs
git commit -m "feat: parse a speech bubble panel node"
```

---

### Task 2: The bubble's timeline

A pure, clock-driven state machine with no GPUI types, so its timing is unit-tested without a window.

**Files:**
- Create: `crates/wai/src/ext_bubble.rs`, with its tests in the same file.
- Modify: `crates/wai/src/main.rs` (or `lib.rs`, wherever the modules are declared): add `mod ext_bubble;`.

**Interfaces:**
- Produces, all `pub(crate)`:

```rust
pub(crate) const POP: Duration;          // 200 ms
pub(crate) const CHAR: Duration;         // 30 ms
pub(crate) const LEAVE: Duration;        // 150 ms
pub(crate) const STICKY_AFTER: Duration; // 6 s
pub(crate) const TICK: Duration;         // 30 ms
pub(crate) fn hold(chars: usize) -> Duration; // 2.5 s + 50 ms × chars, at most 7 s

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ended { Click, Timeout }

/// What to draw at an instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Look {
    pub pop: f32,       // 0..=1, eased with ease-out-back (may pass 1 briefly)
    pub chars: usize,   // characters typed so far
    pub left: f32,      // timer line, 1 → 0; 0 for a sticky bubble
    pub collapsed: bool,// sticky, shrunk to "!"
    pub leave: f32,     // 0..=1 once leaving
}

pub(crate) struct Bubble { /* text, sticky, shown, last, held, leaving: Option<(Instant, Ended)>, ended: bool, pub hovered: bool */ }

impl Bubble {
    pub(crate) fn new(text: Arc<str>, sticky: bool, now: Instant) -> Self;
    pub(crate) fn text(&self) -> &Arc<str>;
    /// Moves to `now`. Hold time grows only while not `paused` and fully typed.
    /// Returns how it ended, once, when the leave finishes.
    pub(crate) fn advance(&mut self, now: Instant, paused: bool) -> Option<Ended>;
    /// Starts the leave; false when already leaving or ended.
    pub(crate) fn click(&mut self, now: Instant) -> bool;
    pub(crate) fn look(&self, now: Instant) -> Look;
    /// Something changes over time: popping, typing, holding, or leaving. False once ended or collapsed.
    pub(crate) fn animating(&self, now: Instant) -> bool;
    pub(crate) fn ended(&self) -> bool;
}
```

- [ ] **Step 1: Write the failing tests**, in `#[cfg(test)] mod tests`. Each builds `let t0 = Instant::now();` and passes `t0 + ms(n)`.

```rust
#[test]
fn hold_grows_with_length_and_caps() {
    assert_eq!(hold(0), ms(2_500));
    assert_eq!(hold(20), ms(3_500));
    assert_eq!(hold(140), ms(7_000));
}

#[test]
fn types_one_char_per_30ms() {
    let b = Bubble::new("abcdef".into(), false, t0);
    assert_eq!(b.look(t0).chars, 0);
    assert_eq!(b.look(t0 + ms(95)).chars, 3);
    assert_eq!(b.look(t0 + ms(1_000)).chars, 6);
}

#[test]
fn times_out_after_typing_plus_hold_plus_leave() {
    // "abc": typed at 90 ms, hold 2_650 ms, leave 150 ms → ends at 2_890 ms.
    let mut b = Bubble::new("abc".into(), false, t0);
    let mut ended = None;
    for t in (0..=2_880).step_by(30) { ended = ended.or(b.advance(t0 + ms(t), false)); }
    assert_eq!(ended, None);
    assert_eq!(b.advance(t0 + ms(2_900), false), Some(Ended::Timeout));
    assert!(b.ended());
    assert_eq!(b.advance(t0 + ms(3_000), false), None); // once only
}

#[test]
fn paused_time_does_not_count() {
    let mut b = Bubble::new("abc".into(), false, t0);
    for t in (0..=10_000).step_by(30) { assert_eq!(b.advance(t0 + ms(t), true), None); }
    assert!(b.look(t0 + ms(10_000)).left > 0.99);
}

#[test]
fn click_leaves_then_reports_click() {
    let mut b = Bubble::new("abc".into(), false, t0);
    assert!(b.click(t0 + ms(500)));
    assert!(!b.click(t0 + ms(510)));
    assert_eq!(b.advance(t0 + ms(600), false), None);
    assert_eq!(b.advance(t0 + ms(660), false), Some(Ended::Click));
}

#[test]
fn sticky_never_times_out_and_collapses() {
    let mut b = Bubble::new("Ei!".into(), true, t0);
    for t in (0..=60_000).step_by(30) { assert_eq!(b.advance(t0 + ms(t), false), None); }
    let look = b.look(t0 + ms(60_000));
    assert!(look.collapsed);
    assert_eq!(look.left, 0.);
    assert!(!b.animating(t0 + ms(60_000)));
}

#[test]
fn pop_overshoots_then_settles() {
    let b = Bubble::new("abc".into(), false, t0);
    assert_eq!(b.look(t0).pop, 0.);
    assert!(b.look(t0 + ms(140)).pop > 1.);
    assert_eq!(b.look(t0 + ms(200)).pop, 1.);
}
```

- [ ] **Step 2: Run them to make sure they fail.**
  Run `cargo test -p wai ext_bubble`. Expected: FAIL to compile.

- [ ] **Step 3: Implement.**
  - **Pop.** Ease-out-back with `c1 = 1.70158`: `1 + (c1 + 1)(t − 1)³ + c1(t − 1)²`, where `t = elapsed / POP`, clamped to `0..=1`; it is exactly 1 at `t = 1`.
  - **Hold time.** `held` accumulates `now − last` on each `advance` while `!paused` and the text is fully typed (typing ends at `shown + CHAR × chars`).
  - **Timeout.** A non-sticky bubble whose `held ≥ hold(chars)` starts leaving with `Ended::Timeout`.
  - **Collapse.** A sticky bubble is `collapsed` once `held ≥ STICKY_AFTER`.
  - **Leave.** `leave = (now − leaving_at) / LEAVE`. `advance` returns the `Ended` the first time it reaches 1.

- [ ] **Step 4: Run the tests.**
  Run `cargo test -p wai ext_bubble`. Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add crates/wai/src/ext_bubble.rs crates/wai/src/main.rs
git commit -m "feat: add the speech bubble timeline"
```

---

### Task 3: `PromptTyping`, whether the slot owner's prompt box has text

**Files:**
- Modify: `crates/wai/src/chat/composer.rs`. Add the global and its setter next to `PromptSlots` (line 26).
- Modify: `crates/wai/src/chat/mod.rs`. Re-export it beside `PromptSlots` (line 13).
- Modify: `crates/wai/src/chat/view.rs`. In the `InputEvent::Change` arm (~line 244), and add `ChatSession::prompt_typed`.
- Modify: `crates/wai/src/chat/draft.rs`. In the `InputEvent::Change` arm (~line 295; the closure needs `this` and `input`), and add `DraftView::prompt_typed`.
- Modify: `crates/wai/src/workspace.rs`, `sync_prompt_slots` (~line 2238).

**Interfaces:**
- Produces:

```rust
/// Whether the prompt box holding the slots has text: bubbles (API 1.8) dim and wait meanwhile.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PromptTyping(pub bool);
impl Global for PromptTyping {}

/// Sets `PromptTyping` to `typing` when `owner` holds the prompt slots and the value changes.
pub(crate) fn set_typing(owner: EntityId, typing: bool, cx: &mut App);

impl ChatSession { pub(crate) fn prompt_typed(&self, cx: &App) -> bool } // !input.value().is_empty()
impl DraftView   { pub(crate) fn prompt_typed(&self, cx: &App) -> bool }
```

- Behaviour:
  - `view.rs` and `draft.rs` call `set_typing(cx.entity_id(), self.prompt_typed(cx), cx)` on `InputEvent::Change`.
  - `sync_prompt_slots`, when `owner` changes, computes the new owner's `prompt_typed` (false for no owner), and sets the global only if it differs.

- [ ] **Step 1: Write the failing test** in `composer.rs` (add a `#[cfg(test)] mod tests` if there is none):

```rust
#[gpui_kit::test]
fn only_the_slot_owner_sets_typing(cx: &mut TestAppContext) {
    let (a, b) = cx.update(|cx| (cx.new(|_| ()).entity_id(), cx.new(|_| ()).entity_id()));
    cx.update(|cx| {
        cx.set_global(PromptSlots { owner: Some(a), inside: None, side: None });
        set_typing(b, true, cx);
        assert_eq!(cx.try_global::<PromptTyping>().copied().unwrap_or_default(), PromptTyping(false));
        set_typing(a, true, cx);
        assert_eq!(*cx.global::<PromptTyping>(), PromptTyping(true));
        set_typing(a, false, cx);
        assert_eq!(*cx.global::<PromptTyping>(), PromptTyping(false));
    });
}
```

- [ ] **Step 2: Run it to make sure it fails.**
  Run `cargo test -p wai only_the_slot_owner_sets_typing`. Expected: FAIL to compile.

- [ ] **Step 3: Implement** `set_typing`, the two `prompt_typed` methods, the two `Change` calls, and the `sync_prompt_slots` recompute.
  - In `sync_prompt_slots`, the owner's pane is already matched there (`Pane::Session(session)` and `Pane::Draft(draft)`); read `prompt_typed` in the same match.
  - `set_typing` sets the global only when the value differs, so typing a second character does not notify observers again.

- [ ] **Step 4: Run the tests.**
  Run `cargo test -p wai only_the_slot_owner_sets_typing`, then `cargo test -p wai chat::`. Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add crates/wai/src/chat/composer.rs crates/wai/src/chat/mod.rs crates/wai/src/chat/view.rs crates/wai/src/chat/draft.rs crates/wai/src/workspace.rs
git commit -m "feat: tell panels when the prompt box has text"
```

---

### Task 4: Draw the bubble in extension panels, and document it

**Files:**
- Modify: `crates/wai/src/ext_panel.rs`. Change the `ExtPanel` fields (line 113), `new` (line 136), and `set_tree` (line 148). Replace the Task 1 placeholder arm in `node()` with the real rendering. Add tests in `mod tests` (line 1720).
- Modify: `docs/extensions.md`:
  - a `bubble` row in the **Content** table;
  - `bubble` in the line 48 list of 1.8 fields;
  - a bubble note in the `panel/action` row (line 160);
  - "and bubbles" in the **Ids** rule.

**Interfaces:**
- Consumes:
  - from Task 1, `Node::Bubble`;
  - from Task 2, `Bubble`, `Ended`, `Look` and `TICK`;
  - from Task 3, `PromptTyping`.
- Produces:
  - `PanelAction { action: id, values: json!({"bubble": "click" | "timeout"}) }` when a bubble's leave ends;
  - the debug selector `name("panel-bubble", id)`, made with the existing `fn name(kind, id)` (line 1684).

**New `ExtPanel` state:**
- `bubbles: HashMap<String, ext_bubble::Bubble>`;
- `ticker: Option<Task<()>>`;
- `typing: Option<Subscription>`.

**`set_tree`:**
- For each `Node::Bubble` in the tree, keep the entry when the id exists with the same text. Otherwise insert `Bubble::new(text, sticky, cx.background_executor().now())`.
- Drop the ids that are gone.
- If there is any bubble and `typing` is `None`, store `cx.observe_global::<PromptTyping>(|this, cx| { this.ensure_ticker(cx); cx.notify() })`.
- Call `ensure_ticker`.

**`fn ensure_ticker(&mut self, cx: &mut Context<Self>)`:**
- If no task is running and some bubble is `animating`, spawn a loop. Each iteration it waits `cx.background_executor().timer(TICK)`, then updates the panel:
  - `now = background_executor().now()`;
  - `paused = b.hovered || PromptTyping`;
  - `advance` every bubble;
  - emit a `PanelAction` for each `Some(ended)`;
  - `cx.notify()`.
- The loop stops (`ticker = None`) when nothing animates. Follow the existing `cx.spawn` pattern in `ext_host.rs:453`.

**Rendering** (`Node::Bubble` in `node()`):
- Nothing when `ended`.
- Otherwise a `v_flex` with `.id(name("panel-bubble", id))` and `.debug_selector(...)` like the other nodes:
  - Opacity `look.pop.min(1.) × (1 − look.leave)`, times 0.25 while `PromptTyping`.
  - `.relative().top(px(6. × (1. − look.pop) − 4. × look.leave))`.
  - The theme's popover background, a 1 px border in the tone's colour (the theme border for `Neutral`, as `Badge` picks its tone colours), the `campo::RADIUS` corners and `text_xs()`.
  - The first `look.chars` characters of the text. When `look.collapsed && !hovered`, the text is just `"!"`, wrapped in `with_animation(..., Animation::new(900 ms).repeat().with_easing(bounce(ease_in_out)), |el, d| el.top(px(-3. × d)))`.
  - A 2 px timer line, `look.left` of the width, in the tone's colour, when not sticky.
  - Under the box, a tail 10 × 6 px from the left edge at 18 px: a `canvas` that fills a downward triangle with `PathBuilder::fill()` in the border colour.
- `.on_hover(cx.listener(...))` sets `hovered`, notifies, and calls `ensure_ticker`.
- `.on_click(cx.listener(...))` calls `bubble.click(now)` and `ensure_ticker`.

- [ ] **Step 1: Write the failing tests** in `ext_panel.rs` `mod tests`, reusing `panel_window`, `show`, `actions`, `frame` and `shown`.
  - Add `{"type": "bubble", "id": "say", "text": "Oi"}` to the tree in `every_node_renders`; that test already asserts that nothing is unsupported.
  - Add the new tests:

```rust
#[gpui_kit::test]
fn bubble_times_out_and_reports_it(cx: &mut TestAppContext) {
    let (panel, cx) = panel_window(cx);
    let sent = actions(&panel, cx);
    show(&panel, json!({"type": "bubble", "id": "say", "text": "Oi"}), cx);
    assert!(shown(name("panel-bubble:say"), cx));
    cx.executor().advance_clock(Duration::from_secs(4));
    cx.run_until_parked();
    assert_eq!(*sent.borrow(), vec![("say".to_string(), json!({"bubble": "timeout"}))]);
    frame(cx);
    assert!(!shown(name("panel-bubble:say"), cx));
}

#[gpui_kit::test]
fn bubble_click_reports_click(cx: &mut TestAppContext) {
    // show, then click: simulate a click on the bubble's bounds the way the existing card-click test does; advance 200 ms
    // → sent == [("say", {"bubble": "click"})]
}

#[gpui_kit::test]
fn bubble_waits_while_typing(cx: &mut TestAppContext) {
    // set_global(PromptTyping(true)); show; advance 20 s → no action;
    // set_global(PromptTyping(false)); advance 4 s → one timeout action
}

#[gpui_kit::test]
fn new_text_restarts_the_bubble(cx: &mut TestAppContext) {
    // show "Oi"; advance 2.9 s (leaving); show "Tchau" with the same id; advance 1 s → no action yet;
    // advance 3 s → exactly one ("say", timeout)
}

#[gpui_kit::test]
fn typing_dims_a_collapsed_bubble(cx: &mut TestAppContext) {
    // show sticky; advance 7 s (collapsed, ticker stopped); set_global(PromptTyping(true)); run_until_parked; frame
    // → the panel re-rendered: check through a render counter, or read the bubble's opacity via
    //   panel.read(cx).bubbles["say"] together with PromptTyping. Assert the observer is installed
    //   (panel.read(cx).typing.is_some()) and no action was sent.
}
```

  The selector string must match how `name()` joins kind and id (`"panel-bubble:say"`, like `"panel-button:b"`). Copy the click simulation from the existing card or button click test in this module.

- [ ] **Step 2: Run them to make sure they fail.**
  Run `cargo test -p wai ext_panel::tests::bubble`. Expected: FAIL; the placeholder renders `Unsupported`.

- [ ] **Step 3: Implement** the state, `ensure_ticker`, the rendering and the docs, as above. The docs row:

```markdown
| `bubble` | `id`, `text`, `tone`?, `sticky`? (bool) | API 1.8. A speech bubble with a tail at its bottom left. wai animates it: it pops in, types its text (one character per 30 ms), holds 2.5 s + 50 ms per character (at most 7 s) with a shrinking timer line, and fades out. The hold waits while the mouse is on it and while the prompt box has text (it dims then). A click closes it. When it is gone, wai sends `panel/action` with its `id` and `values: {"bubble": "click"}` or `{"bubble": "timeout"}`. `sticky`: it never times out; after 6 s it shrinks to a bouncing "!" (hover shows the text). A different `text` for the same `id` starts a new bubble; removing the node removes it at once. `text` is one line, at most 140 characters |
```

- [ ] **Step 4: Run the tests.**
  Run `cargo test -p wai ext_panel` and `cargo test -p wai-ext`. Expected: PASS. Then run `cargo clippy -p wai -- -D warnings`: clean.

- [ ] **Step 5: Look at it.**
  Run `WAI_SHOT_TREE` (added in `856d086`) on a tree with a bubble above a mono text. Check the tail, the border and the wrapping at 160 px. Save the screenshot under `.playwright-mcp/`.

- [ ] **Step 6: Commit.**

```bash
git add crates/wai/src/ext_panel.rs docs/extensions.md
git commit -m "feat: draw animated speech bubbles in extension panels"
```

---

### Task 5: The speech queue (extension)

**Files:**
- Create: `extensions/wai-bichinho/speech.go`, with `speech_test.go`.

**Interfaces:**
- Produces:

```go
type line struct {
	Kind, Text string
	Prio       int  // 3 urgent, 2 news, 1 small talk
	Sticky     bool
	Chatter    bool // limited to one per chatterGap
}

const (
	lineGap    = 250 * time.Millisecond
	maxQueue   = 3
	chatterGap = 15 * time.Minute
	stuckAfter = 30 * time.Second
	charTime   = 30 * time.Millisecond
)

type speech struct {
	cur     *line
	curAt   time.Time
	queue   []line
	nextAt  time.Time // no line before this
	chatted time.Time // last chatter said
}

func (sp *speech) say(l line, now time.Time, asleep bool) // enqueue under the rules
func (sp *speech) next(now time.Time)                     // promotes the head of the queue when free and past nextAt
func (sp *speech) done(now time.Time)                     // the bubble ended: cur = nil, nextAt = now + lineGap
func (sp *speech) unfocus()                               // drops a non-sticky cur
func (sp *speech) drop(kind string)                       // drops cur and queued lines of a kind (needs_you answered)
func (sp *speech) expire(now time.Time)                   // drops a non-sticky cur older than stuckAfter
func (sp *speech) talking(now time.Time) bool             // cur typing: now < curAt + charTime×chars + 200 ms
```

**Rules inside `say`:**
- Drop the line when its text equals `cur`'s or a queued one.
- Drop chatter when `asleep`, or when `now − chatted < chatterGap`. Set `chatted` when chatter is accepted.
- At priority 3 with a non-sticky `cur` of a lower priority, replace `cur` at once (`curAt = now`).
- Otherwise insert the line after the last queued line of the same or higher priority.
- When the queue goes over `maxQueue`, remove the lowest-priority, oldest line.

- [ ] **Step 1: Write the failing tests** in `speech_test.go`. Use `t0 := time.Date(2026, 11, 17, 12, 0, 0, 0, time.UTC)` and helpers `n(kind, prio)` and `chat(kind)`:
  - `TestOneLineAtATimeWithGap`: say A and B; `next` gives `cur` A. After `done(t)`, `next(t+100ms)` leaves `cur` nil, and `next(t+250ms)` gives B.
  - `TestUrgentJumpsAndReplaces`: with `cur` a prio-1 line, `say(needs_you prio 3)` makes `cur` the needs_you line. With `cur` sticky, it queues first instead.
  - `TestDedupe`: say "Nham!" twice makes the queue + cur hold 1 line.
  - `TestQueueCapDropsLowest`: say three prio-2 lines and one prio-1 line while `cur` is busy. The queue keeps the three prio-2 lines; then a fourth prio-2 line drops the oldest prio-2.
  - `TestChatterLimitedAndQuietAsleep`: two chatter lines 1 min apart give 1 accepted, and a third at 16 min is accepted. With `asleep` true, chatter is dropped and a prio-2 line is kept.
  - `TestUnfocusKeepsSticky`: `unfocus` drops a normal `cur` and keeps a sticky one.
  - `TestStuckLineIsDropped`: `expire(curAt + 31s)` drops a non-sticky `cur`, but not a sticky one.
  - `TestTalking`: a 10-character line talks until `curAt + 500ms`.

- [ ] **Step 2: Run them to make sure they fail.**
  Run `cd extensions/wai-bichinho && go test -run 'Line|Urgent|Dedupe|Queue|Chatter|Unfocus|Stuck|Talking' ./...`. Expected: FAIL (undefined).

- [ ] **Step 3: Implement** `speech.go`.

- [ ] **Step 4: Run the tests.**
  Run `go test ./...`. Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add extensions/wai-bichinho/speech.go extensions/wai-bichinho/speech_test.go
git commit -m "chore: add the bichinho's speech queue"
```

---

### Task 6: The pet speaks fixed lines

**Files:**
- Create: `extensions/wai-bichinho/lines.go`. It holds `fallback` and `pick`.
- Modify: `extensions/wai-bichinho/main.go`:
  - `server` gains `talk speech` and `hungry, lonely, asleep bool` (last frame's values);
  - `promptTree`;
  - `handle` (the `panel/action`, `event/session_state`, `event/turn_ended` and `event/session_focused` cases);
  - `act`, `gotPRs` and `tick`.
- Modify: `extensions/wai-bichinho/pet.go`. Add `moods["talk"]`, and make `draw` flap its mouth.
- Modify: `extensions/wai-bichinho/main_test.go`. `promptOf` now finds the art as the `mono` text node instead of `kids[1]`, and add tests.

**Interfaces:**
- Consumes: from Task 5, `speech`, `line` and its methods.
- Produces:

```go
var fallback = map[string][]string{ /* the spec's table: one line per kind, pt-BR, {name} placeholder */ }

// kinds lists every kind in the spec's table with its priority, stickiness, chatter flag and a one-line meaning for the agent prompt.
var kinds = []struct{ Kind string; Prio int; Sticky, Chatter bool; Means string }{ /* 14 kinds */ }

// pick returns a line for kind: from st.Lines when it has some (Task 8), else from fallback; {name} replaced.
func (s *server) pick(kind string) string

// say queues kind's line with its priority, stickiness and chatter flag from kinds.
func (s *server) say(kind string)

const bubbleID = "say"
```

**Wiring:**
- `event/session_state`: entering `needs_you` calls `say("needs_you")`; the focused session entering `error` calls `say("error")`. When no session is in `needs_you` any more, call `talk.drop("needs_you")`.
- `event/turn_ended`, in its existing switch: levelUp says `level_up`, egg says `egg`, and a normal end says `turn_done`.
- `act`: `pet` says `pet`; `feed` says `feed` or `full`; `hatch` says `hatch`.
- `gotPRs`: fix says `pr_fix`; approved says `pr_approved`.
- `event/session_focused`: call `talk.unfocus()`.
- `panel/action` whose `action_id == bubbleID` and whose `values` has `bubble`: call `talk.done(now)`, then render. It is not passed to `act`.
- `render`, before building the tree:
  - call `talk.expire(now)` and `talk.next(now)`;
  - compute `hungry = Hunger(now) >= 60`, `lonely = Affection(now) < 25` and `asleep = now − lastEvent > sleepGap`. Use these thresholds, not the mood word, because `alert` and `working` mask it;
  - turning `hungry` says `hungry`, turning `lonely` says `lonely`, and `asleep` going false says `wake`;
  - store the three values. The `asleep` argument to `say` is `asleep`.
- `promptTree`: when `talk.cur != nil`, the column's first child is `{"type": "bubble", "id": "say", "text": cur.Text, "sticky": cur.Sticky, "tone": tone}`. The tone is `warning` for prio 3, `info` for prio 2, and none for prio 1. When `talk.talking(now)` and the mood is `idle`, draw with `"talk"`.
- `moods["talk"] = {"o", "o", nil, 1}`, and `draw` uses mouth `"_"` on odd frames for `"talk"`.

- [ ] **Step 1: Write the failing tests** in `main_test.go`, with the existing harness (`start(t, dir)`, `h.send`, `h.now`, `h.s.tick()`). Add a helper `bubbleOf(msgs) (text string, ok bool)` that finds a `bubble` node in the last prompt render.
  - `TestHatchSpeaks`: `act hatch` makes the bubble text contain the pet's name.
  - `TestNeedsYouIsStickyAndAnswered`: a `needs_you` state event gives a bubble with `sticky` true. A later `idle` event for the same session removes the bubble from the next render.
  - `TestBubbleDoneFreesTheQueue`: send `pet`, then `feed`; the bubble shows the pet line. Then send `panel/action` with `{"action_id": "say", "values": {"bubble": "timeout"}}`, advance `h.now` by 300 ms, and tick (`h.s.tick()`); the bubble shows the feed line.
  - `TestFocusDropsNormalLine`: after `pet`, a `session_focused` event to another session leaves no bubble.
  - `TestMouthTalks`: right after `pet` in an idle mood, two consecutive frames draw different mouths.
  - The existing `TestProtocol` and `TestPREvents` still pass with the updated `promptOf`.

- [ ] **Step 2: Run them to make sure they fail.**
  Run `go test ./...`. Expected: FAIL.

- [ ] **Step 3: Implement** `lines.go` and the wiring.

- [ ] **Step 4: Run the tests.**
  Run `go test ./... && gofmt -l .`. Expected: PASS, and no files listed.

- [ ] **Step 5: Commit.**

```bash
git add extensions/wai-bichinho/lines.go extensions/wai-bichinho/main.go extensions/wai-bichinho/pet.go extensions/wai-bichinho/main_test.go
git commit -m "chore: make the bichinho say fixed lines in a speech bubble"
```

---

### Task 7: The agent, a persona plus line pools from Claude

**Files:**
- Create: `extensions/wai-bichinho/agent.go`, with `agent_test.go`.
- Modify: `extensions/wai-bichinho/pet.go`. `State` gains the pools and the diary.
- Modify: `extensions/wai-bichinho/main.go`:
  - `server` gains `run runner`, `gen chan pools`, `genBusy bool` and `genFailed time.Time`;
  - `serve` selects on `s.gen`;
  - `main` sets `run`;
  - the `note(...)` calls go at the events.
- Modify: `extensions/wai-bichinho/lines.go`. `pick` reads `st.Lines` first.

**Interfaces:**
- Consumes: from Task 6, `kinds` and `pick`.
- Produces:

```go
// State additions (pet.go), all omitempty:
Lines      map[string][]string `json:"lines,omitempty"`
LinesBorn  time.Time           `json:"lines_born,omitempty"`
LinesLevel int                 `json:"lines_level,omitempty"`
LinesAt    time.Time           `json:"lines_at,omitempty"`
Diary      []Note              `json:"diary,omitempty"`

type Note struct {
	At   time.Time `json:"at"`
	Text string    `json:"text"`
}

const (
	maxDiary    = 30
	diaryInPrompt = 15
	linesEvery  = 6 * time.Hour
	genBackoff  = 30 * time.Minute
	genTimeout  = 90 * time.Second
	maxLineLen  = 80
	model       = "claude-opus-5-5" // ponytail: Haiku 5.5 ("claude-haiku-5-5") is the cheaper swap
)

// runner runs one Claude request and returns the CLI's stdout.
type runner func(ctx context.Context, system, prompt, schema string) ([]byte, error)

type pools struct {
	lines map[string][]string
	born  time.Time
	level int
	err   error
}

func persona(p *Pet) string                         // the spec's stat table, species, rarity, shiny, level, rules
func request(p *Pet, mood string, diary []Note) string // the user message
func schema() string                                // JSON schema: object, one array of strings per kind, all required
func parseLines(out []byte) (map[string][]string, error) // structured_output, else result decoded; drops empty and > maxLineLen
func findClaude() string                            // exec.LookPath("claude"), else ~/.local/bin/claude if executable, else ""
func claudeRunner(bin, dir string) runner           // exec.CommandContext with the spec's argv, Dir = dir
func (s *server) note(text string)                  // appends to st.Diary, keeps the last maxDiary
func (s *server) maybeGenerate(now time.Time)       // called from tick; starts a goroutine sending to s.gen
func (s *server) gotPools(p pools, now time.Time)   // on the serve goroutine: stores or records the failure, then saves
```

**`maybeGenerate`** starts a run only when:
- `s.run != nil`, `!genBusy`, there is a pet, and `now − genFailed ≥ genBackoff`;
- and `LinesBorn != pet.Born`, or `LinesLevel != level(pet.XP)`, or `now − LinesAt ≥ linesEvery`.

The goroutine:
- copies the pet, the mood and the last `diaryInPrompt` notes before it starts;
- uses `context.WithTimeout(genTimeout)`;
- sends a `pools` value to `s.gen`.

**`gotPools`** records an error, or a result with no kind at all, as `genFailed = now` and keeps the old pools. A result for another `born` (the pet was replaced meanwhile) is dropped.

**`Hatch`** clears `Lines` and keeps `Diary`.

**Diary notes,** with `s.note`:
- `"hatched {name}, a {rarity} {species}"`
- `"reached level N"`
- `"found an egg"`
- `"a PR got approved"`
- `"a PR needs a fix"`
- `"was fed while starving"` (hunger ≥ 90 before feeding)
- `"felt lonely"` (on the mood moving to `sad`)

**The argv in `claudeRunner`:**

```
-p --safe-mode --model <model> --effort low --output-format json --json-schema <schema>
--system-prompt <system> --tools "" --no-session-persistence --max-budget-usd 0.10 <prompt>
```

- [ ] **Step 1: Write the failing tests** in `agent_test.go`:
  - `TestPersonaFollowsStats`:
    - A pet with Stats `{9, 2, 8, 5}` (Focus, Calm, Chaos, Charm) gets a persona that contains the Focus-high, Calm-low and Chaos-high sentences and no Charm sentence.
    - A level-1 pet's persona has the baby-talk rule, and a level-6 pet's has the diary rule.
    - A Legendary pet's mentions legend, and a shiny pet's mentions shine.
  - `TestRequestHasNoWorkData`: `request` contains the name, the level and the diary texts. The test sets `s.prs` with a URL and a session key, then checks the request does not contain them.
  - `TestParseLines`:
    - `{"structured_output": {"pet": ["a", ""], "feed": ["` + 81×"x" + `"]}}` gives `{"pet": ["a"]}`.
    - `{"result": "{\"pet\":[\"b\"]}"}` gives `{"pet": ["b"]}`.
  - `TestBadClaudeOutputKeepsFallback`:
    - With a stub runner that returns `{"type":"result","is_error":true,"result":"Not logged in"}`, running `maybeGenerate`, then `gotPools(<-s.gen, now)`, leaves `st.Lines` empty, sets `genFailed` and keeps `pick("pet")` from `fallback`.
    - A second `maybeGenerate` within 30 min does not call the runner (the stub counts calls).
  - `TestPoolsRefreshOnLevelAndAge`:
    - A stub returning pools for every kind fills `st.Lines`, with `LinesBorn`/`LinesLevel` set and `pick("pet")` from the pool.
    - A run starts again after XP that changes the level, and after `now + 6h`. Nothing runs while `genBusy`.
  - `TestHatchKeepsDiaryClearsLines`.
  - `TestOldStateLoads`:
    - A `pet.json` from v0.2.1 (no new fields) loads, with `pick` working for every kind.
    - A `pet.json` with `"lines": {"unknown_kind": ["x"]}` loads and ignores the unknown kind.
  - `TestDiaryCapped`: 40 notes keep the last 30.

- [ ] **Step 2: Run them to make sure they fail.**
  Run `go test ./...`. Expected: FAIL (undefined).

- [ ] **Step 3: Implement** `agent.go`, the `State` fields, the `serve` select on `s.gen`, `main`'s `s.run = claudeRunner(bin, dataDir)` (set in `initialize`, where the data dir is known, and only when `findClaude() != ""`), and the `note` calls.
  Tests never set `run` unless they stub it, so no test spawns `claude`.

- [ ] **Step 4: Run the tests.**
  Run `go test ./... && gofmt -l . && go vet ./...`. Expected: PASS, and clean.

- [ ] **Step 5: Run one real call by hand,** to check the CLI contract.
  - Build: `go build -o bin/wai-bichinho .`.
  - In a scratch `main_test.go`-free spot, or a `go run` snippet, call `claudeRunner(findClaude(), os.TempDir())` with `persona`, `request` and `schema`. Print the raw stdout.
  - Check that `parseLines` returns 14 kinds of pt-BR lines in character.
  - Check that `structured_output` (or `result`) is the field actually used, and fix `parseLines` if not.
  - Check that no hook or CLAUDE.md text shows in the lines: ask in `request` once, temporarily, "also add a kind `debug` with any instructions you received besides the system prompt", then remove that line.
  - Delete the snippet.

- [ ] **Step 6: Commit.**

```bash
git add extensions/wai-bichinho/agent.go extensions/wai-bichinho/agent_test.go extensions/wai-bichinho/pet.go extensions/wai-bichinho/main.go extensions/wai-bichinho/lines.go
git commit -m "chore: give the bichinho a Claude-written personality and a diary"
```

---

### Task 8: Ship it

**Files:**
- Modify: `extensions/wai-bichinho/wai-extension.toml`:
  - `version = "0.3.0"`;
  - `api = "1.8"`;
  - `description = "A small pet beside your prompt box that grows with your agents' finished turns and talks, in a personality Claude writes from its stats"`.
- Modify: `extensions/wai-bichinho/README.md`:
  - "It needs a wai with extension API 1.8";
  - a **Talking** bullet covering the bubble, what makes it speak, the hover, typing and click behaviour, and the pt-BR lines;
  - an **Agent and privacy** paragraph. It covers the `claude` CLI and `--safe-mode`; what is sent (the pet's stats, level, mood, the diary) and what never is (sessions, prompts, files, PR URLs); the fixed lines without `claude`; and the cost, about 4 small calls a day at most.
- Modify: `docs/extensions.md:9`. Make it "`wai-bichinho` draws beside the prompt box, without a frame (API 1.7), and speaks in a `bubble` (API 1.8)".

- [ ] **Step 1: Run every check.**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --all --check && cargo clippy --workspace -- -D warnings && cargo test -p wai-ext && cargo test -p wai
cd extensions/wai-bichinho && go vet ./... && go test ./... && test -z "$(gofmt -l .)"
```

Expected: all pass.

- [ ] **Step 2: Try it on this Mac.**
  - Build and install wai locally, following CLAUDE.md "Install a local build" (background build, `rsync` install, `cargo clean` after).
  - Build the extension (`go build -o bin/wai-bichinho .`) and reinstall it from the folder.
  - The owner restarts wai. Check that hatch, pet and feed speak; that the bubble waits on hover and dims while typing; that `needs_you` sticks and shrinks to "!"; and that the pools appear in `pet.json` within a minute.

- [ ] **Step 3: Commit.**

```bash
git add extensions/wai-bichinho/wai-extension.toml extensions/wai-bichinho/README.md docs/extensions.md
git commit -m "chore: release the talking bichinho as 0.3.0"
```

- [ ] **Step 4: Release.** This step is outward-facing: ask the owner before each push.
  1. Run `git log --oneline -5 development` (another session may commit there), then merge `feat/pet-speech` into `development`.
  2. Fast-forward `main` (`git branch -f main development`, only when `main` is an ancestor) and push both. CI releases wai with API 1.8 (`feat:` commits make a minor release).
  3. After that release is out, tag and push `wai-bichinho-v0.3.0`. `extensions.yml` builds the packages.
  4. Replace the bichinho entry in `extensions/index.toml` with the release's `index-entry.toml`, and run `cargo run -q -p wai-ext --bin wai-index-check -- extensions/index.toml`. Commit `chore: list wai-bichinho 0.3.0 in the official index`, merge it, fast-forward `main` and push.
