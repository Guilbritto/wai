# wai Onda 1, chat screen (1.2, 1.6, 1.7, 1.8): implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the conversation screen gets copy buttons (1.2), a pinned area just above the prompt box where permission and question cards (1.6) and messages sent during a turn (1.7) wait until they are answered or read, and the processing display drawn like the approved mockup (1.8).

**Architecture:** `ChatModel` gets two waiting lists beside `items`: `waiting` (open cards) and `steering` (messages sent into the running turn and not read yet). Both are drawn by one new `render_waiting`: a translucent Aurora glass stack laid over the bottom of the message list, just above the prompt box, which the conversation scrolls behind (owner-mandated, O1). The list gets a bottom inset equal to the stack's measured height, so its last row can always scroll clear of it. Each entry joins `items`, at its end, when it closes. `items` stays append-only, so no row index ever shifts. The "agent read the steer" signal sits in one function, `reads_steer`, so a real signal from the adapter can replace it later. 1.8 restyles rows in `render.rs` and the status line in `input.rs`. It changes no behavior except the grouped label.

**Tech Stack:** Rust 2024, GPUI + gpui-component through `gpui-kit = 0.9.0` (`Clipboard` widget, `TextView::code_block_actions`, `MessageScroller`), ACP through `wai-agent`. No new dependency.

**Spec:** `docs/superpowers/plans/2026-11-11-wai-master-plan.md`, Onda 1 rows 1.2, 1.6, 1.7 and 1.8 (pt-BR). Mockup for 1.8: `.superpowers/brainstorm/38118-1794303719/content/processamento.html`. Adapter behavior behind 1.7: `claude-agent-acp@0.103.0` `steer()` (`~/.npm/_npx/46301f59f66b1e3b/node_modules/@agentclientprotocol/claude-agent-acp/dist/v1/acp-agent.js` ~1717). It answers `{outcome:"injected"}` right after `session.input.push`. Priority is `now` (pre-empts the current generation) unless a permission or elicitation is open (`later`, which waits for it). The CLI's echo of the steer only sets `Turn.steeredAwaitingResult` (~4867), and nothing reaches the client.

## Global Constraints

Binding, from `.superpowers/sdd/master-onda-1/global-constraints.md`:

- Repo `/Users/guilherme.brito/Project/Guilherme/wai`, Rust workspace, GPUI + gpui-component, ACP. Base: branch `development` at 172c454.
- Every shell: `export PATH="$HOME/.cargo/bin:$PATH" _ZO_DOCTOR=0` (background shells have no cargo on PATH; `_ZO_DOCTOR` silences the zoxide banner).
- Disk is tight: run `df -h /System/Volumes/Data` before a build. If it shows under 8 GB free, stop and report. Run `cargo clean` in the worktree when the track is done.
- Never `cargo run` with a fake HOME. Tests use temp dirs, never the developer's real config, gitconfig or shell rc. Real Claude only in `#[ignore]` smoke tests or explicit validation work.
- Do NOT touch `/Applications/wai.app` or the running `wai` process. Never kill `wai`, `claude-agent-acp` or `claude` processes you did not start.
- Another session may also commit to this repo: run `git log` before branching or merging. `crates/wai/src/workspace.rs` (~4.9k lines) is the conflict hot spot. Track 1.5 (new-session draft) also edits `chat/input.rs`.
- Idle CPU < 0.5%, streaming CPU < 10%. Nothing animated or polling in the core when idle.
- UI strings stay in English. Aurora tokens/spec: `docs/superpowers/specs/2026-11-09-wai-visual-aurora-design.md`.
- Never break existing test ids (`tab-*`, `sidebar-*`, `new-session`, `send`, `stop`, permission/question button ids: `perm-allow_once|allow_always|reject_once|reject_always`, `question-submit|decline|cancel`, `field-<key>`).
- The core never touches user credentials and opens no ports.
- Follow existing patterns. No unrequested abstractions. No comments or docs unless they carry a non-obvious why.
- Git: work in worktree `.claude/worktrees/onda-1-chat` on branch `feat/onda-1-chat`, created from `development`. Commit messages are `feat:`/`fix:`/`test:`/`docs:`/`refactor:` with NO scope, NO Co-Authored-By line, NO Jira refs; imperative; first line < 72 chars. No push, no merge into `development` (the controller merges).

Track-specific:

- **Setup (before Task 1):** `git -C /Users/guilherme.brito/Project/Guilherme/wai log --oneline -5`, then `git worktree add .claude/worktrees/onda-1-chat -b feat/onda-1-chat development`. All paths below are relative to that worktree.
- **Gate** (the last step of every task; it must pass before the commit):
  ```sh
  export PATH="$HOME/.cargo/bin:$PATH" _ZO_DOCTOR=0
  df -h /System/Volumes/Data
  cargo build -p wai-agent --bin wai-fake-agent
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```
  Focused runs while doing TDD: `cargo test -p wai <test_name>`. The chat tests start the fake agent, so build it first.
- **No change to `wai-agent`:** the steer signal is derived in the client (Decision D1).
- **New UI copy, exactly:** `Copy message`, `Copy code`, `Thought`, `Edited <files>` (it replaces `✎ Edited <files>`), `Queued`, `Sent`, `Back to the prompt box`. Every other string stays as it is.
- **New test ids:** `("copy-message", row)`, `copy-code` (scoped per code block by `TextView`), `transcript` (the list's area, which the stack overlays), `waiting` (the pinned stack), `("waiting-permission", request id)`, `("waiting-question", request id)`, `("steering", i)`, `turn-status-time`. Kept: `("permission", row)` and `("question", row)` (now the closed one-line rows), `("queued", i)`, `("queued-remove", i)`, `("tool", row)`, `("step", row)`, `("edited", row)`, `("step-output", row)`, `("thought", row)`, `("group", row)`, `turn-status`, `("show-all", row)`, `("open-output", row)`.
- **Mockup to Aurora mapping (dark):** step line fill `aurora::vidro()`, border `aurora::borda()`, text `aurora::nevoa()`, chevron and kind icon `aurora::nevoa_2()`, ✓ `aurora::boreal()`. Output box fill `aurora::noite()`, border `aurora::borda()`, text `aurora::nevoa()`. Light mode uses theme tokens (`muted_foreground`, `border`, `muted`, `success`, `warning`), never Aurora constants.
- **User guide:** each task updates the `docs/user-guide.md` lines that describe what it changes (the lines are named in the task).

## Review Focus

1. **Two waiting cards with the same button ids:** a click on the second card's `perm-allow_once` answers the second card, not the first. Test: Task 7 `number_keys_answer_the_top_permission_card`.
2. **A long message waiting above the prompt box:** its balloon shows at most 3 lines (the stack itself is capped at 60% of the list's area and scrolls, Task 5). Test: Task 8 `messages_sent_during_a_turn_queue_until_it_ends` (clamp assertion).
3. **The same text steered twice** (`ok`, `ok`): both join the chat, one per read, in order, and neither stays pinned. Test: Task 9 `identical_steers_join_one_per_read`.
4. **A background subagent's activity while a steer waits:** its tool calls do not count as the main agent reading the steer. Test: Task 9 `steered_message_joins_when_the_agent_reads_it`.
5. **The agent exits with a card and a steer waiting:** nothing stays pinned. The card closes as `Cancelled`, the steer joins the chat, and both come before the exit notice. Test: Task 9 `exit_unpins_everything` (Task 5 adds the card half, `exit_cancels_waiting_cards`).

## Owner-mandated requirements

- **O1. The pinned stack overlays the list** (controller ruling, 2026-11-11). The owner's words: "um balão fixo, onde as conversas passam atrás dele" (a fixed balloon the conversation passes behind). The master-plan row adds that the balloon uses Aurora glass with a shadow, so the conversation reads behind it.
  - The cards (1.6) and the waiting messages (1.7) form one stack, laid over the bottom of the message list just above the prompt box. The list keeps scrolling under it.
  - The stack is a translucent Aurora glass card with a shadow.
  - The list gets a bottom inset equal to the stack's height, so its last row can scroll clear of the stack.
  - With the stack empty, nothing changes.

  Tasks 5 (the overlay) and 6 (the inset) carry it, and their tests assert it.

## Decisions (planner's calls)

The owner did not decide these. Each line gives the call, then what it costs if wrong.

- **D1. The steer "read" signal:** the first new output of the main agent after `injected`, while no card waits. New output means an `AgentText` or `AgentThought` chunk, or a `ToolCall` with `subagent: None`. `ToolCallUpdate`, usage, plan, title and anything from a subagent do not count. Why: the adapter reports nothing when the CLI consumes the steer. With priority `now` the CLI aborts the running generation, so the next new output is the steered cycle (at worst the tail of the aborted one, milliseconds early). With a card open the adapter uses `later`, and the "no card waits" clause mirrors that. The single swap point is `fn reads_steer(ev: &SessionEvent) -> bool` in `chat/model.rs`. A future adapter notice becomes `SessionEvent::SteerRead` in `wai-agent`, and `reads_steer` returns true only for it. Asking the adapter for that notice is optional upstream work, not needed here. Cost if wrong: the bubble enters the list one chunk early or late. Nothing is lost.
- **D2. Unread steers when the turn ends** (any reason, Cancel included) or the agent exits: they join the list then, before the Edits block and the closing notice. An `injected` reply that comes after the turn ended reads at once. Cost: a steer that a cancel cut off shows as sent, and the user resends it by hand.
- **D3. Steered balloons have no ×:** the message is already in the agent's input, and nothing can take it back. Cost: none functional. The owner may want a disabled × for symmetry.
- **D4. The queue's × puts the message (text and attachments) back in the prompt box,** ahead of the draft, with the tooltip `Back to the prompt box`. The master plan says the × "returns it to the box", while today's × drops it. Cost: one more keystroke to discard a message.
- **D5. How the O1 overlay is built** (the owner mandated the overlay, these details are mine):
  - **Position:** the stack is `absolute()` with `left_3().right_3().bottom_2()` inside the list's area (`transcript`, made `relative()`). It is `occlude()`d so clicks and wheel events over it don't reach the list, and capped at `max_h(relative(0.6))`, scrolling past that.
  - **Glass (dark):** fill `aurora::noite_2().alpha(0.72)`, border `aurora::borda_forte()`, radius 16 px, padding 8 px, shadow `BoxShadow::new(px(0.), px(12.), hsla(0., 0., 0., 0.8)).blur_radius(px(20.)).spread_radius(px(-12.))` (CSS `0 12px 40px -12px`; GPUI's blur is half the CSS one). Light: `theme.background.opacity(0.92)`, `theme.border`, a `0 8 24 -8 black@0.25` shadow.
  - **Cards inside:** the cards drop their opaque base and their own glow. `permission::frame` becomes an `accent.alpha(0.08)` tint with an `accent.alpha(0.55)` border and no shadow, so the glass shows through.
  - **The inset:** a `canvas` filling the stack reports its height in prepaint (the `terminal_view.rs` `fit` pattern). When the height changes, the view stores it, remeasures the last row and notifies. That costs one extra frame per change and nothing at rest. The list's bottom inset is `height + 16 px` (the stack's 8 px bottom offset plus the usual 8 px gap). With the stack empty, the height is 0 and the list keeps its default padding.
  - **Jump button:** the list's "Jump to latest" button is lifted by the same amount (`with_jump_button_style` margin-bottom), so the stack doesn't hide it.

  Cost: alpha 0.72 is a readability guess (one constant). The canvas measurement lags the stack's size by one frame.
- **D6. Closed cards in the list** are one compact line, `<title>` then the answer (`Yes`, `Answered: Blue`, `Declined`, `Cancelled`), styled like a step line, with no diff and no choices. A card closes where the conversation is at that moment, cancelled ones too. Cost: the diff preview is gone after answering (the Edits block and the tool row still show it).
- **D7. Stacking and number keys:**
  - Every waiting card is drawn in full, oldest on top, and only the top card shows the 1/2/3 badges.
  - Keys `1`–`9` with no modifier answer the top card's n-th choice, only when it is a permission card, the prompt box has the caret, and the box is empty (no text, no attachments).
  - A question card keeps its form: digits type.

  Cost: a digit typed into an empty box answers a permission, as in Claude Code's TUI. Moving to ⌘-digit is a one-line change.
- **D8. Agent exit closes waiting cards as `Cancelled`.** Otherwise a dead agent's buttons would stay pinned.
- **D9. Copy buttons:**
  - Every agent message gets an always-visible ghost xsmall copy button at its top right, beside the text, that copies its raw Markdown (read at click time).
  - Every code block gets gpui-component's `Clipboard` through `TextView::code_block_actions`, copying the code only. The language label stays beside it.
  - User bubbles and thoughts get none.

  Cost: noise on short messages. Hover-only is a later tweak.
- **D10. Kind icons come from the bundled Lucide set,** not the mockup's emoji: `execute` SquareTerminal, `read` FileText, `edit` Replace, `search` Search, `fetch` Globe, `delete` Delete, `move` ArrowRight, `subagent` Bot, `think` and thoughts Asterisk, anything else Dash. Cost: looks differ from the mockup's ⌘ and 📄.
- **D11. Thoughts stay as steps:** a compact line `Thought` that unfolds its text in the output box. The mockup shows none, but hiding them would drop the agent's reasoning entirely. Cost if the owner wants them gone: one match arm.
- **D12. The grouped label** is `N steps · ` followed by the run's tool-call titles: first line only, backticks trimmed, each title once, joined `, `. A run with no tool call reads `Thinking`. The titles are the agent's own words (no rewriting to past tense as in the mockup).
- **D13. The status mark keeps one tick a second,** cycling Claude Code's glyphs `✢ ✳ ✶ ✻ ✽`. No rotation animation: a real rotation costs frames for the whole turn. The activity text is boreal while Working and ambar while NeedsYou, and the time plus `Esc to stop` are pushed to the line's right end.
- **D14. Every row sits 8 px from the next** (was 16), messages included, as in the mockup.
- **D15. Mockup colors are mapped to Aurora tokens** (Global Constraints), not copied as hex.

---

## File map

| File | Responsibility in this track |
|---|---|
| `crates/wai/src/chat/model.rs` | `waiting`, `steering`, `Steer`, card close, steer read; `run_label` (D12) |
| `crates/wai/src/chat/render.rs` | `step_line`, `output_box`, `kind_icon`, rows (1.8), copy buttons (1.2), `render_waiting` (the glass overlay stack) and the balloons (1.6/1.7), the list's bottom inset (O1) |
| `crates/wai/src/chat/input.rs` | status line (1.8d), number keys (D7), `take_back`, steer flow (1.7); the queued pills leave it |
| `crates/wai/src/chat/view.rs` | card handlers keyed by request id, `card_diffs`, `waiting_height` and `set_waiting_height`, `load` clears; the chat tests |
| `crates/wai/src/chat/permission.rs` | translucent `frame`, waiting card (badges only on top), `closed` one-line row |
| `crates/wai/src/chat/question.rs` | forms keyed by request id, read from `model.waiting` |
| `crates/wai/src/aurora.rs` | `noite()` no longer test-only |
| `crates/wai/src/notify.rs` | "needs you" body from `model.waiting` |
| `crates/wai/src/workspace.rs` | one test reads `waiting` too (`resolution_stays_with_its_agent`) |
| `docs/user-guide.md` | the lines each task changes |

Order: Task 1 stands alone. Task 2 adds `step_line`, which Tasks 3 and 5 use. Task 4 stands alone. Tasks 5 to 9 build on one another, through `render_waiting`, `waiting_height` and `waiting`.

---

### Task 1: Copy buttons on agent messages and code blocks (1.2)

**Files:**
- Modify: `crates/wai/src/chat/render.rs` (`ChatItem::Agent` arm of `render_row`)
- Test: `crates/wai/src/chat/view.rs` (tests module)
- Docs: `docs/user-guide.md`, "The transcript" bullet list

**Interfaces:**
- Consumes: `gpui_kit::component::clipboard::Clipboard` (`new(id)`, `value(..)`, `value_fn(..)`, `tooltip`, `accessibility_label`), `TextView::code_block_actions(|block: &CodeBlock, &mut Window, &mut App| -> impl IntoElement)`, `CodeBlock::code() -> SharedString`, `CodeBlock::lang()`.
- Produces: element ids `("copy-message", row)` and `copy-code`.

- [ ] **Step 1: Write the failing test** in `view.rs` tests:

```rust
/// Each agent message copies its Markdown; each code block, its code.
#[gpui_kit::test]
fn copy_buttons_copy_the_message_and_its_code(cx: &mut TestAppContext) {
    cx.executor().allow_parking(); // the fake agent is a real process
    let dir = tempfile::tempdir().unwrap();
    let reply = dir.path().join("reply.md");
    fs::write(&reply, "Run this:\n\n```sh\ncargo test\n```\n").unwrap();
    let flags = format!("--reply-file {}", wai_agent::quote(&reply));
    let (workspace, cx) = open(cx, fake_settings(&flags));
    let session = start_session(&workspace, cx);
    turn(&session, cx);
    let copied = |id: ElementId, cx: &mut VisualTestContext| {
        click_edits(None, id, cx);
        cx.read_from_clipboard().and_then(|c| c.text()).unwrap_or_default()
    };
    let message = last_agent_text(&session, cx).unwrap();
    assert_eq!(copied(("copy-message", 1usize).into(), cx), message);
    assert_eq!(copied("copy-code".into(), cx).trim_end(), "cargo test");
}
```

- [ ] **Step 2: Run it:** `cargo test -p wai copy_buttons_copy_the_message_and_its_code`. Expected: FAIL. `click` panics because no element `copy-message` is registered.
- [ ] **Step 3: Implement it in `render_row`'s `ChatItem::Agent` arm:**
  - The bubble becomes `h_flex().items_start().gap_2()` holding `v_flex().flex_1().min_w_0()` (header and text, as today) and a `Clipboard::new(("copy-message", ix))`: `.tooltip("Copy message")`, `.accessibility_label("Copy message")`, xsmall.
  - Its `value_fn` reads `ChatItem::Agent { text }` at row `ix` through a `WeakEntity<ChatSession>` at click time, so a frame copies no text.
  - Its `TextView::new(&md.state)` gets `.code_block_actions(|block, _, cx| h_flex().gap_1().children(block.lang() as a muted text_xs label).child(Clipboard::new("copy-code").value(block.code()).tooltip("Copy code").accessibility_label("Copy code")))`. With actions set, `TextView` hides its own language label, so the closure shows it.
  - Thoughts and user bubbles are unchanged.
- [ ] **Step 4: Run it again:** `cargo test -p wai copy_buttons_copy_the_message_and_its_code`. Expected: PASS.
- [ ] **Step 5: User guide.** Under "The transcript", after "Replies stream in as Markdown.", add: "Each reply has a copy button that copies its Markdown, and each code block one that copies its code."
- [ ] **Step 6: Gate** (Global Constraints), then commit:

```bash
git add crates/wai/src/chat/render.rs crates/wai/src/chat/view.rs docs/user-guide.md
git commit -m "feat: copy buttons on agent messages and code blocks"
```

---

### Task 2: Compact step lines, kind icons, output box under the line (1.8 a, b, e, f)

**Files:**
- Modify: `crates/wai/src/chat/render.rs`: `status_icon`, `chevron`, `render_tool`, the `ChatItem::Thought` arm, `render_item` (group `gap_4` to `gap_2`), `Render::render` (row `pb_4` to `pb_2`)
- Modify: `crates/wai/src/aurora.rs`: drop `#[cfg(test)]` on `noite()`
- Test: `crates/wai/src/chat/view.rs`
- Docs: `docs/user-guide.md`, the Processing table row "Step by step", and "Always shown" (`✎ Edited <file>` becomes `Edited <file>`)

**Interfaces:**
- Produces (in `render.rs`, used by Tasks 3 and 5):
  - `pub(super) fn step_line(cx: &App) -> Div`: the mockup's `.t` row. `h_flex`, gap 6 px, `px(9)` by `py(5)`, radius 8 px, border, 13 px text in regular weight, muted colors per Global Constraints.
  - `fn output_box(cx: &App) -> Div`: the mockup's `.out`. `v_flex`, `p(6/8)`, radius 6 px, border, mono 11 px, muted text, Aurora `noite()` fill (light: `theme.muted`).
  - `fn kind_icon(kind: &str) -> IconName`: the mapping in D10.

- [ ] **Step 1: Write the failing test:**

```rust
/// A step is one compact line of its own, 8 px from the next; unfolded, its output sits in a
/// box under the line.
#[gpui_kit::test]
fn step_lines_are_compact_and_open_below(cx: &mut TestAppContext) {
    cx.executor().allow_parking(); // the fake agent is a real process
    let (workspace, cx) = open(cx, fake_settings("--steps 2"));
    let session = start_session(&workspace, cx);
    turn(&session, cx);
    let bounds = |id: (&'static str, usize), cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.find(id).bounds()
        })
    };
    let (tool, line, next) = (bounds(("tool", 2), cx), bounds(("step", 2), cx), bounds(("step", 3), cx));
    assert_eq!(line.top(), tool.top(), "the line is the step's own box");
    assert!((next.top() - line.bottom() - px(8.)).abs() < px(0.5), "{line:?} {next:?}");
    click_edits(None, ("step", 2usize), cx);
    let (line, output) = (bounds(("step", 2), cx), bounds(("step-output", 2), cx));
    assert!(output.top() >= line.bottom(), "{output:?} is not under {line:?}");
}
```

- [ ] **Step 2: Run it:** `cargo test -p wai step_lines_are_compact_and_open_below`. Expected: FAIL on `line.top() == tool.top()` (today the line sits inside a padded glass card).
- [ ] **Step 3: Implement:**
  - **Tool rows:** `render_tool` returns `v_flex().id(("tool", ix)).gap_1()` (4 px, the mockup's `margin-top:-4px` under an 8 px gap). Its first child is `step_line(cx).id((id, ix))` with:
    - `chevron(open)`;
    - `Icon::new(kind_icon(&tool.kind)).xsmall()` in the muted icon color (edits use `kind_icon("edit")`);
    - the title in a `flex_1 min_w_0 truncate` div (edits read `Edited <files>`);
    - `status_icon`.

    Unfolded, the output goes in `output_box(cx).id(("step-output", ix))`, then the "Show all" / "Open full output in editor" buttons and the diffs, all as siblings under the line.
  - **Status and chevron:** `status_icon` draws Completed as `IconName::Check` xsmall in boreal (light: `theme.success`) and keeps the other three icons, xsmall. `chevron` is xsmall in the muted icon color.
  - **Thought rows:** a `step_line` with id `("thought", ix)`: chevron, `kind_icon("think")`, `Thought`. Unfolded, the `TextView` goes in an `output_box` under it. The click also calls `this.remeasure(ix..ix + 1, cx)`, as tool rows do.
  - **Spacing (D14):** row gap `pb_2` instead of `pb_4`; the group head's `gap_4` becomes `gap_2`.
- [ ] **Step 4: Run it, then the processing tests:** `cargo test -p wai step_lines_are_compact_and_open_below`, then `cargo test -p wai -- steps_are_folded grouped_mode hidden_mode full_output`. Expected: PASS.
- [ ] **Step 5: User guide.**
  - In the Processing table, Step by step reads: "Each step is one folded line with an icon of its kind, its title and ✓ or ✗. A click opens a tool call's output in a box under the line; thinking shows as a `Thought` line that opens the same way."
  - "Always shown" reads `Edited <file>`.
- [ ] **Step 6: Gate,** then commit:

```bash
git add crates/wai/src/chat/render.rs crates/wai/src/aurora.rs crates/wai/src/chat/view.rs docs/user-guide.md
git commit -m "feat: compact step lines with kind icons and output boxes"
```

---

### Task 3: The grouped head names every step (1.8 c)

**Files:**
- Modify: `crates/wai/src/chat/model.rs` (`run_label`), `crates/wai/src/chat/render.rs` (`render_group` uses `step_line`)
- Test: `crates/wai/src/chat/model.rs` tests
- Docs: `docs/user-guide.md`, the Processing table row "Grouped"

**Interfaces:**
- Consumes: `step_line(cx)` (Task 2).
- Produces: `ChatModel::run_label(&self, run: Range<usize>) -> String` (same signature; new text per D12).

- [ ] **Step 1: Write the failing tests:**
  - In `steps_run_between_messages_and_edits`, change the expectations to `m.run_label(1..5) == "4 steps · t, Edit /p/a"`. After the edit completes, add `m.run_label(1..3) == "2 steps · t"`. Keep `run_label(4..5) == "1 step · t"` and `run_label(1..2) == "1 step · Thinking"`.
  - Add a test:

```rust
#[test]
fn run_label_names_each_tool_once() {
    let mut m = ChatModel::default();
    m.push_user("go");
    m.apply(&call("1", "`git status`\nmore", ToolStatus::Completed, None));
    m.apply(&call("2", "Read README.md", ToolStatus::Completed, None));
    m.apply(&call("3", "`git status`", ToolStatus::Completed, None));
    assert_eq!(m.run_label(1..4), "3 steps · git status, Read README.md");
}
```

- [ ] **Step 2: Run them:** `cargo test -p wai -- run_label_names_each_tool_once steps_run_between_messages_and_edits`. Expected: FAIL (today the label names only the latest step).
- [ ] **Step 3: Implement** `run_label` per D12, and draw `render_group` as `step_line(cx).id(("group", head))`: chevron, label `flex_1 truncate`, `status_icon`, with the click unchanged.
- [ ] **Step 4: Run them again,** then `cargo test -p wai grouped_mode_folds_a_run_of_steps_into_one_block`. Expected: PASS (its label `3 steps · Read README.md` still holds, since the title appears once).
- [ ] **Step 5: User guide.** In the Grouped row, the example reads `3 steps · Read README.md, Run git status`, and the row says the head names every step.
- [ ] **Step 6: Gate,** then commit:

```bash
git add crates/wai/src/chat/model.rs crates/wai/src/chat/render.rs docs/user-guide.md
git commit -m "feat: grouped steps head names every step"
```

---

### Task 4: The status line in green, its mark turning, the time at its end (1.8 d)

**Files:**
- Modify: `crates/wai/src/chat/input.rs` (the `status` block of `render_prompt`; new `fn mark`)
- Test: `crates/wai/src/chat/input.rs` tests, `crates/wai/src/chat/view.rs` (`status_line_follows_the_turn`)
- Docs: `docs/user-guide.md`, "The status line" bullet

**Interfaces:**
- Produces: `fn mark(secs: u64) -> &'static str` (private), element id `turn-status-time`.

- [ ] **Step 1: Write the failing tests.**
  - In `input.rs` tests:

```rust
#[test]
fn status_mark_turns_once_a_second() {
    assert_eq!([0, 1, 4, 5].map(super::mark), ["✢", "✳", "✽", "✢"]);
}
```

  - In `status_line_follows_the_turn`, right after the first `assert!(shows("turn-status", cx));`, add:

```rust
let (line, time) = cx.update(|window, _| {
    (window.find("turn-status").bounds(), window.find("turn-status-time").bounds())
});
assert!(line.right() - time.right() <= px(13.), "the time sits at the line's end");
```

- [ ] **Step 2: Run them:** `cargo test -p wai -- status_mark_turns_once_a_second status_line_follows_the_turn`. Expected: FAIL (`mark` is not defined).
- [ ] **Step 3: Implement it (D13):**
  - `mark(secs)` cycles `["✢", "✳", "✶", "✻", "✽"]`.
  - The status row: `gap_2 px_3`, 12.5 px text, colored boreal while `TurnState::Working` and ambar while `NeedsYou` (light: `theme.success` and `theme.warning`).
  - Its children: `mark(secs)`, the activity in a `min_w_0 truncate` div, a `flex_1` spacer, then `div().id("turn-status-time").flex_none()` holding `"{elapsed} · Esc to stop"` at 11.5 px in `nevoa_2` (light: `muted_foreground`), with `.test_support()`.
  - The clock stays the one-second `sync_clock` tick: no animation.
- [ ] **Step 4: Run them again.** Expected: PASS.
- [ ] **Step 5: User guide.** In "The status line", add: "It is green while the agent works and amber while a card waits for you; its mark turns once a second, and the time and `Esc to stop` sit at its right end."
- [ ] **Step 6: Visual check of the processing display** against the mockup, using the scratch-home screenshot procedure of `crates/wai/src/app.rs` (`fn screenshot`):

```sh
export PATH="$HOME/.cargo/bin:$PATH" _ZO_DOCTOR=0
cargo build -p wai --features screenshot && cargo build -p wai-agent --bin wai-fake-agent
SHOT=$(mktemp -d); mkdir -p "$SHOT/home/.config/wai" "$SHOT/project"
for mode in steps grouped; do
  printf 'default_agent = "fake"\n\n[agents.fake]\ncommand = "%s --steps 3 --edit %s"\n\n[conversation]\nprocessing = "%s"\n' \
    "$PWD/target/debug/wai-fake-agent" "$SHOT/project/a.txt" "$mode" > "$SHOT/home/.config/wai/settings.toml"
  HOME="$SHOT/home" WAI_SHOT="$SHOT/$mode.png" WAI_SHOT_PROMPT="what is in this folder?" target/debug/wai "$SHOT/project"
done
```

  Open both PNGs (Read tool) and compare them with options A and B of `processamento.html`: compact muted lines with kind icons and a green ✓, the grouped head naming the steps, the status line in amber with the time at its right end (the shot is taken when the edit's card waits). Report the PNG paths and any difference you see. Do not commit the PNGs.
- [ ] **Step 7: Gate,** then commit:

```bash
git add crates/wai/src/chat/input.rs crates/wai/src/chat/view.rs docs/user-guide.md
git commit -m "feat: green status line with a turning mark and the time at its end"
```

---

### Task 5: Permission and question cards wait in a glass stack over the list (1.6, O1)

**Files:**
- Modify: `crates/wai/src/chat/model.rs`: `waiting`, `on_permission`, `on_question`, `close` (replaces `set_answer`), `after_answer`, `on_exited`, `waiting_question`
- Modify: `crates/wai/src/chat/view.rs`: `card_diffs`, `questions` keyed by id, `handle_permission`, `handle_permission_resolved`, `handle_question`, `handle_question_resolved`, `answer`, `sync_row` (drop the Permission arm), `permission_diffs`, `load`
- Modify: `crates/wai/src/chat/render.rs`:
  - `render_waiting`;
  - the Permission and Question arms of `render_row`;
  - `Render::render`: the list's area becomes `div().id("transcript").relative().flex_1().min_h_0()` with `.test_support()`, holding the list and, laid over its bottom, the stack.
- Modify: `crates/wai/src/chat/permission.rs` (`frame` made translucent per D5, `card` for a waiting request, `closed`), `crates/wai/src/chat/question.rs` (forms by id)
- Modify: `crates/wai/src/notify.rs` (`message`), `crates/wai/src/workspace.rs` (test `resolution_stays_with_its_agent` only)
- Test: `model.rs`, `view.rs`, `notify.rs` tests
- Docs: `docs/user-guide.md`, "Permissions and questions"

**Interfaces:**
- Consumes: `step_line(cx)` (Task 2).
- Produces:
  - `ChatModel.waiting: Vec<ChatItem>`: open `Permission` and `Question` cards (`answered: None`), oldest first. Each moves to the end of `items`, closed, when it is answered or resolved.
  - `ChatModel::on_permission(&mut self, req: PermissionRequest)` and `on_question(&mut self, req: QuestionRequest)`: no return value now.
  - `fn close(&mut self, question: bool, id: u64, answer: &str) -> bool`: true when this model held that card; that card moves to `items` with `answered: Some(answer)`. A second close of the same id is a no-op that returns false.
  - `ChatModel::waiting_question(&self, id: u64) -> Option<&QuestionRequest>`.
  - `on_exited` closes every waiting card as `Cancelled`, before its notice (D8).
  - `ChatSession.card_diffs: HashMap<u64, Vec<DiffView>>` (per waiting permission) and `ChatSession.questions: HashMap<u64, QuestionForm>` (now keyed by request id).
  - `fn permission_diffs(&self, request: &PermissionRequest) -> Vec<DiffView>`: R5, moved from `sync_row`.
  - `fn render_waiting(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement>`: `None` when nothing waits.
    - Otherwise the O1 stack, built as D5 says: `v_flex().id("waiting").absolute().left_3().right_3().bottom_2().max_h(relative(0.6)).overflow_y_scroll().occlude().gap_2().p_2()`, the glass fill, border, radius and shadow, and `.test_support()`.
    - It holds one card per `model.waiting` entry, oldest first. Tasks 8 and 9 add balloons after the cards, and Task 6 adds the height canvas.
    - `Render::render` adds it as the last child of `transcript`, so it paints over the list.
  - `permission::card(request: &PermissionRequest, diffs: &[DiffView], on_choose: OnChoose, on_open: &OnOpen, cx: &App)`: frame id `("waiting-permission", request.id)`, always open.
  - `permission::closed(id: ElementId, title: &str, answer: &str, cx: &App) -> Stateful<Div>`: `step_line` with the title (`flex_1 truncate`) and the answer (`flex_none`, stronger text color).
  - `question::card(request: &QuestionRequest, forms: &mut HashMap<u64, QuestionForm>, window, cx) -> AnyElement`: frame id `("waiting-question", request.id)`, always open. `edit`, `render_field` and `answer_question` take the request id instead of a row.

- [ ] **Step 1: Write the failing model tests** in `model.rs`:

```rust
fn question(id: u64) -> QuestionRequest {
    QuestionRequest { id, session: "s".into(), message: "Pick".into(), fields: vec![], subagent: None }
}

/// A card waits outside the list; once closed it joins the list where the conversation is,
/// after what arrived while it waited, and only once.
#[test]
fn waiting_cards_join_the_list_once_closed() {
    let mut m = ChatModel { in_turn: true, ..Default::default() };
    m.on_permission(perm()); // id 7
    m.apply(&text("meanwhile"));
    m.on_question(question(8));
    assert_eq!(m.items, [ChatItem::Agent { text: "meanwhile".into() }]);
    assert_eq!(m.waiting.len(), 2);
    m.on_permission_answered(7, "Yes");
    assert!(matches!(&m.items[..], [ChatItem::Agent { .. }, ChatItem::Permission { answered: Some(a), .. }] if a == "Yes"));
    assert!(matches!(&m.waiting[..], [ChatItem::Question { .. }]));
    assert_eq!(m.state, TurnState::NeedsYou);
    m.on_question_resolved(8);
    m.on_question_resolved(8);
    assert_eq!(m.items.len(), 3);
    assert!(m.waiting.is_empty());
    assert_eq!(m.state, TurnState::Working);
}

/// Review focus 5: an exit leaves no card pinned.
#[test]
fn exit_cancels_waiting_cards() {
    let mut m = ChatModel::default();
    m.push_user("go");
    m.on_permission(perm());
    m.on_exited(None);
    assert!(m.waiting.is_empty());
    let n = m.items.len();
    assert!(matches!(&m.items[n - 2], ChatItem::Permission { answered: Some(a), .. } if a == "Cancelled"));
    assert!(matches!(&m.items[n - 1], ChatItem::Notice { .. }));
}
```

  Also update `subagent_permission_names_the_subagent` to read the titles from `m.waiting`.
- [ ] **Step 2: Run them:** `cargo test -p wai -- waiting_cards_join_the_list_once_closed exit_cancels_waiting_cards`. Expected: FAIL to compile (no field `waiting`).
- [ ] **Step 3: Implement the model half** (see Interfaces). `after_answer` reads `!self.waiting.is_empty()`. `on_permission_resolved` and `on_question_resolved` keep their rule: recompute the state only when `close` held the card and the state was `NeedsYou`.
- [ ] **Step 4: Write the failing view tests:**
  - In `permission_card_buttons_answer`, after the `NeedsYou` wait and before the click:

```rust
assert!(session.read_with(cx, |s, _| !s.model.items.iter().any(|i| matches!(i, ChatItem::Permission { .. }))));
cx.update(|window, cx| {
    window.render_frame(cx);
    assert!(window.within("waiting").try_find("perm-allow_once").is_some(), "the card is pinned");
    let bounds = |id: &'static str| window.find(id).bounds();
    let (waiting, list, input) = (bounds("waiting"), bounds("transcript"), bounds("input-state"));
    // O1: the stack lies over the list's bottom, inside its area, not below it.
    assert!(list.top() < waiting.top() && waiting.bottom() <= list.bottom(), "{waiting:?} is not over {list:?}");
    assert!(list.bottom() - waiting.bottom() <= px(9.), "{waiting:?} does not sit at the list's bottom");
    assert!(waiting.bottom() <= input.top(), "{waiting:?} is not above {input:?}");
});
```

    And after the `Idle` wait:

```rust
let items = session.read_with(cx, |s, _| s.model.items.clone());
let card = items.iter().position(|i| matches!(i, ChatItem::Permission { .. })).unwrap();
assert!(matches!(&items[card + 1], ChatItem::Agent { text } if text.starts_with("chose:")));
assert!(shows(("permission", card), cx), "the closed line");
assert!(!shows("waiting", cx));
```

  - In `question_card_submits_choice`, after the `NeedsYou` wait: `assert!(cx.update(|w, cx| { w.render_frame(cx); w.within("waiting").try_find("question-submit").is_some() }));`.
  - In `notify.rs` `notification_text`, push the `Permission` card into `model.waiting` instead of `model.items`; the expected texts stay.
  - In `workspace.rs` `resolution_stays_with_its_agent`, the `request` closure searches `s.model.waiting.iter().chain(&s.model.items)`.
- [ ] **Step 5: Run them:** `cargo test -p wai -- permission_card_buttons_answer question_card_submits_choice notification_text resolution_stays_with_its_agent`. Expected: FAIL (`waiting` is not drawn).
- [ ] **Step 6: Implement the view half.**
  - **Handlers:**
    - `handle_permission` computes `permission_diffs(&req)` before the model takes the request, stores them in `card_diffs[id]` and notifies; no row is appended.
    - `answer`, `handle_permission_resolved`, `answer_question` and `handle_question_resolved` call `sync_rows(None, cx)` after the model, because the closed card is a new row. They also drop `card_diffs[id]` and `questions[id]`. Resolutions reach every session of the agent, and a no-op is harmless.
    - `handle_question` only notifies.
  - **Render:**
    - `Render::render` lays out `header, transcript (the list, then render_waiting over it), render_prompt`. The empty state ("No messages yet.") stays inside `transcript`.
    - `permission::frame` takes D5's translucent tint (dark: `accent.alpha(0.08)` fill, `accent.alpha(0.55)` border, no shadow; light unchanged).
    - `render_row` draws closed cards with `permission::closed(("permission", ix).into(), &request.title, answer, cx)` and `(("question", ix).into(), &request.message, answer, cx)`.
  - **Load:** `load` also clears `model.waiting` and `card_diffs`.
  - **Notifications:** `notify::message` takes the "needs you" body from the newest `model.waiting` entry (today it takes the newest open card in `items`). The `diffs` field doc becomes "Per `Tool` row".
  - **Comment:** `hidden_mode_keeps_cards_and_edits`'s comment "Rows: …, its card." becomes "Rows: …, the edit (its card waits above the prompt box)."
- [ ] **Step 7: Run the chat and workspace suites:** `cargo test -p wai -- chat:: notify:: workspace::`. Expected: PASS, including `press_on_prompt_pill_focuses_input`: the note input is still found before the prompt input, since it is drawn above it.
- [ ] **Step 8: User guide.** The "Permission cards" bullet gains: "The card waits on a glass panel just above the prompt box, not in the transcript, and the conversation scrolls on behind it. Several cards stack, the oldest on top. Once answered, a card joins the transcript as one line with your answer, where the conversation is then." The "Questions" bullet: "They wait above the prompt box the same way."
- [ ] **Step 9: Gate,** then commit. The last row can still hide behind the stack until Task 6 adds the list's inset; every test passes either way.

```bash
git add crates/wai/src/chat crates/wai/src/notify.rs crates/wai/src/workspace.rs docs/user-guide.md
git commit -m "feat: pin permission and question cards over the list"
```

---

### Task 6: The list scrolls clear of the pinned stack (O1)

**Files:**
- Modify: `crates/wai/src/chat/view.rs` (`waiting_height` and `set_waiting_height`)
- Modify: `crates/wai/src/chat/render.rs`:
  - the height canvas inside `render_waiting`;
  - `Render::render` gives the list its bottom inset, lifts the jump button by the same amount, and resets the height when nothing waits.
- Test: `crates/wai/src/chat/view.rs`

**Interfaces:**
- Consumes: `render_waiting` and the `transcript` area (Task 5).
- Produces:
  - `ChatSession.waiting_height: Pixels`, documented as "The pinned stack's height at the last paint; 0 while nothing waits".
  - `fn set_waiting_height(&mut self, height: Pixels, cx: &mut Context<Self>)`: on a change only, stores it, remeasures the last row (`self.remeasure(n - 1..n, cx)`) and notifies.
  - **The canvas:** `render_waiting` adds `canvas(move |bounds, _, cx| view.update(cx, |this, cx| this.set_waiting_height(bounds.size.height, cx)), |_, _, _, _| {}).absolute().size_full()`, the `terminal_view.rs` `fit` pattern.
  - **The reset:** when `render_waiting` returns `None` and `waiting_height` is not 0, `Render::render` sets it to 0 and remeasures the last row.
  - **The inset:** with `waiting_height > 0`, the `MessageScroller` gets `.with_list_style(StyleRefinement::default().pb(self.waiting_height + px(16.)))`. The scroller moves list padding onto its last row, so the scroll extent covers it. It also gets `.with_jump_button_style(StyleRefinement::default().mb(self.waiting_height + px(16.)))`. With 0, neither style is set (O1: nothing changes).

- [ ] **Step 1: Write the failing test:**

```rust
/// O1: while a card waits, the list's last row scrolls clear of the stack (the list's bottom
/// inset follows the stack's height); once nothing waits, the inset goes.
#[gpui_kit::test]
fn list_scrolls_clear_of_the_pinned_stack(cx: &mut TestAppContext) {
    cx.executor().allow_parking(); // the fake agent is a real process
    let dir = tempfile::tempdir().unwrap();
    let reply = dir.path().join("long.md");
    let lines: Vec<String> = (1..=80).map(|i| format!("line {i}")).collect();
    fs::write(&reply, lines.join("\n\n")).unwrap();
    // With --ask-permission the reply streams before the tool call: the list overflows.
    let flags = format!("--ask-permission --reply-file {}", wai_agent::quote(&reply));
    let (workspace, cx) = open(cx, fake_settings(&flags));
    let session = start_session(&workspace, cx);
    send("edit", cx);
    wait_until(cx, "the permission card", |cx| state(&session, cx) == TurnState::NeedsYou);
    // The canvas reports the height in prepaint: the inset lands on the next frame.
    let frame = |cx: &mut VisualTestContext| {
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
        });
    };
    frame(cx);
    let last = session.read_with(cx, |s, _| s.model.items.len() - 1); // the edit's tool row
    let clear = |cx: &mut VisualTestContext| cx.update(|window, _| {
        let (row, stack) = (window.find(("tool", last)).bounds(), window.find("waiting").bounds());
        (row.bottom() <= stack.top() + px(0.5), stack.size.height)
    });
    let height = session.read_with(cx, |s, _| s.waiting_height);
    let (is_clear, stack) = clear(cx);
    assert!((height - stack).abs() <= px(2.), "inset {height:?} vs stack {stack:?}");
    assert!(is_clear, "at the tail, the last row sits above the stack");
    // Scrolled up and back: the last row is still reachable, clear of the stack.
    cx.update(|window, cx| {
        let delta = gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(600.)));
        window.scroll("transcript", delta, cx);
    });
    session.update(cx, |s, cx| s.scroller.update(cx, |list, cx| list.scroll_to_end(cx)));
    frame(cx);
    assert!(clear(cx).0, "the last row scrolls clear of the stack");
    // Nothing waits: no inset.
    click_edits(None, "perm-allow_once", cx);
    wait_until(cx, "the turn's end", |cx| state(&session, cx) == TurnState::Idle);
    frame(cx);
    assert!(!shows("waiting", cx));
    assert_eq!(session.read_with(cx, |s, _| s.waiting_height), px(0.));
}
```

- [ ] **Step 2: Run it:** `cargo test -p wai list_scrolls_clear_of_the_pinned_stack`. Expected: FAIL to compile (no field `waiting_height`).
- [ ] **Step 3: Implement** per Interfaces.
  - `set_waiting_height` compares before it stores, so a stack that keeps its size costs no extra frame (streaming CPU target).
  - A list that was following its tail (`is_following_tail()` before the remeasure) keeps following: call `scroll_to_end` after the remeasure in that case.
- [ ] **Step 4: Run it, then the chat suite:** `cargo test -p wai list_scrolls_clear_of_the_pinned_stack`, then `cargo test -p wai -- chat::`. Expected: PASS.
- [ ] **Step 5: Visual check:** rerun the Task 4 Step 6 commands with `processing = "steps"` only. The PNG shows the glass stack with its card over the list's bottom, the list's last row above it, the conversation dimly visible through the glass where it passes behind, and its shadow. Report the path. If the text behind or on the card is hard to read, say so and suggest an alpha for the D5 fill (do not change it).
- [ ] **Step 6: Gate,** then commit:

```bash
git add crates/wai/src/chat/view.rs crates/wai/src/chat/render.rs
git commit -m "feat: the list scrolls clear of the pinned stack"
```

---

### Task 7: Number keys answer the top permission card (1.6, D7)

**Files:**
- Modify: `crates/wai/src/chat/permission.rs` (`choice` takes `n: Option<usize>`, so no badge for `None`; `card` gains `numbered: bool`), `crates/wai/src/chat/render.rs` (`numbered` is true only for the first waiting card), `crates/wai/src/chat/input.rs` (`answer_key` and its capture on the prompt pill)
- Test: `crates/wai/src/chat/view.rs`
- Docs: `docs/user-guide.md`, "Permissions and questions" and the shortcuts table

**Interfaces:**
- Consumes: `model.waiting`, `ChatSession::answer(request: u64, choice: &PermissionChoice, cx)` (Task 5).
- Produces: `fn answer_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>)`, registered as `field.capture_key_down(cx.listener(..))` on the prompt pill (the `field` h_flex holding the textarea), so it fires only while the prompt box has the caret.

- [ ] **Step 1: Write the failing test:**

```rust
/// Review focus 1: with the prompt box empty, 1–9 answer the top card, the oldest; a click
/// answers the card it is on; with text in the box, a digit types.
#[gpui_kit::test]
fn number_keys_answer_the_top_permission_card(cx: &mut TestAppContext) {
    cx.executor().allow_parking(); // the fake agent is a real process
    let (workspace, cx) = open(cx, fake_settings(""));
    let session = start_session(&workspace, cx);
    let request = |id: u64, session: String| PermissionRequest {
        id, session, tool_call_id: "t".into(), title: format!("Run {id}"), diff: None, subagent: None,
        options: vec![
            PermissionChoice { id: "y".into(), name: "Yes".into(), kind: ChoiceKind::AllowOnce },
            PermissionChoice { id: "n".into(), name: "No".into(), kind: ChoiceKind::RejectOnce },
        ],
    };
    // The broker never asked these: an answer fails, and the card closes as Cancelled.
    let closed = |cx: &mut VisualTestContext| session.read_with(cx, |s, _| {
        (s.model.items.iter()).filter_map(|i| match i { ChatItem::Permission { request, .. } => Some(request.id), _ => None }).collect::<Vec<_>>()
    });
    session.update(cx, |s, cx| {
        let id = s.id.clone();
        s.handle_permission(request(100, id.clone()), cx);
        s.handle_permission(request(101, id), cx);
    });
    cx.simulate_keystrokes("2");
    assert_eq!(closed(cx), [100]);
    assert_eq!(typed(&session, cx), "");
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.within(("waiting-permission", 101u64)).click("perm-allow_once", cx);
    });
    assert_eq!(closed(cx), [100, 101]);
    session.update(cx, |s, cx| { let id = s.id.clone(); s.handle_permission(request(102, id), cx) });
    cx.simulate_input("a");
    cx.simulate_keystrokes("1");
    assert_eq!(typed(&session, cx), "a1");
    assert_eq!(closed(cx), [100, 101]);
}
```

  (Imports: `wai_agent::{ChoiceKind, PermissionChoice, PermissionRequest}`.)
- [ ] **Step 2: Run it:** `cargo test -p wai number_keys_answer_the_top_permission_card`. Expected: FAIL at `closed(cx) == [100]` (the `2` was typed into the box).
- [ ] **Step 3: Implement `answer_key` (D7).**
  - It acts on `event.keystroke.key` `"1"`–`"9"` with `!event.keystroke.modifiers.modified()`, and only when `self.input.read(cx).value().is_empty() && self.attachments.is_empty()`.
  - The first `model.waiting` entry must be a `ChatItem::Permission` holding at least n options.
  - It then calls `cx.stop_propagation()` (so the text input never gets the digit) and `self.answer(request.id, &options[n - 1], cx)`.
  - In every other case it returns and lets the key through.
  - Badges: `render_waiting` passes `numbered: k == 0`.
- [ ] **Step 4: Run it again,** then `cargo test -p wai -- permission_ question_ press_on_prompt`. Expected: PASS.
- [ ] **Step 5: User guide.**
  - "Permission cards" gains: "With the prompt box empty, **1**–**9** pick the top card's choice of that number; only the top card shows numbers."
  - The shortcuts table gains the row `| Answer the top permission card | 1–9 | 1–9 | prompt box, while empty |`.
- [ ] **Step 6: Gate,** then commit:

```bash
git add crates/wai/src/chat/permission.rs crates/wai/src/chat/render.rs crates/wai/src/chat/input.rs crates/wai/src/chat/view.rs docs/user-guide.md
git commit -m "feat: number keys answer the top permission card"
```

---

### Task 8: Queued messages wait as pinned balloons; × takes one back (1.7, queue)

**Files:**
- Modify: `crates/wai/src/chat/render.rs`:
  - `fn user_bubble(cx: &App) -> Div`, extracted from the `User | Steered` arm and used by it;
  - `fn pinned_message(id: ElementId, caption: &'static str, text: &str, remove: Option<Button>, cx: &App) -> Stateful<Div>`;
  - `render_waiting` draws one per `self.queued` entry after the cards.
- Modify: `crates/wai/src/chat/input.rs`:
  - remove the `queued` pills from `render_prompt`;
  - `fn take_back(&mut self, message: Message)`;
  - `release_queue(false)` uses `take_back`.
- Test: `crates/wai/src/chat/view.rs` (`messages_sent_during_a_turn_queue_until_it_ends`)
- Docs: `docs/user-guide.md`, the "Any other agent (Codex, Copilot)" bullet

**Interfaces:**
- Consumes: `render_waiting` (Task 5).
- Produces:
  - `pinned_message`: a right-aligned row with `remove` (when given) left of a `user_bubble` at opacity 0.75. The bubble holds the muted caption (`text_xs`) over the text, which is clamped with `line_clamp(3)`. Task 9 reuses it.
  - `take_back`: splices the message's attachments in at the front of `self.attachments`, and sets `self.unsent` to its text. When `unsent` already holds text, the new text goes after it, a blank line apart.
  - Queue entries render as `pinned_message(("queued", i), "Queued", &message.shown(), Some(×), cx)`. The × is `Button::new(("queued-remove", i))` with tooltip and accessibility label `Back to the prompt box`; its click removes the entry, calls `take_back` and notifies.

- [ ] **Step 1: Write the failing test.** Rewrite the middle and end of `messages_sent_during_a_turn_queue_until_it_ends`. After the three sends and `typed == ""`:

```rust
cx.update(|window, cx| {
    window.render_frame(cx);
    assert!(window.within("waiting").try_find(("queued", 2usize)).is_some());
    let (waiting, input) = (window.find("waiting").bounds(), window.find("input-state").bounds());
    assert!(waiting.bottom() <= input.top());
});
click_edits(None, ("queued-remove", 1usize), cx);
cx.update(|window, cx| window.render_frame(cx));
assert_eq!(typed(&session, cx), "second", "back in the prompt box");
assert!(!shows(("queued", 2usize), cx));
assert!(shows(("queued", 1usize), cx));
// Review focus 2: a long message shows at most 3 lines.
let long: Vec<String> = (1..=10).map(|i| format!("line {i}")).collect();
session.update(cx, |s, cx| { s.queued.push(Message { text: long.join("\n"), attachments: Vec::new() }); cx.notify() });
let height = |ix: usize, cx: &mut VisualTestContext| cx.update(|window, cx| { window.render_frame(cx); window.find(("queued", ix)).bounds().size.height });
assert!(height(2, cx) < height(0, cx) * 3., "the long balloon is not clamped");
session.update(cx, |s, cx| { s.queued.pop(); cx.notify() });
```

  It ends with `user_texts == ["go", "first\n\nthird"]`, `!shows("waiting", cx)` and `typed(&session, cx) == "second"`.
- [ ] **Step 2: Run it:** `cargo test -p wai messages_sent_during_a_turn_queue_until_it_ends`. Expected: FAIL (no `waiting` holds `queued`, and × drops the message).
- [ ] **Step 3: Implement** per Interfaces. `stop_puts_queued_messages_back_in_the_prompt_box` keeps passing through `take_back`.
- [ ] **Step 4: Run it, then the queue and stop tests:** `cargo test -p wai -- messages_sent_during stop_puts_queued`. Expected: PASS.
- [ ] **Step 5: User guide.** The bullet reads: "it waits above the prompt box as a `Queued` balloon, with a **×** that puts it back in the prompt box".
- [ ] **Step 6: Gate,** then commit:

```bash
git add crates/wai/src/chat/render.rs crates/wai/src/chat/input.rs crates/wai/src/chat/view.rs docs/user-guide.md
git commit -m "feat: pin queued messages above the prompt box"
```

---

### Task 9: Steered messages wait pinned until the agent reads them (1.7, steering)

**Files:**
- Modify: `crates/wai/src/chat/model.rs`:
  - `Steer`, `steering`, `steer_sent`, `steer_injected`, `steer_dropped`, `read_steers`, `reads_steer`;
  - hooks in `apply`, `on_turn_ended` and `on_exited`.
- Modify: `crates/wai/src/chat/input.rs` (`steer`), `crates/wai/src/chat/render.rs` (`render_waiting` draws `model.steering`), `crates/wai/src/chat/view.rs` (`load` clears `model.steering`)
- Test: `model.rs` and `view.rs` tests
- Docs: `docs/user-guide.md`, the "An agent that can be steered" bullet

**Interfaces:**
- Consumes: `model.waiting` (Task 5), `pinned_message` (Task 8).
- Produces:
  - `#[derive(Debug, Clone, PartialEq)] pub struct Steer { pub text: String, pub injected: bool }`, documented as "a message sent into the running turn; `injected` once the agent took it".
  - `ChatModel.steering: Vec<Steer>`: oldest first. Each one drawn as `pinned_message(("steering", i), "Sent", &steer.text, None, cx)`, after the queue.
  - `ChatModel::steer_sent(&mut self, text: &str)`: pushes `{ text, injected: false }`.
  - `ChatModel::steer_injected(&mut self, text: &str)`: the first entry with this text and `!injected` becomes injected. With no turn running any more (`!self.in_turn`), it reads at once (D2).
  - `ChatModel::steer_dropped(&mut self, text: &str)`: removes the first entry with this text and `!injected`.
  - `fn read_steers(&mut self)`: moves every injected entry, in order, to `items` as `ChatItem::Steered` (through `push_steered`). Entries still waiting for their reply stay.
  - `fn reads_steer(ev: &SessionEvent) -> bool`: D1's signal. `AgentText(_) | AgentThought(_)` → true, `ToolCall(v)` → `v.subagent.is_none()`, anything else → false. Add one `ponytail:` comment naming the ceiling: "a proxy, since the adapter reports no consumption; replace it with an explicit event once it does".
  - `apply` first runs `if self.waiting.is_empty() && reads_steer(ev) { self.read_steers() }`, so the steer lands just before the output that answers it.
  - `on_turn_ended` and `on_exited` call `read_steers()` first.
  - `ChatSession::steer` calls `steer_sent(&message.shown())` and `cx.notify()` before it awaits. Then:
    - `Ok(true)` → `steer_injected`, then `sync_rows(None, cx)`;
    - `Ok(false)` → `steer_dropped`, then `send_message`;
    - `Err` → `steer_dropped`, then today's notice and queue path.

- [ ] **Step 1: Write the failing model tests:**

```rust
fn steered(text: &str) -> ChatItem {
    ChatItem::Steered { text: text.into() }
}

/// Review focus 4: a steer joins just before the main agent's next new output; a call's
/// update or a subagent's call is not one.
#[test]
fn steered_message_joins_when_the_agent_reads_it() {
    let mut m = ChatModel::default();
    m.push_user("go");
    m.apply(&tool("1", ToolStatus::InProgress));
    m.steer_sent("x");
    m.steer_dropped("x"); // the turn had ended: it went as a prompt
    assert!(m.steering.is_empty());
    m.steer_sent("b");
    m.apply(&text("a")); // its reply has not come: not taken yet
    m.steer_injected("b");
    m.apply(&SessionEvent::ToolCallUpdate(ToolCallPatch {
        id: "1".into(),
        title: None,
        status: Some(ToolStatus::Completed),
        content: None,
        locations: None,
    }));
    m.apply(&call("s", "Sub", ToolStatus::InProgress, Some("c")));
    assert!(!m.items.contains(&steered("b")));
    m.apply(&text("ok"));
    let n = m.items.len();
    assert_eq!(m.items[n - 2..], [steered("b"), ChatItem::Agent { text: "ok".into() }]);
    assert!(m.steering.is_empty());
}

/// With a card waiting the adapter holds the steer: it is read only after the answer.
#[test]
fn steer_waits_for_the_cards() {
    let mut m = ChatModel::default();
    m.push_user("go");
    m.on_permission(perm());
    m.steer_sent("b");
    m.steer_injected("b");
    m.apply(&text("x"));
    assert!(!m.items.contains(&steered("b")));
    m.on_permission_answered(7, "Yes");
    m.apply(&text("y"));
    let n = m.items.len();
    assert!(matches!(&m.items[n - 3], ChatItem::Permission { .. }), "{:?}", m.items);
    assert_eq!(m.items[n - 2..], [steered("b"), ChatItem::Agent { text: "y".into() }]);
}

/// D2: an unread steer joins when the turn ends, before its closing rows; a late injection reads at once.
#[test]
fn unread_steers_join_when_the_turn_ends() {
    for reason in [StopReason::EndTurn, StopReason::Cancelled] {
        let mut m = ChatModel::default();
        m.push_user("go");
        m.apply(&edit("t", "/p/a", None, "a", ToolStatus::Completed));
        m.steer_sent("b");
        m.steer_injected("b");
        m.on_turn_ended(&reason);
        let at = m.items.iter().position(|i| *i == steered("b")).expect("joined");
        let edits = m.items.iter().position(|i| matches!(i, ChatItem::Edits { .. })).unwrap();
        assert!(at < edits, "{reason:?}");
        m.steer_sent("c");
        m.steer_injected("c");
        assert_eq!(m.items.last(), Some(&steered("c")), "{reason:?}");
        assert!(m.steering.is_empty());
    }
}

/// Review focus 3: the same text twice joins once per read, in order.
#[test]
fn identical_steers_join_one_per_read() {
    let mut m = ChatModel::default();
    m.push_user("go");
    m.steer_sent("ok");
    m.steer_sent("ok");
    m.steer_injected("ok");
    m.apply(&text("a"));
    assert_eq!(m.steering, [Steer { text: "ok".into(), injected: false }]);
    m.steer_injected("ok");
    m.apply(&text("b"));
    assert_eq!(m.items.iter().filter(|i| **i == steered("ok")).count(), 2);
    assert!(m.steering.is_empty());
}

/// Review focus 5: an exit leaves nothing pinned.
#[test]
fn exit_unpins_everything() {
    let mut m = ChatModel::default();
    m.push_user("go");
    m.on_permission(perm());
    m.steer_sent("b");
    m.steer_injected("b");
    m.on_exited(None);
    assert!(m.waiting.is_empty() && m.steering.is_empty());
    let n = m.items.len();
    assert!(m.items[..n - 1].contains(&steered("b")));
    assert!(matches!(&m.items[n - 1], ChatItem::Notice { .. }));
}
```

- [ ] **Step 2: Run them:** `cargo test -p wai -- steered_message_joins steer_waits unread_steers identical_steers exit_unpins`. Expected: FAIL to compile (no `steer_sent`).
- [ ] **Step 3: Implement the model half** per Interfaces.
- [ ] **Step 4: Write the failing view test,** and update `message_sent_during_a_turn_steers_it`'s doc to "it shows as the user's once the turn reads it" (its assertions stand):

```rust
/// A message steered while a card waits stays pinned until the agent reads it, then joins
/// the chat after the answered card and before the reply to it.
#[gpui_kit::test]
fn steered_message_stays_pinned_until_read(cx: &mut TestAppContext) {
    cx.executor().allow_parking(); // the fake agent is a real process
    let (workspace, cx) = open(cx, fake_settings("--steering --ask-permission"));
    let session = start_session(&workspace, cx);
    send("edit", cx);
    wait_until(cx, "the permission card", |cx| state(&session, cx) == TurnState::NeedsYou);
    send("also", cx);
    wait_until(cx, "the steer taken", |cx| {
        session.read_with(cx, |s, _| s.model.steering.first().is_some_and(|s| s.injected))
    });
    let also = ChatItem::Steered { text: "also".into() };
    assert!(cx.update(|w, cx| { w.render_frame(cx); w.within("waiting").try_find(("steering", 0usize)).is_some() }));
    assert!(!session.read_with(cx, |s, _| s.model.items.contains(&also)));
    click_edits(None, "perm-allow_once", cx);
    wait_until(cx, "the turn's end", |cx| state(&session, cx) == TurnState::Idle);
    let items = session.read_with(cx, |s, _| s.model.items.clone());
    let at = |f: &dyn Fn(&ChatItem) -> bool| items.iter().position(|i| f(i)).unwrap();
    let card = at(&|i| matches!(i, ChatItem::Permission { .. }));
    let steer = at(&|i| *i == also);
    let reply = at(&|i| matches!(i, ChatItem::Agent { .. }));
    assert!(card < steer && steer < reply, "{items:?}");
    assert_eq!(items[reply], ChatItem::Agent { text: "chose:allow-once Steered: also".into() });
    assert!(!shows(("steering", 0usize), cx));
}
```

- [ ] **Step 5: Run it:** `cargo test -p wai steered_message_stays_pinned_until_read`. Expected: FAIL (no `steering` balloon, or the steer is already in the list).
- [ ] **Step 6: Implement the view half:** `steer` as in Interfaces, `render_waiting` draws `model.steering`, and `load` clears it. `handle_turn_ended` and `handle_agent_exited` already call `sync_rows(None)`.
- [ ] **Step 7: Run the steering tests:** `cargo test -p wai -- steer`. Expected: PASS, including `message_sent_during_a_turn_steers_it` and `steer_without_a_turn_sends_a_prompt`.
- [ ] **Step 8: User guide.** The steering bullet reads: "An agent that can be steered (Claude Code) gets the message inside the running turn. It waits above the prompt box as a `Sent` balloon until the agent reads it, then joins the transcript there. wai takes the agent's next reply, thought or tool call as the sign that it read the message; while a card waits, the one after you answer it. A message still unread when the turn ends joins the transcript then. If the turn ended meanwhile, it goes as a new prompt. If steering fails, the message is queued instead, and the transcript says so."
- [ ] **Step 9: Gate,** then commit. Then run `cargo clean` in the worktree (the track is done) and report the branch and the PNG paths from Tasks 4 and 6.

```bash
git add crates/wai/src/chat docs/user-guide.md
git commit -m "feat: steered messages join the chat once the agent reads them"
```
