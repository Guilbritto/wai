# Writing wai extensions

An extension is a folder with a `wai-extension.toml` manifest. It can be manifest-only (templates, prompt commands, agents, MCP servers) or run a program of its own. That program can be in any language: it speaks line-delimited JSON-RPC 2.0 over stdio and can draw panels in wai's sidebar or main area and show items in its top and bottom bars.

This guide describes API version 1.3 (protocol 1), as implemented by the latest wai release. The [first-party extensions](../extensions) are working references:
- `wai-navegador` is manifest-only;
- `wai-waycarbon` and `wai-rotinas` are written in Go.

## The manifest

`wai-extension.toml`, at the top of the extension's folder:

```toml
id = "hello-go"                 # required
name = "Hello (Go)"             # required
version = "0.1.0"               # required
description = "A minimal wai extension"
api = "1.3"                     # optional: the oldest extension API it works with
permissions = ["process", "panels", "bar", "notify"]

[process]                       # optional: the extension's own program
command = "bin/hello-go"
args = []

[[contributes.panels]]
id = "main"
title = "Hello"
icon = "hand"                   # optional: a Lucide icon for the activity bar
location = "main"               # optional: "sidebar" (default) or "main"

[[contributes.items]]
id = "prs"
bar = "bottom"                  # "top" or "bottom"
title = "Pull requests"
```

**Unknown fields are refused** at every level, and so is a manifest that is not valid UTF-8 or is over 1 MB.

| Field | Type | Rules |
|---|---|---|
| `id` | string | 1–63 characters from `a-z`, `0-9` and `-`, not starting with `-`. It names the install folder |
| `name` | string | Shown in the extension list and the consent dialog |
| `version` | string | Free form. wai shows it; it does not compare versions |
| `description` | string, optional | Shown last in the consent dialog, cut to fit |
| `api` | string, optional | The oldest extension API the extension needs, `major.minor` (`"1.3"`). Absent means 1.0. A wai with an older API refuses the extension: `<name> needs a newer wai (extension API <api>; this wai has 1.3)`. Set it to `"1.3"` when you use `items` or the `bar` permission, so an older wai refuses the extension with that message instead of an unknown-field error. Anything not `major.minor` is an invalid manifest. wai 0.8 and older do not know the field and refuse the manifest |
| `permissions` | array of strings | See [Permissions](#permissions). An unknown permission is refused |
| `process.command` | string | See [Commands and paths](#commands-and-paths). Needs `process` |
| `process.args` | array of strings, optional | Arguments, passed as given (no shell) |
| `contributes.templates` | array of tables | `name`, optional `agent`, optional `prompt`. Same as `templates.toml` in the [user guide](user-guide.md#templates) |
| `contributes.commands` | array of tables | `id`, `title`, optional `prompt`. See [Contributions](#contributions) |
| `contributes.agents` | array of tables | `name`, `command`. Needs `process` |
| `contributes.mcp_servers` | array of tables | `name`, `command`, optional `args` (array), optional `env` (table of strings). Needs `process` |
| `contributes.claude_plugins` | array of strings | Claude Code plugin names to suggest |
| `contributes.panels` | array of tables | `id`, `title`, optional `icon`, optional `location`. See [Panels](#panels). Needs `panels` |
| `contributes.items` | array of tables | `id`, `bar` (`"top"` or `"bottom"`), `title`. At most 3. See [Bar items](#bar-items). Needs `bar` |
| `contributes.themes` | array of strings | Accepted, but not used in this version |

**Extra rules:**
- A command without `prompt` must have a `[process]` to serve it.
- An extension with no `[process]` has no program of its own. Its agents and MCP servers still run when a session needs them.

### Commands and paths

`process.command`, and the `command` of each agent and MCP server, is one of two things:
- **a path relative to the extension's folder that contains a `/`**, such as `bin/hello` or `./hello`. It may not be absolute, may not climb out with `..`, and on macOS and Linux may not contain `\`.
- **a bare program name**, such as `npx` or `node`. It is looked up on wai's `PATH`, not in the extension's folder: `command = "hello"` does **not** run `<folder>/hello`.

**How bare names are found.** wai looks a bare name up itself, in the absolute folders on its own `PATH` only (on Windows also with each `PATHEXT` extension). It never uses the current or project folder, or an empty or relative `PATH` entry.
- A program that is not found fails with `<command> is not on PATH`.
- An MCP server whose command is not found is left out of sessions, and wai logs `wai: MCP server <name>: <command> is not on PATH`.
- wai opened from Finder or the Dock takes `PATH` from your login shell, cached from the previous launch. With no cache and no answer within 3 seconds, wai keeps macOS's default (`/usr/bin:/bin:/usr/sbin:/sbin`), which has no `npx`. Started from a terminal, wai has the terminal's `PATH`. See the [user guide](user-guide.md#agents-and-path).

## Permissions

Calls in both directions are checked against what the user **granted** at install time, not against the manifest on disk. A denied request is answered with error `-32001`, `Permission denied: <permission>`.

| Permission | What it allows |
|---|---|
| `process` | Run `[process]`. Also required to contribute `agents` and `mcp_servers`, which run code |
| `sessions:read` | `sessions/list`. Receive `event/session_state` and `event/turn_ended` |
| `sessions:control` | `sessions/open` and `sessions/prompt`. It does not include `sessions:read` |
| `notify` | `notify`: system notifications |
| `panels` | Contribute panels, `panel/render`, `panel/open`, and receive `panel/action` |
| `bar` | Contribute bar items, `item/update`, and receive `item/action`. Shown as *Show items in the top and bottom bars* |
| `network` | Nothing is enforced. It tells the user the extension uses the network |
| `fs:<path>` | Nothing is enforced. It tells the user the extension uses `<path>`, which must be a relative path inside the extension's folder |

`event/file_saved`, `command/run` and `initialize` need no permission.

## Installing and consent

- **Sources.** Users install from a folder or from a git URL (`https://…` or `git@host:path`). wai copies the folder, or clones the repository with depth 1, into a staging area, then reads the manifest.
- **The consent dialog.** It lists every permission in plain words, then the warning `wai cannot restrict files or network access in this version; only install extensions you trust.`, the source, and the description.
  - Nothing is installed or run before **Install**.
  - Consent is all or nothing: the extension gets exactly what it asks for.
- **What a stage refuses:**
  - symbolic links anywhere in the folder;
  - anything that is not a regular file or a folder;
  - more than 100 MB or 10,000 entries.

  A folder is copied whole, so keep build output (`target/`, `node_modules/`) out of the folder you install.
- **Where things go.** The extension goes to `<config>/extensions/<id>/`, its data folder is `<config>/extension-data/<id>/` and the consent goes to `<config>/extensions.json`. It starts enabled.
- **It is a copy.** Rebuilding in your source folder does not change the installed extension.
- **Updating.** Install the same id again (folder or git URL). wai asks for consent again, stops the extension, replaces its folder and starts it again. It stays enabled or disabled as it was, and its data folder is untouched. A project suggestion of an installed id also goes through this.
- **Project suggestions.** A project can suggest an extension in `.wai/extensions.toml`, but never install one (see the [user guide](user-guide.md#what-a-projects-wai-may-set)).

## The process

When an enabled extension with `[process]` and the `process` permission starts (at launch, after install, or on enable):
- **Working folder:** it runs from **wai's config folder** (`~/.config/wai`, `%APPDATA%\wai`). It never runs from the project or the extension's folder.
- **Its own folder** arrives as `WAI_EXT_DIR` and in `initialize`.
- **Environment:** it inherits the rest of wai's environment, including `PATH` and any secrets in it.
- **Process tree:** it gets its own process group on macOS and Linux, or a kill-on-close job object on Windows. When wai stops it, everything it started dies too.
- **Stopping.** wai stops it on quit, disable and uninstall, with a kill and no shutdown message. Save your state as you go.
- **Restarts.**
  - If it exits on its own, wai restarts it, at most 3 times. After that it is marked `Crashed`, showing the last 20 lines of its stderr.
  - Re-enabling it resets the count.
  - When the user opens another folder, running extensions are restarted with the new `project`.
- **Logging.** Write logs to **stderr**, never stdout. Stdout carries only protocol messages.
- **Data.** A folder of your own arrives as `WAI_EXT_DATA` and as `data_dir` in `initialize`, `<config>/extension-data/<id>/`, created before you start. It is not inside your extension folder, so updating or reinstalling does not touch it, and uninstalling leaves it. Keep state and credentials-free caches there.

## The protocol

### Framing

- **JSON-RPC 2.0, one message per line,** in UTF-8, on stdin and stdout. Write compact JSON with a single `\n` after each message, and flush.
- **Requests and notifications.** A request has an `id` and gets one response. A notification has none and gets no response. Either side may send both.
- **Size.** A line over **4 MB** is skipped, and wai answers `{"id": null, "error": {"code": -32600, "message": "Message larger than 4 MB"}}`. wai's own messages are small.
- **Errors wai returns:**

  | Code | Meaning |
  |---|---|
  | `-32700` | Not JSON (`"id": null`) |
  | `-32600` | Not a JSON-RPC message, or a line over 4 MB (`"id": null`) |
  | `-32601` | `Method not found: <method>` |
  | `-32602` | Bad params, or an unknown panel id |
  | `-32001` | `Permission denied: <permission>` |
  | `-32000` | wai could not do it (the message says why) |

- **Your own input.** wai holds up to 256 messages for an extension that does not read its stdin; past that it drops them and logs it. Answer wai's requests promptly.

### Host → extension

| Method | Kind | Params | Notes |
|---|---|---|---|
| `initialize` | request, id `0` | `{"protocol": 1, "api": "1.3", "wai_version": "1.2.3", "ext_id", "granted": [permissions], "project": "<folder>", "ext_dir": "<folder>", "data_dir": "<folder>"}` | Answer within **10 s** with any result (`{}` is fine), or the extension is marked `Failed`. An error answer also fails it (`initialize failed: <message>`) |
| `command/run` | request | `{"id": "<command id>"}` | The user ran one of your commands without `prompt` from the palette. wai ignores the answer |
| `panel/action` | request | `{"panel": "<panel id>", "action_id": "<id>", "values": {…}}` | A list item, card, table row, button or menu item was clicked (after its confirmation, if it has one). `action_id` is its `id`. `values` has every form field in the panel by id: inputs as strings, selects as the chosen option (or `null`), checkboxes as booleans. Needs `panels` |
| `item/action` | request | `{"item": "<item id>"}` | The user clicked one of your bar items. wai ignores the answer. The usual reply is a `panel/open` or a `sessions/open`. Needs `bar` |
| `event/session_state` | notification | `{"agent", "session", "state"}` | `state` is `idle`, `working`, `needs_you` or `error`. Needs `sessions:read` |
| `event/turn_ended` | notification | `{"agent", "session", "reason"}` | `reason` is `end_turn`, `cancelled`, `max_tokens`, `refusal`, or the agent's own. Needs `sessions:read` |
| `event/file_saved` | notification | `{"path": "<absolute path>"}` | The user saved a file in wai |

- Paths are absolute, and on Windows they never carry the `\\?\` prefix.
- Answer `initialize` before anything else. API 1.1, 1.2 and 1.3 only add fields and methods; ignore the ones you do not know.

### Extension → host

| Method | Needs | Params | Result |
|---|---|---|---|
| `sessions/list` | `sessions:read` | none | `[{"agent", "session", "title" (string or null), "state"}]`, one per open session |
| `sessions/open` | `sessions:control` | `{"agent"?: string, "prompt"?: string, "send"?: bool}` | `{"agent", "session"}` once the session has started |
| `sessions/prompt` | `sessions:control` | `{"agent", "session", "text"}` | `null` at once |
| `notify` | `notify` | `{"title", "body"?}` | `null` |
| `panel/render` | `panels` | `{"panel": "<panel id>", "tree": {…}}` | `null` |
| `panel/open` | `panels` | `{"panel": "<panel id>"}` | `null` once shown |
| `item/update` | `bar` | `{"item": "<item id>", "text"?, "icon"?, "tooltip"?, "tone"?}` | `null` |

**`sessions/open`** starts a session with `agent`, or with the user's default agent.
- **The prompt:** `prompt` is filled in and not sent, unless `send` is `true`.
- **Pane and focus:** the session never takes the keyboard focus or the focused pane, and never replaces a session the user started. It goes to the pane of your previous session, else an empty pane, else a new pane while there are fewer than four. When there is no room, it opens only in the sidebar's Open list.
- **Limits:**
  - at most **one every 2 seconds** per extension, or `Too many sessions opened; try again later`;
  - `No agent named <agent>` for an unknown agent;
  - `A session is already starting there` while your previous one is still starting;
  - `The session was closed` if the user closed it while it started.

**`sessions/prompt`** sends `text` to the session if it is idle. Otherwise the prompt is dropped (logged), and the result is still `null`.

**`notify`:**
- It shows a system notification. The title is cut to 100 characters and the body to 300, each on one line.
- At most **one every 2 seconds** per extension; extra ones are dropped and logged.

**`panel/render`** replaces a panel's tree (see [Panels](#panels)).
- At most **100 renders per second** per extension: extra renders are coalesced, keeping each panel's latest tree, and the last one always lands.
- A panel id not in your manifest gets `-32602`.

**`panel/open`** shows a panel: a sidebar panel becomes the sidebar's tab, a main panel opens in a pane or its pane is focused (see [Panels](#panels)).
- It never takes the keyboard focus.
- At most **one every 2 seconds** per extension, or `Too many panel opens; try again later`.
- A panel id not in your manifest gets `-32602`.
- Typical use: a palette command without `prompt` → `command/run` → `panel/open`.

**`item/update`** replaces a bar item's whole state (see [Bar items](#bar-items)).
- `text` is cut to 32 characters, `tooltip` to 500, each on one line. `icon` is a Lucide name; an unknown name draws no icon. `tone` is `neutral`, `info`, `success`, `warning` or `danger`; anything else is `neutral`.
- It shares `panel/render`'s limit: at most **100 per second** per extension, coalesced with each item keeping its last state, and the last one always lands.
- An item id not in your manifest gets `-32602`.

**Notifications or requests.** You may send any of these as a notification (no `id`) when you do not need the answer, as the examples below do. A denied notification is dropped without an answer.

## Panels

A panel exists while your process runs, and its content is the last tree you rendered. Before your first render it says `Waiting for the extension…`. Each panel has a button in the activity bar (the column of icons on the left) showing its `icon`, or the first letters of its `title` when it has no icon or wai does not know the name.

**Where it shows (`location`):**
- **`sidebar`** (the default): a sidebar tab titled by `title`, cut to 24 characters.
- **`main`:** a pane in the main area, beside the sessions, with a header (icon, title, your extension's name, ×).
  - Its activity-bar button opens the pane, or focuses it if it is open. The button shows selected while the pane is on screen. There is at most one pane per panel.
  - It takes the focused pane, as a session does; the session there stays open in the sidebar. If the focused pane is the editor, it opens in a new pane beside it.
  - Closing the pane does not stop your extension, and what the user typed stays: the panel is the same when it opens again.
  - The pane closes when your process stops, fails, crashes or is disabled. Panes are not part of work areas and are not saved between runs.

**`icon`:** a [Lucide](https://lucide.dev/icons) icon name in kebab-case, such as `git-pull-request`, `database` or `rocket`. wai ships the whole Lucide set it bundles; an unknown name falls back to the initials and does not fail the install. The `icon` node and the `icon` fields of nodes take the same names.

### Trees

A tree is JSON nodes. Every field without `?` is required.

**Layout:**

| `type` | Fields | Notes |
|---|---|---|
| `column` | `children` (array of nodes) | Stacks children vertically |
| `row` | `children` (array of nodes) | Places children side by side; they wrap when there is no room |
| `card` | `id`?, `title`?, `subtitle`?, `badge`?, `icon`?, `children`? | A bordered card: a header (icon, title, subtitle, badge), then its children. With `id`, the whole card is clickable (also with Enter or Space) and sends `panel/action` with that `id`. Card ids may repeat |
| `section` | `id`, `title`, `open`? (bool, default `true`), `badge`?, `children` | A block the user can collapse. `open` is an initial value (see the rules) |
| `divider` | | A horizontal line |
| `spacer` | | In a row, takes the free space: what follows goes to the end |

**Content:**

| `type` | Fields | Notes |
|---|---|---|
| `text` | `text`, `muted`? (bool), `mono`? (bool) | Up to 10,000 characters. `mono`: monospace |
| `markdown` | `text` | Up to 10,000 characters |
| `badge` | `text`, `tone`? | |
| `icon` | `name`, `tone`? | An unknown name shows as `Unsupported element` |
| `link` | `text`, `url` | Opens `url` in the default browser; its tooltip shows the URL. Only `https://` and `http://` URLs, of at most 2,048 characters; anything else is unsupported. Sends no action |
| `alert` | `tone`, `title`?, `text` | A coloured notice. `tone` is `info`, `success`, `warning` or `danger`. `text` wraps, up to 10,000 characters |
| `kv` | `items` (array of `{"key", "value"}`) | Keys and values side by side, one line each. At most 200 items |
| `progress` | `value` (number) | From 0 to 1, clamped |
| `spinner` | `label`? | A loading indicator |

**Lists, tables and logs:**

| `type` | Fields | Notes |
|---|---|---|
| `list` | `items`, `fill`? (bool) | Items: `{"id", "title", "subtitle"?, "badge"?}`; clicking one sends `panel/action` with its `id`. At least 160 px wide |
| `table` | `columns` (array of `{"title", "width"?}`), `rows` (array of `{"id"?, "cells"}`), `fill`? | `width` is in pixels; columns without one share the rest. A cell is a string (one line) or a `text`, `badge`, `link`, `icon`, `button`, `menu`, `spinner` or `progress` node; any other node is unsupported. Extra cells are dropped, missing ones are blank. A row with `id` is clickable and sends `panel/action` with that `id` (row ids may repeat). At most 20 columns and 1,000 rows; the rest are dropped |
| `log` | `text`, `follow`? (bool, default `true`), `fill`? | Monospace output, one line per line, with a **Copy** button. Keeps the **last** 100,000 characters, and ANSI escape codes (colours) are removed. With `follow` it stays at the bottom as you re-render it, unless the user scrolled up |

Without `fill`, a list, table or log shows 12 rows (lines) and scrolls the rest. With `fill` it takes the panel's remaining height; that works only when every node above it is a `column` (the panel's root included), and is ignored anywhere else.

**Controls:**

| `type` | Fields | Notes |
|---|---|---|
| `button` | `id`, `label`, `style`?, `icon`?, `disabled`? (bool), `confirm`? | `style`: `primary`, `danger`, or default. Clicking sends `panel/action` with its `id` |
| `menu` | `label`?, `icon`? (default `ellipsis`), `items` (array of `{"id", "label", "danger"?, "confirm"?}`) | A button that opens a dropdown. Picking an item sends `panel/action` with the item's `id` (item ids may repeat). At most 50 items |
| `input` | `id`, `placeholder`, `value`?, `multiline`? (bool), `rows`? (number) | A text field. With `multiline`, a text area of `rows` lines (default 4, at most 20). Its value arrives in `values` |
| `select` | `id`, `options` (array of strings), `value`? | At most 100 options; with no choice, the first |
| `checkbox` | `id`, `label`, `checked`? (bool) | |

**`tone`** is `neutral` (the default), `info`, `success`, `warning` or `danger`, in the theme's colours.

**`confirm`** on a `button` or a `menu` item is `{"title", "text"?, "confirm_label"?}`. The click opens a dialog with your extension's name, the title and the text, **Cancel**, and a button labelled `confirm_label` (default `Confirm`), red when the button is `danger` style or the item is `danger`. Only that button sends `panel/action`, with the form's values at that moment; Cancel, Esc or a click outside send nothing. A `confirm` that is present but malformed makes the node unsupported, so a broken confirmation never turns into no confirmation.

**Clicks inside clickable things:** a click on a button, menu, link or form field inside a clickable card or table row goes to that control only.

### Rules

- **Required fields:** a node with a missing or mistyped field, or an unknown `type`, shows as `Unsupported element`. Fields the schema does not name are ignored. A node type newer than the user's wai also shows as unsupported; declare `api` to get a clear refusal instead.
- **Text:** strings other than `text`, `markdown` and `alert`'s `text` are one line of at most 500 characters.
- **Ids:** ids are 1–256 bytes. Buttons, form fields and sections share one set of ids per tree, and a repeated id makes that node unsupported. List item, card, table row and menu item ids may repeat.
- **Size:** a tree deeper than 32 levels or with more than 2,000 nodes is refused whole, and the panel shows `Panel too large`. A node in a table cell counts as a node; string cells, columns, rows, `kv` items and menu items do not (their own caps bound them).
- **Initial values:** `value`, `checked` and a section's `open` apply when the node first appears, or when you send a *different* initial value. Otherwise what the user typed, chose or collapsed stays across your renders.

### Two complete trees

They are the [samples](samples) wai's tests render. A list of open pull requests ([`samples/meus-prs.json`](samples/meus-prs.json)), for a `main` panel:

```json
{
  "type": "column",
  "children": [
    {
      "type": "row",
      "children": [
        { "type": "icon", "name": "git-pull-request", "tone": "info" },
        { "type": "text", "text": "Open pull requests" },
        { "type": "badge", "text": "3" },
        { "type": "spacer" },
        { "type": "button", "id": "refresh", "label": "Refresh", "icon": "refresh-cw" }
      ]
    },
    {
      "type": "table",
      "fill": true,
      "columns": [
        { "title": "Repo", "width": 110 },
        { "title": "Title" },
        { "title": "CI", "width": 80 },
        { "title": "Updated", "width": 70 },
        { "title": "", "width": 36 }
      ],
      "rows": [
        {
          "id": "pr:wai#42",
          "cells": [
            "wai",
            { "type": "link", "text": "Extension panels v2", "url": "https://github.com/example/wai/pull/42" },
            { "type": "badge", "text": "passing", "tone": "success" },
            "2 h",
            {
              "type": "menu",
              "items": [
                { "id": "checkout:wai#42", "label": "Check out in a session" },
                {
                  "id": "close:wai#42",
                  "label": "Close",
                  "danger": true,
                  "confirm": { "title": "Close pull request #42?", "text": "Its branch stays.", "confirm_label": "Close" }
                }
              ]
            }
          ]
        },
        {
          "id": "pr:api#317",
          "cells": [
            "api",
            { "type": "link", "text": "Paginate the inventory list", "url": "https://github.com/example/api/pull/317" },
            { "type": "spinner", "label": "running" },
            "5 h",
            { "type": "menu", "items": [{ "id": "checkout:api#317", "label": "Check out in a session" }] }
          ]
        },
        {
          "id": "pr:web#88",
          "cells": [
            "web",
            { "type": "link", "text": "Fix the date filter", "url": "https://github.com/example/web/pull/88" },
            { "type": "badge", "text": "failing", "tone": "danger" },
            "1 d",
            { "type": "menu", "items": [{ "id": "checkout:web#88", "label": "Check out in a session" }] }
          ]
        }
      ]
    }
  ]
}
```

A database restore form with a confirmed danger button, progress and output ([`samples/restore.json`](samples/restore.json)):

```json
{
  "type": "column",
  "children": [
    {
      "type": "alert",
      "tone": "warning",
      "title": "Staging only",
      "text": "Restoring replaces the target database."
    },
    {
      "type": "card",
      "title": "Restore a backup",
      "subtitle": "From the nightly snapshots",
      "icon": "database",
      "children": [
        { "type": "select", "id": "env", "options": ["staging", "dev"], "value": "staging" },
        { "type": "input", "id": "snapshot", "placeholder": "Snapshot id", "value": "2026-11-12" },
        { "type": "input", "id": "note", "placeholder": "Why (optional)", "multiline": true, "rows": 3 },
        { "type": "checkbox", "id": "anonymize", "label": "Anonymize personal data", "checked": true },
        {
          "type": "row",
          "children": [
            { "type": "spacer" },
            {
              "type": "button",
              "id": "restore",
              "label": "Restore",
              "style": "danger",
              "icon": "rotate-ccw",
              "confirm": { "title": "Restore staging?", "text": "Staging's data is replaced by the snapshot.", "confirm_label": "Restore" }
            }
          ]
        }
      ]
    },
    {
      "type": "section",
      "id": "last",
      "title": "Last run",
      "badge": "running",
      "open": true,
      "children": [
        {
          "type": "kv",
          "items": [
            { "key": "Started", "value": "14:02" },
            { "key": "By", "value": "you" }
          ]
        },
        { "type": "progress", "value": 0.4 },
        { "type": "divider" },
        { "type": "log", "text": "pg_restore: connecting\npg_restore: creating TABLE inventories\npg_restore: \u001b[33mwarning\u001b[0m: 2 roles skipped\n", "follow": true }
      ]
    }
  ]
}
```

## Bar items

An item is a small pill in the top bar or the bottom bar, declared in the manifest with `contributes.items` (at most 3 per extension; more is an invalid manifest, and so is `items` without the `bar` permission).

- **Content:** none until your first `item/update`. An update with neither `text` nor `icon` hides the item. The user clicking it sends `item/action`.
- **Lifetime:** an item is drawn only while your process runs, and waits for a new `item/update` after a restart.
- **Top bar:** after the status pill, at most 4 extension items in total, text drawn at 24 characters. **Bottom bar:** after wai's own items.
- **Telling them apart:** each item's tooltip ends with `— <extension name>`. The user can hide any item with a right click on the bar.

```toml
permissions = ["process", "bar"]
api = "1.3"

[[contributes.items]]
id = "prs"
bar = "bottom"
title = "Pull requests"
```

```json
{"jsonrpc": "2.0", "method": "item/update", "params": {"item": "prs", "text": "3 PRs", "icon": "git-pull-request", "tooltip": "2 green, 1 failing", "tone": "warning"}}
```

## Contributions

- **Templates** appear after the user's and the project's, in **New from template** and the palette.
- **Commands** appear in the palette under **Commands**.
  - A command with `prompt` opens a session with the default agent and the prompt filled in, not sent.
  - A command without `prompt` sends `command/run` to your process. It is listed only while the process runs.
- **Agents** become `<id>/<name>`. The user can pick them from the palette or name them in templates.
  - `command` is one program, with no arguments. A wrapper script is the usual way to pass arguments.
  - The agent starts from the config folder, like any agent.
- **MCP servers** are passed to every session wai starts, loads or forks, for every agent. The agent starts them over stdio in the project folder, with your `args` and `env`.
- **Claude plugins** are listed under the extension in the Extensions view with **Install in terminal**.
  - That types `claude plugin install <name>` into wai's terminal **without pressing Enter**, so the user confirms.
  - A name outside `[A-Za-z0-9._@/-]`, longer than 214 characters, or starting with `-` shows `Invalid name`.
  - The view lists up to 20.

Agents and MCP servers come only from enabled extensions that were granted `process`. A project's `.wai/` can never add them.

## Security model

**What wai enforces:**
- **Consent:** nothing installs or runs without the user's consent, and a project can only suggest an extension.
- **Permission checks:** every call in both directions is checked against the granted permissions.
- **Neutral start:** wai starts agents and extension processes from the config folder, and bare names come only from absolute `PATH` folders, so a repository cannot plant a program that wai itself starts. MCP servers are started by the agent, in the project folder (see below).
- **Hardened installs:**
  - git runs with a fixed argument list and no shell;
  - only https and ssh are allowed, and ssh runs in batch mode (no prompts);
  - LFS filters do not run, and the system gitconfig is not read;
  - symlinks are refused.
- **Bounded input:** message size, render rate, notification and session-open rates, panel tree size and text length are all capped, and panel trees are parsed off the UI thread. A broken or hostile panel shows as unsupported or `Panel too large`; it cannot crash wai.
- **Clean stops:** an extension's whole process tree dies with it.
- **Claude plugins** are never installed without the user pressing Enter.

**What v1 does not isolate:**
- **Your rights:** the extension's process runs as you. It can read and write any file you can, use the network, read the environment it inherits, and start any program.
- **`network` and `fs:`** are declarations only.
- **Contributed agents and MCP servers** run with the same rights, inside every agent session.
- **MCP servers run in the project folder:** the agent starts them there, so a launcher that reads its current folder picks up what the repository put in it. npx, for one, reads the project's `.npmrc` and runs a matching package from its `node_modules`. An npx-based server should pass `--prefix=/` before the package, as wai-navegador does: `args = ["--prefix=/", "-y", "<package>@<version>"]`.
- **The project's Claude settings:** claude-agent-acp itself already loads the project's own Claude settings (`.claude/`), as Claude Code does in a terminal. wai neither adds to nor limits what they can do.
- **Windows:** a child that the extension starts in its first instant, before it joins the job object, can outlive it.

Real isolation (a sandbox) is planned for after a public catalogue exists.

## A minimal Go extension

It shows a panel with a counter and a button. The button bumps the counter and sends a notification.

`wai-extension.toml`:

```toml
id = "hello-go"
name = "Hello (Go)"
version = "0.1.0"
description = "A minimal wai extension"
permissions = ["process", "panels", "notify"]

[process]
command = "bin/hello-go"

[[contributes.panels]]
id = "main"
title = "Hello"
```

`main.go`:

```go
// A minimal wai extension: one panel with a button that counts clicks and notifies.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
)

type message struct {
	ID     json.RawMessage `json:"id,omitempty"`
	Method string          `json:"method,omitempty"`
	Params json.RawMessage `json:"params,omitempty"`
}

type obj = map[string]any

// Encode writes one compact JSON value and a newline: one message per line.
var out = json.NewEncoder(os.Stdout)

func send(m obj) {
	m["jsonrpc"] = "2.0"
	out.Encode(m)
}

func render(clicks int) {
	send(obj{"method": "panel/render", "params": obj{"panel": "main", "tree": obj{
		"type": "column", "children": []obj{
			{"type": "text", "text": fmt.Sprintf("Clicked %d times", clicks)},
			{"type": "button", "id": "hi", "label": "Say hi", "style": "primary"},
		}}}})
}

func main() {
	clicks := 0
	in := bufio.NewScanner(os.Stdin)
	in.Buffer(make([]byte, 64*1024), 4<<20) // wai sends lines of up to 4 MB
	for in.Scan() {
		var m message
		if json.Unmarshal(in.Bytes(), &m) != nil || m.Method == "" {
			continue // not a call: a response, or junk
		}
		switch m.Method {
		case "initialize":
			send(obj{"id": m.ID, "result": obj{}})
			render(clicks)
		case "panel/action":
			send(obj{"id": m.ID, "result": nil})
			var p struct {
				ActionID string `json:"action_id"`
			}
			json.Unmarshal(m.Params, &p)
			if p.ActionID == "hi" {
				clicks++
				render(clicks)
				send(obj{"method": "notify", "params": obj{"title": "Hello", "body": "Hi from Go"}})
			}
		default:
			if m.ID != nil { // a request this extension does not serve
				send(obj{"id": m.ID, "error": obj{"code": -32601, "message": "Method not found"}})
			}
		}
	}
}
```

Build it, then try it by hand before installing:

```sh
go mod init hello-go
go build -o bin/hello-go .
printf '%s\n' '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":1}}' \
  '{"jsonrpc":"2.0","id":1,"method":"panel/action","params":{"panel":"main","action_id":"hi","values":{}}}' \
  | ./bin/hello-go
```

It answers `initialize`, renders "Clicked 0 times", answers the action, renders "Clicked 1 times" and sends `notify`. Then install the folder in wai: **Extensions → Install from folder…**.

## A minimal Rust extension

The same extension, with `serde_json`. The installable folder is `ext/`, which holds only the manifest and the binary, so the build output stays out of it.

```sh
cargo new hello-rs
cd hello-rs
cargo add serde_json
```

`src/main.rs`:

```rust
//! A minimal wai extension: one panel with a button that counts clicks and notifies.

use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

/// One compact JSON value and a newline: one message per line.
fn send(msg: Value) {
    let mut out = io::stdout().lock();
    writeln!(out, "{msg}").and_then(|()| out.flush()).ok();
}

fn render(clicks: u32) {
    send(json!({"jsonrpc": "2.0", "method": "panel/render", "params": {"panel": "main", "tree": {
        "type": "column", "children": [
            {"type": "text", "text": format!("Clicked {clicks} times")},
            {"type": "button", "id": "hi", "label": "Say hi", "style": "primary"},
        ]}}}));
}

fn main() {
    let mut clicks = 0;
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        let id = msg.get("id").cloned();
        match msg["method"].as_str() {
            Some("initialize") => {
                send(json!({"jsonrpc": "2.0", "id": id, "result": {}}));
                render(clicks);
            }
            Some("panel/action") => {
                send(json!({"jsonrpc": "2.0", "id": id, "result": null}));
                if msg["params"]["action_id"] == "hi" {
                    clicks += 1;
                    render(clicks);
                    let params = json!({"title": "Hello", "body": "Hi from Rust"});
                    send(json!({"jsonrpc": "2.0", "method": "notify", "params": params}));
                }
            }
            // A request this extension does not serve; notifications (`event/*`) need no answer.
            Some(_) if id.is_some() => send(json!({"jsonrpc": "2.0", "id": id,
                "error": {"code": -32601, "message": "Method not found"}})),
            _ => {} // a response to a request of ours, or junk
        }
    }
}
```

`ext/wai-extension.toml`: the Go manifest with `id = "hello-rs"`, `name = "Hello (Rust)"` and `command = "bin/hello-rs"`.

```sh
cargo build --release
mkdir -p ext/bin && cp target/release/hello-rs ext/bin/
printf '%s\n' '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":1}}' | ext/bin/hello-rs
```

Then install `ext/` from **Extensions → Install from folder…**.

On Windows, a binary built there is `hello-rs.exe`, and the manifest's `command` must name it that way.
