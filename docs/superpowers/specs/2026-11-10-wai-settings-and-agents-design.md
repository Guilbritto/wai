# wai settings screen and agent choice

> 2026-11-10. First of three projects agreed with the owner, in this order:
> 1. settings and agent choice (this document);
> 2. how the agent's processing shows in the chat (display modes and a live status line);
> 3. wai driven by the AI (wai's own tools, offered to every agent).
>
> Choices marked **(owner)** are the owner's. Everything else is the builder's call, marked **(call)** where it shapes what gets built.

## Intent

Today wai works with Claude only, in practice:
- `Settings::default` hardcodes `default_agent = "claude"` and a single `[agents.claude]` (`crates/wai-settings/src/lib.rs`);
- using another agent means writing `[agents.<name>]` into `~/.config/wai/settings.toml` by hand;
- there is no settings screen at all, and settings are read only at launch.

The owner wants what Zed, VS Code and Cursor have: a screen where you choose which AI wai uses, with no AI chosen in advance ("não deixar setado default o Claude"). wai should be a home for several coding agents, not a Claude shell, and choosing one should be a click, not a TOML edit.

The owner does not have a `settings.toml` today, so they will meet the first-run flow below themselves.

## Goals

1. **A settings screen**, opened as a tab among the panes. **(owner)**
2. **Four built-in agents:** Claude, Codex, Antigravity and Copilot. **(owner)**
3. **No default agent out of the box.** wai never picks one for you. **(owner)**
4. **A status per agent**, so you can see which ones can run.
5. **Model choice:** per session in the chat header, plus a default model per agent in Settings. **(owner)**
6. **Live apply:** changes from the screen or from editing `settings.toml` take effect without restarting wai. **(call: needed for the screen, and the base project 3 builds on)**

## Non-goals

- **No login screen.** wai still never shows one, and never reads, stores or forwards credentials (README). When an agent is signed out, wai says which command to run in a terminal.
- **No installing CLIs.** wai shows how to install a missing one; it does not install it.
- **No chat display modes here.** They are project 2, which adds a **Conversation** section to this screen.
- **No AI editing of wai here.** That is project 3, which adds a **Configure with AI** button and wai's own tools. This project only provides the live apply they rely on.
- **No editor for project settings.** The screen edits the user's file. It only *shows* when a project's `.wai/settings.toml` overrides something.

## Anti-goals

These would make the work a failure even if it technically works:
- quietly choosing an agent, even when only one is ready **(owner)**;
- changing anything for someone whose file already sets `default_agent` **(owner)**;
- letting the screen, a file reload or a cloned repository change *what wai executes* (an agent's command, the terminal shell) without the user confirming. R15 stays as it is;
- losing the user's comments or formatting in `settings.toml`;
- overwriting a `settings.toml` that has a syntax error.

## Constraints

- **R15** (`Settings::load`): a project's `.wai/settings.toml` may not set `agents`, `terminal` or `updates`; its `default_agent` can only name an agent the defaults or the user define. The new screen writes the user's file only.
- **Config reads** go through `read_config`: regular files only, 1 MB at most.
- **ACP crate:** `agent-client-protocol = "5"` (schema 1.15.1).
  - Session config options are part of it, with the categories `model`, `mode` and `thought_level`.
  - `unstable_auth_status` exists behind a feature flag. wai enables only `unstable_session_fork` today.
- **PATH:** opened from Finder, wai takes its `PATH` from the login shell (`shell_path.rs`). Detecting a CLI must use the same `PATH` wai spawns agents with.
- **`default_agent` is read in many places:**
  - `workspace.rs`: launch (`start_agent`), new session, templates, dragging History rows, `agent_problem`;
  - `sidebar.rs`: History and Fork.

  Every one of them has to handle "no default yet".
- **The UI is English** (Sessions, New session, …). The screen follows suit, so its labels below are English. **(call)**
- **Platforms:** macOS on Apple silicon is primary, but Windows x86_64 must still build in CI.
- **Copilot's ACP mode is a public preview.**
  - Issue [github/copilot-cli#845](https://github.com/github/copilot-cli/issues/845) reports that it writes files and runs commands without sending `session/request_permission`, so no permission card would show. It may already be fixed; this was not confirmed.
  - It sends no reasoning over ACP.

## The design

### Opening and placement **(owner)**

- **Opening:** **⌘,** (Ctrl+, on Windows), a ⚙ button in the top bar, or **Settings** in the command palette.
- **Placement:** it opens as a tab among the panes. In the layout it behaves like a session pane:
  - it can be dragged and split;
  - **⌘W** closes it;
  - there is at most one, and opening it again focuses it.
- **Relaunch:** it is not restored. **(call)**
- **Layout inside:** the sections on the left, and a search field on top that filters rows across all sections.
- **Footer:** **Open settings.toml** opens the user's file in wai's editor.

### Sections

**AI** (the section the first run opens):
- **The list:** a radio list, one row per agent. It holds the four built-ins plus any agent the user added by hand.
- **Each row shows:**
  - the name and the status (below);
  - **Test**, which starts the agent once to learn its status and models;
  - **Default model** (below);
  - **Advanced**, folded, which shows the command. Changing it asks for confirmation (below).
- **Copilot's row** carries a warning **(call)**: "Copilot may edit files and run commands without asking." Drop it once #845 is confirmed fixed.
- **Extension agents** (`<ext-id>/<name>`) are listed too, labelled with their extension. They can be the default and get Status, Test and Default model like the others. **(call)**
- **Project override (call, tightening R15):**
  - A project's `default_agent` is honoured only when the user has already chosen a default and the name is:
    - the user's own default; or
    - an agent with an `[agents.<id>]` entry in the user's file; or
    - an agent from an installed extension.
  - It never selects a built-in the user has not picked, and never skips the first-run flow. Otherwise it is ignored with a notice, as R15 notices work today.
  - When it is honoured, the section says "This project uses <agent>", and the radio still changes the user's choice.

**Appearance:** Theme, Dark or Light. It applies at once.

**Terminal:** Shell, a path. Empty means the system default. It applies to terminals opened afterwards, and changing it asks for confirmation (it changes what wai runs).

**Updates:** Check for updates (on/off) and the repository. Same meaning as today.

Projects 2 and 3 add their own section and button later; nothing here needs to anticipate them beyond a list of sections.

### Built-in agents

| id | Name | Command | If the CLI is missing | If signed out |
|---|---|---|---|---|
| `claude` | Claude | `npx -y @agentclientprotocol/claude-agent-acp@0.101.0` (today's pin) | install Node.js | `claude /login` |
| `codex` | Codex | `npx -y @agentclientprotocol/codex-acp@2.6.0` | install Node.js | the Codex login command |
| `antigravity` | Antigravity | `agy acp` | link to the Antigravity CLI install page | `agy`'s login |
| `copilot` | Copilot | `copilot --acp --stdio` | `npm install -g @github/copilot` | `copilot` then `/login` |

- **Who chose what:** the list of four is the owner's. The commands and hints are the call.
- **Pinning:** versions are pinned the way Claude's is today.
- **The builder confirms before shipping:**
  - each command, by a real handshake on a machine that has the CLI (in the spirit of `crates/wai-agent/tests/real_claude.rs`);
  - each install and login hint.
- **User entries:** `[agents.<id>]` in the user's file still works.
  - An entry with a built-in id replaces that built-in's command.
  - Any other id adds an agent, shown with its id as its name.

### No default agent **(owner)**

- **The setting:** `default_agent` becomes optional, and `Settings::default` sets none.
- **A file that already sets `default_agent`:** behaves exactly as today.
- **With none chosen:**
  - at launch no agent starts, and wai opens Settings on the **AI** section;
  - **⌘N**, **+ New session**, a template without an agent and the empty pane all lead to the **AI** section, which says "Choose an AI to start a session." None of them shows an error;
  - the History in the sidebar is empty and says the same;
  - the palette's **New session with <agent>** still works for a specific agent.
- **No automatic pick,** even when only one agent is ready.
- **Choosing one:**
  - writes `default_agent` to the user's file;
  - starts that agent in the background, as the default agent starts at launch today;
  - lets the History fill in.

### Status **(call)**

| Status | When |
|---|---|
| CLI not found | the command's program (its first word, so `npx` for Claude and Codex) is not on wai's `PATH`; checked when the section opens, cheap |
| Not checked | the program exists but the agent has not been started this run |
| Starting… | it is starting (the first `npx` run downloads the adapter) |
| Ready | it started and is signed in |
| Not signed in · run `<hint>` | it reported that auth is required (today's `AgentEvent::AuthRequired`; add ACP auth status where the agent supports it) |
| Failed | it exited; shows the last lines of its error output, as sessions do today |

- **Learning a status means starting the agent.** That happens when you pick it, when you press **Test**, or when a session starts.
- **Test** stops the process again unless that agent is the default or has open sessions.

### Models **(owner: yes; details are the call)**

- **In a session:** the header gets a picker for each session config option the agent offers in the `model` and `thought_level` categories, next to today's mode picker.
  - When the agent also offers `mode` as a config option, that replaces the old mode picker, so there are no duplicates.
  - When the agent offers none of these, no picker shows.
- **A change in a session** applies to that session only, from then on.
- **In Settings:** **Default model** per agent is stored in a table of its own in the user's file, `[models]` with `<agent id> = "<value id>"`. Keeping it out of `[agents]` means an extension agent needs no `command` entry to have a default model. A project file may not set `[models]`. **(call)**
- **Which sessions it applies to:** only brand-new ones. Resumed, forked and restored sessions keep whatever the agent restores. If the agent no longer offers that model, the new session shows a one-line notice and keeps the agent's own choice. **(call)**
- **A model picked inside a session** is not saved by wai across relaunches. Add that if it turns out to matter. **(call)**
- **Where the list of models comes from:** the agent, so wai learns it only after starting the agent once.
  - wai keeps the last list it saw per agent in `state.json`.
  - Before any start, the field says "Start a session or press Test to see the models."

### Saving and live apply **(call)**

- **Saving:** there is no Save button. Each change is written at once to the user's `settings.toml`.
  - Comments and formatting are kept (`toml_edit`, already in `Cargo.lock`).
  - The file is created when missing.
- **Watching:** wai watches the user's `settings.toml` and the project's `.wai/settings.toml`. On a change it reloads and applies.
  - Its own writes must not loop or flicker.
  - A file with an error keeps the last good settings in force. wai shows a notice with the error, and the screen becomes read-only with "settings.toml has an error: … Open" until the file is fixed. The screen never writes over a broken file.
- **A broken file at launch:** wai no longer stops. Today, opened from Finder, it would silently fail to open.
  - It opens with the defaults (so no agent) and shows Settings with the same read-only error and **Open**.
  - Fixing the file applies through the watcher.
  - Update the user-guide line that says an invalid file stops wai. **(call)**
- **When each change takes effect:**

| Change | Takes effect |
|---|---|
| Theme | at once, in every window |
| Default agent | for the next sessions; open sessions keep their agent |
| Default model | for the next sessions of that agent |
| An agent's command | for the next start of that agent; running sessions are never killed silently |
| Shell | for terminals opened afterwards |
| Updates | at the next check |

### Confirming what wai runs **(call, following the owner's agreement in the design talk)**

One rule, wherever the value comes from: **wai runs an agent command or a terminal shell only after you have approved that exact value.**

- **Approved values:** wai keeps them in `state.json`, which is wai's own file. That way an edit to `settings.toml` cannot approve itself.
  - The built-in commands are approved from the start.
  - Today's values are approved on the first launch of this version, so nothing changes for an existing setup.
- **The question:** before running a value that is not approved, wai shows a dialog with the agent (or "Terminal shell") and the old and new value.
- **When it is asked:**
  - at launch;
  - on a reload;
  - when a reload adds a new `[agents.<id>]`;
  - when you switch the default to an agent whose command is not approved yet;
  - when you edit the value on the screen.
- **Approve:** the value is added to the approved list and used.
- **Decline:** wai keeps the last approved value for this run (or leaves that agent unavailable when there is none) and does not write to `settings.toml`. It asks again next time it would run that value. A declined edit never runs, not even at the next launch.

This covers an agent editing `settings.toml` with its own file tools. Project 3 reuses the same dialog. **(call, following the owner's agreement in the design talk that changing what wai runs always needs wai's own confirmation)**

### Docs

- **`docs/user-guide.md`:**
  - a Settings section;
  - the first-run flow;
  - the four agents and their hints;
  - model pickers.
- **README:**
  - "First run" no longer says the default agent is `claude`;
  - "Known limitations" drops "settings are read at launch" (the line about project settings applying only to the launch folder stays if it is still true).

## Left to the builder

- How the settings tab plugs into the panes and layout code, and which gpui-component widgets it uses.
- The search's matching rules.
- The process lifecycle for **Test**, and status caching within a run.
- How config options and the old `SetSessionMode` path live side by side for agents that send only one of them.
- Extending `wai-fake-agent` to offer config options and auth status, plus tests for:
  - the first-run flow;
  - writes that keep comments;
  - a broken file;
  - confirmation on command changes;
  - R15 (the existing tests stay).

## Open risks

- **Copilot:** besides #845, it is unknown whether Copilot and Antigravity offer model config options. If they do not, they simply get no model picker.
- **Live theme switch:** changing the theme live must also restyle the Aurora backdrop and cards (`aurora.rs`), not just gpui-component's theme.
