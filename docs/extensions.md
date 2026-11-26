# Writing wai extensions

An extension is a folder with a `wai-extension.toml` manifest. It can be manifest-only (templates, prompt commands, agents, MCP servers) or run a program of its own. That program can be in any language: it speaks line-delimited JSON-RPC 2.0 over stdio and can draw panels in wai's sidebar, main area or prompt box and show items in its top and bottom bars.

Users install an extension from a folder, from a git URL, or from the Extensions page's **Available** list, which downloads a prebuilt package from an index. Anyone can publish one: see [Publishing](#publishing).

This guide describes API version 1.11 (protocol 1), as implemented by the latest wai release. The [first-party extensions](../extensions) are working references:
- `wai-navegador` is manifest-only;
- `wai-rotinas` and `wai-bichinho` are written in Go; `wai-bichinho` shows a pet in a dock at the sidebar's foot (API 1.9) and speaks in a `bubble` (API 1.8).

## The manifest

`wai-extension.toml`, at the top of the extension's folder:

```toml
id = "hello-go"                 # required
name = "Hello (Go)"             # required
version = "0.1.0"               # required
description = "A minimal wai extension"
api = "1.4"                     # optional: the oldest extension API it works with
permissions = ["process", "panels", "bar", "notify"]

[process]                       # optional: the extension's own program
command = "bin/hello-go"
args = []

[[contributes.panels]]
id = "main"
title = "Hello"
icon = "hand"                   # optional: a Lucide icon for the activity bar
location = "main"               # optional: "sidebar" (default), "main", "prompt" or "prompt-side"
# wide = true                   # optional, sidebar only: takes the main area's place while shown

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
| `api` | string, optional | The oldest extension API the extension needs, `major.minor` (`"1.4"`). Absent means 1.0. A wai with an older API refuses the extension: `<name> needs a newer wai (extension API <api>; this wai has 1.11)`. Set it to `"1.11"` when you use `sessions/open` `background`, `sessions/show`, `tabs` `end` or the `switch` node, to `"1.10"` when you use the `listen` or `sessions:content` permission, `sessions/read`, `sessions/cancel`, `permission/answer` or `question/answer`, to `"1.9"` when you use a `sidebar-foot` panel or the `pixels` node, to `"1.8"` when you use a `plain` list, list item `icon`, `tone`, `selected`, `lead`, `note` or `indent`, a `text` `style`, a `column` `framed`, `title`, `badge`, `tone` or `collapsed`, `tabs` `style` or a tab `tone`, `select` `segmented`, `input` `icon`, `submit` or `clear`, `button` `icon_only`, `link` `icon` or `icon_only`, or the `bubble` node, to `"1.7"` when you use `events/emit`, an `events:<id>` permission or a panel's `frame`, to `"1.6"` when you use a `prompt` or `prompt-side` panel, `event/session_focused`, the `path` node, `wide`, `row.fill`, `column.width` or `card.selected`, to `"1.5"` when you use `tabs`, to `"1.4"` when you use `card`, and to `"1.3"` or later when you use `items` or the `bar` permission, so an older wai refuses the extension with that message instead of an unknown-field error. Anything not `major.minor` is an invalid manifest. wai 0.8 and older do not know the field and refuse the manifest |
| `permissions` | array of strings | See [Permissions](#permissions). An unknown permission is refused |
| `process.command` | string | See [Commands and paths](#commands-and-paths). Needs `process` |
| `process.args` | array of strings, optional | Arguments, passed as given (no shell) |
| `contributes.templates` | array of tables | `name`, optional `agent`, optional `prompt`. Same as `templates.toml` in the [user guide](user-guide.md#templates) |
| `contributes.commands` | array of tables | `id`, `title`, optional `prompt`. See [Contributions](#contributions) |
| `contributes.agents` | array of tables | `name`, `command`. Needs `process` |
| `contributes.mcp_servers` | array of tables | `name`, `command`, optional `args` (array), optional `env` (table of strings). Needs `process` |
| `contributes.claude_plugins` | array of strings | Claude Code plugin names to suggest |
| `requires.claude_plugins` | array of tables | `name`, `marketplace` (`owner/repo` or an `https://` URL of letters, digits and `._~:/@%+=,-`). At most 5. API 1.5. Installed after consent, see [Installing and consent](#installing-and-consent) |
| `contributes.panels` | array of tables | `id`, `title`, optional `icon`, optional `location`, optional `width` (prompt panels only), optional `wide` (sidebar panels, API 1.6), optional `frame` (`prompt-side` panels, API 1.7). At most one each of `prompt`, `prompt-side` and `sidebar-foot` panel; `width`, `wide` and `frame` are rejected on `sidebar-foot`. See [Panels](#panels). Needs `panels` |
| `contributes.items` | array of tables | `id`, `bar` (`"top"` or `"bottom"`), `title`. At most 3. See [Bar items](#bar-items). Needs `bar` |
| `contributes.themes` | array of strings | Paths, relative to the extension's folder, of theme-set `.json` files (same format as `~/.config/wai/themes/*.json`, see the [user guide](user-guide.md#appearance)). They show in Settings > Appearance while the extension is enabled. A path that is missing, outside the folder or not a theme set is skipped with a notice |

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
- wai opened from Finder or the Dock takes its environment, `PATH` and the tokens your rc files export, from your login shell, cached from the previous launch. With no cache and no answer within 3 seconds, wai keeps macOS's default (`PATH` `/usr/bin:/bin:/usr/sbin:/sbin`, which has no `npx`). Started from a terminal, wai has the terminal's `PATH`. See the [user guide](user-guide.md#agents-and-path).

## Permissions

Calls in both directions are checked against what the user **granted** at install time, not against the manifest on disk. A denied request is answered with error `-32001`, `Permission denied: <permission>`.

| Permission | What it allows |
|---|---|
| `process` | Run `[process]`. Also required to contribute `agents` and `mcp_servers`, which run code |
| `sessions:read` | `sessions/list`. Receive `event/session_state`, `event/turn_ended` and `event/session_focused` |
| `sessions:content` | `sessions/read`. Receive `event/session_update`, `event/permission_requested`, `event/permission_resolved`, `event/question_requested` and `event/question_resolved`: what the user and the agents write. Shown as *Read your sessions' conversations*. It does not include `sessions:read`. API 1.10 |
| `sessions:control` | `sessions/open`, `sessions/prompt`, and (API 1.10) `sessions/cancel`, `permission/answer` and `question/answer`, and (API 1.11) `sessions/show`. It does not include `sessions:read` |
| `notify` | `notify`: system notifications |
| `panels` | Contribute panels, `panel/render`, `panel/open`, and receive `panel/action` |
| `bar` | Contribute bar items, `item/update`, and receive `item/action`. Shown as *Show items in the top and bottom bars* |
| `network` | Nothing is enforced. It tells the user the extension uses the network |
| `listen` | Nothing is enforced. It tells the user the extension listens for connections on a network port. Shown as *Listen for connections on a network port*. API 1.10 |
| `fs:<path>` | Nothing is enforced. It tells the user the extension uses `<path>`, which must be a relative path inside the extension's folder |
| `events:<id>` | Receive `event/ext` from extension `<id>` (see [Events between extensions](#events-between-extensions)). `<id>` need not be installed. API 1.7 |

`event/file_saved`, `command/run` and `initialize` need no permission.

## Installing and consent

- **Sources.** Users install from a folder, from a git URL (`https://…` or `git@host:path`), or from an index's listing. wai copies the folder, clones the repository with depth 1, or downloads the listing's package for the user's platform and checks it against the SHA-256 the index pins, then unpacks it into a staging area and reads the manifest. A package whose manifest id is not its listing's id is refused.
- **The consent dialog.** It lists every permission in plain words, then the warning `wai cannot restrict files or network access in this version; only install extensions you trust.`, the source, and the description.
  - A required Claude Code plugin is one more line: `Install Claude Code plugin <name> from <marketplace>`.
  - Nothing is installed or run before **Install**.
  - Consent is all or nothing: the extension gets exactly what it asks for.
- **Required Claude plugins.** After **Install** (and **Update**) commits the extension, wai runs, off the main thread, `claude plugin install <name> --marketplace <marketplace> --scope user --json` for each one.
  - It skips a plugin that `claude plugin list --json` already shows (an id starting with `<name>@`).
  - stdin is closed, `GIT_TERMINAL_PROMPT=0` is set so a private marketplace never waits for a password, the working folder is the config folder, and it is killed after 5 minutes.
  - wai never passes `-y`: a plugin that runs a command its marketplace declares needs a person to confirm, so `claude` refuses without a terminal and wai reports a failure. **Install in terminal** then runs it with the user at the prompt.
  - A notification says `Installed Claude Code plugin <name>` or `Could not install Claude Code plugin <name>: <reason>`. The extension stays installed and enabled either way.
- **What a stage refuses:**
  - symbolic links anywhere in the folder;
  - anything that is not a regular file or a folder;
  - more than 100 MB or 10,000 entries;
  - in a package, also absolute paths, `..` and an entry listed twice.

  A folder is copied whole, so keep build output (`target/`, `node_modules/`) out of the folder you install.
- **Where things go.** The extension goes to `<config>/extensions/<id>/`, its data folder is `<config>/extension-data/<id>/` and the consent goes to `<config>/extensions.json`. It starts enabled.
- **It is a copy.** Rebuilding in your source folder does not change the installed extension.
- **Updating.** Install the same id again (folder, git URL, or **Update to <version>** in the Available list). wai asks for consent again, stops the extension, replaces its folder and starts it again. It stays enabled or disabled as it was, and its data folder is untouched. A project suggestion of an installed id also goes through this.
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
| `initialize` | request, id `0` | `{"protocol": 1, "api": "1.11", "wai_version": "1.2.3", "ext_id", "granted": [permissions], "project": "<folder>", "ext_dir": "<folder>", "data_dir": "<folder>"}` | Answer within **10 s** with any result (`{}` is fine), or the extension is marked `Failed`. An error answer also fails it (`initialize failed: <message>`) |
| `command/run` | request | `{"id": "<command id>"}` | The user ran one of your commands without `prompt` from the palette. wai ignores the answer |
| `panel/action` | request | `{"panel": "<panel id>", "action_id": "<id>", "values": {…}}` | A list item, card, table row, button or menu item was clicked (a button or menu item with a `confirm` only after the user confirms), or a `switch` changed. `action_id` is its `id`. `values` has every form field in the panel by id: inputs and paths as strings, selects as the chosen option (or `null`), checkboxes as booleans. Needs `panels`. A `bubble` sends one when it is gone: `action_id` is its `id`, and `values` is `{"bubble": "click"}` or `{"bubble": "timeout"}` |
| `item/action` | request | `{"item": "<item id>"}` | The user clicked one of your bar items. wai ignores the answer. The usual reply is a `panel/open` or a `sessions/open`. Needs `bar` |
| `event/session_state` | notification | `{"agent", "session", "state"}` | `state` is `idle`, `working`, `needs_you` or `error`. Needs `sessions:read` |
| `event/turn_ended` | notification | `{"agent", "session", "reason"}` | `reason` is `end_turn`, `cancelled`, `max_tokens`, `refusal`, or the agent's own. Needs `sessions:read` |
| `event/file_saved` | notification | `{"path": "<absolute path>"}` | The user saved a file in wai |
| `event/session_focused` | notification | `{"agent", "session"}` | The focused pane's session, the one whose prompt box shows your prompt panels; both `null` when the focused pane is not a session (a draft, a file, a panel) or the main area is closed. Sent when it changes, and once after `initialize` when wai knows it. API 1.6. Needs `sessions:read` |
| `event/ext` | notification | `{"source", "name", "data"}` | An event another extension emitted. API 1.7. Needs `events:<source>` |
| `event/session_update` | notification | `{"agent", "session", "total", "items": [{"index", "item"}]}` | Transcript items that are new or changed, each with its index (see [Transcript items](#transcript-items)); `total` is the transcript's length now. At most **10 per second** per session: changes meanwhile join the next one. API 1.10. Needs `sessions:content` |
| `event/permission_requested` | notification | `{"agent", "session", "request", "title", "options": [{"id", "name", "kind"}]}` | An agent asks to run a tool call. `request` is a number, unique per agent (not per session); `title` is the tool call's. `kind` is `allow_once`, `allow_always`, `reject_once` or `reject_always`. API 1.10. Needs `sessions:content` |
| `event/permission_resolved` | notification | `{"agent", "session", "request", "option"}` | The request closed: `option` is the chosen option's `id`, whether the user chose it in wai or an extension did, or `null` when the agent withdrew it (a cancelled turn, say). API 1.10. Needs `sessions:content` |
| `event/question_requested` | notification | `{"agent", "session", "request", "message", "fields": [{"key", "title", "description", "required", "kind", "choices": [{"value", "label", "description"}]}]}` | An agent asks the user to fill a form. `kind` is `single`, `multi`, `text`, `boolean` or `number`; `choices` is empty but for `single` and `multi`; `description` may be `null`. API 1.10. Needs `sessions:content` |
| `event/question_resolved` | notification | `{"agent", "session", "request", "answer"}` | The question closed: `answer` is `accept`, `decline` or `cancel` (also when the agent withdrew it). API 1.10. Needs `sessions:content` |

- Paths are absolute, and on Windows they never carry the `\\?\` prefix.
- Answer `initialize` before anything else. API 1.1 to 1.11 only add fields and methods; ignore the ones you do not know.

### Extension → host

| Method | Needs | Params | Result |
|---|---|---|---|
| `sessions/list` | `sessions:read` | none | `[{"agent", "session", "title" (string or null), "state"}]`, one per open session |
| `sessions/open` | `sessions:control` | `{"agent"?: string, "prompt"?: string, "send"?: bool, "background"?: bool}` | `{"agent", "session"}` once the session has started |
| `sessions/prompt` | `sessions:control` | `{"agent", "session", "text"}` | `null` at once |
| `sessions/show` | `sessions:control` | `{"agent", "session"}` | `null` once shown. API 1.11 |
| `sessions/read` | `sessions:content` | `{"agent", "session", "from"?: int, "limit"?: int}` | `{"items": [item], "total": int}`. API 1.10 |
| `sessions/cancel` | `sessions:control` | `{"agent", "session"}` | `null`. API 1.10 |
| `permission/answer` | `sessions:control` | `{"agent", "session", "request": int, "option": "<option id>"}` | `null` once the agent has the answer. API 1.10 |
| `question/answer` | `sessions:control` | `{"agent", "session", "request": int, "answer": "accept"\|"decline"\|"cancel", "values"?: {…}}` | `null` once the agent has the answer. API 1.10 |
| `notify` | `notify` | `{"title", "body"?}` | `null` |
| `events/emit` | none | `{"name", "data"?}` | `null` |
| `panel/render` | `panels` | `{"panel": "<panel id>", "tree": {…}}` | `null` |
| `panel/open` | `panels` | `{"panel": "<panel id>"}` | `null` once shown |
| `item/update` | `bar` | `{"item": "<item id>", "text"?, "icon"?, "tooltip"?, "tone"?, "card"?}` | `null` |

**`sessions/open`** starts a session with `agent`, or with the user's default agent.
- **The prompt:** `prompt` is filled in and not sent, unless `send` is `true`.
- **Pane and focus:** the session never takes the keyboard focus or the focused pane, and never replaces a session the user started. It goes to the pane of your previous session, else an empty pane, else a new pane while there are fewer than four. When there is no room, it opens only in the sidebar's Open list.
- **Background:** with `background` (API 1.11) the session takes no pane: it opens in the open list only, as when no pane is free.
- **Limits:**
  - at most **one every 2 seconds** per extension, or `Too many sessions opened; try again later`;
  - `No agent named <agent>` for an unknown agent;
  - `A session is already starting there` while your previous one is still starting;
  - `The session was closed` if the user closed it while it started.

**`sessions/prompt`** sends `text` to the session if it is idle. Otherwise the prompt is dropped (logged), and the result is still `null`.

**`sessions/show`** (API 1.11) does what a click on the session's row in the open list does: it shows the session in the focused pane and focuses it. It counts with `panel/open`: at most one of the two per 2 s. If the session is not open, the error is `No session <agent> <session>`.

**`sessions/read`** returns a page of the session's transcript (see [Transcript items](#transcript-items)): `limit` items from index `from`. Without `from`, the last `limit` items. `limit` defaults to 200 and is at most 500. `total` is the transcript's length, so the page starts at `total` minus the number of items when you left out `from`. A session that is not open is an error, `No open session <agent> <session>`; so is it for `sessions/cancel`, `permission/answer` and `question/answer`.

**`sessions/cancel`** stops the session's running turn, as wai's Stop does: the turn ends with `event/turn_ended` reason `cancelled`. A session with no turn running is left alone, and the result is still `null`.

**`permission/answer`** answers a pending permission request with one of its options' `id`, as a click on that option in wai: the agent gets it, and wai closes the card as answered.
- The first answer wins. A request already answered, in wai or by an extension, withdrawn, or never asked is an error, `Request <request> is already answered or unknown`, and the agent keeps the first answer.
- An option the request does not offer is an error, `No option <option> in request <request>`.

**`question/answer`** answers a pending question, as the card's Submit, Decline or Cancel in wai.
- `accept` sends `values`: one per field, by `key`. A `single` field takes a choice's `value`, a `multi` field an array of them, a `text` field a string, a `boolean` field `true` or `false`, a `number` field a number.
- Before it reaches the agent, an `accept` is checked: a required field needs a value (an empty string or array is none), each value must fit its field's kind and choices, and every key must be a field. Otherwise it is an error naming the field, `Field <key>: <why>`, and the question still waits.
- Any other `answer` than `accept`, `decline` or `cancel` is an error.
- As for permissions, the first answer wins: `Request <request> is already answered or unknown`.

#### Transcript items

`sessions/read` and `event/session_update` carry the transcript as wai shows it: the conversation in order, then the permission requests and questions still waiting for an answer. Each item is an object with a `kind`:

| `kind` | Fields |
|---|---|
| `user` | `text`: a message the user sent |
| `agent` | `text`: the agent's answer, as Markdown |
| `thought` | `text`: the agent's thinking |
| `tool` | `title`, `tool_kind` (`read`, `edit`, `execute`, …) and `status` (`pending`, `in_progress`, `completed` or `failed`). Never the tool call's output |
| `permission` | `request`, `title`, `options` (as in `event/permission_requested`) and `answered`: `null` while it waits, then what wai's card shows, the chosen option's name or `Cancelled` |
| `question` | `request`, `message`, `fields` (as in `event/question_requested`) and `answered`: `null` while it waits, then what wai's card shows (`Answered: …`, `Declined` or `Cancelled`) |
| `notice` | `text` and `level` (`info` or `error`): wai's own line, as `Cancelled` |
| `edits` | `files`: the paths a finished turn edited |

An item's index can change only for waiting cards: when one is answered it joins the conversation, and the cards still waiting move after it. `event/session_update` sends every index that changed, so applying its items by index keeps a copy of the transcript in step.

**`notify`:**
- It shows a system notification. The title is cut to 100 characters and the body to 300, each on one line.
- At most **one every 2 seconds** per extension; extra ones are dropped and logged.

**`panel/render`** replaces a panel's tree (see [Panels](#panels)).
- At most **100 renders per second** per extension: extra renders are coalesced, keeping each panel's latest tree, and the last one always lands.
- A panel id not in your manifest gets `-32602`.

**`panel/open`** shows a panel: a sidebar panel becomes the sidebar's tab, a main panel opens in a pane or its pane is focused (see [Panels](#panels)). A prompt panel is always shown with the focused prompt box: `panel/open` does nothing for it.
- It never takes the keyboard focus.
- At most **one every 2 seconds** per extension, shared with `sessions/show`, or `Too many panel opens; try again later`.
- A panel id not in your manifest gets `-32602`.
- Typical use: a palette command without `prompt` → `command/run` → `panel/open`.

**`item/update`** replaces a bar item's whole state (see [Bar items](#bar-items)). `card` (API 1.4) is a display-only tree drawn when the pointer rests on the item.
- `text` is cut to 32 characters, `tooltip` to 500, each on one line. `icon` is a Lucide name; an unknown name draws no icon. `tone` is `neutral`, `info`, `success`, `warning` or `danger`; anything else is `neutral`.
- It shares `panel/render`'s limit: at most **100 per second** per extension, coalesced with each item keeping its last state, and the last one always lands.
- An item id not in your manifest gets `-32602`.

**Notifications or requests.** You may send any of these as a notification (no `id`) when you do not need the answer, as the examples below do. A denied notification is dropped without an answer.

## Events between extensions

An extension sends `events/emit` with a `name` and optional `data`; wai forwards it as `event/ext`, with `source` set to the emitter's id, to every other running extension granted `events:<emitter id>`.

- `name` matches `[a-z0-9][a-z0-9._-]{0,63}`, and `data` is any JSON of at most 16 KiB serialized. A request with either wrong gets an `INVALID_PARAMS` error; a notification is dropped and logged.
- At most 20 events per second per emitter; the excess is dropped and logged.
- An emitter never receives its own events, even with `events:<its own id>`.
- There is no queue: an extension that is not running misses the event, and the source need not be installed.
- wai does not check payloads: the emitter documents its event names and `data` in its README, and listeners ignore names they do not know.

## Panels

A panel exists while your process runs, and its content is the last tree you rendered. Before your first render it says `Waiting for the extension…`. Each sidebar or main panel has a button in the activity bar (the column of icons on the left) showing its `icon`, or the first letters of its `title` when it has no icon or wai does not know the name. Prompt panels have none.

**Where it shows (`location`):**
- **`sidebar`** (the default): a sidebar tab titled by `title`, cut to 24 characters.
  - **`wide = true`** (API 1.6): while the tab is shown, the sessions area hides and the sidebar takes the whole window. Clicking the panel's button again, another sidebar tab, or opening a session, a `main` panel, a file or the terminal brings the sessions back on the Sessions tab. A `main` panel cannot be wide.
- **`main`:** a pane in the main area, beside the sessions, with a header (icon, title, your extension's name, ×).
  - Its activity-bar button opens the pane, or focuses it if it is open. The button shows selected while the pane is on screen. There is at most one pane per panel.
  - It takes the focused pane, as a session does; the session there stays open in the sidebar. If the focused pane is the editor, it opens in a new pane beside it.
  - Closing the pane does not stop your extension, and what the user typed stays: the panel is the same when it opens again.
  - The pane closes when your process stops, fails, crashes or is disabled. Panes are not part of work areas and are not saved between runs.
- **`prompt`** (API 1.6): inside the prompt box of the focused pane's session or draft, at its right, behind a divider. The box grows to the taller of the two.
- **`prompt-side`** (API 1.6): beside that prompt box, at its right, in a box of its own. With `frame = false` (API 1.7) the box is not drawn and your tree floats: anchored to the prompt box's bottom, it grows up over what is above instead of moving it. Use it when your tree's height changes.
  - Only the focused pane's prompt box shows them: when the user focuses another pane, they move there. `event/session_focused` tells you which session that is.
  - `width` is the slot's width in px, 80 to 320 (a value outside is clamped). The default is 160 for `prompt` and 200 for `prompt-side`.
  - They hide while the prompt box would be left narrower than 360 px: in a narrow pane, or a split. `prompt` gives way last.
  - Each location shows one panel: the first one in the extensions list. Another extension's panel there waits until that one stops.
  - Keep the tree small: a few lines of `text` (`mono` for ASCII art), a `progress`, a `badge`, a `menu`. A click in the slot does not move the caret into the prompt box.
- **`sidebar-foot`** (API 1.9): a dock at the bottom of the sidebar, under every tab, with a 1 px top border and its tree's natural height.
  - It has no activity-bar button, and `panel/open` on it does nothing. `width`, `frame` and `wide` are invalid on it.
  - At most one per manifest; across extensions the first one wins, as for the prompt slots.
  - It does not scroll: the tree takes its own height, and a foot's height is capped at half the sidebar: a taller tree overflows its box rather than squeezing the tabs. A `bubble` floats above the dock instead of growing it, and a `menu` shows only while the mouse is over the dock (it keeps its space).

**`icon`:** a [Lucide](https://lucide.dev/icons) icon name in kebab-case, such as `git-pull-request`, `database` or `rocket`. wai ships the whole Lucide set it bundles; an unknown name falls back to the initials and does not fail the install. The `icon` node and the `icon` fields of nodes take the same names. A panel's `icon` may instead be the path of an `.svg` file in your extension's folder (`icons/panel.svg`): a relative path without `..`, at most 32 KB, a regular file (not a symlink). wai draws it as a mask in the activity bar's colours, so its fill colour does not matter, and shows the initials when the file is missing or too big. The shared release workflow packs it.

### Trees

A tree is JSON nodes. Every field without `?` is required.

**Layout:**

| `type` | Fields | Notes |
|---|---|---|
| `column` | `children` (array of nodes), `width`? (px), `framed`?, `title`?, `badge`?, `tone`?, `collapsed`? (bool) | Stacks children vertically. `width` (API 1.6, 24 to 2,000) fixes its width in a row; without it, the columns of a row share the width left. `framed` (API 1.8): rounded, 1 px border, glass background, 12 px padding, at least as tall as the space it gets, so a `spacer` in it pins what follows to the bottom. `title` (API 1.8): a header with a 3 px accent bar in `tone` (default `neutral`, violet), the title as an uppercase label and the `badge` pill at the right; implies `framed`. `collapsed` (API 1.8): a 28 px strip with the `badge` and the title's letters stacked vertically; its children are not drawn, its `width` is ignored; implies `framed` |
| `row` | `children` (array of nodes), `fill`? (bool) | Places children side by side; they wrap when there is no room. `fill` (API 1.6): when every node above it is a `column`, the row takes the height left and each child scrolls on its own (a list beside a detail); anywhere else it is a plain row |
| `card` | `id`?, `title`?, `subtitle`?, `badge`?, `icon`?, `selected`? (bool), `children`? | A bordered card: a header (icon, title, subtitle, badge), then its children. With `id`, the whole card is clickable (also with Enter or Space) and sends `panel/action` with that `id`. Card ids may repeat. `selected` (API 1.6) draws it highlighted, for the item a detail shows |
| `section` | `id`, `title`, `open`? (bool, default `true`), `badge`?, `children` | A block the user can collapse. `open` is an initial value (see the rules) |
| `tabs` | `id`, `value`?, `style`?, `tabs` (array of `{"id", "title", "badge"?, "tone"?, "children"}`) | A tab bar; the selected tab's `children` show below it. At most 10 tabs. `value` is the initial tab id (default the first), an initial value (see the rules). The selected tab id is sent in `values` under the node's `id`. API 1.5. `style` (API 1.8) `"segmented"` draws the bar as a rounded box of pills, the selected one violet-tinted with a violet border. A tab's `tone` (API 1.8) is its badge's tone, in both styles. `end`? (array of nodes, API 1.11) draws them at the right end of the bar's row, on every tab. The bar then takes only its tabs' width. Their fields are in `values` like any other |
| `divider` | | A horizontal line |
| `spacer` | | In a row, takes the free space: what follows goes to the end |

`tabs` rules:

- Switching tabs is local: no round trip, no `panel/action`. Form fields in hidden tabs keep what the user typed and still go in `values`.
- The node's `id` joins the id set (buttons, form fields, sections, tabs); tab ids may repeat across trees but not within one `tabs`. `title` and `badge` are one-line strings. Hidden tabs' nodes count toward the 2,000-node and 32-level limits.
- `fill` works through it: a `tabs` node whose ancestors are all `column`s passes the remaining height to its selected tab's children.
- An older wai shows `Unsupported element`; set `api = "1.5"` in the manifest when you use it.

**Content:**

| `type` | Fields | Notes |
|---|---|---|
| `text` | `text`, `muted`? (bool), `mono`? (bool), `style`? | Up to 10,000 characters; `\n` breaks the line. `mono`: monospace. `style` (API 1.8): `strong` (semibold 13 px), `heading` (semibold 16 px) or `label` (uppercase, 10.5 px, bold, muted); combines with `mono` and `muted` |
| `markdown` | `text` | Up to 10,000 characters |
| `badge` | `text`, `tone`? | |
| `icon` | `name`, `tone`? | An unknown name shows as `Unsupported element` |
| `link` | `text`, `url`, `icon`?, `icon_only`? (bool) | Opens `url` in the default browser; its tooltip shows the URL. With `icon` (API 1.8) it is drawn as a button showing the icon and the text, with the URL as tooltip; with `icon_only` too, only the icon, with `text` as tooltip and accessible label. Only `https://` and `http://` URLs, of at most 2,048 characters; anything else is unsupported. Sends no action |
| `alert` | `tone`, `title`?, `text` | A coloured notice. `tone` is `info`, `success`, `warning` or `danger`. `text` wraps, up to 10,000 characters |
| `kv` | `items` (array of `{"key", "value"}`) | Keys and values side by side, one line each. At most 200 items |
| `bubble` | `id`, `text`, `tone`?, `sticky`? (bool) | API 1.8. A speech bubble with a tail at its bottom left. wai animates it: it pops in, types its text (one character per 30 ms), holds 2.5 s + 50 ms per character (at most 7 s) with a shrinking timer line, and fades out. The hold waits while the mouse is on it and while the prompt box has text (it dims then). A click closes it. When it is gone, wai sends `panel/action` with its `id` and `values: {"bubble": "click"}` or `{"bubble": "timeout"}`. `sticky`: it never times out; after 6 s it shrinks to a bouncing "!" (hover shows the text). A different `text` for the same `id` starts a new bubble; removing the node removes it at once. `text` is one line, at most 140 characters |
| `pixels` | `rows` (array of strings), `scale`? (1 to 8), `id`? | API 1.9. A grid of cells: `#` is a full cell (the theme's foreground), `+` a dim one (40% of it), any other character is empty and not painted. At most 64 rows, each cut to 256 characters; shorter rows are padded to the longest; an empty `rows` is unsupported. `scale` is px per cell (default 2, clamped to 1 to 8). The node is exactly `longest row × scale` by `rows × scale` px; a narrower container clips it. With `id`, a click sends `panel/action` with it and the cursor is a pointer; the `id` is shared with buttons and fields, and a repeat makes the node unsupported |
| `progress` | `value` (number) | From 0 to 1, clamped |
| `spinner` | `label`? | A loading indicator |

**Lists, tables and logs:**

| `type` | Fields | Notes |
|---|---|---|
| `list` | `items`, `fill`? (bool), `plain`? (bool) | Items: `{"id", "title", "subtitle"?, "badge"?, "icon"?, "tone"?, "selected"?, "lead"?, "note"?, "indent"?}`; clicking one sends `panel/action` with its `id`. At least 160 px wide. `plain` (API 1.8): rows are borderless and transparent (violet on hover or when selected) instead of glass cards, 32 px tall. Item fields (API 1.8): `icon` is a Lucide name drawn before the text in the item's `tone` (an unknown name is dropped); `tone` colors the icon and the badge (the badge stays neutral without it); `selected` (bool) draws a violet border and the hover background; `lead` is muted 12 px text before the title on the same line; `note` is muted text after the title on the same line (title and note truncate together); `indent` (bool) insets the row 16 px, for a child under a group row |
| `table` | `columns` (array of `{"title", "width"?}`), `rows` (array of `{"id"?, "cells"}`), `fill`? | `width` is in pixels; columns without one share the rest. A cell is a string (one line) or a `text`, `badge`, `link`, `icon`, `button`, `menu`, `spinner` or `progress` node; any other node is unsupported. Extra cells are dropped, missing ones are blank. A row with `id` is clickable and sends `panel/action` with that `id` (row ids may repeat). At most 20 columns and 1,000 rows; the rest are dropped |
| `log` | `text`, `follow`? (bool, default `true`), `fill`? | Monospace output, one line per line, with a **Copy** button. Keeps the **last** 100,000 characters, and ANSI escape codes (colours) are removed. With `follow` it stays at the bottom as you re-render it, unless the user scrolled up |

Without `fill`, a list, table or log shows 12 rows (lines) and scrolls the rest. With `fill` it takes the panel's remaining height; that works only when every node above it is a `column` (the panel's root included), and is ignored anywhere else.

**Controls:**

| `type` | Fields | Notes |
|---|---|---|
| `button` | `id`, `label`, `style`?, `icon`?, `icon_only`? (bool), `disabled`? (bool), `confirm`? | `style`: `primary`, `danger`, or default. Clicking sends `panel/action` with its `id`. `icon_only` (API 1.8): only the icon, with `label` as tooltip and accessible label; it needs `icon`, else it is ignored |
| `menu` | `label`?, `icon`? (default `ellipsis`), `items` (array of `{"id", "label", "danger"?, "confirm"?}`) | A button that opens a dropdown. Picking an item sends `panel/action` with the item's `id` (item ids may repeat). At most 50 items |
| `input` | `id`, `placeholder`, `value`?, `multiline`? (bool), `rows`? (number), `icon`?, `submit`? (bool), `clear`? (bool) | A text field. With `multiline`, a text area of `rows` lines (default 4, at most 20). Its value arrives in `values`. `icon` (API 1.8) draws a Lucide icon inside the field, at the left. `submit` (API 1.8, one-line inputs only): Enter sends `panel/action` with the input's `id`, and so does clearing it with the × of `clear`. `clear` (API 1.8) shows an × inside the field only while it has text; clicking it empties the field |
| `select` | `id`, `options` (array of strings), `value`?, `segmented`? (bool) | At most 100 options; with no choice, the first. `segmented` (API 1.8) draws the options as a segmented control; choosing one sends `panel/action` with the select's `id` and the choice in `values[id]`. A plain dropdown sends nothing |
| `path` | `id`, `pick` (`"file"` or `"folder"`), `placeholder`?, `value`?, `editable`? (bool, default true) | A file or folder path, chosen with the system picker or dropped from Finder. Editable, it is also a text field; with `editable: false`, only the picker sets it. It shows your home folder as `~`; its value arrives in `values` as the full path (`""` when empty). API 1.6 |
| `checkbox` | `id`, `label`, `checked`? (bool) | |
| `switch` | `id`, `label`?, `checked`? (bool) | API 1.11. An on/off switch, its label to the left. Each change sends `panel/action` with the switch's `id` and the new value in `values[id]`, where it is a boolean like a checkbox's. |

**`tone`** is `neutral` (the default), `info`, `success`, `warning` or `danger`, in the theme's colours.

**`confirm`** on a `button` or a `menu` item is `{"title", "text"?, "confirm_label"?}`. The click opens a dialog with your extension's name, the title and the text, **Cancel**, and a button labelled `confirm_label` (default `Confirm`), red when the button is `danger` style or the item is `danger`. Only that button sends `panel/action`, with the form's values at that moment; Cancel, Esc or a click outside send nothing. A `confirm` that is present but malformed makes the node unsupported, so a broken confirmation never turns into no confirmation.

**Clicks inside clickable things:** a click on a button, menu, link or form field inside a clickable card or table row goes to that control only.

### Rules

- **Required fields:** a node with a missing or mistyped field, or an unknown `type`, shows as `Unsupported element`. Fields the schema does not name are ignored. A node type newer than the user's wai also shows as unsupported; declare `api` to get a clear refusal instead.
- **Text:** strings other than `text`, `markdown` and `alert`'s `text` are one line of at most 500 characters.
- **Ids:** ids are 1–256 bytes. Buttons, form fields, sections and bubbles share one set of ids per tree, and a repeated id makes that node unsupported. List item, card, table row and menu item ids may repeat.
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
- **Card (API 1.4):** `card` is a node tree, as in a panel, drawn in a card when the pointer rests on the item, instead of the tooltip. Only `column`, `row`, `text`, `badge`, `progress`, `icon`, `divider`, `spacer` and `kv` are drawn (any other node shows as unsupported); at most 200 nodes and 8 levels deep, or the item has no card. The card ends with `— <extension name>`. An update without `card` removes it. Nothing in a card is clickable: the item itself is.
- **Telling them apart:** each item's tooltip ends with `— <extension name>`. The user can hide any item with a right click on the bar.

```toml
permissions = ["process", "bar"]
api = "1.4"

[[contributes.items]]
id = "prs"
bar = "bottom"
title = "Pull requests"
```

```json
{"jsonrpc": "2.0", "method": "item/update", "params": {"item": "prs", "text": "3 PRs", "icon": "git-pull-request", "tooltip": "2 green, 1 failing", "tone": "warning"}}
```

A card with one block:

```json
{"jsonrpc": "2.0", "method": "item/update", "params": {"item": "usage", "text": "16%", "card": {"type": "column", "children": [
  {"type": "row", "children": [{"type": "text", "text": "Current session"}, {"type": "spacer"}, {"type": "text", "text": "16% used"}]},
  {"type": "progress", "value": 0.16},
  {"type": "text", "text": "Resets 00:20", "muted": true}]}}}
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
- **Themes** (`contributes.themes`) are theme-set files that join the user's own themes in Settings > Appearance while the extension is enabled. A theme-only extension needs no `[process]` and no permission:

  ```toml
  id = "sunset"
  name = "Sunset"
  version = "0.1.0"

  [contributes]
  themes = ["themes/sunset.json"]
  ```

  `themes/sunset.json` has the format of `~/.config/wai/themes/*.json` (see the [user guide](user-guide.md#appearance)). Paths stay inside the extension's folder, files over 1 MB are skipped, and a name already taken is skipped with a notice.
- **Claude plugins** are listed under the extension in the Extensions view with **Install in terminal**, required ones (`requires.claude_plugins`) first.
  - That types `claude plugin install <name>` (for a required one, with `--marketplace <marketplace>`) into wai's terminal **without pressing Enter**, so the user confirms.
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
- **Pinned packages:** a package from an index downloads over https only (redirects too), at most 100 MB, and must match the SHA-256 the index lists. A user's own index cannot replace an id the official index lists, and a project cannot add an index.
- **Bounded input:** message size, render rate, notification and session-open rates, panel tree size and text length are all capped, and panel trees are parsed off the UI thread. A broken or hostile panel shows as unsupported or `Panel too large`; it cannot crash wai.
- **Clean stops:** an extension's whole process tree dies with it.
- **Claude plugins** are never installed without the user's consent: a required plugin is named in the consent dialog and installed when the user presses Install; a suggested one is typed into the terminal for the user to run.

**What v1 does not isolate:**
- **Your rights:** the extension's process runs as you. It can read and write any file you can, use the network, read the environment it inherits, and start any program.
- **`network`, `listen` and `fs:`** are declarations only.
- **Contributed agents and MCP servers** run with the same rights, inside every agent session.
- **MCP servers run in the project folder:** the agent starts them there, so a launcher that reads its current folder picks up what the repository put in it. npx, for one, reads the project's `.npmrc` and runs a matching package from its `node_modules`. An npx-based server should pass `--prefix=/` before the package, as wai-navegador does: `args = ["--prefix=/", "-y", "<package>@<version>"]`.
- **The project's Claude settings:** claude-agent-acp itself already loads the project's own Claude settings (`.claude/`), as Claude Code does in a terminal. wai neither adds to nor limits what they can do.
- **Windows:** a child that the extension starts in its first instant, before it joins the job object, can outlive it.

Real isolation (a sandbox) is planned. Until then, every entry in the official index is reviewed by hand before it is merged.

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

On Windows, a binary built there is `hello-rs.exe`. Keep `command = "bin/hello-rs"`: on Windows wai runs `bin/hello-rs.exe` for it, so one manifest serves every platform.

## Publishing

Anyone can publish an extension, as with VS Code or Zed. wai lists extensions from **indexes**: TOML files on the web that point at prebuilt packages, each pinned by its SHA-256. The Extensions page shows them under **Available**; installing one downloads the package for the user's platform and goes through the same consent as any install. Nothing is compiled on the user's machine.
- The **official index** is [`extensions/index.toml`](../extensions/index.toml) in this repository, and every wai reads it.
- Users can add their own indexes in their settings (see the [user guide](user-guide.md#extensions)).
- Your packages stay in your own repository's GitHub Releases (or any https host). An index only points at them.

In short: put your extension in its own repository, release it with the shared workflow, then open a pull request that adds the release's `index-entry.toml` to the official index.

### Packages

A package is a `.tar.gz` with `wai-extension.toml` at its root (a leading `./` is fine) and everything the extension needs at run time: its program, a README, a LICENSE.
- Regular files and folders only: no links, absolute paths, `..` or entry listed twice.
- At most 100 MB downloaded, and 100 MB and 10,000 entries unpacked.
- A file with any execute bit is installed as `0755`, any other as `0644`.
- **One package per platform:** `darwin-arm64`, `darwin-x64`, `linux-x64`, `linux-arm64`, `windows-x64`, `windows-arm64`. A package that runs everywhere (manifest-only, or a program found on `PATH` such as `npx`) is `any`. wai takes its own platform's package, else `any`, else shows `Not available for this platform`.
- **Windows:** for `command = "bin/x"`, wai runs `bin/x.exe` on Windows. Keep one manifest for every platform and ship `bin/x.exe` in the Windows packages.

### Releasing with the shared workflow

For an extension written in Go, or a manifest-only one, add `.github/workflows/release.yml` to its repository:

```yaml
name: release
on:
  push:
    tags: ['v*']
jobs:
  release:
    permissions:
      contents: write
    uses: Guilbritto/wai/.github/workflows/extension-release.yml@main
    # with:
    #   path: ext   # when wai-extension.toml is not at the repository's root
```

To release, set `version` in the manifest, commit, and push a matching tag:

```sh
git tag v0.2.0 && git push origin v0.2.0
```

The workflow:
- fails unless the tag ends in `v<version>`, the manifest's version;
- when `process.command` is a path (`bin/x`), builds the Go program in the extension's folder (its `go.mod` there) for the six platforms with `CGO_ENABLED=0`, one package each; otherwise it makes one `any` package;
- packs the manifest, `README*`, `LICENSE*` and the program. Anything else the extension reads at run time must be embedded in the program (Go's `embed`), or you build the packages yourself (below);
- publishes a GitHub release with the packages, `SHA256SUMS.txt` and `index-entry.toml`, the entry to paste into an index.

**Another language, or extra files.** Build each platform's program, pack it by the rules above, and write the entry by hand:

```sh
mkdir -p stage/bin
cp wai-extension.toml README.md LICENSE stage/
cp target/aarch64-apple-darwin/release/my-extension stage/bin/
tar -czf my-extension-0.2.0-darwin-arm64.tar.gz -C stage .
shasum -a 256 my-extension-0.2.0-darwin-arm64.tar.gz
```

### The index format

```toml
[[extensions]]
id = "my-extension"                                  # the manifest's id
name = "My extension"
version = "0.2.0"                                    # the manifest's version
description = "One line"                             # optional
repository = "https://github.com/me/my-extension"    # optional

[extensions.packages.darwin-arm64]
url = "https://github.com/me/my-extension/releases/download/v0.2.0/my-extension-0.2.0-darwin-arm64.tar.gz"
sha256 = "<64 hex digits>"

[extensions.packages.windows-x64]
url = "…"
sha256 = "…"
```

- Every URL must be `https://`, and so must every redirect.
- An entry that does not validate is skipped, and the rest still list. Unknown fields and platforms are ignored, so a newer index keeps working in an older wai.
- The first index to list an id keeps it, and the official index comes first.

### Listing it in the official index

1. Open a pull request to this repository that changes only `extensions/index.toml`. To add your extension, append the release's `index-entry.toml`. To update it, replace your entry with the new release's.
2. CI (the `index` check) runs `wai-index-check` on the proposed file. Every entry must parse and ids must be unique. Every package must download, match its SHA-256, unpack under wai's rules, and hold a manifest with the entry's id and version. A platform package must also hold its program.
3. The owner reviews the extension and merges it. wai reads the index from `main`, so the listing shows in wai once the merge reaches `main`.

Run the same check before you open the pull request (it needs `curl`):

```sh
cargo run -q -p wai-ext --bin wai-index-check -- extensions/index.toml
```

### Your own index

Host a file in the same format at any https URL. A raw file in a GitHub repository works: `https://raw.githubusercontent.com/<owner>/<repo>/main/index.toml`. Users add the URL with **Add index…** on the Extensions page (or to `[extensions] indexes` in their settings), and its extensions show with `from <host>`. It cannot replace an id the official index lists.

**A private index.** Keep the extension in a private GitHub repository and release it with the shared workflow and `with: latest: true`: the latest release's `index-entry.toml` is then the index. Users click **Add index…** and type `owner/repo`; wai reads the index and downloads the package with their `gh` login, which needs access to the repository. Only people with access see the extension.

### First-party extensions

The extensions in this repository's `extensions/` folder release from here on a tag `<id>-v<version>` (`wai-rotinas-v0.1.0`), through `.github/workflows/extensions.yml` and the same shared workflow. They never use `v*` tags, which belong to wai's own releases. Their entries go into the official index like anyone's.
