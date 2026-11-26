# Pet PR Bubble Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Clicking the pet's `pr_fix` / `pr_approved` bubble opens that PR; PR bubbles stay until clicked or until the PR leaves that state.

**Architecture:** wai's `bubble` node gets an optional `url` it opens on click (API 1.10). wai-bichinho attaches the PR URL to its PR lines, makes them sticky, lets urgent lines step in front of them, and drops them when the PR moves on.

**Tech Stack:** Rust (gpui-kit 0.9, `crates/wai-ext`, `crates/wai`), Go (`extensions/wai-bichinho`).

**Spec:** `docs/superpowers/specs/2026-11-26-pet-pr-bubble-design.md`

## Global Constraints

- Extension API becomes `"1.10"`; `api_version` compares numerically, so `1.10 > 1.9`.
- `url` is checked with `wai_ext::panel::web_url` (http/https, ≤ 2,048 chars). Present but invalid or not a string → `Node::Unsupported`.
- wai commits: `feat:`/`fix:`, no scope, no Co-Authored-By. Extension commits: `chore:` only.
- Run every command from the worktree `.claude/worktrees/pet-pr-bubble`. Never touch `crates/wai/src/chat/*` (another session's work on the main checkout).

## Review Focus

1. A sticky PR bubble is up and a session needs you → `needs_you` shows now; the PR bubble comes back after. (Task 2, `TestUrgentStepsInFrontOfStickyPR`)
2. A PR is merged or fixed while its bubble shows or waits in the queue → that bubble goes away. (Task 2, `TestPRBubbleLeavesWithItsPR`)
3. Two PRs need a fix at once with the same fixed line → each gets its own bubble, one after the other. (Task 2, `TestSameLineTwoPRs`)
4. An extension sends `url: "javascript:…"` → the bubble is unsupported, nothing opens. (Task 1, `bubble_url_is_checked`)
5. The sticky bubble has shrunk to "!" → a click on it still opens the PR. (Task 1, `bubble_click_opens_its_url`)

---

### Task 1: wai — `bubble.url` (API 1.10)

**Files:**
- Modify: `crates/wai-ext/src/panel.rs` (`Node::Bubble` ~92, parser ~755, tests ~1090)
- Modify: `crates/wai/src/ext_panel.rs` (`bubble()` ~338, render site ~793, tests ~2606)
- Modify: `crates/wai-ext/src/manifest.rs:96`, `crates/wai-ext/tests/manifest.rs:312,319`, `crates/wai-ext/tests/host.rs:128`
- Modify: `docs/extensions.md` (lines 9, 48, 158, 286)

**Interfaces:**
- Produces: `Node::Bubble { id: String, text: Arc<str>, tone: Tone, sticky: bool, url: Option<Arc<str>> }`; tree JSON `{"type":"bubble","id","text","tone"?,"sticky"?,"url"?}`.

- [ ] **Step 1: Failing parser tests** in `panel.rs`: add `url: None` to both expectations in `bubble_parses`, and

```rust
#[test]
fn bubble_url_is_checked() {
    let pr = "https://github.com/o/r/pull/1";
    let Ok(Node::Bubble { url, .. }) = parse(&json!({"type": "bubble", "id": "b", "text": "Oi", "url": pr})) else { panic!() };
    assert_eq!(url.as_deref(), Some(pr));
    for bad in [json!("javascript:alert(1)"), json!("https://"), json!(3)] {
        assert_eq!(parse(&json!({"type": "bubble", "id": "b", "text": "Oi", "url": bad})), Ok(Node::Unsupported));
    }
}
```

- [ ] **Step 2: Failing UI test** in `ext_panel.rs` next to `bubble_click_reports_click`: add `assert_eq!(cx.opened_url(), None);` at its end, and `bubble_click_opens_its_url`: sticky bubble with `url` `https://github.com/o/r/pull/1`, advance 7,000 ms (collapsed to "!"), `click(name("panel-bubble:say"))`, advance 200 ms; assert `cx.opened_url() == Some(url)` and `sent == [("say", {"bubble": "click"})]`.

- [ ] **Step 3: Run, expect compile failure / FAIL**
`cargo test -p wai-ext bubble && cargo test -p wai bubble_click`

- [ ] **Step 4: Implement.** Parser: `url` absent → `None`; present → `Some(..)` only if `as_str()` passes `web_url`, else return `None` from the arm (→ Unsupported, as `link` does). Render: pass `url.clone()` into `bubble(&self, id, tone, sticky, url: Option<Arc<str>>, cx)`; when `Some`, the bubble gets `.cursor_pointer()`, the URL as tooltip (same `Tooltip` pattern as `link`, ~1340), and `on_click` calls `cx.open_url(&url)` before `b.click(now)`.

- [ ] **Step 5: API 1.10.** `API = "1.10"`; manifest tests: the "newer" case uses `api = "1.11"` and both messages say `this wai has 1.10`; `host.rs` initialize expects `"api": "1.10"`. Docs: line 48 `this wai has 1.10` and a leading clause `Set it to "1.10" when you use a bubble url,`; line 158 `"api": "1.10"`; line 286 bubble row gets `url`? — `API 1.10. Opens url (as link checks it) on the click that closes the bubble; pointer cursor and the URL as tooltip`; line 9 mentions the pet's PR bubble needs API 1.10.

- [ ] **Step 6: Run, expect PASS**
`cargo test -p wai-ext && cargo test -p wai ext_panel` → all pass.

- [ ] **Step 7: Commit**
`git commit -am "feat: let a speech bubble open a URL on click"`

---

### Task 2: wai-bichinho — PR bubbles carry their URL and stick

**Files:**
- Modify: `extensions/wai-bichinho/speech.go`, `lines.go`, `main.go` (`gotPRs`, `render`, `dockTree`)
- Test: `extensions/wai-bichinho/speech_test.go`, `main_test.go`
- Modify: `extensions/wai-bichinho/wai-extension.toml` (`api = "1.10"`), `README.md` (API 1.10; PR bullet: click opens the PR, stays until clicked or the PR moves on)

**Interfaces:**
- Consumes: `bubble.url` from Task 1.
- Produces: `line.URL string`; `func (sp *speech) drop(match func(line) bool)` (replaces `drop(kind string)`); `func (s *server) sayURL(kind, url string)` with `say(kind)` = `sayURL(kind, "")`; `kinds` `pr_fix` and `pr_approved` get `Sticky: true`.

- [ ] **Step 1: Failing speech tests** (`speech_test.go`):
  - `TestUrgentJumpsAndReplaces` second half now expects: sticky `a` (prio 1) up, `b` (2) queued, `needs_you` said → `cur.Kind == "needs_you"`, `queue` kinds `[b, a]`.
  - `TestSameLineTwoPRs`: two `line{Kind: "pr_fix", Text: "x", Prio: 2, Sticky: true}` with URLs `/1` and `/2` → one shown, the other queued (dedup is text **and** URL).
  - `TestDropKind` uses `drop(func(l line) bool { return l.Kind == ... })`.

- [ ] **Step 2: Failing server tests** (`main_test.go`), with a `urlOf(msgs)` helper next to `bubbleOf`:
  - `TestPRBubbleOpensItsPR`: after hatch + `bubbleDone`, first `prs.mine` `[pull/1 waiting, pull/2 ci_failed]` → bubble sticky, `url == .../pull/2`; then `[pull/1 approved, pull/2 ci_failed]` after the bubble is done → `pr_approved` bubble with `url == .../pull/1`.
  - `TestUrgentStepsInFrontOfStickyPR`: PR bubble up, `needs_you` session state → bubble text is the `needs_you` line; session back to `idle` → after `h.now += 300ms` and `tick`, the PR bubble (same URL) is back.
  - `TestPRBubbleLeavesWithItsPR`: PR bubble for `pull/1` up, then `prs.mine` `[pull/1 waiting]` → no bubble; same with `pr_approved` and the PR gone from the list.

- [ ] **Step 3: Run, expect FAIL** — `cd extensions/wai-bichinho && go test ./...`

- [ ] **Step 4: Implement.**
  - `speech.say`: dedup on `Text` and `URL`; a prio 3 line preempts any current line with `Prio < 3`; if that line is sticky it goes back into the queue at the head of its priority (after higher prios, before equal ones), then the queue cap applies.
  - `gotPRs`: remember the first URL (in event order) that changed into attention, the first that changed into `approved`, and on the first event the first in attention; say with that URL. Before saying, `s.talk.drop` PR lines whose PR left their state: `pr_fix` when `!attention(s.prs[l.URL])`, `pr_approved` when `s.prs[l.URL] != "approved"`.
  - `render` calls `s.talk.drop(func(l line) bool { return l.Kind == "needs_you" })`.
  - `dockTree`: `bubble["url"] = c.URL` when not empty.

- [ ] **Step 5: Run, expect PASS** — `go test ./... && go vet ./...`

- [ ] **Step 6: Commit**
`git commit -am "chore: open the PR from the pet's bubble and keep PR bubbles up"`

---

### Task 3: Release (ask the owner before pushing)

- [ ] Merge `feat/pet-pr-bubble` into `development` (check `git log --oneline -5 development` first; another session may commit there), fast-forward `main`, push both → CI releases wai with API 1.10.
- [ ] After the wai release is out: bump `wai-extension.toml` `version = "0.6.0"` (`chore: release PR bubbles as wai-bichinho 0.6.0`), tag `wai-bichinho-v0.6.0` as 0.5.0 was, then update `extensions/index.toml` from the release's `index-entry.toml` (`chore: list wai-bichinho 0.6.0 in the official index`).
