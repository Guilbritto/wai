# wai: the `wai` MCP server (the AI changes wai's settings)

> 2026-11-12. Item 3.9 of `docs/superpowers/plans/2026-11-12-wai-execucao-pendencias.md` ("servidor MCP `wai` + Configure with AI"), narrowed by the owner to settings only. Choices marked **(owner)** are the owner's; **(call)** are mine, listed again under "My calls". Builds on the personalization spec (`2026-11-12-wai-personalization-design.md`, item 3.8): plan it only after both of its tracks are merged.

## Intent

The owner wants to ask, in the middle of any conversation, things like *"Aumente o tamanho da fonte"* or *"mude o tema para xpto"*, and have wai change. Today an agent can already edit `~/.config/wai/settings.toml` with its file tools and wai applies it live, but it has to guess the file's keys, the theme names and the installed fonts, and nothing in wai says the AI changed something or lets you take it back.

Rule the owner set for these choices: look at how other IDEs do it and do the same. Prior art: Zed's agent edits `settings.json` with its normal file tools and Zed reloads it; JetBrains (built-in MCP server) and Claude Code's VS Code integration (the `ide` MCP server) give the agent typed tools into the IDE.

## Goals

1. **Every session has wai's tools.** wai adds a `wai` MCP server to the MCP server list of every session it starts, resumes or forks, whatever the agent. **(owner: "em qualquer conversa")**
2. **The tools cover settings only:** read the settings in effect; change appearance (theme mode, light and dark theme, fonts and sizes, zoom), shortcuts, and the conversation and update preferences; list what can be picked (themes, installed font families, actions with their shortcuts). **(owner: "só configuração por ora", no window control)**
3. **A change applies at once, and wai says so with Undo:** a notice like `Claude changed font size 13 → 15 · Undo`. Undo puts back the values that change replaced. **(owner)**
4. **What changes what runs still asks.** Agent commands and the shell keep going through wai's approval dialog, as every outside edit of `settings.toml` does today (`apply_settings` → `unapproved` → `ask_approval`). **(owner, from the master plan; already true)**

## Non-goals

- Tools for the window: panes, files, sessions, the terminal, the sidebar. **(owner: later)**
- A **Configure with AI** button or a dedicated settings session: the owner asks in any conversation. **(call)**
- Changing a project's `.wai/settings.toml`. **(call)**
- Installing, enabling or removing extensions; changing an agent's command, the shell or the update repository. **(call)**
- Other MCP clients (Claude Desktop, Cursor) driving wai.

## Anti-goals (a failure even if it works)

- **A network port.** The core never opens one; the server talks to the agent over stdio only.
- **Idle CPU above 0.5%.** The server waits on stdin and runs only while its session runs; wai learns about changes from the folder watcher it already has, never by polling.
- **A tool call that leaves a broken `settings.toml` or `keymap.toml`.** Every value is checked before the write, a file that does not parse is left untouched, and the call fails with a message the agent can relay.
- **A change the owner did not see.** Every applied change gets its notice, even when one prompt causes several.
- **Undo that clobbers a newer edit.** If the key changed again since, Undo does not overwrite it and says so.
- **Losing the user's comments or layout** in `settings.toml` or `keymap.toml`.

## Constraints

- wai passes MCP servers to the agent on `session/new`, `session/load` and `session/fork` (`wai-agent/src/actor.rs:670-695`, `McpServer { name, command, args, env }`, stdio only). Today the list comes from enabled extensions (`ext_host.rs` `mcp_servers`, `agents.rs` `mcp_servers`).
- Settings writes go through `wai_settings::edit::write_change`, which keeps the rest of the file as written and refuses a file that does not parse. 3.8 adds the `[fonts]` and `[theme]` keys and `keymap.toml`, plus their parsing rules.
- wai reloads `settings.toml` from the user-folder watcher (`workspace.rs` `watch_settings`); 3.8 adds `keymap.toml` to it.
- `main` starts with the update handoff (`update::macos::on_start`), `shell_path::import`, the single-instance lock and log rotation. The server mode must run before all of them: it is a short-lived helper and must not take the lock, rotate the log or start an update.
- In modes other than bypass, Claude Code asks permission for MCP tools like for any tool, through wai's permission dialog. The owner runs in bypass.
- Windows (Fase 4) must work without a separate design.

## Approaches

| | What | Trade-off |
|---|---|---|
| **A (picked)** | The wai binary itself runs as the server (`wai --mcp`). It checks and writes `settings.toml`/`keymap.toml` with the same code the Settings screen uses; the running wai applies the change through its existing watcher. For the Undo notice, the server leaves a note of what it changed in the watched folder. | Zed's model (file plus live reload) with typed tools, so the agent stops guessing. No socket, no port, the same on Windows. Two programs write the same file, so a rare race is possible (see "My calls" 7). |
| B | `wai --mcp` only relays to the running wai over a local socket (a 0600 file, no network port); wai applies and notifies, as JetBrains does. | It answers from the live window, but puts a listener and a protocol in the core and needs another variant on Windows. Worth it only once the AI drives the window. |
| C (dropped) | No server: tell the agent where the files are. | No Undo notice tied to the AI, and the agent keeps guessing names. Against goal 1. |

**(owner: A)**

## Current state (facts to build on)

- `crates/wai/src/main.rs`: `wai [PATH]`. The first positional argument is the project.
- `wai-settings/src/edit.rs`: `Change` (default agent, theme, shell, agent command, model, update check and repo, processing) and `write_change`.
- `wai-settings/src/approve.rs`: `Approval::{Agent, Shell}`. The update repository is not approved on an outside edit; this spec keeps the tool away from it rather than changing that.
- Notices with a button already exist (`workspace.rs` `announce_update`: `Notification::info(..).action(..)`).
- The test agent records the MCP servers it receives (`wai-fake-agent.rs` `record_mcp`).
- No MCP server library is in `Cargo.lock` (no `rmcp`).

## My calls (not confirmed by the owner; check them while reading)

1. **The command is `wai --mcp`,** run from `std::env::current_exe()` of the running wai, under the server name `wai`. A flag, because a bare word would collide with `wai [PATH]`. `main` checks for it first, before the update handoff, and serves stdio until stdin closes.
2. **wai passes the context in `env`:** the user settings folder (so tests and `WAI_STATE_PATH` runs keep away from the real one), the session's project folder, and the session id plus the agent's display name for the notice. The server needs nothing else from the running wai.
3. **The tools:**
   - `get_settings`: the settings in effect, each value with its source (default, user or project). A project override is named, so the agent can say "this project sets the dark theme in `.wai/settings.toml`".
   - `set_settings`: one or more key → value changes in one call, applied as one write and one notice. Only the keys in goal 2 are accepted. Anything else is refused with a message pointing at Settings.
   - `list_themes`: name and mode (light or dark) of the bundled themes and those of enabled extensions.
   - `list_fonts`: the installed font families.
   - `list_shortcuts`: every action with its keys and context, marked default or user.
   - `set_shortcut`: binds or unbinds keys for an action in a context, written to `keymap.toml` with the rules of 3.8.

   Names and arguments are short and descriptive, as Claude Code's own tools are. Each description says what wai does with the change (applies at once, shows Undo).
4. **Writes check first.** Values are checked against the same rules wai loads with:
   - a theme name must exist for its mode;
   - a font family must be installed;
   - sizes and zoom must be in range;
   - a key and an action must exist.

   A refused value fails the call with the reason. The files are written the way 3.8 writes them, keeping comments.
5. **The note for the notice:** after a successful write, the server writes a small JSON note (session, agent, and each key with its old and new value) next to `settings.toml`, in the folder wai already watches. wai shows the notice when the note appears and then deletes the note. A note older than wai's start is ignored. The notice uses the agent's display name (`Claude changed …`). Several keys from one call make one notice: `Claude changed theme, font size · Undo`. The notice autohides like other info notices; the change stays.
6. **Undo** writes the old values back, but only for keys whose value is still the one the AI wrote. A key changed since is left alone, and the notice says `font size changed since; not undone`. Undo is a normal settings write by wai, so it gets no notice of its own.
7. **The race:** the server and wai's Settings screen both use `write_change`, which reads, edits and writes the whole file. Each write goes to a temporary file and is renamed into place, so a reader never sees half a file. The race is two near-simultaneous edits where the second drops the first. It needs the owner and an agent to change a setting in the same instant. **ponytail:** no file lock; add one if a lost write is ever seen.
8. **The server is always on,** with no setting to turn it off. Like every MCP tool, the agent's own permission rules govern it, and an agent that does not speak MCP ignores it.
9. **A project override:** `set_settings` still writes the user file, and the tool result says the project's `.wai/settings.toml` overrides that key, so nothing visible changes. The notice says so too: `Claude changed dark theme (this project overrides it) · Undo`.
10. **Shortcuts get the same notice and Undo.** The note records each changed binding (context, keys, old and new action, where "none" covers an unbind). Undo restores them in `keymap.toml` under the rule of call 6.
11. **The server runs without a window but links the same code.** It is the wai binary, so it has the bundled themes (`aurora.json`, "wai Light"). It finds extension themes from the user folder in `env`, which holds the extensions and their enabled state. The checks 3.8 adds (theme names, keymap rules, size ranges) live in GPUI-free functions that wai and the server share.
12. **Size ranges are 6–100 px for every font size, as Zed and VS Code clamp them.** Zoom is −4…+8 (3.8 call 6). Settings uses the same limits.
13. **Theme mode writes `[theme] mode`.** A legacy `theme = "dark"` left in the file stays where it is and is ignored, per 3.8 call 8.
14. **MCP over stdio is written by hand** (JSON-RPC lines: `initialize`, `tools/list`, `tools/call`) with `serde_json`, which wai already uses. The builder may take `rmcp` instead if the hand-written part outgrows a small module.

## Left to the builder

- The exact tool names, argument schemas and descriptions.
- The note's file name and format, and how wai tells its own Undo write from an outside one.
- Where the font family list comes from in a process without a window (the same source GPUI's text system uses).
- Tests:
  - the fake agent receives `wai` in every start, resume and fork;
  - each tool's success and refusal over a pipe;
  - a write keeps comments;
  - the notice appears and Undo restores, or refuses after a later edit;
  - `wai --mcp` takes no lock and starts no update.
- Docs: a "Change settings by asking the agent" part in `docs/user-guide.md`, with the owner's examples.
