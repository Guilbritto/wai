# wai user guide

This guide describes wai 0.1.0. On Windows, read **ctrl** where it says **⌘**, except where the shortcut table says otherwise.

## The window

- **Top bar.** It shows wai's version and the commit it was built from (`0.2.0 · 01dd232`), the project's name and how many agents are working or need you, and holds the buttons **New from template** (shown when templates exist), **Extensions**, the command palette, **Settings** (⚙), the terminal and **New session**. On macOS it is the title bar: drag it to move the window, double-click it to zoom.
- **Sidebar.** The tabs **Sessions**, **History**, **Files**, **Search** and **Agents**, plus one tab per extension panel.
- **Panes.** Up to four sit side by side. Each holds a chat session, the editor (one editor pane at most) or Settings (one at most). **⌘⌥→** moves the focus to the next pane.
- **Terminal.** It sits at the bottom, toggled with **ctrl-`**.

**The look:** the default dark theme is Aurora: an indigo night with a soft, still aurora behind floating glass cards. Set `theme = "light"` for the light one, which keeps plain panels and no aurora (see [Settings](#settings)).

**On relaunch** wai restores the project, the open sessions with their history, which pane each was in, whether the terminal was open, and the sidebar width. Editor tabs are not restored, except unsaved ones (see [Unsaved edits after a quit](#unsaved-edits-after-a-quit)).

**Opening another folder:** **⌘O** opens it in the same window. If any file has unsaved edits, wai asks first.

## Sessions

### Starting

There are four ways to start a session:
- **⌘N** or the **+** button starts one with the default agent (`default_agent`, which you choose in [Settings](#the-settings-screen); until you do, these open Settings instead).
- **New from template** (the file icon in the top bar) starts one from a template.
- The palette's **New session with &lt;agent&gt;** picks the agent.
- A row in the **History** tab reopens a past session.

While the agent prepares the session, its pane shows `Preparing <agent>…`. **⌘W** there takes the pane off the screen; the session still opens, in the sidebar.

**The default agent** starts with wai, so its history is listed without waiting. With none chosen, no agent starts, the **History** tab says `Choose an AI in Settings to see its history.` and Settings says `Choose an AI to start a session.`

### Chatting

- **Sending:** **Enter** sends the prompt and **Shift+Enter** adds a new line. **Send** turns into **Stop** while the agent works, and **Esc** in a session also stops the turn.
- **Attaching files:** drop them on the prompt box. PNG, JPEG, GIF and WebP files up to 5 MB go as images; any other file goes as a reference to its path.
- **The header** shows:
  - the session's title;
  - a picker for each of the agent's `model` and `thought_level` options, when it offers them;
  - the agent's mode picker, when the agent has modes (Claude Code's include default, plan and accept-edits modes);
  - the context used, as `used / size`.
- **Model pickers:** a change applies to that session only, from then on, and wai does not keep it across relaunches. A new session starts with the **Default model** from Settings, if you set one; if the agent no longer offers it, the session says `<agent> no longer offers the model <model>; using <current>.` and keeps the agent's own choice. Resumed, forked and restored sessions keep what the agent restores. An agent that offers no such option shows no picker.
- **The transcript:**
  - Replies stream in as Markdown, and thinking is folded.
  - Tool calls show their output, up to 2,000 lines, with **Show all**.
  - A very large output offers **Open full output in editor**. That opens a read-only copy kept under `<config>/scratch/`, which is cleared at each launch.

### Status

Each open session has a coloured dot in the sidebar:

| Dot | Meaning |
|---|---|
| Teal in Aurora, cyan in light mode | `Working` |
| Amber, labelled `Needs you` | A permission card or a question waits for you |
| Grey | Idle |
| Red | `Error`: the agent exited or failed |

- **The Agents tab** also labels a session `Done` when its last turn ended normally.
- **When an agent exits,** its sessions show the last lines of its error output and a **Restart** button.
- **When an agent reports that it is not signed in,** wai shows `<agent> is not signed in. Open a terminal and run its login (for Claude: claude /login).` It never shows a login screen of its own.

### Permissions and questions

- **Permission cards.** When an agent asks before acting, the session shows a card with the diff of the edit (when there is one) and the agent's own choices, such as **Yes**, **Yes, always** and **No**. The session reads `Needs you` until you answer.
- **Questions.** When Claude asks a multiple-choice question, wai shows it as a form with **Submit** and **Decline**. A form wai cannot display is declined, and the transcript says so.

### Reviewing edits

At the end of a turn that changed files, an **Edits (N files)** block lists each file:
- **Open** shows it in the editor at the changed line.
- **Revert** restores the file as it was before the agent's edit. It first says what will happen.

Revert refuses in these cases:
- the file changed after the edit;
- the edited text is no longer found exactly once;
- undoing would delete a file that existed before the edit;
- the file is tab-indented or uses CRLF line endings.

### History, side by side, and forks

- **The Sessions tab** lists the open sessions. **The History tab** lists the default agent's past sessions for this project; its **All projects** switch shows every project's history. Choosing another default agent lists that agent's history instead.
- **Opening a row:** a click shows the session in the focused pane. **Open to the side** adds a pane, while there are fewer than four.
- **Following a session:** moving to a session from another folder (opening its row, clicking its pane, **⌘⌥→**, closing the pane beside it) opens that folder, as **⌘O** does: the editor pane closes, after asking about unsaved edits, and Files, Search, History and templates follow. Relaunching and sessions opened by extensions never move you; a session whose folder is gone just shows.
- **Fork:** for agents that can load and fork sessions, **Fork** on an open or past session opens a copy with the same history beside it.
- **Closing a pane:** **⌘W** in a session pane, or the **×** at the right of its header, takes the pane off the screen. The session keeps running and stays in the **Open** list; click it to show it again. In a split, an empty pane has its own **×**, which closes the pane.
- **Ending a session:** the **×** on its **Open** row ends the session, cancels its turn and closes its pane. The agent keeps the session in its history.
- **Arranging panes:** drag a session by its pane's title, or drag an **Open** or **History** row, onto a pane. Drop it on the pane's left or right quarter to open it in a new pane on that side, or on the middle to show it there. A session from another pane swaps places; any other takes the pane, and the session it showed stays open in the sidebar. An empty pane is filled wherever you drop. With four panes, a row from the sidebar always takes the pane it is dropped on, while a pane can still be moved to either side. An editor with unsaved edits is never replaced: the session opens beside it. The layout is kept on relaunch.

### Templates

A template names an agent and a first prompt. The prompt is filled in but **not sent**.

`~/.config/wai/templates.toml` (`%APPDATA%\wai\templates.toml` on Windows):

```toml
[[template]]
name = "Fix a failing test"
prompt = "The test below fails. Find the cause and fix it without changing the test:\n"

[[template]]
name = "Review with my-agent"
agent = "my-agent"   # optional; the default agent if left out
prompt = "Review the uncommitted changes in this repository."
```

- **Where templates come from:**
  - A project can add its own in `<project>/.wai/templates.toml`. One with the same name replaces yours.
  - Enabled extensions add theirs after these.
- **Reloading:** templates are read again when you open another folder.
- **Unknown agent:** a template naming an agent wai does not know shows `Template "<name>" uses unknown agent "<agent>"`.

## Files and the editor

### The Files tab

- It shows the project tree, listed folder by folder.
- Inside a git repository it hides what `.gitignore` (and your global git ignores) exclude.
- It updates when files change on disk.
- A click opens the file in the editor pane, which has tabs.

### Editing

- **⌘S** saves.
- **Highlighting:** syntax is highlighted by file extension.
- **Line endings:** CRLF files keep their line endings.
- **Size limits:**
  - files over 20 MB open read-only, with a banner;
  - files over 200 MB are refused (`Too large to open in wai`);
  - binary files are refused (`Binary file`);
  - text that is not UTF-8 is refused (`Not a UTF-8 text file`).

### Save safety

wai writes a file only when you ask it to: through **Save**, **Revert**, or a permission you give an agent.

- **Changes on disk.** wai never saves over a change on disk that you have not seen.
  - When a file changes on disk and its tab has no edits, the tab reloads.
  - When the tab has edits, it shows **Changed on disk** (or **Deleted on disk**) with **Reload** and **Keep mine**. Save is refused until you pick one.
- **Atomic saves.** A save writes a new temporary file and renames it over the original. The file's permissions are kept, and a symlinked file is saved through the link.
- **Unsaved edits.** Closing a tab with edits asks **Save**, **Don't save** or **Cancel**. Quitting, closing the window or opening another folder asks **Save all**, **Discard** or **Cancel**.

### Unsaved edits after a quit

wai keeps a backup of every tab with unsaved edits:
- about a second after you stop typing;
- again on every quit macOS reports, including Quit from the Dock, logout and shutdown.

**Where backups go:** each one is `<config>/recovery/<n>.json`. On macOS and Linux its file is private to you (mode 0600), in a private folder (0700); on Windows the folder inherits the permissions of `%APPDATA%`. The original file is never touched.

**On the next launch** each backup reopens as an unsaved tab with the banner **Recovered unsaved changes**.
- **When the file changed on disk meanwhile:** the tab also shows **Changed on disk**, and Save waits for **Reload** or **Keep mine**.
- **When the file is gone:** the tab reopens under its old path.

**When a backup is deleted:** when you save or discard the tab, or reload it.

**Not covered:**
- A force-quit or crash loses at most the last second of typing.
- A buffer over 50 MB is not backed up, and wai says so once (`Too large to back up: <name>`).
- A failed backup also shows a notice once (`Could not back up unsaved changes: <error>`).

## Search

The Search tab searches file contents across the project:
- **Options:** **Aa** matches case and **.\*** reads the query as a regular expression.
- **When it runs:** typing pauses briefly before it searches, and a new query cancels the old one.
- **Results:** they arrive in batches, grouped by file. A click opens the file at the line.
- **What it skips:** it skips what `.gitignore` excludes inside a git repository.
- **Limit:** it stops at 10,000 matches (`Showing first 10,000`).

## The Agents tab

The Agents tab has one live card per open session. Each card shows:
- the status;
- the agent's plan (8 rows, then `+N more`);
- the tool call in progress;
- the context used (`used / size (pct%)`).

Clicking a card shows that session in the focused pane.

## The terminal

**ctrl-`** shows or hides a terminal in the project folder.

- **Shell:** `terminal.shell` from your settings, else `$SHELL`, else `/bin/sh`. On Windows the fallbacks are `%COMSPEC%`, then `powershell.exe`.
- **Scrollback:** 10,000 lines.
- **Copy and paste:** select with the mouse, then copy with **⌘C** and paste with **⌘V**. On Windows and Linux use **ctrl-shift-c** and **ctrl-shift-v**.
- **Keys on Windows and Linux:** while the terminal has focus, **ctrl-c/v/n/w/o/q** go to the shell. wai's own actions move to **ctrl-shift-n/w/o/q**.

## The command palette

**⌘⇧P** opens the palette. Type to filter, **Enter** runs a row, **Esc** closes. Its groups are:

| Group | Rows |
|---|---|
| wai | New session, Toggle terminal, Open folder…, Search, Files, Agents, Extensions, Settings |
| Templates | Every template |
| Commands | The enabled extensions' commands |
| Agents | `New session with <agent>`, one per agent |

## Keyboard shortcuts

| Action | macOS | Windows and Linux | Where |
|---|---|---|---|
| Quit | ⌘Q | ctrl-q (ctrl-shift-q in the terminal) | anywhere |
| New session | ⌘N | ctrl-n (ctrl-shift-n in the terminal) | window |
| Close the pane (the session stays open), Settings, or the editor's active tab | ⌘W | ctrl-w (ctrl-shift-w in the terminal) | window |
| Save | ⌘S | ctrl-s | window |
| Open folder | ⌘O | ctrl-o (ctrl-shift-o in the terminal) | window |
| Command palette | ⌘⇧P | ctrl-shift-p | window |
| Settings | ⌘, | ctrl-, | window |
| Focus next pane | ⌘⌥→ | ctrl-alt-→ | window |
| Show or hide the terminal | ctrl-` | ctrl-` | window |
| Stop the turn | Esc | Esc | chat session |
| Send the prompt / new line | Enter / Shift+Enter | Enter / Shift+Enter | prompt box |
| Copy the selection | ⌘C | ctrl-shift-c | terminal |
| Paste | ⌘V | ctrl-shift-v | terminal |

## Notifications

**System notifications.** wai sends one only while its window is in the background:
- when a session needs you (`<title> needs you`);
- when a turn finishes normally (`<title> is done`).

Extensions with the `notify` permission can send their own, at most one every two seconds each.

**In-app notices** appear in the window. They carry:
- settings that were ignored;
- a project's extension suggestion;
- install errors;
- the update notice.

## The update check

It is **off by default**. When you turn it on:
- once per launch, with no timer, it reads `https://api.github.com/repos/<repo>/releases/latest` for the repository in `updates.repo`;
- if that release is newer, it shows `wai <version> is available` with **Open release page**.

It never downloads or installs anything. When it fails (offline, rate-limited, a bad answer), the error goes to wai's log and nothing is shown.

```toml
[updates]
check = true
repo = "owner/wai"
```

Only your own settings file can turn it on; a project's `.wai/settings.toml` cannot.

## Settings

| | macOS and Linux | Windows |
|---|---|---|
| Config folder (`<config>`) | `~/.config/wai` | `%APPDATA%\wai` |
| Settings | `<config>/settings.toml` | `<config>\settings.toml` |
| Templates | `<config>/templates.toml` | `<config>\templates.toml` |

- **When they apply:** at once, with no restart. wai watches your `settings.toml` and the project's `.wai/settings.toml`, and changes made in the [Settings screen](#the-settings-screen) are written to your file.
- **A broken settings file:** wai does not stop. It keeps the last good settings (the defaults at launch, so no agent), and the Settings screen turns read-only with `settings.toml has an error: …` and **Open**. An edit that breaks the file while wai runs also shows a notice with the error. Fix the file and it applies again. The screen never writes over a broken file.
- **Unknown keys** are ignored.

### The Settings screen

- **Opening:** **⌘,** (ctrl-, on Windows and Linux), the ⚙ button in the top bar, or **Settings** in the command palette. It opens as a pane tab. There is at most one: opening it again focuses it. **⌘W** closes it, and it is not restored on relaunch.
- **Arranging:** drag its **Settings** title onto a pane like a session's title. It moves or splits as a session does.
- **Layout:** the sections on the left, a **Search settings** field on top that filters the rows of every section, and **Open settings.toml** at the bottom, which opens your file in the editor.
- **No Save button.** Each change is written at once to your `settings.toml`, keeping its comments and formatting, and applies as the table below says.

**Sections**
- **AI** (the first run opens it): a radio list with one row per agent: the four built-ins, those you added and those from extensions. Each row has:
  - its status: `CLI not found`, `Not checked`, `Starting…`, `Ready`, `Not signed in · run <hint>` or `Failed: <last error line>`;
  - **Test**, which starts the agent to learn its status and models. The agent keeps running afterwards;
  - **Default model**, a picker once wai has seen the agent's models; before that it says `Start a session or press Test to see the models.`;
  - **Advanced**, folded, which shows the **Command**. Changing it asks `Run a new command?` (see [Agent commands](#agent-commands)).
- **Appearance:** **Theme**, Dark or Light, applies at once.
- **Terminal:** **Shell**, a path; empty is the system default. It applies to terminals opened afterwards, and changing it asks `Run a new command?`.
- **Updates:** **Check for updates** and **Repository**.

**First run.** With no `default_agent`, no agent starts and wai opens Settings on **AI**, saying `Choose an AI to start a session.` Choosing a row writes `default_agent` and starts that agent. wai never picks one for you, even when only one is ready. **⌘N**, **+**, a template without an agent and an empty pane lead to the same place; the palette's **New session with <agent>** still starts a specific agent.

| Change | Takes effect |
|---|---|
| Theme | at once |
| Default agent | for the next sessions; open sessions keep their agent |
| Default model | for the next sessions of that agent |
| An agent's command | for the next start of that agent; running sessions are not killed |
| Shell | for terminals opened afterwards |
| Updates | at the next check |

Every setting and its default:

| Key | Default | Meaning |
|---|---|---|
| `default_agent` | unset | The agent ⌘N starts, whose history the sidebar lists, and that templates without `agent` use. Unset, wai starts no agent and opens Settings |
| `theme` | `"dark"` | `"dark"` (Aurora) or `"light"` |
| `agents.<name>.command` | the built-in's command (see below) | How to start the ACP agent `<name>` (see below) |
| `models.<name>` | unset | The id of the model a new session of agent `<name>` starts with |
| `terminal.shell` | unset | The terminal's program (no arguments); unset means `$SHELL`, then `/bin/sh` |
| `updates.check` | `false` | Turns on the update check |
| `updates.repo` | unset | `owner/repo` on GitHub; unset means nothing is checked |

An example:

```toml
default_agent = "claude"
theme = "dark"

[agents.claude]
command = "npx -y @agentclientprotocol/claude-agent-acp@0.101.0"

[agents.my-agent]
command = "MY_AGENT_LOG=debug /opt/my-agent/bin/my-agent --acp"

[terminal]
shell = "/bin/zsh"
```

### Agent commands

The `command` is one of two forms:
- **A command line.** It is split like a POSIX shell line on every platform, so quote arguments with spaces. Backslashes are escapes there, so for Windows paths prefer the JSON form. Leading `NAME=value` words set environment variables for the agent.
- **A JSON object, written as a string:** `command = '{"command": "/path/to/agent", "args": ["--acp"], "env": {"NAME": "value"}}'`.

**The built-in agents.** There are four, and none is the default until you choose it. An entry `[agents.<id>]` with a built-in's id replaces its command; any other id adds an agent, shown with its id as its name.

| Id | Name | Command | If the CLI is missing | If signed out |
|---|---|---|---|---|
| `claude` | Claude | `npx -y @agentclientprotocol/claude-agent-acp@0.101.0` | Install Node.js (it provides npx) | `claude /login` |
| `codex` | Codex | `npx -y @agentclientprotocol/codex-acp@2.6.0` | Install Node.js (it provides npx) | `npx @openai/codex login` |
| `antigravity` | Antigravity | `agy acp` | https://antigravity.google/docs/cli | `agy login` |
| `copilot` | Copilot | `copilot --acp --stdio` | `npm install -g @github/copilot` | `copilot`, then `/login` |

Only Claude's command was checked end to end. Codex's command downloads and starts, but was not signed in when tried; Antigravity and Copilot were not tried, and no install or login hint was checked.

**Copilot's ACP mode is a public preview.** Its row carries the warning `Copilot may edit files and run commands without asking.`

**Approving a command.** wai runs an agent command or a terminal shell only after you approve that exact value. The built-in commands are approved from the start. When a value is new or changed (by editing `settings.toml`, by a reload, or on the screen), wai asks `Run a new command?` with the old and new value and **Allow** or **Don't allow**.
- **Allow** adds it to the approved list in wai's own `state.json` and uses it.
- **Don't allow** keeps the last approved value for this run (an agent never approved stays unavailable) and writes nothing to `settings.toml`: a value edited on the screen is asked about before it is written, so the file stays as it was. wai asks again when you pick that agent as the default, when the value changes, and at the next launch; other reloads leave it alone.

Enabled extensions can add agents. They appear as `<extension-id>/<name>`.

Agents start from the config folder, never from inside a project, so a repository's `.npmrc` or similar files cannot change what runs.

### Agents and PATH

wai looks up a bare program name such as `npx` itself.
- It searches only the absolute folders on its own `PATH`: never the current or project folder, and never an empty or relative `PATH` entry.
- The same rule applies to MCP servers and extension programs.
- A command that is not found fails with `<command> is not on PATH`.

**The app launched from Finder, the Dock or Launchpad** gets macOS's default `PATH`, `/usr/bin:/bin:/usr/sbin:/sbin`, which has neither `npx` nor `node`. So on macOS, when wai was not started from a terminal (no `TERM`), it uses your login shell's `PATH` instead:
- It asks `$SHELL -i -l`, or zsh if `SHELL` is not an absolute program. The shell runs from the config folder and starts from macOS's `PATH`. Your `.zprofile` and `.zshrc` apply as in a terminal.
- It imports `PATH` only. The new `PATH` is the shell's absolute folders, then those of macOS's it lacks. Relative entries are dropped.
- It caches the answer in `~/.config/wai/shell-path`. A launch that finds the cache uses it at once, then asks the shell again in the background for the next launch. **So wai sees a `PATH` change, such as a new Node version, one launch late:** the first launch after the change still uses the cache. To pick a change up at once, delete that file or start wai from a terminal.
- wai uses the cache only if it is a regular file of yours, not a symlink, not writable by group or others and at most 64 KB. Otherwise it asks the shell as on a first launch.
- It logs `wai: PATH from cache (<n> entries)`, or `wai: PATH from login shell (<n> entries, <ms> ms)` on a launch that waited for the shell. Only a launch without a usable cache waits, as long as your `.zshrc` takes. A Finder launch sends these lines nowhere; to see them, run `env -u TERM /Applications/wai.app/Contents/MacOS/wai` in a terminal.
- With no usable cache, if the shell gives no `PATH` within 3 seconds, wai stops it, keeps macOS's `PATH` and logs `wai: kept the launch PATH, none from <shell>: <reason>`.

Started from a terminal (`open /Applications/wai.app`, or `wai` on your `PATH`), wai skips this and uses the terminal's environment.

If an agent is still not found, for example because your shell gave no `PATH` in time:
- start wai from a terminal;
- or give the agent an absolute program and a `PATH` that contains `node`. For Node installed in `/usr/local/bin`:

  ```toml
  [agents.claude]
  command = "PATH=/usr/local/bin:/usr/bin:/bin /usr/local/bin/npx -y @agentclientprotocol/claude-agent-acp@0.101.0"
  ```

  With Homebrew on Apple silicon, use `/opt/homebrew/bin` in both places.
- MCP servers that extensions contribute with a bare `npx` (such as wai-navegador's) have no per-server override, so for those, start wai from a terminal.

### What a project's `.wai/` may set

A repository you open can carry a `.wai/` folder. Because opening a cloned repository must never choose what wai runs, it is limited:

| File | Allowed | Not allowed |
|---|---|---|
| `.wai/settings.toml` | `theme`; `default_agent`, when you have already chosen a default and it names that default, an agent with an `[agents.<id>]` entry in your file, or an agent of an installed extension | `agents` and `terminal` (commands, shells, environment); `models`; `updates`. These are ignored with a notice |
| `.wai/templates.toml` | Templates, as above | — |
| `.wai/extensions.toml` | Suggesting extensions | Installing anything |

- **File limits:** each file is read only if it is a regular file of at most 1 MB.
- **A broken project file** is skipped with a notice. wai still opens.
- **A project's `default_agent`** never picks a built-in you have not chosen and never skips the first run; otherwise it is ignored with a notice. When it is honoured, the AI section says `This project uses <agent>`, and the radio still changes your own choice.
- **Which project:** settings, templates and suggestions are read again when you open another folder.

A suggestion looks like this:

```toml
[[suggest]]
id = "wai-waycarbon"
source = "https://github.com/owner/wai-waycarbon"   # or git@host:path, or a folder inside the project
```

wai shows `This project suggests extension <id>` with **Review**. Review opens the same consent dialog as any install; nothing is installed until you approve it.

## Extensions

The **Extensions** button opens the list of installed extensions. There you can:
- turn each one on or off (**Enabled**);
- **Uninstall** it;
- install from a folder (**Install from folder…**) or a git URL (**Install from git URL…**, `https://` or `git@`).

**Consent.** Every install shows the extension's name, version and source, and **every permission it asks for**, with the warning `wai cannot restrict files or network access in this version; only install extensions you trust.`

- Nothing is installed until you click **Install**, and consent is all or nothing.
- To update an extension, uninstall it and install the new version.
- **Claude Code plugins** an extension suggests are listed under it with **Install in terminal**. That types `claude plugin install <name>` into the terminal without pressing Enter: you confirm with Enter.

**When an extension stops.**
- An extension that fails to start shows `Failed: <reason>`.
- One that keeps crashing is restarted up to three times, then shows `Crashed:` with its last error lines.
- Turning it off and on again gives it three more tries.

First-party extensions live in `extensions/`. Each has its own README:
- `wai-waycarbon`: WayCarbon templates and a Jira panel;
- `wai-navegador`: a browser for agents through Playwright's MCP server;
- `wai-rotinas`: scheduled scripts;
- `wai-remoto`: phone access, design only.

To write your own, see [extensions.md](extensions.md).
