# Writing wai extensions

An extension is a folder with a `wai-extension.toml` manifest. It can be manifest-only (templates, prompt commands, agents, MCP servers) or run a program of its own. That program can be in any language: it speaks line-delimited JSON-RPC 2.0 over stdio and can draw panels in wai's sidebar.

This guide describes protocol version 1, as implemented by wai 0.5.0. The [first-party extensions](../extensions) are working references:
- `wai-navegador` is manifest-only;
- `wai-waycarbon` and `wai-rotinas` are written in Go.

## The manifest

`wai-extension.toml`, at the top of the extension's folder:

```toml
id = "hello-go"                 # required
name = "Hello (Go)"             # required
version = "0.1.0"               # required
description = "A minimal wai extension"
permissions = ["process", "panels", "notify"]

[process]                       # optional: the extension's own program
command = "bin/hello-go"
args = []

[[contributes.panels]]
id = "main"
title = "Hello"
```

**Unknown fields are refused** at every level, and so is a manifest that is not valid UTF-8 or is over 1 MB.

| Field | Type | Rules |
|---|---|---|
| `id` | string | 1–63 characters from `a-z`, `0-9` and `-`, not starting with `-`. It names the install folder |
| `name` | string | Shown in the extension list and the consent dialog |
| `version` | string | Free form. wai shows it; it does not compare versions |
| `description` | string, optional | Shown last in the consent dialog, cut to fit |
| `permissions` | array of strings | See [Permissions](#permissions). An unknown permission is refused |
| `process.command` | string | See [Commands and paths](#commands-and-paths). Needs `process` |
| `process.args` | array of strings, optional | Arguments, passed as given (no shell) |
| `contributes.templates` | array of tables | `name`, optional `agent`, optional `prompt`. Same as `templates.toml` in the [user guide](user-guide.md#templates) |
| `contributes.commands` | array of tables | `id`, `title`, optional `prompt`. See [Contributions](#contributions) |
| `contributes.agents` | array of tables | `name`, `command`. Needs `process` |
| `contributes.mcp_servers` | array of tables | `name`, `command`, optional `args` (array), optional `env` (table of strings). Needs `process` |
| `contributes.claude_plugins` | array of strings | Claude Code plugin names to suggest |
| `contributes.panels` | array of tables | `id`, `title`. Needs `panels` |
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
| `panels` | Contribute panels, `panel/render`, and receive `panel/action` |
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
- **Where things go.** The extension goes to `<config>/extensions/<id>/` and the consent to `<config>/extensions.json`. It starts enabled.
- **It is a copy.** Rebuilding in your source folder does not change the installed extension.
- **Updating.** There is no upgrade in v1: uninstall, then install again. An id that is already installed is refused.
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
- **Data.** There is no per-extension data folder yet. If you keep files in the config folder (the cwd), prefix them with your id.

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
| `initialize` | request, id `0` | `{"protocol": 1, "wai_version": "0.5.0", "ext_id", "granted": [permissions], "project": "<folder>", "ext_dir": "<folder>"}` | Answer within **10 s** with any result (`{}` is fine), or the extension is marked `Failed`. An error answer also fails it (`initialize failed: <message>`) |
| `command/run` | request | `{"id": "<command id>"}` | The user ran one of your commands without `prompt` from the palette. wai ignores the answer |
| `panel/action` | request | `{"panel": "<panel id>", "action_id": "<id>", "values": {…}}` | A list item or button was clicked. `action_id` is the item's or button's `id`. `values` has every form field in the panel by id: inputs as strings, selects as the chosen option (or `null`), checkboxes as booleans. Needs `panels` |
| `event/session_state` | notification | `{"agent", "session", "state"}` | `state` is `idle`, `working`, `needs_you` or `error`. Needs `sessions:read` |
| `event/turn_ended` | notification | `{"agent", "session", "reason"}` | `reason` is `end_turn`, `cancelled`, `max_tokens`, `refusal`, or the agent's own. Needs `sessions:read` |
| `event/file_saved` | notification | `{"path": "<absolute path>"}` | The user saved a file in wai |

- Paths are absolute, and on Windows they never carry the `\\?\` prefix.
- Answer `initialize` before anything else.

### Extension → host

| Method | Needs | Params | Result |
|---|---|---|---|
| `sessions/list` | `sessions:read` | none | `[{"agent", "session", "title" (string or null), "state"}]`, one per open session |
| `sessions/open` | `sessions:control` | `{"agent"?: string, "prompt"?: string, "send"?: bool}` | `{"agent", "session"}` once the session has started |
| `sessions/prompt` | `sessions:control` | `{"agent", "session", "text"}` | `null` at once |
| `notify` | `notify` | `{"title", "body"?}` | `null` |
| `panel/render` | `panels` | `{"panel": "<panel id>", "tree": {…}}` | `null` |

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

**Notifications or requests.** You may send any of these as a notification (no `id`) when you do not need the answer, as the examples below do. A denied notification is dropped without an answer.

## Panels

A panel is a sidebar tab titled by `contributes.panels[].title`, cut to 24 characters. It exists while your process runs, and its content is the last tree you rendered. Before your first render it says `Waiting for the extension…`.

A tree is JSON nodes:

| `type` | Fields | Notes |
|---|---|---|
| `column` | `children` (array of nodes) | Stacks children vertically |
| `row` | `children` (array of nodes) | Places children side by side; they wrap when there is no room |
| `text` | `text`, `muted`? (bool) | Up to 10,000 characters |
| `markdown` | `text` | Up to 10,000 characters |
| `list` | `items`, `fill`? (bool) | Items: `{"id", "title", "subtitle"?, "badge"?}`; clicking one sends `panel/action` with its `id`. Without `fill` it shows 12 rows and scrolls the rest; with `fill` it takes the panel's remaining height. At least 160 px wide |
| `button` | `id`, `label`, `style`? | `style`: `primary`, `danger`, or default. Clicking sends `panel/action` with its `id` |
| `input` | `id`, `placeholder`, `value`? | A text field; its value arrives in `values` |
| `select` | `id`, `options` (array of strings), `value`? | At most 100 options; with no choice, the first |
| `checkbox` | `id`, `label`, `checked`? (bool) | |
| `badge` | `text` | |
| `progress` | `value` (number) | From 0 to 1, clamped |

**Rules:**
- **Required fields:** every field without `?` is required. A node with a missing or mistyped field, or an unknown `type`, shows as `Unsupported element`. Fields the schema does not name are ignored.
- **Text:** strings other than `text` and `markdown` are one line of at most 500 characters.
- **Ids:** ids are 1–256 bytes. Buttons and form fields share one set of ids per tree, and a repeated id makes that node unsupported. List item ids may repeat.
- **Size:** a tree deeper than 32 levels or with more than 2,000 nodes is refused whole, and the panel shows `Panel too large`.
- **Initial values:** `value` and `checked` set a field when it first appears, or when you send a *different* initial value. Otherwise what the user typed or chose stays across your renders.

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
