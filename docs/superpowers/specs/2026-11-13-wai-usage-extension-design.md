# wai: the usage extension and hover cards for bar items

> 2026-11-13. Choices marked **(owner)** are the owner's; **(call)** are mine, listed again under "My calls". API version: **1.4**.

## Intent

Owner, verbatim: *"O que eu quero é item que ao passar o mouse mostre os gastos de token parecido com o que temos dentro da central de IA"*, with a screenshot of Claude Code's `/usage`: "Current session", "Current week (all models)", "Current week (Fable)", each a progress bar, `N% used` and `Resets <time> (<zone>)`.

wai's own `wai.usage` item only says `Usage ok` / `Near usage limit` / `Usage limit reached`. The owner wants the percentages, at a glance and in detail on hover.

From the mockup (`.superpowers/brainstorm/…/usage-item.html`) the owner picked **option A** **(owner)**: a bottom-bar item `16% · 59%`; hovering it opens a card with the three bars, as in the screenshot.

## Goals

1. **wai, API 1.4:** an extension bar item may carry a **hover card**, a small display-only tree drawn instead of the one-line tooltip.
2. **`wai-usage`**, a first-party extension in `extensions/wai-usage/` (Go, like `wai-rotinas`) **(call)**: a bottom-bar item with the session and week percentages, and the card with every usage window the account reports.

## Non-goals

- Token counts or cost from local transcripts (ccusage style), history or charts.
- The screenshot's "Plugin skill-listing footprint" and "What's contributing to your usage?" parts **(call)**.
- Notifications near the limit.
- Removing or changing wai's own `wai.usage` item; the user can hide it with a right click **(call)**.
- API-key (pay-as-you-go) accounts: they have no plan windows.
- Distribution of the extension's binary (installing from git without a local build): the next design **(owner)**.
- Clickable things inside a hover card.

## Anti-goals (a failure even if it works)

- **Logging the user out of Claude Code.** The extension never refreshes the OAuth token and never writes Claude Code's credentials: refreshing rotates the refresh token Claude Code holds.
- The token going anywhere but `api.anthropic.com`, or into a log, stderr, a panel or the data folder.
- A card an extension could make look like wai's own UI: wai always adds the `— <extension name>` line.
- An API 1.3 extension behaving differently.
- Idle CPU above 0.5 % in wai; the extension wakes only on its own timer.

## Today (from the code)

- **Items:** `item/update` → `wai_ext::panel::item_state` (`crates/wai-ext/src/panel.rs:328`), parsed off the UI thread in the coalescer (`render`, `crates/wai-ext/src/host.rs:690`). `ItemState { text, icon, tooltip, tone }`.
- **Drawing:** `bars::ext_item` (`crates/wai/src/bars.rs:211`) → `bottom_item`, whose `.tooltip(...)` is a gpui-component `Tooltip` with `item_tooltip` (`<tooltip> — <ext name>`).
- **Trees:** `wai_ext::panel::parse` and `Node`, with `MAX_DEPTH` 32, `MAX_NODES` 2,000, `progress` (0..1). Panels draw them in `ext_panel.rs` (`ExtPanel::node`, a method with form state).
- **API:** `manifest.rs:81` `API = "1.3"`; `check_api` refuses a manifest whose `api` is newer.
- **Go extensions:** `extensions/wai-rotinas` (stdio JSON-RPC, `bin/` built with `go build`, gitignored).
- **Usage source:** Claude Code 2.1.323 calls `GET https://api.anthropic.com/api/oauth/usage` (header `anthropic-beta: oauth-2025-04-20`) and reads windows `five_hour`, `seven_day`, `seven_day_opus`, `seven_day_sonnet`, and others (`seven_day_oauth_apps`, `seven_day_cowork`, …), each with `utilization` and `resets_at`. Its `/usage` labels them "Current session", "Current week (all models)", "Current week (<model>)". The endpoint is **undocumented**. Credentials: macOS Keychain (service named after "Claude Code" + `-credentials`), elsewhere `~/.claude/.credentials.json`, JSON `claudeAiOauth.accessToken`.

## Approaches

| | Approach | Trade-off |
|---|---|---|
| **A (owner's pick)** | Hover card: `item/update` gains a `card` tree, drawn on hover. | Exactly the screenshot; needs API 1.4 and a small read-only renderer. |
| B | API 1.3 only: one-line tooltip, click opens a sidebar panel with `progress` nodes. | No wai change; the bars are a click away, not on hover. |
| C | Multi-line tooltip, bars drawn with `▰▱` characters. | Tiny wai change; looks like text art, not the screenshot. |

## Design

### wai: hover cards (API 1.4)

- **`item/update` gains `card`** (optional): a tree in the panel schema, limited to display nodes: `column`, `row`, `text`, `badge`, `icon`, `kv`, `progress`, `divider`, `spacer`. Any other node type in a card draws as `Unsupported element`, as in panels **(call)**.
- **Caps:** at most **200 nodes** and **depth 8** **(call)**; beyond that the card is dropped and the plain tooltip shows. Parsed in the coalescer with the rest of `item_state`, never on the UI thread.
- **Drawing:** on hover over the item, wai shows the card instead of the tooltip: about **320 px wide** **(call)**, the theme's popover look, a last line `— <extension name>` in muted text that the extension cannot remove or style. It closes when the pointer leaves the item. Nothing in it is clickable.
- **Without `card`**, the item behaves exactly as in 1.3. Each `item/update` still replaces the item's whole state, so an update without `card` removes the previous card.
- **Live:** an update that arrives while the card is open redraws it with the new tree **(call)**.
- **Placement:** gpui-component 0.9 has a `HoverCard` (`hover_card.rs`); the builder uses it if it fits, else a deferred element. The card opens above the item and stays inside the window; the pointer does not need to reach it.
- **Accessibility:** the item's accessible label stays its text; `tooltip` stays its plain description, so authors should still send it.
- **Renderer:** the builder decides whether to share code with `ExtPanel::node` or write a small read-only renderer for the nine node types; `progress` must look the same as in panels.
- `API = "1.4"`; `docs/extensions.md` documents `card`, its node list and caps, with the usage card as the example.

### `wai-usage`

**Manifest** (`extensions/wai-usage/wai-extension.toml`):

```toml
id = "wai-usage"
name = "Usage"
version = "0.1.0"
description = "Your Claude plan's usage limits in the bottom bar"
api = "1.4"
permissions = ["process", "bar", "network"]

[process]
command = "bin/wai-usage"

[[contributes.items]]
id = "usage"
bar = "bottom"
title = "Usage"
```

**Reading the token** (each fetch, never cached on disk):
- macOS: `security find-generic-password -s <service> -w`, run with a fixed argument list; the exact service name is verified by the builder.
- elsewhere, or if the Keychain has none: `~/.claude/.credentials.json` (`$CLAUDE_CONFIG_DIR` if set).
- Read only. On `401`, it does not refresh: it shows the "login expired" state below.

**Fetching:**
- At start, every **5 minutes** **(call)**, and on a click on the item (the `item/action` request API 1.3 already sends).
- `network` is an existing, declaration-only permission (`docs/extensions.md`); nothing blocks `security` or the file read.
- `GET /api/oauth/usage`, `Authorization: Bearer <token>`, `anthropic-beta: oauth-2025-04-20`, 15 s timeout; the builder confirms the headers and response shape against one real call and Claude Code's binary.
- `429`: wait `Retry-After`, else 15 minutes. Other errors: keep the last good values and say so in the card.
- A click while a fetch runs does nothing.

**The item:**
- Text `16% · 59%` (`five_hour` · `seven_day`, rounded); a window missing from the response is left out (`16%` alone). Icon `gauge`.
- Tone: **warning** when any window in the card (per-model weeks included) is at or above **80 %**, **danger** at **100 %** **(call)**, neutral otherwise.
- Tooltip (plain description): `Session 16% · Week 59% · resets 00:20`, the reset being the session's (`five_hour`).

**The card** (as the owner's screenshot):
- One block per window with a value, in this order: `five_hour` "Current session", `seven_day` "Current week (all models)", then each per-model week, `seven_day_<model>` → "Current week (<Model>)" (`opus` → Opus, `sonnet` → Sonnet, any other model name capitalised) **(call)**. Other windows (`oauth_apps`, `cowork`, `overage_*`) are left out **(call)**.
- Each block: a `row` with the label and `N% used` at the end, a `progress` (utilization / 100, clamped), a muted `Resets 00:20` (today) or `Resets Nov 14 at 23:00`, local time, 24 h, the zone's name after it as in the screenshot (`(America/Sao_Paulo)`).
- Last line, muted: `Updated 14:05`, an absolute time so the card never needs a redraw to stay true **(call)**.

**States:**

| State | Item text | Card |
|---|---|---|
| Starting, no data yet | `Usage …` | "Loading usage…" |
| No credentials | `Usage` (neutral) | "Sign in to Claude Code to see your usage." |
| `401` | `Usage` (warning) | "Your Claude login expired. Use Claude Code once to refresh it." |
| Network or server error, with old values | the old values | the old blocks + "Couldn't update at 14:10: <reason>" |
| Same, no old values | `Usage` (warning) | "Couldn't load usage: <reason>" |
| A response it cannot read | `Usage ?` | "Unexpected response from Claude; the extension may need an update." |

- **Logs** go to stderr, without the token or the response body.
- **Build:** `go build -o bin/wai-usage .`, as `wai-rotinas`; `bin/` added to `.gitignore`. A README with build and install steps.

## Constraints

- No new Go dependencies beyond the standard library **(call)**.
- wai's renderer uses theme tokens only, light and dark.
- Windows and Linux: the credentials file path; the Keychain only on macOS.

## Acceptance

- **wai:** a card parses with the nine node types; another type draws unsupported; over 200 nodes or depth 8 drops the card; no `card` behaves as 1.3; the card ends with `— <extension name>`; `API` is `1.4` and `check_api` accepts `api = "1.4"`.
- **wai-usage** (Go tests, `httptest` server and a fake token source):
  - text, tone and card for 16 / 59 / 0, for 85 % (warning) and 100 % (danger), and with a window missing;
  - each row of the states table;
  - `429` with and without `Retry-After`;
  - a `401` never triggers a write to the credentials or a token refresh;
  - the token never appears in stderr.
- Run by hand: installed from the folder, the item shows the owner's real numbers and the card matches `/usage`.
- `docs/extensions.md` (API 1.4, `card`) and the extension's README updated.

## My calls

1. The extension is in Go, in `extensions/wai-usage/`, with only the standard library.
2. A card holds only the nine display node types; others are unsupported; 200 nodes, depth 8, about 320 px wide.
3. Every 5 minutes, plus a click, plus at start.
4. Warning at 80 %, danger at 100 %.
5. Per-model weeks are shown; `oauth_apps`, `cowork` and overage windows are not.
6. `Updated HH:MM` instead of "N min ago".
7. wai's own `wai.usage` stays; the user hides it if they want.
8. The skill footprint and "what's contributing" parts of `/usage` are out.

## Left to the builder

- The Keychain service name, the request headers and the response shape, checked against Claude Code's binary and one real call.
- Sharing the renderer with `ExtPanel` or not.
- The card's exact spacing, within the theme.
