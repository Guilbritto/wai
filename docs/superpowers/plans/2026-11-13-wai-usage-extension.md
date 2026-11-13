# Usage extension and item hover cards Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extension bar items can carry a hover card (API 1.4), and a new Go extension `wai-usage` shows the Claude plan's usage windows in the bottom bar with that card.

**Architecture:** `item/update` gains an optional `card` tree, parsed off the UI thread with the panel parser under smaller caps and limited to nine display node types; wai draws it in gpui-component's `HoverCard` instead of the tooltip. `wai-usage` reads Claude Code's OAuth token (read only), polls `GET /api/oauth/usage`, and sends `item/update` with text, tone and card.

**Tech Stack:** Rust (gpui, gpui-component 0.9), Go 1.22+ standard library only.

**Spec:** `docs/superpowers/specs/2026-11-13-wai-usage-extension-design.md`

Work on a branch `feat/usage-extension` from `development`. Commits: `feat:` / `fix:` / `docs:`, no scope, no `Co-Authored-By`.

## Global Constraints

- API version `1.4`; an extension without `card` behaves exactly as in 1.3.
- Card: node types `column`, `row`, `text`, `badge`, `icon`, `kv`, `progress`, `divider`, `spacer` only; others draw `Unsupported element`. At most **200 nodes**, **depth 8**; beyond, no card (the tooltip shows). About **320 px** wide. Last line `— <extension name>`, always added by wai.
- wai: theme tokens only, light and dark; no timers; the card is parsed in the coalescer, never on the UI thread.
- wai-usage: never refreshes the OAuth token, never writes Claude Code's credentials; the token goes only to `https://api.anthropic.com` and never into logs, stderr, the card or the data folder.
- wai-usage: Go standard library only; fetch at start, every **5 min**, on click; 15 s HTTP timeout; `429` waits `Retry-After`, else **15 min**.
- Tone: warning when any window in the card is **≥ 80 %**, danger at **≥ 100 %**.

## Review Focus

1. **The Mac sleeps for hours.** Go timers pause in sleep; the card must not show a pre-sleep value for 5 more minutes after waking. Scheduling compares wall-clock time at least once a minute (as `wai-rotinas`' `maxNap`). Test in Task 4.
2. **A reset time passes.** After `resets_at`, the shown percent is stale: the next fetch is due at the earliest future `resets_at` (+5 s) when that is sooner than 5 min. Test in Task 4.
3. **The Keychain asks for permission.** The first `security` read may open a macOS dialog and block; the read has a 60 s timeout and then counts as "no credentials" for that round. Test in Task 4.
4. **Odd numbers.** `utilization` above 100, negative or `null`; `resets_at` `null` or unparseable: text shows the real rounded number, `progress` is clamped by wai, a missing reset drops the "Resets" line, a `null` window is left out. Test in Task 3.
5. **A card that changes or disappears while open.** An update with a new card redraws it; an update without `card` removes it; the extension stopping removes the item. Test in Task 2.

---

### Task 1: `card` in `item/update` and API 1.4 (wai-ext)

**Files:**
- Modify: `crates/wai-ext/src/panel.rs` (`Parser`, `parse`, `ItemState`, `item_state`, tests)
- Modify: `crates/wai-ext/src/manifest.rs:81`
- Modify: `crates/wai-ext/tests/manifest.rs:226-239`, `crates/wai-ext/tests/host.rs:124`

**Interfaces:**
- Produces: `ItemState.card: Option<Arc<Node>>`; `pub const CARD_DEPTH: usize = 8; pub const CARD_NODES: usize = 200;` in `panel.rs`; `API = "1.4"`.

- [ ] **Step 1: Write the failing tests** in `panel.rs`'s test module:

```rust
#[test]
fn item_card_keeps_display_nodes_only() {
    let card = |tree| item_state(&json!({"text": "x", "card": tree})).card;
    let ok = json!({"type": "column", "children": [
        {"type": "row", "children": [{"type": "text", "text": "Current session"}, {"type": "spacer"},
                                     {"type": "badge", "text": "16% used"}, {"type": "icon", "name": "gauge"}]},
        {"type": "progress", "value": 0.16}, {"type": "divider"},
        {"type": "kv", "items": [{"key": "a", "value": "b"}]}]});
    let node = card(ok).expect("card");
    let mut unsupported = 0;
    node.walk(&mut |n| unsupported += matches!(n, Node::Unsupported) as usize);
    assert_eq!(unsupported, 0);
    let button = json!({"type": "column", "children": [{"type": "button", "id": "b", "label": "Go"}]});
    assert_eq!(*card(button).unwrap(), Node::Column(vec![Node::Unsupported]));
    assert_eq!(item_state(&json!({"text": "x"})).card, None);
    assert_eq!(card(json!("not a tree")), Some(Arc::new(Node::Unsupported)));
}

#[test]
fn item_card_caps() {
    let card = |tree| item_state(&json!({"text": "x", "card": tree})).card;
    let flat = |n| json!({"type": "column", "children": vec![json!({"type": "divider"}); n]});
    assert!(card(flat(CARD_NODES - 1)).is_some()); // the column is a node
    assert!(card(flat(CARD_NODES)).is_none());
    let mut deep = json!({"type": "divider"});
    for _ in 1..CARD_DEPTH { deep = json!({"type": "column", "children": [deep]}); }
    assert!(card(deep.clone()).is_some());
    assert!(card(json!({"type": "column", "children": [deep]})).is_none());
}
```

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test -p wai-ext item_card`
Expected: FAIL (no field `card`, no `CARD_DEPTH`).

- [ ] **Step 3: Implement**
  - `Parser` gets `max_depth` and `max_nodes`; `count` uses them; `parse` builds it with `MAX_DEPTH`/`MAX_NODES` (panels unchanged).
  - `fn card(tree: &Value) -> Option<Node>`: parse with `CARD_DEPTH`/`CARD_NODES`, `Err(TooLarge)` → `None`, then a recursive pass turns every node outside the nine types into `Node::Unsupported` (`Column`/`Row` keep their children, filtered).
  - `item_state` sets `card: o.get("card").and_then(card).map(Arc::new)`; `Node` already derives `Clone`/`PartialEq`, so `ItemState` keeps its derives.
  - Update the module doc's first line to mention `item/update`'s `card`.

- [ ] **Step 4: Bump the API**
  - `manifest.rs:81`: `pub const API: &str = "1.4";`
  - `tests/manifest.rs`: the "newer" case uses `"1.5"` and the messages say `this wai has 1.4`; add `assert!(with_api("\"1.4\"", "").is_ok());`.
  - `tests/host.rs:124`: `"api": "1.4"`.

- [ ] **Step 5: Run the crate's tests**

Run: `cargo test -p wai-ext`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/wai-ext
git commit -m "feat: let bar items carry a hover card (API 1.4)"
```

### Task 2: Draw the hover card (wai) and document it

**Files:**
- Modify: `crates/wai/src/ext_panel.rs` (new free fn near `unsupported`, line ~1027)
- Modify: `crates/wai/src/bars.rs:164-262` (`bottom_item`, `ext_item`)
- Modify: `crates/wai/src/ext_host.rs` tests (near `extension_item_click_and_hide`, line ~1283)
- Modify: `crates/wai-ext/src/bin/wai-fake-ext.rs` if a card-sending flag is needed by the test
- Modify: `docs/extensions.md`

**Interfaces:**
- Consumes: `ItemState.card` (Task 1).
- Produces: `pub(crate) fn hover_card(node: &Node, ext_name: &SharedString, cx: &App) -> AnyElement` in `ext_panel.rs`, whose root has `debug_selector "item-card"`.

- [ ] **Step 1: Write the failing test** in `ext_host.rs`'s tests, following `extension_item_click_and_hide` (install with `ITEMS`, start extensions, wait for `("bottombar-item", 0)`):

```rust
/// Hovering an item with a card shows the card, ending with the extension's name; a later
/// update without a card removes it.
#[gpui_kit::test]
fn extension_item_hover_card(cx: &mut TestAppContext) {
    // ... install an extension whose first update is
    // {"item": "prs", "text": "3 PRs", "card": {"type": "column", "children": [
    //     {"type": "text", "text": "Current session"}, {"type": "progress", "value": 0.16}]}}
    // hover: cx.simulate_mouse_move(center of debug_bounds(("bottombar-item", 0))), then
    // advance the clock past HoverCard's 600 ms open delay and run_until_parked.
    assert!(cx.debug_bounds("item-card").is_some());
    // the card's text includes "Current session" and ends with "— Bars"
    // send {"item": "prs", "text": "3 PRs"} (no card), wait, hover again:
    assert!(cx.debug_bounds("item-card").is_none());
}
```

Use the fake extension's existing way of sending a scripted `item/update` (`PRS_UPDATE` in `install`); add a second scripted update only if `wai-fake-ext` has no way to send two.

- [ ] **Step 2: Run it and see it fail**

Run: `cargo test -p wai extension_item_hover_card`
Expected: FAIL (`item-card` not drawn).

- [ ] **Step 3: Implement**
  - `hover_card`: `v_flex().w(px(320.)).gap_2()` with the tree and a last `div` in `muted_foreground`, text `format!("— {ext_name}")`. Column/Row render as `v_flex`/`h_flex` with `gap_1`; the seven leaves render as in `ExtPanel::node` (`Text`, `Badge`, `Progress`, `Icon`, `Divider`, `Spacer`, `Kv`). Prefer moving those leaf arms into one free fn used by both, so `progress` looks the same in both places; anything else → `unsupported(muted)`.
  - `bottom_item`: skip `.tooltip(...)` when `status.tooltip` is empty.
  - `ext_item`: when `state.card` is `Some`, the item has no tooltip (empty `Status.tooltip` in the bottom bar, no `.tooltip` on the top pill) and is returned wrapped in `HoverCard::new((id, "card"))` with `.trigger(item)` and `.content(move |_, _, cx| hover_card(&card, &ext_name, cx))`, anchored so it opens above a bottom-bar item and below a top-bar one. Without a card nothing changes.

- [ ] **Step 4: Run wai's tests**

Run: `cargo test -p wai`
Expected: all pass, `top_bar_fits_narrow_window` included.

- [ ] **Step 5: Document API 1.4** in `docs/extensions.md`:
  - every "1.3" that names the current API becomes "1.4" (lines 5, 18, 45, 146, 155, 426); line 45 says to set `"1.4"` when using `card`;
  - the `item/update` row gains `"card"?`; the `item/update` notes and **Bar items** gain one paragraph: what `card` is, the nine node types, 200 nodes / depth 8, drawn on hover instead of the tooltip, ends with `— <extension name>`, an update without `card` removes it, nothing in it is clickable;
  - a short example card (one "Current session" block: row with label and `16% used`, progress `0.16`, muted `Resets 00:20`).

- [ ] **Step 6: Format, lint, commit**

Run: `cargo fmt --all && cargo clippy -p wai -p wai-ext --all-targets -- -D warnings`
Expected: clean.

```bash
git add crates/wai crates/wai-ext/src/bin docs/extensions.md
git commit -m "feat: show an extension item's card on hover"
```

### Task 3: wai-usage — reading the usage and building the item (Go, pure)

**Files:**
- Create: `extensions/wai-usage/go.mod` (`module wai-usage`, `go 1.22`)
- Create: `extensions/wai-usage/usage.go`
- Create: `extensions/wai-usage/usage_test.go`
- Create: `extensions/wai-usage/testdata/usage.json`

**Interfaces:**
- Produces (used by Task 4):

```go
type node = map[string]any

type window struct {
	Label  string    // "Current session", "Current week (all models)", "Current week (Opus)"
	Used   float64   // percent, as the API sends it
	Resets time.Time // zero when unknown
}

var (
	errNoLogin    = errors.New("no Claude login")
	errExpired    = errors.New("Claude login expired")
	errUnreadable = errors.New("unexpected response")
)

// view is what the item shows: the last good windows, when they were fetched, and the last fetch's error (nil if it worked).
type view struct {
	windows []window
	updated time.Time
	failed  time.Time // when err happened
	err     error
}

func parseUsage(body []byte) ([]window, error) // errUnreadable when not a JSON object
func (v view) update(now time.Time, loc *time.Location, zone string) node // item/update params
func zoneName() string // "America/Sao_Paulo": $TZ, else /etc/localtime's link after "zoneinfo/", else ""
```

- [ ] **Step 1: Check the real response, once, without printing the token**

Run, on the owner's Mac (the token only passes through a pipe):

```bash
security find-generic-password -s "Claude Code-credentials" -w | python3 -c 'import json,sys;print(json.load(sys.stdin)["claudeAiOauth"]["accessToken"])' \
 | xargs -I{} curl -s -m 15 https://api.anthropic.com/api/oauth/usage -H "Authorization: Bearer {}" -H "anthropic-beta: oauth-2025-04-20" | python3 -m json.tool
```

Expected: an object with `five_hour`, `seven_day` and `seven_day_*` keys, each `null` or `{"utilization": <percent>, "resets_at": "<RFC 3339>"}`. If the Keychain service name differs, find it with `security dump-keychain | grep -i claude` (names only) and use that in Task 4. Save the output, numbers as they are, as `testdata/usage.json`. Note which `seven_day_*` keys are per-model (the ones `/usage` labels "Current week (<model>)"); everything else is left out.

- [ ] **Step 2: Write the failing tests** in `usage_test.go` (`loc := time.FixedZone("BRT", -3*3600)`, `now := 2026-11-13 14:05 BRT`):
  - `TestParseUsage`: the fixture gives the windows in the order session, week (all models), then per-model weeks; `Used` and `Resets` as sent; `null` windows and non-model windows absent; `parseUsage([]byte("[]"))` → `errUnreadable`.
  - `TestOddNumbers`: `utilization` 104 → text `104%`, tone `danger`; `-3` → `-3%` (shown as is); `null` utilization → window left out; `resets_at` `null` or `"soon"` → zero `Resets`, no "Resets" line in the card.
  - `TestItemText`: windows 16 / 59 / 0 → `text` `"16% · 59%"`, `icon` `"gauge"`, `tone` `"neutral"`, `tooltip` `"Session 16% · Week 59% · resets 00:20"`; only `seven_day` → `"59%"`; an Opus week at 85 → `"warning"`; any at 100 → `"danger"`.
  - `TestCard`: the card is a `column`; per window a `row` `[text label, spacer, text "16% used"]`, a `progress` `0.16`, a muted text `"Resets 00:20 (America/Sao_Paulo)"` (today) or `"Resets Nov 14 at 23:00 (America/Sao_Paulo)"`; with `zone == ""` the parentheses are left out; last node muted `"Updated 14:05"`.
  - `TestStates` — one case per row of the spec's states table, `update` returning:

    | view | text | tone | card's text |
    |---|---|---|---|
    | no windows, `err == nil`, zero `updated` | `Usage …` | neutral | `Loading usage…` |
    | `err = errNoLogin` | `Usage` | neutral | `Sign in to Claude Code to see your usage.` |
    | `err = errExpired` | `Usage` | warning | `Your Claude login expired. Use Claude Code once to refresh it.` |
    | windows + `err = errors.New("timeout")`, failed at 14:10 | old values | from values | old blocks + `Couldn't update at 14:10: timeout` |
    | no windows + `errors.New("timeout")` | `Usage` | warning | `Couldn't load usage: timeout` |
    | `err = errUnreadable` | `Usage ?` | neutral | `Unexpected response from Claude; the extension may need an update.` |


- [ ] **Step 3: Run them and see them fail**

Run: `cd extensions/wai-usage && go test ./...`
Expected: FAIL (undefined: `parseUsage`, …).

- [ ] **Step 4: Implement `usage.go`** to the interface above. Error reasons are one line, cut to 120 characters. Time formats: `15:04` today, `Jan 2 at 15:04` otherwise, both in `loc`.

- [ ] **Step 5: Run the tests**

Run: `cd extensions/wai-usage && go vet ./... && go test ./...`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add extensions/wai-usage
git commit -m "feat: read the Claude plan usage for the usage extension"
```

### Task 4: wai-usage — token, fetching, the server, packaging

**Files:**
- Create: `extensions/wai-usage/token.go`, `extensions/wai-usage/main.go`, `extensions/wai-usage/main_test.go`
- Create: `extensions/wai-usage/wai-extension.toml` (exactly the spec's manifest), `extensions/wai-usage/README.md`
- Modify: `.gitignore` (add `extensions/wai-usage/bin/`)

**Interfaces:**
- Consumes: Task 3's `window`, `view`, `parseUsage`, `view.update`, errors, `zoneName`.
- Produces:

```go
type tokenSource func(ctx context.Context) (string, error) // errNoLogin when there is none

// claudeToken: on macOS `security find-generic-password -s <service> -w` (exec.CommandContext, fixed args,
// 60 s timeout), then $CLAUDE_CONFIG_DIR/.credentials.json or ~/.claude/.credentials.json;
// JSON claudeAiOauth.accessToken, trimmed. Read only.
func claudeToken(ctx context.Context) (string, error)

// fetch: GET usageURL with the bearer token and anthropic-beta header, 15 s timeout.
// 401 → errExpired; 429 → rateLimited{retry} (Retry-After seconds, else 15 min);
// other non-2xx → an error naming the status; body → parseUsage.
func fetch(ctx context.Context, c *http.Client, usageURL, token string) ([]window, error)
type rateLimited struct{ retry time.Duration } // implements error

// next: when the next fetch is due, wall-clock: updated+5m, or 5 s after the earliest future
// reset if sooner, or the 429's retry; the scheduler sleeps min(until due, 1 min) and re-checks.
func next(v view, retry time.Duration, now time.Time) time.Time
```

- [ ] **Step 1: Write the failing tests** in `main_test.go`, against an `httptest.Server` and a fake `tokenSource` returning `"sekrit"`, the server's output captured in a `bytes.Buffer`, `log.SetOutput` to another buffer:
  - `TestInitializeThenUpdate`: feed `initialize` (id 0) → a response with id 0, then an `item/update` notification for item `usage` whose `params.text` is `"16% · 59%"`.
  - `TestClickRefreshes`: an `item/action` request `{"item": "usage"}` → a second request reaches the server; a click while a fetch is in flight adds none.
  - `TestExpiredNeverRefreshes`: server answers 401; a temp `CLAUDE_CONFIG_DIR/.credentials.json` is byte-identical afterwards; the server saw exactly one request, to `/api/oauth/usage`; the update says `Usage` / warning.
  - `TestRateLimited`: 429 with `Retry-After: 120` → `next` is now+120 s; without it → now+15 min.
  - `TestNext`: `updated` 14:00 → 14:05; a reset at 14:02 → 14:02:05; a reset in the past is ignored; after a simulated sleep (`now` 3 h later) the fetch is due at once.
  - `TestTokenNeverLogged`: run every path above; neither the log buffer nor the output buffer contains `"sekrit"`.
  - `TestKeychainTimeout`: `claudeToken` with a context already past its deadline falls back to the file, and with no file returns `errNoLogin`.

- [ ] **Step 2: Run them and see them fail**

Run: `cd extensions/wai-usage && go test ./...`
Expected: FAIL (undefined: `fetch`, `next`, …).

- [ ] **Step 3: Implement** `token.go` and `main.go` to the interfaces, reading stdin as `wai-rotinas/main.go` does (`serve`, `handle`, `write` under a mutex). `main` wires `claudeToken`, `http.Client{Timeout: 15 * time.Second}` and `https://api.anthropic.com/api/oauth/usage`; the URL and token source are parameters so the tests can swap them. Logs: one line per failed fetch with the error, never the token or body. Answer `item/action` with `null`.

- [ ] **Step 4: Run the tests and build**

Run: `cd extensions/wai-usage && gofmt -l . && go vet ./... && go test ./... && go build -o bin/wai-usage .`
Expected: no gofmt output, tests PASS, `bin/wai-usage` exists.

- [ ] **Step 5: README** (as `wai-rotinas/README.md`): what it shows, build (`go build -o bin/wai-usage .`), install from the folder, that it reads Claude Code's login without changing it, that the endpoint is undocumented, and that wai's own `Usage ok` item can be hidden with a right click.

- [ ] **Step 6: Commit**

```bash
git add .gitignore extensions/wai-usage
git commit -m "feat: add the usage extension"
```

### Task 5: Check it on the owner's Mac

- [ ] **Step 1:** Build and install a local wai with Task 1–2 (project `CLAUDE.md`, "Install a local build on this Mac"), in the background; never quit the running wai. Ask the owner to restart it.
- [ ] **Step 2:** In wai, Extensions → install from folder `extensions/wai-usage`; accept the consent (process, bar, network).
- [ ] **Step 3:** The bottom bar shows `NN% · NN%`; hovering shows the card. Compare with `/usage` in a Claude session: same percentages and reset times. Click the item: `Updated` changes.
- [ ] **Step 4:** `cargo clean` (the disk is nearly full).
