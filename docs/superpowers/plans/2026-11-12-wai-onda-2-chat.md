# wai Onda 2, chat track (2.4, 3.6, 3.1, 3.2, Onda 1 leftovers): implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the conversation header shows what the conversation cost (2.4); Revert works on tab-indented and CRLF files (3.6); one subagent can be stopped on its own (3.1); a subagent's own transcript opens in a side panel, and the Agents tab lists finished subagents too (3.2); the Onda 1 chat leftovers that still apply are fixed.

**Architecture:**
- **Cost:** `ChatModel` already reads `cost_usd`; it now keeps the last known value, and `render_header` draws it as a pill next to the context pill.
- **Revert:** `edits.rs` gets a fallback for a hunk that no longer matches byte for byte: it compares the file as Claude Code shows it (CRLF as LF, each leading tab as two spaces) and writes the old text back in the file's own form.
- **Subagents, protocol side:** `wai-agent` stops dropping a child session's prompt, text, thinking and checklist. `Subagents::route` sends them to the root session as two new events, `SubagentSaid` and `SubagentPlan`, and `SubagentPatch` carries the child's `cancel` capability. Stopping one subagent needs no new API. `AgentHandle::cancel(child_id)` already answers the child's own open requests `cancelled` and sends `session/cancel` for the child id. claude-agent-acp 0.103.0 grants `capabilities.cancel` to every child and stops only that child (`cancelChildSession` → `NativeSubagentRuntime.cancelChild` → `Query.stopTask`).
- **Subagents, app side:** each `Subagent` in `ChatModel` keeps an append-only Markdown transcript and its checklist. The transcript opens in a gpui-component `Sheet` on the right. The sheet holds a small `SubagentTranscript` view that the session feeds on its existing 100 ms flush. Stop and Open transcript sit on the subagent's card in the chat and on its line in the Agents tab. `workspace.rs` is not touched.

**Tech Stack:** Rust 2024; GPUI + gpui-component through `gpui-kit = "=0.9.0"` (`Sheet`/`WindowExt::open_sheet`, `TextViewState`, `Button`); `agent-client-protocol = "5"` (lock 5.0.0, schema 1.15.1, feature `unstable_subagents`); `@agentclientprotocol/claude-agent-acp@0.103.0`. No new dependency.

**Spec:**
- `docs/superpowers/plans/2026-11-12-wai-execucao-pendencias.md` (pt-BR), Fase 1, "chat" row.
- `docs/superpowers/plans/2026-11-11-wai-master-plan.md`, rows 2.4, 3.1, 3.2 and 3.6. Its line numbers are from 0.5.0 and are stale.
- `docs/superpowers/plans/2026-11-10-wai-acp-v2-and-subagents.md`, "Not in this plan, on purpose".
- `/Users/guilherme.brito/Project/Guilherme/wai/.superpowers/sdd/master-onda-1/track-a-report.md`, "Deferred minors".
- Sources read for the calls below:
  - claude-agent-acp 0.103.0 under `~/.npm/_npx/46301f59f66b1e3b/node_modules/@agentclientprotocol/claude-agent-acp/dist/`:
    - `v1/acp-agent.js`: `cancel` ~5385, `cancelChildSession` ~5397, `perTaskStopAffordance` ~7608, the cancelled-permission branch ~6858, `usage_update` with `cost` ~4170;
    - `native-subagents.js`: `cancelChild` ~679, `announceNativeSubagent` ~1373, `publishPrompt` ~1459;
    - `diff.js`: `patchUpdateFromDiffToolResponse` ~146, "Claude converts leading tabs to spaces and CRLF line endings to LF".
  - Claude Code's tab rule, from the SDK binary `@anthropic-ai/claude-agent-sdk-darwin-arm64/claude`: `function Fle(e){…return e.replace(/^\t+/gm,(n)=>"  ".repeat(n.length))}`.
  - ACP schema 1.15.1: `Cost.amount` is the "Total cumulative cost for session"; `SubagentSessionCapabilities.cancel` "Permits the client to cancel this child's current work without ending the session".

> **Scope change (2026-11-12, controller):** execute **Tasks 1, 2 and 8 only**, then "Track finish". Tasks 3–7 (3.1 stop one subagent, 3.2 its transcript, the Agents tab) are **deferred**: another session is building the owner-approved sidebar redesign (`feat/sidebar-redesign`, spec `docs/superpowers/specs/2026-11-12-wai-sidebar-redesign-design.md` on that branch), which puts the session's agents under the prompt box and opens a subagent in a drawer above it, not a side sheet. 3.1/3.2 get re-planned on top of that drawer after it merges; Tasks 3–4 (protocol and model side) are the starting point then. Decisions D3–D9 below apply only to the deferred tasks.

## Decisions to confirm

The owner did not decide these. Each line gives the call, then what it costs if wrong.

- **D1. Cost pill:**
  - It shows the agent's own `cost.amount`, USD only (already filtered in `convert.rs`), as `$0.42` (two decimals) in its own pill right of the context pill. There is no pill until the agent reports a cost.
  - A usage update without a cost keeps the last one. Claude sends a cost only at each turn's end; its mid-turn and compaction updates carry none.
  - It is what the adapter reports for its running `query()`. After a Restart or a restore, the count starts again for that run; it is not the conversation's lifetime total. On a subscription it is the API-equivalent price.
  - Cost if wrong: a misleading figure after a restore. Relabelling or hiding it is a one-line change.
- **D2. Revert re-tabs what it puts back.**
  - Claude Code reports each leading tab as two spaces. In a file with at least one tab-indented line, Revert turns every two leading spaces of the old text into one tab (an odd space stays a space) before writing it.
  - A line that had tabs and then spaces in its indentation comes back with tabs only.
  - A file that mixes CRLF and LF line ends is still refused.
  - Cost: a whitespace-only difference on such lines, visible in `git diff`.
- **D3. Stopping one subagent sends `session/cancel` with the child's session id.**
  - It is offered only while the subagent is live and the agent granted `capabilities.cancel`.
  - Its open cards close as `Cancelled` at once: ACP requires the client to answer them, and the adapter turns that into a denial.
  - Nothing changes the line optimistically. It reads `Stopped` when the agent reports `idle`/`cancelled`, because the adapter says accepting the stop "is no evidence that the child stopped".
  - Cost: a second or so before the line flips.
- **D4. Where Stop lives:** a square icon (`Stop subagent`) on the subagent's card line in the chat and on its line in the Agents tab. The session's own Stop keeps stopping only the session's own work. Cost: none functional; one icon more per live subagent.
- **D5. The transcript panel is a gpui-component `Sheet` from the right**, 480 px wide, with its overlay. `×`, Esc or a click outside closes it. One is open at a time. It opens from the `Open transcript` icon on the subagent's card, or from a click on its line in the Agents tab.
  - Not a pane: a pane would change `workspace.rs`, the conflict hot spot.
  - Cost: it is modal, so you close it to type in the chat. Moving it into a pane later is a workspace change.
- **D6. What the transcript holds:** one append-only Markdown text, under the subagent's status line (`<title> · <status>`) and its checklist. The text holds:
  - `**Prompt**` and each message sent to the subagent;
  - its thinking, as a quote (`> `);
  - each of its tool calls as `→ <title>`, at first sight;
  - its text.

  Its usage, title changes and commands stay dropped. A tool call's later title or status changes do not reach the transcript (the chat shows them). Cost: a stale tool title in the panel.
- **D7. Agents tab lines:**
  - Live subagents come first, in the order announced, with the tool call in progress.
  - Then the 3 most recent finished ones (`MAX_FINISHED`), newest first, then `+N more finished`.
  - Finished labels are `Done`, `Stopped`, `Failed`, `Ended` and `Unknown`; the dot is red for `Failed` and grey for the others.
  - Every older subagent still opens from its card in the chat.
  - Cost: taller cards in busy sessions. The cap is one constant.
- **D8. Restart and restore clear the session's subagents before the replay.** claude-agent-acp replays the children (`subagent-replay.js`). Without the clear, transcripts and lines would double. A subagent the replay does not bring back leaves the Agents tab. Cost: losing the lines of a crashed run's subagents.
- **D9. A stale `×` on a queued message does nothing.** The handler remembers the queue's length when it was drawn and ignores the click if the queue changed since. This also fixes M9: two `×` in one frame can no longer both apply. Cost: a click that lands right as the queue changes has to be repeated.
- **D10. Won't fix:** Onda 1 minor "copy-message writes `""` when its row is gone". gpui-component's `Clipboard` always writes what `value_fn` returns. Avoiding that means replacing the component (and losing its copied ✓) or cloning every message's text on every frame. The row disappears between a frame and a click only when the session reloads. Cost: an empty clipboard in that race.

## Dropped, with the reason

- **Minor "copy-message shows on a still-streaming or empty row"** (Task 1): planner decision D9 of Onda 1 ("always visible"). It is by design, not a defect.
- **Minor "`status_icon` InProgress/Pending use theme tokens, not Aurora"** (Task 2): already done. The chat redesign maps them to `aurora::nevoa()` and `aurora::nevoa_2()` in dark mode (`render.rs` `status_icon`).
- **Minor "copy-message writes `""` if the row is gone"**: kept as a known gap, D10.
- **3.1 stays in:** stopping one subagent is possible over ACP v1 on 0.103.0. wai declares `subagents: {}`, which selects the adapter's RFD form. That form grants `capabilities.cancel` per child and routes `session/cancel` for a child id to `stopTask` without cancelling the parent. No invented message.

## Global Constraints

From `/Users/guilherme.brito/Project/Guilherme/wai/.superpowers/sdd/master-onda-1/global-constraints.md`, updated:

- Repo `/Users/guilherme.brito/Project/Guilherme/wai`, Rust workspace, GPUI + gpui-component, ACP. Base: branch `development` at `a818cc8` (0.6.2).
- Every shell: `export PATH="$HOME/.cargo/bin:$PATH" _ZO_DOCTOR=0`.
- Disk is tight: run `df -h /System/Volumes/Data` before each build. Under 8 GB free, stop and report. Run `cargo clean` in the worktree once the track is done, after the fix wave.
- Never `cargo run` with a fake HOME. Tests use temp dirs, never the developer's real config. Real Claude only in `#[ignore]` smoke tests.
- Do NOT touch `/Applications/wai.app` or the running `wai`. Never kill `wai`, `claude-agent-acp` or `claude` processes you did not start.
- Another session may also commit to this repo: run `git log --oneline -5 development` before branching or merging.
- Idle CPU < 0.5%, streaming CPU < 10%. A subagent's streamed words must not repaint the chat. They reach an open transcript on the existing 100 ms flush (`STREAM_FLUSH`) and wake nothing when no transcript is open.
- UI strings in English. Aurora tokens: `docs/superpowers/specs/2026-11-09-wai-visual-aurora-design.md`.
- Never break existing test ids (`tab-*`, `sidebar-*`, `new-session`, `send`, `stop`, `perm-*`, `question-*`, `field-<key>`, `("agent-card", i)`, `("tool", row)`, `("step", row)`, `("step-output", row)`, `("thought", row)`, `("queued", i)`, `("queued-remove", i)`, `waiting`, `transcript`).
- Follow existing patterns. No unrequested abstractions. Comments only for a non-obvious why.
- **Hands off (the self-update agent's):** `crates/wai/src/update.rs`, the Updates section of `settings_view.rs`, the quit-confirmation path in `workspace.rs`, `.github/workflows/*`, `CLAUDE.md`. This track changes nothing in `workspace.rs`.
- **Git:** worktree `.claude/worktrees/onda2-chat` on branch `feat/onda2-chat`, from `development`. If the controller names them differently, its names win. A remote `origin` exists: still **no push**, and no merge into `development` (the controller merges, chat first). Commits are `feat:`/`fix:`/`test:`/`docs:` with no scope, no Co-Authored-By, no Jira ref; imperative; first line < 72 chars.
- **Owner's pace:**
  - Per task: write its tests and code, run only `cargo check --workspace --all-targets`, then commit. No `cargo test`, no clippy, no per-task review.
  - Once, after Task 8: the full gate (Track finish), one final whole-branch review, and one fix wave for every failing test and every Critical/Important finding, then the gate again.
- **New UI copy, exactly:** `Stop subagent`, `Open transcript`, `Transcript` (the sheet's title), `**Prompt**` and `→ ` (transcript Markdown), `Done`, `Stopped`, `Failed`, `Ended`, `Unknown` (subagent labels), `+N more finished`, and `$<amount>` with two decimals.
- **New test ids:** `session-cost`, `("subagent-stop", row)`, `("open-transcript", row)`, `subagent-transcript` (the panel's body), `("agents-subagent", k)`, `("agents-subagent-stop", k)`, `("thought-output", row)`. `k` indexes `model.subagents`. gpui-component's sheet surface is `sheet-content`.
- **User guide:** each task updates the `docs/user-guide.md` lines it names.

## Review Focus

1. **A usage update with no cost after one with a cost** (Claude's mid-turn and compaction updates): the pill keeps the last cost and does not vanish. Tests: Task 1 `usage_keeps_the_last_cost` and `header_shows_the_conversation_cost`.
2. **A file that mixes CRLF and LF line ends:** Revert refuses (`CHANGED`) and never writes a half-converted file. Test: Task 2 `revert_refuses_mixed_line_ends`.
3. **A click on a subagent's line or on its Stop in the Agents tab:** it does only that. It must not also show the card's session, and Stop must not also open the transcript. Test: Task 7 `agents_tab_line_opens_the_transcript_and_stops_it`.
4. **Restart or restore of a session that ran subagents:** the replay must not double their transcripts or lines. Test: Task 4 `restart_clears_the_subagents`.
5. **Stop on a subagent that finished meanwhile, or whose agent grants no `cancel`:** no button is shown and nothing is sent. Tests: Task 4 `stoppable_needs_a_live_subagent_that_allows_it`; Task 5 asserts the button is gone once `Stopped`.

---

## File map

| File | Responsibility in this track |
|---|---|
| `crates/wai-agent/src/types.rs` | `SubagentPatch.can_stop`, `SubagentPart`, `SessionEvent::SubagentSaid`, `SessionEvent::SubagentPlan` |
| `crates/wai-agent/src/convert.rs` | `can_stop` from `capabilities`; a child's incoming message text |
| `crates/wai-agent/src/subagents.rs` | route a child's prompt, text, thinking and plan to the root |
| `crates/wai-agent/src/actor.rs` | doc line on `AgentHandle::cancel` (a child id stops only that child) |
| `crates/wai-agent/src/bin/wai-fake-agent.rs` | `--subagent`: grant `cancel`, send the child's prompt and a late text, honour a child cancel |
| `crates/wai-agent/tests/fake_agent.rs` | the routed traffic; cancelling one subagent |
| `crates/wai/src/chat/model.rs` | cost kept; `Subagent` transcript, plan, `can_stop`; `stoppable` |
| `crates/wai/src/chat/render.rs` | cost pill; Stop and Open transcript on the subagent card; minors |
| `crates/wai/src/chat/edits.rs` | Revert through Claude's view of a file |
| `crates/wai/src/editor/mod.rs` | re-export `line_ends` |
| `crates/wai/src/chat/transcript.rs` (new) | `SubagentTranscript`, the sheet's view |
| `crates/wai/src/chat/view.rs` | `stop_subagent`, `open_transcript`, `sync_transcript`, `load` clears subagents, the flush; tests |
| `crates/wai/src/chat/mod.rs` | `mod transcript;`, export `Subagent` |
| `crates/wai/src/agents_view.rs` | finished lines, `listed`, `subagent_status`, line click and Stop, `plan_row` shared |
| `docs/user-guide.md` | the lines each task names |

Order: Tasks 1, 2 and 8 stand alone. Task 3 feeds Task 4, which feeds Tasks 5 and 6. Task 7 uses 5 and 6.

---

### Task 1: The conversation's cost in the header (2.4)

**Files:**
- Modify: `crates/wai/src/chat/model.rs` (`SessionEvent::Usage` arm of `apply`, ~line 459)
- Modify: `crates/wai/src/chat/render.rs` (`usage_label` neighbourhood; `render_header`, ~line 314)
- Test: `crates/wai/src/chat/model.rs`, `crates/wai/src/chat/render.rs`, `crates/wai/src/chat/view.rs` (tests modules)
- Docs: `docs/user-guide.md`, "**The header**" bullet

**Interfaces:**
- Consumes: `UsageInfo.cost_usd: Option<f64>` (exists), `aurora::pill(cx) -> Div`.
- Produces: `fn cost_label(usd: f64) -> String` in `render.rs`; element id `session-cost` whose aria label is the shown text.

- [ ] **Step 1: Write the tests**

`model.rs` tests:

```rust
/// Review focus 1: Claude reports the cost only at a turn's end; its other usage updates carry
/// none and must not hide it.
#[test]
fn usage_keeps_the_last_cost() {
    let mut m = ChatModel::default();
    let usage = |cost_usd| SessionEvent::Usage {
        used: 1,
        size: 2,
        cost_usd,
        rate_limit: None,
    };
    m.apply(&usage(Some(0.42)));
    m.apply(&usage(None));
    assert_eq!(m.usage.as_ref().unwrap().cost_usd, Some(0.42));
    m.apply(&usage(Some(0.5)));
    assert_eq!(m.usage.unwrap().cost_usd, Some(0.5));
}
```

`render.rs` tests (add `cost_label` to the `use super::{…}` line):

```rust
#[test]
fn cost_reads_in_dollars() {
    assert_eq!([0.0, 0.4237, 12.3].map(cost_label), ["$0.00", "$0.42", "$12.30"]);
}
```

`view.rs` tests (add `SessionEvent` to the `use wai_agent::{…}` line; `pill` is the existing helper):

```rust
/// 2.4: the header shows what the conversation cost, as the agent reports it.
#[gpui_kit::test]
fn header_shows_the_conversation_cost(cx: &mut TestAppContext) {
    cx.executor().allow_parking(); // the fake agent is a real process
    let (workspace, cx) = open(cx, fake_settings(""));
    let session = start_session(&workspace, cx);
    let usage = |cost_usd| SessionEvent::Usage {
        used: 43_000,
        size: 1_000_000,
        cost_usd,
        rate_limit: None,
    };
    assert_eq!(pill("session-cost", cx), None, "no cost reported yet");
    session.update(cx, |s, cx| s.handle_event(&usage(Some(0.4237)), cx));
    assert_eq!(pill("session-cost", cx).as_deref(), Some("$0.42"));
    session.update(cx, |s, cx| s.handle_event(&usage(None), cx));
    assert_eq!(pill("session-cost", cx).as_deref(), Some("$0.42"));
}
```

- [ ] **Step 2: Implement**
  - `model.rs`: in the `Usage` arm, `cost_usd: cost_usd.or(self.usage.as_ref().and_then(|u| u.cost_usd))`.
  - `render.rs`: `fn cost_label(usd: f64) -> String` returns `format!("${usd:.2}")`.
  - `render_header`: right after the usage pill, when `self.model.usage` has a `cost_usd`, add `aurora::pill(cx).id("session-cost").flex_none().aria_label(label.clone()).child(label).test_support()`. Mark the `.test_support()` with the usual `// Registers the id so tests can find it.`

- [ ] **Step 3: Docs.** In `docs/user-guide.md`, the header bullet becomes: "**The header** shows the session's title (drag it to move the pane), the context used as `used / size`, what the conversation has cost so far as the agent reports it (`$0.42`; it starts again after a Restart), and the close button."

- [ ] **Step 4: Check.** `cargo check --workspace --all-targets`. Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add crates/wai/src/chat/model.rs crates/wai/src/chat/render.rs crates/wai/src/chat/view.rs docs/user-guide.md
git commit -m "feat: show the conversation's cost in the header"
```

---

### Task 2: Revert accepts tab-indented and CRLF files (3.6)

**Files:**
- Modify: `crates/wai/src/chat/edits.rs` (`undo`, ~line 191; new private helpers beside `only_on_lines`; module doc "Revert rules")
- Modify: `crates/wai/src/editor/mod.rs` (`pub use file::{line_ends, load, save};`)
- Test: `crates/wai/src/chat/edits.rs` tests
- Docs: `docs/user-guide.md`, "Revert refuses in these cases"

**Interfaces:**
- Consumes: `editor::line_ends(text: &str, crlf: bool) -> String` (exists in `editor/file.rs`; re-exported here); `only_on_lines(hay, needle) -> Option<usize>` (exists).
- Produces (private to `edits.rs`):
  - `fn as_claude_shows(text: &str) -> String`
  - `fn as_file_writes(text: &str, tabs: bool, crlf: bool) -> String`
  - `fn file_offset(now: &str, seen: &str, at: usize) -> usize`
  - `fn as_shown(now: &str, old: &str, new: &str) -> Option<Undo>`

- [ ] **Step 1: Write the tests** in `edits.rs` tests (the helper `undo_snippet` exists):

```rust
/// Claude's hunks show each leading tab as two spaces: the hunk still finds its place, and the
/// old lines go back with tabs.
#[test]
fn snippet_revert_reads_tabs_as_claude_shows_them() {
    let now = "fn f() {\n\tif x {\n\t\tb()\n\t}\n}\n";
    assert_eq!(
        undo_snippet(now, Some("  if x {\n    a()\n  }"), "  if x {\n    b()\n  }"),
        Ok("fn f() {\n\tif x {\n\t\ta()\n\t}\n}\n".into())
    );
    // An odd space stays a space.
    assert_eq!(
        undo_snippet("\tb\n", Some("   a"), "  b"),
        Ok("\t a\n".into())
    );
}

/// Claude's hunks have LF line ends: a CRLF file matches them and keeps its CRLF.
#[test]
fn snippet_revert_keeps_crlf() {
    let hunk = (Some("a\nb\nc"), "a\nb2\nc");
    assert_eq!(
        undo_snippet("top\r\na\r\nb2\r\nc\r\nend\r\n", hunk.0, hunk.1),
        Ok("top\r\na\r\nb\r\nc\r\nend\r\n".into())
    );
    assert_eq!(
        undo_snippet("x\r\n\tnew\r\ny\r\n", Some("x\n  old\ny"), "x\n  new\ny"),
        Ok("x\r\n\told\r\ny\r\n".into()),
        "tabs and CRLF together"
    );
    // A needle that ends with its line break.
    assert_eq!(
        undo_snippet("k\r\nb2\r\n", Some("b\n"), "b2\n"),
        Ok("k\r\nb\r\n".into())
    );
}

/// Review focus 2: a file that mixes line ends could not have been written by Claude's edit as
/// it reported it.
#[test]
fn revert_refuses_mixed_line_ends() {
    assert_eq!(
        undo_snippet("a\r\nb2\nc\r\n", Some("a\nb\nc"), "a\nb2\nc"),
        Err(RevertError::Changed)
    );
    // A real change in a CRLF file is still a change.
    assert_eq!(
        undo_snippet("a\r\nb3\r\nc\r\n", Some("a\nb\nc"), "a\nb2\nc"),
        Err(RevertError::Changed)
    );
}

/// The whole file, through the disk: a CRLF file comes back CRLF.
#[test]
fn revert_puts_back_a_crlf_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("win.txt");
    fs::write(&path, "a\r\nb2\r\n").unwrap();
    revert(&diff(&path, Some("a\nb\n"), "a\nb2\n"), false).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "a\r\nb\r\n");
}
```

- [ ] **Step 2: Re-export `line_ends`** from `crates/wai/src/editor/mod.rs`.

- [ ] **Step 3: Implement the fallback** in `edits.rs`:
  - `as_claude_shows`: CRLF becomes LF. Then each line's leading run of tabs becomes two spaces per tab, as Claude Code's own `/^\t+/gm` → `"  ".repeat(n)`.
  - `as_file_writes`: when `tabs`, each line's leading run of `n` spaces becomes `n / 2` tabs, then `n % 2` spaces. Then `editor::line_ends(.., crlf)`.
  - `file_offset` maps an offset of `seen = as_claude_shows(now)` that starts or ends a line, as `only_on_lines` guarantees, to the same place in `now`. Both texts have the same lines:

    ```rust
    fn file_offset(now: &str, seen: &str, at: usize) -> usize {
        let line = seen[..at].matches('\n').count();
        let begin: usize = now.split_inclusive('\n').take(line).map(str::len).sum();
        if at == 0 || seen.as_bytes()[at - 1] == b'\n' {
            return begin;
        }
        let text = now[begin..].split_inclusive('\n').next().unwrap_or("");
        let text = text.strip_suffix('\n').unwrap_or(text);
        begin + text.strip_suffix('\r').unwrap_or(text).len()
    }
    ```

  - `as_shown(now, old, new)`:
    1. `crlf = now.contains("\r\n")`. If `crlf` and `now.replace("\r\n", "")` still contains `'\n'` (mixed line ends), return `None`.
    2. `tabs = now.lines().any(|l| l.starts_with('\t'))`. Neither `crlf` nor `tabs`: return `None`.
    3. Let `seen`, `new`, `old` be `as_claude_shows` of each. If `seen == new`, return `Whole(as_file_writes(&old, tabs, crlf))`.
    4. `at = only_on_lines(&seen, &new)?`. Map `at` and `at + new.len()` to `a` and `b` with `file_offset`. Return `Snippet(now[..a] + as_file_writes(&old, tabs, crlf) + now[b..])`.
  - `undo`: keep the exact checks first. Where `only_on_lines(now, new)` fails today, return `as_shown(now, old, new).ok_or(RevertError::Changed)`.
  - Module doc, "Revert rules", second bullet: add "A file with tabs or CRLF is compared as Claude reports it (leading tabs as two spaces, LF line ends), and the old text goes back in the file's own form."

- [ ] **Step 4: Docs.** In `docs/user-guide.md`, replace "the file is tab-indented or uses CRLF line endings." with "the file mixes CRLF and LF line endings." After the list, add: "In a tab-indented file, the lines Revert puts back are indented with tabs: Claude reports each tab as two spaces."

- [ ] **Step 5: Check.** `cargo check --workspace --all-targets`. Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add crates/wai/src/chat/edits.rs crates/wai/src/editor/mod.rs docs/user-guide.md
git commit -m "fix: revert files indented with tabs or with CRLF line ends"
```

---

### Task 3 (DEFERRED, do not execute): A subagent's transcript and its stop right reach wai (3.1, 3.2, protocol side)

**Files:**
- Modify: `crates/wai-agent/src/types.rs` (`SubagentPatch`, `SessionEvent`)
- Modify: `crates/wai-agent/src/convert.rs` (`subagent_patch`, `subagent_reply`)
- Modify: `crates/wai-agent/src/subagents.rs` (`route`; the module doc)
- Modify: `crates/wai-agent/src/actor.rs` (doc of `AgentHandle::cancel`)
- Modify: `crates/wai-agent/src/bin/wai-fake-agent.rs` (`subagent_start`, `subagent_finish`, the header doc of `--subagent`)
- Modify: `crates/wai/src/chat/model.rs` (compile only: `can_stop: None` in `on_exited`'s patch and the tests' `subagent` helper; a `None` arm for the two new events)
- Test: `crates/wai-agent/src/subagents.rs` tests, `crates/wai-agent/tests/fake_agent.rs`

**Interfaces:**
- Produces (public, `wai_agent::*`):

```rust
/// What a subagent said, for its transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentPart {
    /// A message sent to it: its prompt, or a later one from its parent.
    Prompt,
    Text,
    Thought,
}

// in `SessionEvent`:
/// A subagent's prompt, text or thinking, a chunk at a time.
SubagentSaid { id: SessionId, part: SubagentPart, text: String },
/// A subagent's checklist, replaced whole.
SubagentPlan { id: SessionId, entries: Vec<PlanEntry> },

// in `SubagentPatch`:
/// The agent lets the client stop it alone (`capabilities.cancel`); `None` means unchanged.
pub can_stop: Option<bool>,
```

- Produces (crate-private): `convert::subagent_prompt(m: acp::SessionMessage, session: &str) -> Option<String>`.
- Unchanged and relied on by Task 5: `AgentHandle::cancel(session: SessionId)` with a child id answers the child's parked requests `cancelled` (`Broker::cancel_pending` matches the session that asked) and sends `session/cancel` for that id.

- [ ] **Step 1: Write the tests**

`subagents.rs` tests:
- In `subagent_updates_route_to_the_root`, the expected patch gains `can_stop: None`.
- Its two `None` asserts become:

```rust
let said = |part, text: &str| {
    Some((
        "r".to_string(),
        SessionEvent::SubagentSaid { id: "c".into(), part, text: text.into() },
    ))
};
assert_eq!(s.route("c".into(), agent_chunk("x")), said(SubagentPart::Text, "x"));
let thought = acp::SessionUpdate::AgentThoughtChunk(acp::ContentChunk::new(
    acp::ContentBlock::Text(acp::TextContent::new("hm")),
));
assert_eq!(s.route("c".into(), thought), said(SubagentPart::Thought, "hm"));
assert_eq!(
    s.route("c".into(), plan()),
    Some((
        "r".into(),
        SessionEvent::SubagentPlan {
            id: "c".into(),
            entries: vec![PlanEntry { content: "Sub step".into(), status: "pending".into() }],
        }
    ))
);
// Its prompt: a message to it from another session. Its own outgoing message is not one.
let message = |from: &str, to: &str| {
    acp::SessionUpdate::SessionMessage(
        acp::SessionMessage::new("p")
            .sender_session_id(acp::SessionId::new(from))
            .recipient_session_id(acp::SessionId::new(to))
            .content(vec![acp::ContentBlock::Text(acp::TextContent::new("Look"))]),
    )
};
assert_eq!(s.route("c".into(), message("r", "c")), said(SubagentPart::Prompt, "Look"));
assert_eq!(s.route("c".into(), message("c", "g")), None);
```

In `states_and_replies_map`, the bare patch gains `can_stop: None`. Add:

```rust
let can_stop = |caps| convert::subagent_patch(acp::SubagentUpdate::new("c").capabilities(caps)).can_stop;
let cancel = acp::SubagentSessionCapabilities::new().cancel(acp::SessionCancelCapabilities::new());
assert_eq!(can_stop(cancel), Some(true));
assert_eq!(can_stop(acp::SubagentSessionCapabilities::new()), Some(false));
```

`tests/fake_agent.rs` (add `SubagentPart` to its `use wai_agent::{…}` line):
- In `subagent_traffic_reaches_the_parent`, the `patch` closure gains `can_stop: Some(true)`. Its doc becomes "…its announcement, its stop right and states, its tool call tagged with it, its prompt, text and plan, its reply". Add:

```rust
let said = |part, text: &str| SessionEvent::SubagentSaid {
    id: sub.clone(),
    part,
    text: text.into(),
};
for want in [
    said(SubagentPart::Prompt, "Look at lib.rs"),
    said(SubagentPart::Text, "sub text"),
    said(SubagentPart::Text, " and done"),
] {
    assert!(updates.contains(&&want), "{want:?} not in {updates:?}");
}
assert!(updates.iter().any(|e| matches!(e,
    SessionEvent::SubagentPlan { id, entries } if *id == sub && entries.len() == 1)));
```

- New test:

```rust
/// 3.1: `cancel` with a subagent's id stops only it: its open card closes, it reports
/// `Stopped`, and the session's own turn runs to its end.
#[test]
fn cancelling_a_subagent_stops_only_it() {
    run(async {
        let reply = vec!["word"; 50].join(" ");
        let (agent, rx) = spawn(&[
            "--subagent", "--background", "--ask-permission",
            "--reply", &reply, "--chunk-delay-ms", "20",
        ]);
        ready(&rx).await;
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
        agent.prompt(session.id.clone(), hello());
        let request = permission(&rx).await;
        agent.cancel(request.subagent.clone().expect("the subagent asked"));
        let (mut resolved, mut stopped, mut ended) = (false, false, None);
        until(&rx, |e| {
            match e {
                AgentEvent::PermissionResolved { id } if *id == request.id => resolved = true,
                AgentEvent::TurnEnded { reason, .. } => ended = Some(reason.clone()),
                e if subagent_state(e, SubagentState::Stopped) => stopped = true,
                _ => {}
            }
            resolved && stopped && ended.is_some()
        })
        .await;
        assert_eq!(ended, Some(StopReason::EndTurn), "the session's turn went on");
    });
}
```

- [ ] **Step 2: Implement `wai-agent`**
  - `types.rs`: the items in Interfaces. `SubagentPatch` keeps its ponytail note.
  - `convert.rs`:
    - `subagent_patch` sets `can_stop` from `u.capabilities`: `Value(c)` → `Some(c.cancel.is_some())`, `Null` → `Some(false)`, `Undefined` → `None`.
    - Factor the "sender is another session, text blocks joined by `\n`" part of `subagent_reply` into a private helper returning `(sender id, text)`. `subagent_reply` keeps its `<tools-used-before-summary>` strip. `subagent_prompt` returns only the text.
  - `subagents.rs`:
    - In `route`, add an arm before `(update, Some(root))`: `(U::SessionMessage(m), Some(root))` → `convert::subagent_prompt(m, &session)` as `SubagentSaid { part: Prompt }` for `root`.
    - In the child branch, map `AgentText` and `AgentThought` to `SubagentSaid` (`Text`, `Thought`) and `Plan(entries)` to `SubagentPlan`, all with `id: session`.
    - The ponytail line becomes "a subagent's usage, title, modes and commands are not shown." Update the module doc's first sentence to match.
  - `actor.rs`: doc of `cancel`: "`session` may be a subagent's id (one the agent granted `cancel`): only that subagent stops, and only its own open requests are answered `cancelled`."

- [ ] **Step 3: Implement the fake agent** (and its header doc for `--subagent`):
  - **Announce:** add `.capabilities(acp::SubagentSessionCapabilities::new().cancel(acp::SessionCancelCapabilities::new()))`.
  - **Prompt:** right after the announcement, send on the child a `SessionMessage` `sub-prompt` from the parent to the child with the text `Look at lib.rs`.
  - **`subagent_finish`, stopped path:** after the answer, or after the background wait, check whether the child was stopped. It was when `is_cancelled(&sub.child)`. After an answer ending in `:cancelled`, it also was if that becomes true within 1 s, checked every 10 ms. A stopped child completes `sub-read` as `Failed`, sends `idle` with `IdleStopReason::Cancelled` on the parent, and sends no reply.
  - **`subagent_finish`, otherwise:** as today, but first send the child text ` and done` (an `AgentMessageChunk` on the child).

- [ ] **Step 4: Keep `wai` compiling** in `crates/wai/src/chat/model.rs`:
  - add `can_stop: None` to the patch in `on_exited` and in the tests' `subagent` helper;
  - add the arm `SessionEvent::SubagentSaid { .. } | SessionEvent::SubagentPlan { .. } => None` to `apply` (Task 4 replaces it).

- [ ] **Step 5: Check.** `cargo check --workspace --all-targets`. Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add crates/wai-agent crates/wai/src/chat/model.rs
git commit -m "feat: route subagent transcripts and stop rights to the session"
```

---

### Task 4 (DEFERRED, do not execute): Each subagent keeps its transcript, checklist and stop right

**Files:**
- Modify: `crates/wai/src/chat/model.rs` (`Subagent`, `patch_subagent`, `apply`, `subagent_tool`, `on_exited`)
- Modify: `crates/wai/src/chat/view.rs` (`handle_event`, `load`)
- Modify: `crates/wai/src/chat/mod.rs` (`pub use model::{…, Subagent, …}`)
- Test: `crates/wai/src/chat/model.rs` tests, `crates/wai/src/chat/view.rs` tests

**Interfaces:**
- Consumes (Task 3): `SessionEvent::SubagentSaid { id, part, text }`, `SessionEvent::SubagentPlan { id, entries }`, `SubagentPatch.can_stop`, `SubagentPart`.
- Produces:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Subagent {
    pub id: String,
    pub title: String,
    pub state: SubagentState,
    /// The agent lets wai stop it alone (`capabilities.cancel`).
    pub can_stop: bool,
    /// Its checklist, replaced whole.
    pub plan: Vec<PlanEntry>,
    /// What it was sent, thought, ran and said, as Markdown. Append-only: the transcript panel
    /// pushes only its new tail.
    pub transcript: String,
    /// The part the transcript ends in while it can still grow; `None` after a prompt or a step.
    last: Option<SubagentPart>,
}
impl Subagent {
    pub fn new(id: String, title: String, state: SubagentState) -> Self; // can_stop false, empty rest
    /// Running, or waiting on the user.
    pub fn is_live(&self) -> bool;
}
impl ChatModel {
    pub fn subagent(&self, id: &str) -> Option<&Subagent>;
    /// Whether wai can stop subagent `id` now: it is live and its agent allows it.
    pub fn stoppable(&self, id: &str) -> bool;
}
```

- [ ] **Step 1: Write the tests**

`model.rs` tests (add `SubagentPart` to the `use wai_agent::{…}` line):

```rust
fn said(id: &str, part: SubagentPart, text: &str) -> SessionEvent {
    SessionEvent::SubagentSaid { id: id.into(), part, text: text.into() }
}

/// 3.2: the transcript reads as Markdown: the prompt under a bold head, thinking quoted, each
/// tool call on its own line, streamed chunks joined.
#[test]
fn subagent_transcript_reads_as_markdown() {
    let mut m = ChatModel::default();
    m.apply(&subagent("c", Some("Explore"), Some(SubagentState::Running)));
    m.apply(&said("c", SubagentPart::Prompt, "Find the bug"));
    m.apply(&said("c", SubagentPart::Thought, "Looking\naround"));
    m.apply(&said("c", SubagentPart::Thought, " more"));
    m.apply(&call("t1", "Read lib.rs", ToolStatus::InProgress, Some("c")));
    m.apply(&said("c", SubagentPart::Text, "Found "));
    m.apply(&said("c", SubagentPart::Text, "it"));
    assert_eq!(
        m.subagents[0].transcript,
        "**Prompt**\n\nFind the bug\n\n> Looking\n> around more\n\n→ Read lib.rs\n\nFound it"
    );
    // The session's own call and an unknown subagent's words touch no transcript.
    m.apply(&call("t2", "Run ls", ToolStatus::InProgress, None));
    assert_eq!(m.apply(&said("x", SubagentPart::Text, "?")), None);
    assert!(m.subagents[0].transcript.ends_with("Found it"));
}

#[test]
fn subagent_plan_and_stop_right_follow_the_agent() {
    let mut m = ChatModel::default();
    m.apply(&subagent("c", Some("Explore"), Some(SubagentState::Running)));
    assert!(!m.subagents[0].can_stop);
    let entries = vec![PlanEntry { content: "Sub step".into(), status: "pending".into() }];
    m.apply(&SessionEvent::SubagentPlan { id: "c".into(), entries: entries.clone() });
    assert_eq!(m.subagents[0].plan, entries);
    assert!(m.plan.is_empty(), "not the session's plan");
    m.apply(&SessionEvent::Subagent(SubagentPatch {
        id: "c".into(),
        title: None,
        state: None,
        can_stop: Some(true),
    }));
    assert!(m.subagents[0].can_stop);
}

/// Review focus 5.
#[test]
fn stoppable_needs_a_live_subagent_that_allows_it() {
    let mut m = ChatModel::default();
    let patch = |state, can_stop| SessionEvent::Subagent(SubagentPatch {
        id: "c".into(),
        title: None,
        state: Some(state),
        can_stop,
    });
    m.apply(&patch(SubagentState::Running, None));
    assert!(!m.stoppable("c"), "no cancel granted");
    m.apply(&patch(SubagentState::NeedsYou, Some(true)));
    assert!(m.stoppable("c"));
    m.apply(&patch(SubagentState::Done, None));
    assert!(!m.stoppable("c"), "finished");
    assert!(!m.stoppable("nobody"));
}
```

In `subagent_gets_a_card_and_a_line`, the expected array becomes `[Subagent::new("c".into(), "Explore".into(), SubagentState::Running)]`.

`view.rs` tests:

```rust
/// Review focus 4: Restart replays the session, and the agent replays its subagents: the old
/// ones go first, so nothing doubles.
#[gpui_kit::test]
fn restart_clears_the_subagents(cx: &mut TestAppContext) {
    cx.executor().allow_parking(); // the fake agent is a real process
    let (workspace, cx) = open(cx, fake_settings("--subagent"));
    let session = start_session(&workspace, cx);
    send("hi", cx);
    wait_until(cx, "the subagent's run", |cx| {
        session.read_with(cx, |s, _| {
            s.model.state == TurnState::Idle && !s.model.subagents.is_empty()
        })
    });
    cx.update(|window, cx| session.update(cx, |s, cx| s.load(false, window, cx)));
    wait_until(cx, "the replay", |cx| state(&session, cx) == TurnState::Idle);
    assert!(session.read_with(cx, |s, _| s.model.subagents.is_empty()));
}
```

- [ ] **Step 2: Implement**
  - `model.rs`:
    - **`Subagent`:** fields and methods as in Interfaces, plus a private `fn say(&mut self, part: Option<SubagentPart>, text: &str)`, where `None` is a tool call's title.
    - **`say`:** a part starts anew when it is a prompt, a tool line, or differs from `last`. When it starts anew and the transcript is not empty, it pushes `\n\n`. Then:
      - Prompt pushes `**Prompt**\n\n` and the text;
      - Thought pushes `> ` (only when it starts anew) and the text with each `\n` as `\n> `;
      - Text pushes the text;
      - a tool line pushes `→ ` and the title.

      `last` becomes `part` for Text and Thought, and `None` for a prompt or a tool line.
    - **`patch_subagent`:** builds new lines with `Subagent::new`, and sets `can_stop` from `patch.can_stop` when it is `Some`.
    - **`apply`:**
      - `SubagentSaid` calls `say(Some(part), text)` on the known subagent and returns `None`;
      - `SubagentPlan` replaces its `plan` and returns `None`;
      - a `ToolCall` with `subagent: Some(id)` also calls `say(None, &v.title)` on that subagent.
    - `subagent_tool` and `on_exited` use `is_live`.
  - `view.rs`:
    - `handle_event`: right after `update_model(.. apply ..)`, a `SubagentSaid` event returns without `cx.notify()`. A subagent's words repaint nothing until Task 6 shows them.
    - `load`: add `self.model.subagents.clear();` beside `self.model.items.clear();` (D8).
  - `mod.rs`: export `Subagent`.

- [ ] **Step 3: Check.** `cargo check --workspace --all-targets`. Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add crates/wai/src/chat/model.rs crates/wai/src/chat/view.rs crates/wai/src/chat/mod.rs
git commit -m "feat: keep each subagent's transcript, checklist and stop right"
```

---

### Task 5 (DEFERRED, do not execute): Stop one subagent from its card (3.1)

**Files:**
- Modify: `crates/wai/src/chat/view.rs` (new `stop_subagent` beside `stop`)
- Modify: `crates/wai/src/chat/render.rs` (`render_tool`: the subagent card's line)
- Test: `crates/wai/src/chat/view.rs` tests
- Docs: `docs/user-guide.md`, "**A subagent's card**" bullet list under "Permissions and questions"

**Interfaces:**
- Consumes (Task 4): `ChatModel::stoppable(&self, id: &str) -> bool`. Existing: `AgentHandle::cancel(SessionId)`.
- Produces: `pub fn stop_subagent(&self, id: &str)` on `ChatSession`; element id `("subagent-stop", row)`.

- [ ] **Step 1: Write the test** in `view.rs` tests:

```rust
/// 3.1: a background subagent waiting on its card, after the turn, stops from its own card: the
/// card closes, the subagent reads Stopped, and the session is idle.
#[gpui_kit::test]
fn a_subagent_stops_from_its_card(cx: &mut TestAppContext) {
    cx.executor().allow_parking(); // the fake agent is a real process
    let (workspace, cx) = open(cx, fake_settings("--subagent --background --ask-permission"));
    let session = start_session(&workspace, cx);
    let sub = session.read_with(cx, |s, _| format!("{}-sub", s.id));
    send("edit", cx);
    wait_until(cx, "the subagent to wait on its card", |cx| {
        session.read_with(cx, |s, _| s.model.done && s.model.stoppable(&sub))
    });
    let row = session.read_with(cx, |s, _| {
        s.model.items.iter().position(|i| matches!(i, ChatItem::Tool(t) if t.id == sub))
    });
    let row = row.expect("the subagent's card");
    click_edits(None, ("subagent-stop", row), cx);
    wait_until(cx, "the subagent to stop", |cx| {
        session.read_with(cx, |s, _| {
            s.model.subagents.iter().any(|a| a.id == sub && a.state == SubagentState::Stopped)
        })
    });
    assert!(session.read_with(cx, |s, _| s.model.waiting.is_empty()), "its card closed");
    assert_eq!(state(&session, cx), TurnState::Idle);
    assert!(!shows(("subagent-stop", row), cx), "nothing left to stop");
}
```

- [ ] **Step 2: Implement**
  - `view.rs`: `stop_subagent` calls `self.handle.cancel(id.to_string())` only when `self.model.stoppable(id)`. It sets nothing optimistically (D3).
  - `render.rs`: in `render_tool`, when `tool.kind == "subagent"` and `self.model.stoppable(&tool.id)`, add after the title a `Button::new(("subagent-stop", ix)).ghost().xsmall().icon(IconName::Square).tooltip("Stop subagent").accessibility_label("Stop subagent")`. Its `on_click` calls `cx.stop_propagation()` first (the line's own click unfolds it), then `this.stop_subagent(&id)`.

- [ ] **Step 3: Docs.** Under "**A subagent's card**", after "**Stop** cancels only the session's own cards.", add: "**Stopping one subagent:** the square on its card, or on its line in the Agents tab, stops that subagent only, when the agent allows it (Claude Code does). Its open cards close as `Cancelled`, and it reads `Stopped` once the agent confirms."

- [ ] **Step 4: Check.** `cargo check --workspace --all-targets`. Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add crates/wai/src/chat/view.rs crates/wai/src/chat/render.rs docs/user-guide.md
git commit -m "feat: stop one subagent from its card"
```

---

### Task 6 (DEFERRED, do not execute): A subagent's transcript in a side panel (3.2)

**Files:**
- Create: `crates/wai/src/chat/transcript.rs`
- Modify: `crates/wai/src/chat/mod.rs` (`mod transcript;`)
- Modify: `crates/wai/src/chat/view.rs` (field `transcript_panel`; `open_transcript`, `sync_transcript`; `handle_event`; the flush in `schedule_flush`; an empty-delta guard in `sync_row`)
- Modify: `crates/wai/src/chat/render.rs` (`render_tool`: Open transcript on the subagent card)
- Modify: `crates/wai/src/agents_view.rs` (`plan_row` becomes `pub(crate)`; new `subagent_status`)
- Test: `crates/wai/src/chat/view.rs` tests, `crates/wai/src/agents_view.rs` tests
- Docs: `docs/user-guide.md`, the "**Subagents.**" bullet under "The transcript"

**Interfaces:**
- Consumes (Task 4): `ChatModel::subagent(&self, id) -> Option<&Subagent>`, `Subagent.{title, state, plan, transcript}`. gpui-component: `WindowExt::open_sheet(cx, |Sheet, &mut Window, &mut App| -> Sheet)`, `Sheet::{title, size}`, `TextViewState::{markdown, push_str, set_text}`, `TextView::new(&Entity<TextViewState>)`.
- Produces:
  - `pub fn subagent_status(state: SubagentState) -> &'static str` in `agents_view.rs`: `Working`, `Needs you`, `Done`, `Stopped`, `Failed`, `Ended`, `Unknown`.
  - `pub(crate) fn plan_row(entry: &PlanEntry, cx: &App) -> impl IntoElement` (was private).
  - `pub fn open_transcript(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>)` on `ChatSession`.
  - `pub(super) transcript_panel: Option<WeakEntity<SubagentTranscript>>` on `ChatSession`.
  - In `transcript.rs`:

```rust
/// A subagent's transcript in the window's side sheet: its status, its checklist, then what it
/// was sent, thought, ran and said. Its session feeds it on the stream flush.
pub(crate) struct SubagentTranscript {
    session: WeakEntity<ChatSession>,
    pub(super) id: String,
    pub(super) md: Entity<TextViewState>,
    pushed: usize,
}
impl SubagentTranscript {
    pub(super) fn new(session: WeakEntity<ChatSession>, id: String, text: &str, cx: &mut Context<Self>) -> Self;
    /// Takes the transcript's new tail; starts over when it shrank (the session reloaded).
    pub(super) fn sync(&mut self, text: &str, cx: &mut Context<Self>);
}
impl Render for SubagentTranscript { /* below */ }
```

- [ ] **Step 1: Write the tests**

`agents_view.rs` tests:

```rust
#[test]
fn subagent_status_names_every_state() {
    use SubagentState as S;
    let all = [S::Running, S::NeedsYou, S::Done, S::Stopped, S::Failed, S::Ended, S::Unknown];
    assert_eq!(
        all.map(subagent_status),
        ["Working", "Needs you", "Done", "Stopped", "Failed", "Ended", "Unknown"]
    );
}
```

`view.rs` tests:

```rust
/// 3.2: a subagent's card opens its transcript in the side sheet, and the sheet follows what
/// the subagent says next.
#[gpui_kit::test]
fn subagent_transcript_opens_from_its_card_and_follows_it(cx: &mut TestAppContext) {
    cx.executor().allow_parking(); // the fake agent is a real process
    let (workspace, cx) = open(cx, fake_settings("--subagent --background --ask-permission"));
    let session = start_session(&workspace, cx);
    let sub = session.read_with(cx, |s, _| format!("{}-sub", s.id));
    send("edit", cx);
    wait_until(cx, "the subagent to wait on its card", |cx| {
        session.read_with(cx, |s, _| s.model.done && s.model.stoppable(&sub))
    });
    let row = session.read_with(cx, |s, _| {
        s.model.items.iter().position(|i| matches!(i, ChatItem::Tool(t) if t.id == sub))
    });
    click_edits(None, ("open-transcript", row.expect("the subagent's card")), cx);
    assert!(shows("sheet-content", cx));
    assert!(shows("subagent-transcript", cx));
    let text = |cx: &mut VisualTestContext| {
        session.read_with(cx, |s, cx| {
            let panel = s.transcript_panel.as_ref().and_then(|p| p.upgrade());
            let panel = panel.expect("the open panel");
            panel.read(cx).md.read(cx).rendered_text().to_string()
        })
    };
    let first = text(cx);
    for part in ["Prompt", "Look at lib.rs", "→ Read lib.rs", "sub text"] {
        assert!(first.contains(part), "{part:?} not in {first:?}");
    }
    // The sheet's overlay covers the card: answer it as its Allow button does.
    session.update(cx, |s, cx| {
        let Some(ChatItem::Permission { request, .. }) = s.model.waiting.first().cloned() else {
            panic!("the subagent's card");
        };
        let allow = request.options.iter().find(|o| o.kind == ChoiceKind::AllowOnce).cloned();
        s.answer(request.id, &allow.expect("an allow choice"), cx);
    });
    wait_until(cx, "the subagent's last words", |cx| text(cx).contains("sub text and done"));
}
```

- [ ] **Step 2: Implement `transcript.rs`**
  - `new` makes `md` with `TextViewState::markdown(text, cx).selectable(true)`, as `view.rs` `sync_row` does, and `pushed = text.len()`.
  - `sync`:
    - when `text.len() >= pushed` and `pushed` is a char boundary of `text`, push `text[pushed..]` with `push_str`;
    - otherwise `set_text(text)`;
    - then `pushed = text.len()` and `cx.notify()`.
  - `render`: a `v_flex().id("subagent-transcript").gap_2()` with `.test_support()` and the usual comment. While the session and the subagent exist, it holds:
    - a `text_sm` line `format!("{} · {}", title, agents_view::subagent_status(state))`;
    - `plan_row` for each checklist entry;
    - then `TextView::new(&self.md)`.

    Otherwise it holds only the `TextView`.

- [ ] **Step 3: Implement `view.rs`**
  - `transcript_panel: None` in `new`.
  - **`open_transcript`:**
    1. Return when `self.model.subagent(id)` is `None`.
    2. Build `SubagentTranscript::new(cx.weak_entity(), id.into(), &sub.transcript, cx)` in `cx.new`. Take the weak session handle before `cx.new`.
    3. Store its `downgrade()` in `transcript_panel`.
    4. `window.open_sheet(cx, move |sheet, _, _| sheet.title("Transcript").size(px(480.)).child(view.clone()))`.
  - **`sync_transcript(&mut self, cx)`:** upgrade `transcript_panel`; when that fails, set it to `None` and return. Otherwise call `sync` with the subagent's transcript, or `""` when the subagent is gone.
  - **`handle_event`:**
    - The `SubagentSaid` early return (Task 4) now calls `self.schedule_flush(cx)` first, but only when `self.transcript_panel.is_some()`. With no panel, nothing wakes.
    - Every other event calls `self.sync_transcript(cx)` right after the model applied it, before the `None` branch's `return cx.notify()`. That carries state, plan and tool lines.
  - **`schedule_flush`:** after syncing the rows, the closure calls `this.sync_transcript(cx)`.
  - **`sync_row`:** for a row with markdown, return early when `delta` is empty, so a flush that only carries a subagent's words costs the chat nothing.

- [ ] **Step 4: Implement `render.rs`.** In `render_tool`, for `tool.kind == "subagent"`, add after the Stop button (Task 5) a `Button::new(("open-transcript", ix)).ghost().xsmall().icon(IconName::FileText).tooltip("Open transcript").accessibility_label("Open transcript")`. Its `on_click` calls `cx.stop_propagation()`, then `this.open_transcript(&id, window, cx)`.

- [ ] **Step 5: Docs.** The "**Subagents.**" bullet becomes: "Each subagent the agent runs (Claude Code's Agent tool) shows as a step card with its title, its status and, once it answers, its answer. Its tool calls stay in the transcript, as steps. **Open transcript** (the page icon on its card) opens a panel on the right with its status, its checklist, and what it was sent, thought, ran and said; it follows the subagent live. Esc or × closes it."

- [ ] **Step 6: Check.** `cargo check --workspace --all-targets`. Expected: no errors.

- [ ] **Step 7: Commit**

```bash
git add crates/wai/src/chat crates/wai/src/agents_view.rs docs/user-guide.md
git commit -m "feat: open a subagent's transcript in a side panel"
```

---

### Task 7 (DEFERRED, do not execute): The Agents tab lists finished subagents, opens and stops them (3.1, 3.2)

**Files:**
- Modify: `crates/wai/src/agents_view.rs` (`subagent_line`, new `listed` and `MAX_FINISHED`, the subagent rows of `card`)
- Test: `crates/wai/src/agents_view.rs` tests
- Docs: `docs/user-guide.md`, "## The Agents tab"

**Interfaces:**
- Consumes: `chat::Subagent` (Task 4), `ChatModel::{subagent_tool, stoppable}`, `ChatSession::{stop_subagent, open_transcript}` (Tasks 5, 6), `subagent_status` (Task 6).
- Produces:

```rust
/// Most finished subagents a card lists, newest first, after the live ones.
pub const MAX_FINISHED: usize = 3;

/// `{title} · {status}`, then ` · {tool}` while it is live.
pub fn subagent_line(title: &str, state: SubagentState, tool: Option<&str>) -> String;

/// The subagents a card lists, as indices into `subs`: the live ones in the order announced,
/// then up to `MAX_FINISHED` finished ones, newest first; and how many finished ones it leaves out.
pub fn listed(subs: &[Subagent]) -> (Vec<usize>, usize);
```

Element ids `("agents-subagent", k)` and `("agents-subagent-stop", k)`, `k` an index into `model.subagents`.

- [ ] **Step 1: Write the tests**

Replace `subagent_lines_show_only_live_ones` with:

```rust
/// Every subagent has a line; only a live one names its tool call.
#[test]
fn subagent_lines_name_every_state() {
    use SubagentState as S;
    assert_eq!(
        subagent_line("Explore", S::Running, Some("Read lib.rs")),
        "Explore · Working · Read lib.rs"
    );
    assert_eq!(subagent_line("Explore", S::NeedsYou, None), "Explore · Needs you");
    assert_eq!(subagent_line("Explore", S::Done, Some("Read")), "Explore · Done");
    assert_eq!(subagent_line("Explore", S::Stopped, None), "Explore · Stopped");
    assert_eq!(
        summary("t", None, &[], &["a · Working".into()], None),
        "t; a · Working"
    );
}

#[test]
fn listed_puts_live_first_then_the_newest_finished() {
    use SubagentState as S;
    let states = [S::Done, S::Running, S::Failed, S::Stopped, S::NeedsYou, S::Done];
    let subs: Vec<Subagent> = (states.iter().enumerate())
        .map(|(i, s)| Subagent::new(format!("s{i}"), format!("S{i}"), *s))
        .collect();
    assert_eq!(listed(&subs), (vec![1, 4, 5, 3, 2], 1));
    assert_eq!(listed(&subs[..2]), (vec![1, 0], 0));
}
```

In `agents_tab_lists_running_subagents`, rename it `agents_tab_lists_live_and_finished_subagents`. Its doc becomes "…has a line under the session's card; once it finishes, the line says so". Its last assert becomes:

```rust
assert!(text.contains("; Explore the code · Done"), "{text:?}");
```

New test (Review Focus 3):

```rust
/// A subagent's line opens its transcript, and its Stop stops it; neither shows the card's
/// session in the focused pane.
#[gpui_kit::test]
fn agents_tab_line_opens_the_transcript_and_stops_it(cx: &mut TestAppContext) {
    cx.executor().allow_parking(); // the fake agent is a real process
    let (workspace, cx) = open(cx, fake_settings("--subagent --background --ask-permission"));
    new_session(&workspace, 1, cx);
    let first = workspace.read_with(cx, |w, _| w.sessions[0].clone());
    cx.simulate_input("edit");
    cx.simulate_keystrokes("enter");
    let sub = first.read_with(cx, |s, _| format!("{}-sub", s.id));
    wait_until(cx, "the subagent to wait on its card", |cx| {
        first.read_with(cx, |s, _| s.model.done && s.model.stoppable(&sub))
    });
    new_session(&workspace, 2, cx);
    let second = workspace.read_with(cx, |w, _| w.sessions[1].clone());
    cx.update(|window, cx| {
        workspace.update(cx, |w, cx| w.show(second.clone(), false, window, cx))
    });
    let focused = |cx: &mut VisualTestContext| {
        workspace.read_with(cx, |w, _| w.panes[w.focused_pane].session().cloned())
    };
    click("tab-agents", cx);
    click(("agents-subagent", 0usize), cx);
    assert!(found("sheet-content", cx), "the transcript opened");
    assert_eq!(focused(cx), Some(second.clone()), "the card did not take the click");
    cx.update(|window, cx| window.close_sheet(cx));
    click(("agents-subagent-stop", 0usize), cx);
    wait_until(cx, "the subagent to stop", |cx| {
        first.read_with(cx, |s, _| {
            s.model.subagents.iter().any(|a| a.state == SubagentState::Stopped)
        })
    });
    assert!(!found("sheet-content", cx), "Stop did not open the transcript");
    assert_eq!(focused(cx), Some(second));
}
```

The tests module's imports gain `gpui_kit::component::WindowExt as _` (for `close_sheet`), `crate::chat::Subagent`, `listed` and `subagent_status`. `click` (from `workspace::tests`) and `found` exist.

- [ ] **Step 2: Implement**
  - **`subagent_line`:** `format!("{title} · {}", subagent_status(state))`, plus ` · {tool}` when the state is live and `tool` is `Some`.
  - **`listed`:** the live indices in order, then the finished ones from the newest back, `MAX_FINISHED` of them. The hidden count is whatever finished ones are left.
  - **`card`:**
    - Build the lines from `listed(&m.subagents)`. The dot is `TurnState::Working` for Running, `NeedsYou` for NeedsYou, `Error(String::new())` for Failed, and `Idle` otherwise.
    - Each line is an `h_flex().id(("agents-subagent", k))` with `.aria_label(line)` and `.test_support()`. Its `on_click` (a `cx.listener`) calls `cx.stop_propagation()`, then `session.update(cx, |s, cx| s.open_transcript(&id, window, cx))`.
    - When `m.stoppable(&id)`, the line ends with `Button::new(("agents-subagent-stop", k)).ghost().xsmall().icon(IconName::Square).tooltip("Stop subagent").accessibility_label("Stop subagent")`. Its `on_click` calls `cx.stop_propagation()`, then `session.read(cx).stop_subagent(&id)`.
    - After the lines, when `hidden > 0`, a muted `text_xs` line `+{hidden} more finished`.
    - The card's `summary` gets every listed line.
  - Remove the `ponytail: no cap` comment: the cap is now `MAX_FINISHED` for finished ones, and live ones stay uncapped.

- [ ] **Step 3: Docs.** In "## The Agents tab", the subagent bullet becomes: "its subagents, one line each: the live ones first (`title · Working` or `title · Needs you`, then the tool call in progress), then the last 3 that finished (`Done`, `Stopped`, `Failed`, `Ended` or `Unknown`), then `+N more finished`. Clicking a line opens its transcript; a live one has a square that stops it;". After "Clicking a card shows that session…", add: "A click on a subagent's line, or on its square, does not."

- [ ] **Step 4: Check.** `cargo check --workspace --all-targets`. Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add crates/wai/src/agents_view.rs docs/user-guide.md
git commit -m "feat: list finished subagents in the Agents tab"
```

---

### Task 8: The Onda 1 chat leftovers that still apply

Verified against `a818cc8`. Each bullet names the report's minor, where it sits now, and the fix.

- **Final review M9 + fix wave "stale × click"** (`input.rs` `take_back`; render.rs ~735, the `queued-remove` handler): today the handler removes whatever sits at `ix`. Fix: capture `let len = self.queued.len();` when drawing, and in the handler `if this.queued.len() != len { return; }` in place of the bound check (D9).
- **Fix wave "closed question title is `""` when the message starts with a newline"** (render.rs ~539, `request.message.lines().next()`): use a new `fn first_line(text: &str) -> &str` (the first line with non-blank text, trimmed; `""` when none).
- **Final review M5 "render_waiting clones `model.waiting` per frame"** (render.rs ~716): iterate `self.model.waiting.iter()`; the closure borrows other fields of `self`. If the borrow checker refuses, `std::mem::take` the list and put it back after collecting. Never clone it.
- **Final review M6 "waiting_height excludes the 2 px border"** (render.rs ~766, the canvas): report `bounds.size.height + px(2.)`, with a comment: the canvas fills the stack inside its 1 px border.
- **Task 2 "the step-output test checks only `>=` and never a Thought"** (view.rs `step_lines_are_compact_and_open_below`): assert the exact gap, and add the Thought box with the id `("thought-output", row)`.
- **Fix wave "doc line over 100 columns"** (render.rs ~387, `render_group`'s doc): rewrap.

**Files:**
- Modify: `crates/wai/src/chat/render.rs`
- Test: `crates/wai/src/chat/view.rs`, `crates/wai/src/chat/render.rs` tests

**Interfaces:** none new outside `render.rs`. `first_line` is private to it.

- [ ] **Step 1: Write the tests**

`render.rs` tests:

```rust
#[test]
fn first_line_skips_blank_lines() {
    assert_eq!(first_line("\n\n  Pick one\nmore"), "Pick one");
    assert_eq!(first_line("Pick"), "Pick");
    assert_eq!(first_line(" \n"), "");
}
```

`view.rs` tests:

```rust
/// D9: an × drawn before the queue changed does nothing (two clicks in one frame).
#[gpui_kit::test]
fn stale_queue_clicks_do_nothing(cx: &mut TestAppContext) {
    cx.executor().allow_parking(); // the fake agent is a real process
    let (workspace, cx) = open(cx, fake_settings(&slow_reply(200, 20)));
    let session = start_session(&workspace, cx);
    send("go", cx);
    wait_until(cx, "the first chunk", |cx| last_agent_text(&session, cx).is_some());
    for message in ["first", "second", "third"] {
        send(message, cx);
    }
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.click(("queued-remove", 0usize), cx);
        window.click(("queued-remove", 1usize), cx);
    });
    cx.update(|window, cx| window.render_frame(cx));
    let queued = session.read_with(cx, |s, _| {
        s.queued.iter().map(|m| m.text.clone()).collect::<Vec<_>>()
    });
    assert_eq!(queued, ["second", "third"]);
    assert_eq!(typed(&session, cx), "first");
}
```

In `step_lines_are_compact_and_open_below`:
- Replace the `output.top() >= line.bottom()` assert with an exact gap, taken from the rem size so the theme's font size does not matter:

```rust
let gap = cx.update(|window, _| window.rem_size() * 0.25); // `gap_1`
assert!(
    (output.top() - line.bottom() - gap).abs() < px(0.5),
    "{output:?} is not {gap:?} under {line:?}"
);
click_edits(None, ("thought", 1usize), cx);
let (line, output) = (bounds(("thought", 1), cx), bounds(("thought-output", 1), cx));
assert!((output.top() - line.bottom() - gap).abs() < px(0.5), "{output:?} {line:?}");
```

- Its doc gains "…; a Thought opens the same way."

In `list_scrolls_clear_of_the_pinned_stack`, the `<= px(2.)` tolerance becomes `< px(0.5)`.

- [ ] **Step 2: Implement** the six fixes above in `render.rs`. The Thought's box becomes `output_box(cx).id(("thought-output", ix)).child(…).test_support()` with the usual comment.

- [ ] **Step 3: Check.** `cargo check --workspace --all-targets`. Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add crates/wai/src/chat/render.rs crates/wai/src/chat/view.rs
git commit -m "fix: tidy the Onda 1 chat leftovers"
```

---

## Track finish (once, after Task 8)

- [ ] **Gate:**

```sh
export PATH="$HOME/.cargo/bin:$PATH" _ZO_DOCTOR=0
df -h /System/Volumes/Data
cargo build -p wai-agent --bin wai-fake-agent
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: every command exits 0. The chat, Agents and `wai-agent` tests start the fake agent, so it is built first.

- [ ] **Final review:** one whole-branch review against this plan, its Review Focus and Decisions.
- [ ] **One fix wave** for every failing test and every Critical or Important finding, then the gate again.
- [ ] **Optional look** (owner's 0.6 check comes later anyway): the cost pill, with the fake agent. If screenshots are taken through `app.rs`'s offscreen path, the scratch HOME must seed `$HOME/.config/wai/state.json` with `"approved": ["<exact fake-agent command>"]` (Onda 1 track A gotcha).
- [ ] **Manual check with real Claude** (owner, item 0.6): the cost pill after a turn; Revert an edit to a Go file or a Makefile.
- [ ] `cargo clean` in the worktree.
