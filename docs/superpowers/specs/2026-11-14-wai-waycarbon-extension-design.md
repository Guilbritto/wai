# wai-waycarbon 0.2: a WayCarbon cockpit — design

Status: design, ready to plan. The owner was away while this was written ("Tudo de uma vez"), so every
choice marked **[my call]** was made without them and is listed again at the end for review.

## Intent

A WayCarbon developer's day runs on one loop: pick a Jira card (`EC-1234`), analyse it, plan, build,
open a draft PR, get it reviewed, fix the review, merge, move the card; then deploy. Every step already
has a skill in the WayCarbon Claude plugin (`waycarbon/claude-code`, installed as `waycarbon@waycarbon`):
`/waycarbon:solve-card`, `code-review`, `fix-review`, `recheck-review`, `deploy`, and more.

What is missing is **knowing what needs you right now** and **getting to the right skill in one click**.
wai-waycarbon becomes that cockpit:

- **Work:** your PRs and why each needs you, the PRs waiting for your review, your Jira cards, and the
  cards whose state disagrees with their PR. A click opens a session with the right `/waycarbon:*`
  command filled in, never sent.
- **Help:** ask one colleague, by GitHub login, to do something you lack the rights for (a deploy, a
  command on an environment you cannot reach). Designed in
  [`2026-11-14-help-requests-design.md`](2026-11-14-help-requests-design.md); this document only records
  where the build departs from it.
- Installing the extension installs the WayCarbon plugin it depends on, after the user's consent.

## Goals

1. One activity-bar icon (the WayCarbon "W" redrawn as a ribbon), opening a sidebar panel titled
   `WayCarbon` with two tabs, **Work** and **Help**.
2. Work tab from the `gh` and `jira` CLIs the user already has. The extension never reads, stores or
   sends their tokens: the CLIs hold them.
3. When `gh` or `jira` is missing or signed out, the tab says so and teaches the fix (install command,
   **Sign in with GitHub** running `gh auth login --web` and showing the code), with **Refresh**.
4. A manifest can require a Claude Code plugin. The consent dialog lists it; **Install** installs the
   extension and then the plugin (like VS Code's `extensionDependencies`). A failed plugin install never
   undoes the extension: the panel warns and offers **Install plugin** again.
5. Help requests, end to end: the extension's Help tab and the relay service.
6. One bottom-bar item counting everything that needs you (Work + Help), with a hover card.

## Non-goals

- Picking the repository folder for a session. `sessions/open` has no folder; sessions start in wai's
  open project, and every prefilled command carries the PR **URL** (not a bare number), so the
  plugin's skills find the repository themselves.
- Running anything automatically. Every click fills a prompt; the user sends it.
- Writing new skills. The plugin owns the workflow; the extension only routes to it.
- Hosting the relay or creating the GitHub App (owner-only, see the end).
- Badges on the activity-bar icon (wai has no API for it; the bottom-bar item covers it).

## Anti-goals (a failure even if it works)

- wai core touching credentials, or the extension handling the `gh`/`jira` tokens.
- A plugin installed without the user having seen its name in the consent dialog or pressed a button.
- CPU busy while idle: no tight polling. Work refreshes at most every 10 minutes; Help is pushed.
- Text from another person rendered as anything but plain `text`.

## Constraints

- wai core: Rust 2024, GPUI (`gpui-kit` 0.9). Extension API today is **1.4**; this adds **1.5**.
- Extensions: Go, stdlib only (`go 1.22`, no third-party modules), JSON-RPC over stdio, programs run
  with argv only, never a shell. CI already runs `go vet ./... && go test ./...` in
  `extensions/wai-waycarbon`.
- Follow Zed/VS Code where they have prior art (icons, dependencies, device sign-in).
- Releases are CI's job; nothing here bumps wai's version or pushes.

## Approaches

**How to split the work.** (a) Everything inside the extension, using only today's API: two panels
for the tabs, a Lucide icon, the plugin installed by the extension on first start. (b) **Small,
general additions to wai core (plugin dependencies, an SVG panel icon, a `tabs` node), then the
extension on top.** (c) A dedicated WayCarbon view built into wai.

The owner chose (b) during the conversation: one icon with tabs (not two panels), the redrawn logo
(not a Lucide icon), and the plugin listed in the consent dialog (option 1, "like
`extensionDependencies`"). (a) cannot give any of the three; (c) puts company code in a general tool.
Each core addition is general: any extension can use it.

**Help transport.** HELP-REQUESTS.md says WebSocket. Go's stdlib has no WebSocket, and the extension
has no third-party modules. **[my call]** Server-sent events (`GET /events`) for relay → client, plain
`POST`s for client → relay. Both stdlib, both sides; same messages, same push (no polling), works
through any HTTP load balancer. Upgrade path: swap the transport, the messages stay.

**Relay storage.** HELP-REQUESTS.md says SQLite. SQLite in Go needs cgo or a large module.
**[my call]** One JSON file, written whole (temp file + rename) under a mutex on every change.
Ceiling: a few thousand open requests (the retention rule keeps it far below); switch to SQLite past
that. Marked with a `ponytail:` comment.

## Part 1 — wai core (API 1.5)

### 1.1 Required Claude plugins

Manifest:

```toml
api = "1.5"

[[requires.claude_plugins]]
name = "waycarbon"                      # the plugin's bare name
marketplace = "waycarbon/claude-code"   # owner/repo on GitHub, or an https git URL
```

- **Validation (parse time; an invalid entry makes the manifest invalid, since it is a dependency):**
  `name` as `plugin_install` already checks (`[A-Za-z0-9._-]`, no leading `-`, ≤ 214);
  `marketplace` is `owner/repo` from `[A-Za-z0-9._-]` or an `https://` URL ≤ 2,048 characters with no
  whitespace; neither starts with `-` (they become argv). At most 5 entries.
- **Consent dialog:** one line per plugin under "It asks to:":
  `Install Claude Code plugin waycarbon from waycarbon/claude-code`.
- **On Install (and on Update):** after the store commits, wai runs, off the main thread, per plugin:
  `claude plugin install <name> --marketplace <marketplace> --scope user --json`
  - `claude` is found the way wai finds any bare program (absolute `PATH` folders only).
  - stdin is null; env adds `GIT_TERMINAL_PROMPT=0` (a private marketplace must never hang on a
    password prompt); cwd is the config folder; killed after 5 minutes.
  - **Never `-y`.** A plugin that runs a marketplace-declared command needs a person to confirm it;
    without a TTY `claude` refuses, and wai reports that as a failure (the user then has **Install in
    terminal**). The `waycarbon` plugin is a plain relative-source plugin with no declared command
    (checked in its `marketplace.json`), so it installs without one.
  - Skip a plugin that `claude plugin list --json` already shows (an `id` starting with `<name>@`).
    **[my call]** prefix match, since the marketplace's own name is only known after it is added.
- **Result:** a notification, `Installed Claude Code plugin waycarbon`, or
  `Could not install Claude Code plugin waycarbon: <last stderr line, ≤ 300 chars>`. The extension stays
  installed and enabled either way.
- **Extensions view:** under the extension, required plugins are listed like suggested ones, with
  **Install in terminal**, which types
  `claude plugin install <name> --marketplace <marketplace>` without Enter (today's mechanism).
- **Docs/security model:** "Claude plugins are never installed without the user pressing Enter" becomes
  "…without the user's consent: a required plugin is named in the consent dialog and installed when the
  user presses Install; a suggested one is typed into the terminal for the user to run."
- `contributes.claude_plugins` (suggestions) is unchanged.

**[my call]** Keeping the plugin current is the extension's job, not core's (1.4 below).

### 1.2 An SVG panel icon

- `contributes.panels[].icon` may also be a relative path ending in `.svg`, inside the extension's
  folder (`relative_inside`), at most 32 KB. Anything else is a Lucide name, as today.
- **Drawing [my call]:** as VS Code draws activity-bar icons, the SVG is a **mask**: its shape and
  opacity, in the rail's own colours (idle muted, selected foreground). gpui's
  `svg().external_path(<absolute path>)` does it. A two-tone logo keeps its two tones as opacity. The
  mockup showed the selected icon in brand teal; masking keeps the rail uniform and follows VS Code.
  Full colour is a one-line change (`img(path)`) if the owner prefers it.
- Missing, unreadable or over 32 KB: the title's initials, as for an unknown Lucide name; the install
  does not fail. Same rule for the `main` pane header, which shows the panel's icon.
- **Release workflow:** `extension-release.yml` also packs every panel `icon` that is a path, keeping
  its relative folder.

### 1.3 A `tabs` node

| `type` | Fields | Notes |
|---|---|---|
| `tabs` | `id`, `value`?, `tabs` (array of `{"id", "title", "badge"?, "children"}`) | A tab bar; the selected tab's `children` show below it. At most 10 tabs. `value`: the initial tab id (default the first), under the initial-value rule. The selected tab id is sent in `values` under the node's `id` |

- Switching tabs is local (no round trip). Form fields in hidden tabs keep what the user typed.
- The node's `id` joins the per-tree id set (buttons, form fields, sections); tab ids may repeat across
  trees but not within one `tabs`. `title` and `badge` are one-line strings. Hidden tabs' nodes count
  toward the 2,000-node and 32-level limits.
- `fill` works through it: a `tabs` node whose ancestors are all `column`s passes the remaining height
  to its selected tab's children.
- An older wai shows `Unsupported element`; extensions using it declare `api = "1.5"`.

### 1.4 API version

`API` becomes `"1.5"`; `docs/extensions.md` documents 1.1–1.5 as additive.

## Part 2 — the extension (`extensions/wai-waycarbon` 0.2.0)

### Manifest

```toml
id = "wai-waycarbon"
name = "WayCarbon"
version = "0.2.0"
api = "1.5"
description = "What needs you at WayCarbon: PRs, reviews, Jira cards and help requests, one click from the right /waycarbon command"
permissions = ["process", "panels", "bar", "sessions:control", "sessions:read", "notify", "network"]

[process]
command = "bin/wai-waycarbon"

[[requires.claude_plugins]]
name = "waycarbon"
marketplace = "waycarbon/claude-code"

[[contributes.panels]]
id = "waycarbon"
title = "WayCarbon"
icon = "icon.svg"

[[contributes.items]]
id = "attention"
bar = "bottom"
title = "WayCarbon: needs you"
```

- **[my call]** `contributes.claude_plugins = ["beehive-waycarbon"]` is dropped: Beehive is
  discontinued and the required `waycarbon` plugin replaces it.
- **[my call]** The three session templates stay; they cost nothing and the owner did not ask to
  remove them.
- `icon.svg` (viewBox `0 0 24 24`), the chosen "W de fita":
  ```svg
  <path d="M2.2 5h3.4l3.9 14H6.1z"/>                     <!-- full opacity -->
  <path d="M6.1 19h3.4l3.9-14H10z" fill-opacity=".45"/>
  <path d="M10 5h3.4l3.9 14h-3.4z"/>
  <path d="M13.9 19h3.4l3.9-14h-3.4z" fill-opacity=".45"/>
  ```

### Panel layout

```
WAYCARBON
[ Work 3 ][ Help 1 ]
@gbrito · updated 10:42                     [↻]
▾ Needs you  3
  ┌ #84 waycarbon/inventories   CI failed ┐  click → session
  ┌ #88 …                Changes requested┐
▾ Review requests  1
▾ My cards  4
▸ Waiting on others  2   (collapsed)
```

The tabs node is the root's first child; each tab is a `column`.

### Work tab

**Data (one refresh = these calls, argv only, in parallel, 30 s timeout each):**
- `gh api graphql -f query=<Q>` with one query: `viewer { login }` and four `search(type: ISSUE)`es
  over the org (constant `waycarbon`, `WAI_WAYCARBON_ORG` overrides):
  1. `is:pr is:open author:@me org:<org>` — mine;
  2. `is:pr is:open review-requested:@me org:<org>` — asked of me;
  3. `is:pr is:open label:review org:<org> -author:@me` — the team's review queue (the label
     `watching-review-queue` uses);
  4. `is:pr is:merged author:@me org:<org> merged:>=<today − 30 days>` — for card/PR mismatches.
     **[my call]** 30 days; a card whose PR merged earlier shows as having no PR.

  Per PR: `number, title, url, isDraft, headRefName, repository { nameWithOwner }, reviewDecision,
  mergeable, labels(first: 10) { nodes { name } }, commits(last: 1) { nodes { commit {
  statusCheckRollup { state } } } }`.
- `jira me`, then `jira issue list -a <me> -s~Done --plain --no-headers --columns key,summary,status`
  (today's calls).
- Card ↔ PR: a Jira key (`[A-Z][A-Z0-9]+-\d+`) in the PR's title or branch. A card may have several
  PRs (proto + service). **[my call]** It is "PR merged" only when it has a merged PR and no open one;
  with open PRs it shows the one whose reason comes first in the table below.
- A PR both requested of me and in the `review` queue shows once, badge `Requested`; it counts once.

**When:** at start, on ↻, every 10 minutes, and after **Move to Done**. One refresh at a time.

**Sections and clicks.** Every click opens a session with the prompt filled in, not sent. PR cards
also have a `link` "Open on GitHub".

| Section | What | Badge | Click fills |
|---|---|---|---|
| Needs you | my open PR with a conflict (`mergeable: CONFLICTING`) | `Conflict` (danger) | `Resolve the merge conflicts on <url>.` |
| | …CI failed (`FAILURE`/`ERROR`) | `CI failed` (danger) | `The CI fails on <url>. Find out why and fix it.` |
| | …changes requested | `Changes requested` (warning) | `/waycarbon:fix-review <url>` |
| | …approved, CI green, not draft | `Ready to merge` (success) | `Merge <url>, then move its Jira card.` |
| Review requests | asked of me, or in the `review` queue | `Review` / `Requested` | `/waycarbon:code-review <url>` |
| My cards | a card with no PR | its status | `/waycarbon:solve-card <KEY>` |
| | a card whose PR merged but is not Done | `PR merged` (warning) + **Move to Done** button (confirm) | `/waycarbon:deploy` |
| | a card with an open PR | `#<n>` | the PR's own click |
| Waiting on others (closed) | my other open PRs (draft, review pending, CI running) | state | `/waycarbon:recheck-review <url>` |

A PR gets the first reason that applies, in the table's order. **Move to Done** runs
`jira issue move <KEY> Done` (`WAI_WAYCARBON_DONE` overrides the status name; **[my call]**, since the
board's status names were not checked).

**States (alerts at the top of the tab; the rest still renders what it can):**
- `gh` not on PATH: `GitHub CLI not found` + a `log` node with `brew install gh` (it has Copy) + Refresh.
- `gh` signed out (`gh auth status` fails): **Sign in with GitHub** → runs
  `gh auth login --web --hostname github.com --git-protocol https --skip-ssh-key`, stdin null. Without a
  TTY gh prints `First copy your one-time code: XXXX-XXXX` on stderr and waits (checked with gh 2.87).
  The tab shows the code, a link to `https://github.com/login/device`, a spinner and **Cancel** (kills
  it). Exit 0 → refresh; else an alert with gh's last line.
- `jira` not on PATH: `brew install ankitpokhrel/jira-cli/jira-cli`, then `jira init`. `jira me`
  failing: `Run jira init in a terminal`.
- Plugin missing (`claude plugin list --json` has no `waycarbon@…`): a warning,
  `The WayCarbon Claude plugin is not installed, so the /waycarbon commands will not work.`, and
  **Install plugin**, running the same `claude plugin install` line as core (never `-y`,
  `GIT_TERMINAL_PROMPT=0`), with a spinner; failure shows the last stderr line.
- **Plugin updates [my call]:** at start, at most once a day (a timestamp in `WAI_EXT_DATA`):
  `claude plugin marketplace update <marketplace name>` then `claude plugin update waycarbon@<marketplace name>`
  (name from the installed id). Silent; errors go to stderr.

### Help tab

Built as HELP-REQUESTS.md says (flows 1–6, rules, limits, security), with these differences:

- **Transport** (above): `GET /events` (SSE, `Authorization: Bearer`) carries `sync`, `request.new`,
  `request.updated`, `people`; a `: ping` comment every 30 s; reconnect 1 s → 60 s backoff.
  Actions are `POST /requests` (`to`, `project`, `text`), `POST /requests/{id}/{cancel|accept|decline|reply}`.
  Sign-in: `POST /auth/device` → `{user_code, verification_uri, device_code, interval}`;
  `POST /auth/device/poll` `{device_code}` → `428` pending, `403` not a member, `200 {token, login}`;
  `POST /auth/signout` revokes.
- **One panel, a tab** (not a second panel titled `Help requests`).
- **No relay yet [my call]:** the relay URL constant is empty until the owner hosts it; with neither it
  nor `WAI_WAYCARBON_RELAY` set, the tab says `Help requests need the WayCarbon relay, which is not set
  up yet.` and nothing connects.
- **Reply-to sessions [my call]:** the requester's chosen `{agent, session}` per request, and whether
  the reply was sent to it, live in `$WAI_EXT_DATA/help.json` (the relay never sees them). After a wai
  restart the session is checked against `sessions/list`; gone → **Open in a new session**. "Replies
  not yet sent" is this local flag.
- **Project info:** `git -C <initialize.project> remote get-url origin` and `… branch --show-current`.
  Not a git folder, or no project: the request goes without them (`project` empty).
- **Layout:** signed out → **Sign in with GitHub** (the relay's device flow; never `gh`'s token).
  Signed in → `@login` + **Sign out**; `Incoming` cards (sender, time, repo/branch, full text as `text`,
  **Accept**, **Decline** + optional reason); `Sent` cards (`Waiting for @x` + **Cancel**,
  `Accepted by @x`, reply + **Send to session** / **Open in a new session**, declined reason);
  a `New request` section (collapsed while there are requests): **To** input with the signed-in people
  below (online badge; a click fills **To**), **Reply goes to** (`select` from `sessions/list`), the
  suggested draft prompt, **Request** (multiline, 8,000 max), `Do not paste secrets.`, **Send**.
- Notifications (`notify`): `@x asks for help` + first line; `@x replied`; `@x declined`.

### Bottom-bar item

`attention`: hidden at 0; otherwise icon `bell`, text = Needs you + Review requests + card mismatches +
incoming waiting requests + replies not yet sent to a session, tone `warning`. Card: one `kv` line per
non-zero group. Click → `panel/open`.

## Part 3 — the relay

- **Where [my call]:** `extensions/wai-waycarbon/relay/`, a `package main` in the extension's Go
  module: stdlib only, covered by CI's `go test ./...` with no workflow change, and not packed into
  extension releases (the workflow packs only the program). Moving it to its own repository later is a
  `git mv` and a `go.mod`.
- **Config (env):** `RELAY_ADDR`, `RELAY_DATA` (the JSON file), `RELAY_ORG`, `RELAY_GITHUB_CLIENT_ID`,
  `RELAY_GITHUB_APP_ID`, `RELAY_GITHUB_APP_KEY` (PEM file), `RELAY_GITHUB_INSTALLATION_ID`,
  `RELAY_GITHUB_API` / `RELAY_GITHUB_WEB` (default github.com; tests point them at a fake).
- **GitHub:** device flow against `<web>/login/device/code` and `<web>/login/oauth/access_token`;
  `GET <api>/user`; membership `GET <api>/orgs/<org>/members/<login>` with an installation token
  (an RS256 JWT signed with stdlib `crypto/rsa`, exchanged at
  `POST <api>/app/installations/<id>/access_tokens`, cached until it expires).
- Everything else as HELP-REQUESTS.md: ids, tokens (32 random bytes, SHA-256 stored, 30 days), states,
  24 h expiry, 7-day retention, limits (8,000 chars, 64 KB body, 20/h per sender, 10 waiting per
  recipient, 4 streams per user), hourly membership re-check, logs without text or tokens.
- A `Dockerfile`-free README section: how to run it, the env, and what the GitHub App needs
  (device flow enabled, org permission *Members: read*, installed on the org).

## Testing

- **Core:** manifest parsing (requires, svg icon, tabs) in `wai-ext` unit tests; the consent line and the
  post-install `claude` run with a fake `claude` on PATH (records argv); tabs rendering and switching in
  `ext_panel` tests; the SVG icon in the rail.
- **Extension:** Go tests with fake `gh`, `jira` and `claude` scripts on PATH (as `main_test.go` does
  today) and an `httptest` relay; `crates/wai/src/waycarbon.rs` keeps an end-to-end test of the panel.
- **Relay:** `httptest` with a fake GitHub (device flow, user, membership, installation token) covering
  sign-in, each state transition, permission checks, limits, expiry and SSE delivery.

## Order

One branch. Core first (1.1–1.4, each with its tests and docs), then the relay, then the extension.
The manifest's `api = "1.5"` already makes an older wai refuse the new extension with
`WayCarbon needs a newer wai`, so the two releases need no extra gate.

## Left to the builder

File layout inside the extension, the exact GraphQL text, Go type names, how `ext_panel` keeps hidden
tabs' state, test structure, and wording not quoted above.

## Owner-only (cannot be done here)

1. Host the relay: where, which domain, who owns it; then set the URL constant.
2. Create the WayCarbon GitHub App (device flow on, *Members: read*, installed on the org) and give the
   relay its ids and key.
3. Access to `waycarbon/claude-code` for every user (the plugin install clones it with their git
   credentials).
4. Check `Done` is the Jira status name for **Move to Done**.

## My calls, for review

SSE + POST instead of WebSocket · JSON file instead of SQLite · relay inside the extension's module ·
SVG icon drawn as a mask (not brand colour) · prefix match for "already installed" · plugin updates done
by the extension, daily · 30 days of merged PRs · "PR merged" only with no open PR ·
reply-to sessions kept locally · Beehive suggestion dropped, templates kept · Help tab showing "not set up yet" while the
relay URL is empty · `Done` as the default status for Move to Done · `brew` install hints (macOS first).
