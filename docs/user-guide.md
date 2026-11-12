# wai user guide

This guide describes the latest wai release. On Windows, read **ctrl** where it says **⌘**, except where the shortcut table says otherwise.

## The window

- **Top bar.** It shows wai's version and the commit it was built from (`1.2.3 · 01dd232`), the project's name and how many agents are working or need you, and holds the buttons **New from template** (shown when templates exist), **Extensions**, the command palette, **Settings** (⚙), the terminal, the Agents panel (only while it is on the right) and **New session**. On macOS it is the title bar: drag it to move the window, double-click it to zoom.
- **Sidebar.** The tabs **Sessions**, **History**, **Files**, **Search** and **Agents**, plus one tab per extension panel.
- **Agents panel.** Agents can leave the sidebar for its own panel right of the panes, which collapses with **⌘⌥B** (see [The Agents tab](#the-agents-tab)).
- **Panes.** Up to four sit side by side. Each holds a chat session or the editor (one editor pane at most). **⌘⌥→** moves the focus to the next pane.
- **Terminal.** It sits at the bottom, toggled with **ctrl-`**.

**The look:** the default dark theme is Aurora: an indigo night with a soft, still aurora behind floating glass cards. Set `theme = "light"` for the light one, which keeps plain panels and no aurora (see [Settings](#settings)).

**On relaunch** wai restores the project, the open sessions with their history, which pane each was in, whether the terminal was open, where Agents was (and whether its panel was collapsed), and the sidebar width. Editor tabs are not restored, except unsaved ones (see [Unsaved edits after a quit](#unsaved-edits-after-a-quit)).

**Opening another folder:** **⌘O** opens it in the same window. If any file has unsaved edits, wai asks first.

## Sessions

### Starting

There are four ways to start a session:
- **⌘N**, the **+** button or a double click on the **Sessions** tab starts one with the default agent (`default_agent`, which you choose in [Settings](#the-settings-screen); until you do, these open Settings instead).
- **New from template** (the file icon in the top bar) starts one from a template.
- The palette's **New session with &lt;agent&gt;** picks the agent.
- A row in the **History** tab reopens a past session.

While the agent prepares the session, its pane shows `Preparing <agent>…`. **⌘W** there takes the pane off the screen; the session still opens, in the sidebar.

**The default agent** starts with wai, so its history is listed without waiting. With none chosen, no agent starts, the **History** tab says `Choose an AI in Settings to see its history.` and Settings says `Choose an AI to start a session.`

### Chatting

- **Sending:** **Enter** sends the prompt and **Shift+Enter** adds a new line. While the agent works, **Stop** stands beside **Send**, and **Esc** in a session also stops the turn.
- **Sending during a turn:** you can keep typing and sending while the agent works.
  - An agent that can be steered (Claude Code) gets the message inside the running turn. It waits above the prompt box as a `Sent` balloon until the agent reads it, then joins the transcript there. wai takes the agent's next reply, thought or tool call as the sign that it read the message; while a card waits, the one after you answer it. A message still unread when the turn ends joins the transcript then. If the turn ended meanwhile, it goes as a new prompt. If steering fails, the message is queued instead, and the transcript says so.
  - Any other agent (Codex, Copilot) gets it after the turn: it waits above the prompt box as a `Queued` balloon, with a **×** that puts it back in the prompt box. When the turn ends, the queued messages go out together as one prompt. If you stop the turn, or it ends any other way, they go back into the prompt box, before anything typed since, and their attachments back to chips, unsent.
- **Commands and skills:** a `/` first in the prompt box lists the agent's slash commands just above it (for Claude Code: the built-ins it supports, your custom commands and your skills), each with its description and, when it takes input, a hint of what to type.
  - What follows the `/` filters the list: names that start with it first, then names or descriptions that contain it. When nothing matches, the list hides.
  - **↑**/**↓** move, **Enter**, **Tab** or a click puts `/name ` in the box, and **Esc** closes the list and keeps the text.
  - The list closes once a space follows the name; **Enter** then sends, and the agent runs the command. During a turn, it goes into the turn or the queue like any message.
  - Until the agent sends its commands, the list reads `No commands from <agent> yet`.
- **The status line:** while a turn runs, a line just above the prompt box says what the agent is doing (`Reading README.md…`, `Running git log…`, `Thinking…` between tool calls, `Waiting for you…` while a card waits), for how long, and `Esc to stop`. Its text is grey while the agent works and amber while a card waits for you; a green ring turns while the agent works and stands still while it waits. The time and `Esc to stop` sit at its right end. It goes away when the turn ends.
- **Attaching files:** drop them on the prompt box, pick them with the **+** at its left (files and folders, several at once), or paste them with **⌘V**: an image you copied, such as a screenshot, or files copied in Finder. Text pastes as text. Each attachment shows as a chip above the box until you send or remove it.
  - PNG, JPEG, GIF and WebP files up to 5 MB go as images; any other file, of any kind, goes as a reference to its path, which the agent reads with its own tools.
  - A pasted image is saved in the system's temporary folder as `wai-pasted/<id>.<ext>`. A TIFF, BMP, ICO or PNM image, which agents do not take, is saved as a PNG; an SVG goes as a file.
  - An agent that does not take images gets each image as a reference to its file instead.
- **The header** shows the session's title (drag it to move the pane), the context used as `used / size`, and the close button.
- **The strip on the prompt box:** a tab on the box's top edge, 18 px in from its left, with the session's folder, its branch (a fork icon for a worktree; only when known) and the agent (`✳ Claude Code`). They are fixed once the session runs, so they are labels, not menus. In a narrow pane the branch and folder names shrink and truncate first.
- **The bar under the prompt:** inside the box, under the text: **+** to attach, then the agent's mode picker (Claude Code's include default, plan and accept-edits), a gap, then a picker for each of the agent's `model` and `thought_level` options, when it offers them, then Stop (during a turn) and Send. An agent with no modes and no model options shows only **+** and Send.
- **Model and mode pickers:** a change applies to that session only, from then on, and wai does not keep it across relaunches. A new session starts with the **Default model** from Settings, if you set one; if the agent no longer offers it, the session says `<agent> no longer offers the model <model>; using <current>.` and keeps the agent's own choice. Resumed, forked and restored sessions keep what the agent restores. An agent that offers no such option shows no picker.
- **The transcript:**
  - Replies stream in as Markdown.
  - Each turn of the agent is one card headed `✦ <agent>`, holding its steps and its replies in order; your prompts are balloons on the right; a closed permission card or a notice between them splits the card.
  - Each reply has a copy button that copies its Markdown, and each code block one that copies its code.
  - The agent's steps (its tool calls and its thinking) show as the **Processing** setting says:

    | Processing | Steps |
    |---|---|
    | **Step by step** (the default) | Each step is one line in the reply's card: ✓ or ✗ (or a mark while it runs), then its title, commands in code style. A click opens a tool call's output in a box under the line; thinking shows as a `Thought` line that opens the same way |
    | **Grouped** | The steps between two replies fold into one block, such as `3 steps · Read README.md, Run git status`; the head names every step. A click lists the step lines, each still folded |
    | **Hidden** | No steps |

  - **Subagents.** Each subagent the agent runs (Claude Code's Agent tool) shows as a step card with its title, its status and, once it answers, its answer. Its tool calls stay in the transcript, as steps. Its own text and checklist are not shown.
  - **Always shown,** whatever the setting: your prompts, the replies, permission cards and questions, notices, and file edits. An edit that was written reads `Edited <file>` (a click shows its diff), and the turn still ends with its **Edits** block (see [Reviewing edits](#reviewing-edits)).
  - An unfolded tool call shows its output, up to 2,000 lines, with **Show all**.
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

- **Permission cards.** When an agent asks before acting, the session shows a card with the diff of the edit (when there is one) and the agent's own choices, such as **Yes**, **Yes, always** and **No**. The session reads `Needs you` until you answer. The card waits on a glass panel just above the prompt box, not in the transcript, and the conversation scrolls on behind it. Several cards stack, the oldest on top. With the prompt box empty, **1**–**9** pick the top card's choice of that number; only the top card shows numbers. Once answered, a card joins the transcript as one line with your answer, where the conversation is then.
- **Questions.** When Claude asks a multiple-choice question, wai shows it as a form with **Submit** and **Decline**. A form wai cannot display is declined, and the transcript says so. They wait above the prompt box the same way.
- **A subagent's card** is titled `<subagent>: <request>` and shows in the session's chat.
  - **Stop** cancels only the session's own cards.
  - A background subagent's card stays open, even after the turn ends, until you answer it; the session reads `Needs you` meanwhile.
  - A subagent the turn was waiting on stops with the turn, and its card closes as `Cancelled`.

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
- **Right-click menus:** an **Open** row's menu has **Rename**, **Fork**, **Move to** each other group, **New group with this session** and, for a session in a group, **Remove from group**. A group header's menu has **Rename group** and **Delete group**, and a **History** row's has **Fork**. The rows themselves keep only **Open to the side** and, on **Open** rows, the **×** that ends the session.
- **Renaming a session:** **Rename** in its **Open** row's menu, or a double click on the row, turns its title into a text box. **Enter**, or clicking elsewhere, keeps the name; **Esc** does not. The name shows wherever the session's title does, is kept on relaunch and is never sent to the agent, whose own titles no longer replace it. An empty name gives the agent's title back. Ending the session drops the name.
- **Groups:** the **+** beside **Open** adds a group, ready for its name. Put a session in a group from its row's menu, or by dragging its **Open** row onto the group's header; take it out with **Remove from group**, or by dragging it onto the sessions above the groups (or the **Open** heading). A session is in one group at most. A group's header has a chevron that folds it, its name and how many sessions it holds; a double click on the name renames it. **Delete group** deletes the group only: its sessions stay open, in no group. A session that ends leaves its group, and an empty group stays until you delete it. Groups are kept on relaunch.
- **Reordering:** drag an **Open** row onto another to take its place: dragged down, it lands after that row; dragged up, before it. A row dropped on a row of another group joins that group. The **Agents** cards follow the order, which is kept on relaunch.
- **Opening a row:** a click shows the session in the focused pane. **Open to the side** adds a pane, while there are fewer than four.
- **Following a session:** moving to a session from another folder (opening its row, clicking its pane, **⌘⌥→**, closing the pane beside it) opens that folder, as **⌘O** does: the editor pane closes, after asking about unsaved edits, and Files, Search, History and templates follow. Relaunching and sessions opened by extensions never move you; a session whose folder is gone just shows.
- **Fork:** for agents that can load and fork sessions, **Fork** in an open or past session's menu opens a copy with the same history beside it.
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
- its running subagents, one line each: `title · Working` or `title · Needs you`, then the tool call in progress. A line goes away when its subagent finishes. A background subagent keeps its line after the turn ends, until it finishes;
- the tool call in progress;
- the context used (`used / size (pct%)`).

Clicking a card shows that session in the focused pane; dragging it onto a pane works as an **Open** row does.

**On the right.** The button in the Agents header, or **Move Agents to the right** in the command palette, moves Agents out of the sidebar into its own panel right of the panes, as wide as the sidebar. The sidebar then has no Agents tab. **Move to the sidebar** (same button, same palette row) puts it back.

**Collapsing.** On the right, the header's **Collapse** button hides the panel. The top bar's Agents button and **⌘⌥B** show it again; both toggle it. The button shows how many sessions need you. In the palette, **Agents** shows the panel too.

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
| wai | New session, Toggle terminal, Open folder…, Search, Files, Agents, Move Agents to the right (or to the sidebar), Extensions, Settings |
| Templates | Every template |
| Commands | The enabled extensions' commands |
| Agents | `New session with <agent>`, one per agent |

## Keyboard shortcuts

| Action | macOS | Windows and Linux | Where |
|---|---|---|---|
| Quit | ⌘Q | ctrl-q (ctrl-shift-q in the terminal) | anywhere |
| New session | ⌘N | ctrl-n (ctrl-shift-n in the terminal) | window |
| Close the pane (the session stays open) or the editor's active tab | ⌘W | ctrl-w (ctrl-shift-w in the terminal) | window |
| Save | ⌘S | ctrl-s | window |
| Open folder | ⌘O | ctrl-o (ctrl-shift-o in the terminal) | window |
| Command palette | ⌘⇧P | ctrl-shift-p | window |
| Settings | ⌘, | ctrl-, | window |
| Close Settings | Esc | Esc | Settings |
| Focus next pane | ⌘⌥→ | ctrl-alt-→ | window |
| Show or hide the terminal | ctrl-` | ctrl-` | window |
| Show or hide the Agents panel, while it is on the right | ⌘⌥B | ctrl-alt-b | window |
| Stop the turn | Esc | Esc | chat session |
| Send the prompt / new line | Enter / Shift+Enter | Enter / Shift+Enter | prompt box |
| Answer the top permission card | 1–9 | 1–9 | prompt box, while empty |
| In the `/` menu: move / complete / close | ↑ ↓ / Enter or Tab / Esc | ↑ ↓ / Enter or Tab / Esc | prompt box |
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

## Updates

Every push to the `main` branch of `Guilbritto/wai` publishes a release on GitHub, and an installed wai updates itself from it.

- **Automatic updates** is on by default. On each launch wai asks GitHub for the latest release, downloads it in the background, checks its SHA-256 against the release's `SHA256SUMS.txt`, and shows `wai <version> is ready` with **Restart now**. If you have unsaved edits, wai asks before restarting.
- On macOS the new `wai.app` replaces the old one in place and the previous version stays next to it as `wai.app.old`. On Windows the installer runs for your user only, with no administrator rights, into `%LOCALAPPDATA%\wai`.
- Downloads are kept in `~/Library/Caches/wai-updates` (macOS) or `%LOCALAPPDATA%\wai-updates` (Windows).
- Only releases of `Guilbritto/wai` are installed. Pointing **Repository** at another repository only notifies you of its newer release, with **Open release page**; nothing is downloaded.
- When wai cannot update itself where it is installed (a folder your account cannot write to, such as `/Applications` for a standard user), it notifies you instead, and Settings → Updates shows **Open release page**. On macOS install wai into `~/Applications`.
- Failures (offline, rate-limited, a bad answer) go to wai's log and show nothing.

The releases are unsigned. On the first launch macOS needs right-click then **Open**, and Windows SmartScreen needs **More info** then **Run anyway**. Without a Developer ID, macOS may also ask again for access to folders such as Documents after each update.

```toml
[updates]
check = true
repo = "Guilbritto/wai"
```

Only your own settings file can change these; a project's `.wai/settings.toml` cannot.

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

Settings is a modal over the window, not a pane.

- **Opening:** **⌘,** (ctrl-, on Windows and Linux), the ⚙ button in the top bar, or **Settings** in the command palette. It opens on the page you asked for. Opening it while it is open shows that page and refreshes its data.
- **Closing:** **Esc**, a click outside it or its **×**. Closing drops the screen; it is not restored on relaunch.
- **Size:** 88% of the window each way, centered. The maximize button in the header grows it to the window and back; the size is not kept.
- **Header:** `Settings`, then the context in muted text: `(<default agent> · <project>)`, or `(<project>)` with no default agent.
- **Nav** (left, scrolls) in three groups, each row with its count on the right:
  - **wai**: AI (agents), Appearance, Conversation, Terminal, Updates, Extensions (installed);
  - **✳ Claude**: Plugins, MCP Servers, Skills, Instructions, Agents, Hooks (rows, once loaded);
  - **Other agents**: Codex, Copilot, Antigravity, each marked **Soon**.
- **Content:** the page title (display font) and its description, with the page's actions on the right. Under them a search field, `Search <page>…`, filters that page only and is cleared on every page change.
- **Nested dialogs** (the command approval, an extension's consent) open above Settings and close back to it.
- **Open**, **Open settings.toml**, the error banner's **Open** and an extension's **Install in terminal** close Settings first, then do their work.
- **No Save button.** Each change is written at once to your `settings.toml`, keeping its comments and formatting, and applies as the table below says.

**wai pages**

- **AI** (the first run opens it): a radio list with one row per agent: the four built-ins, those you added and those from extensions. Each row has:
  - its status: `CLI not found`, `Not checked`, `Starting…`, `Ready`, `Not signed in · run <hint>` or `Failed: <last error line>`;
  - **Test**, which starts the agent to learn its status and models. The agent keeps running afterwards;
  - **Default model**, a picker once wai has seen the agent's models; before that it says `Start a session or press Test to see the models.`;
  - **Advanced**, folded, which shows the **Command**. Changing it asks `Run a new command?` (see [Agent commands](#agent-commands)).
  - Below them, an **Advanced** group: `settings.toml` with **Open**, which opens your file in the editor.
- **Appearance:** **Theme**, Dark or Light, applies at once.
- **Conversation:** **Processing**, how a chat shows the agent's steps: Step by step, Grouped or Hidden (see [Chatting](#chatting)). It applies at once to every open session. Permission cards, questions and file edits always show.
- **Terminal:** **Shell**, a path; empty is the system default. It applies to terminals opened afterwards, and changing it asks `Run a new command?`.
- **Updates:** **Automatic updates** (switch), **Version** (the installed version, its status and one button: **Check now**, **Restart now** or **Open release page**) and **Repository** (see [Updates](#updates)).
- **Extensions:** extensions add panels, commands, agents and MCP servers to wai. They run with your permissions. Install from a folder or a git URL. The **Installed** group lists each one with its version badge, permissions, any problem, the Claude plugins it suggests (**Install in terminal**), an **Enabled** switch and **Uninstall** (see [Extensions](#extensions)).

The search filters the current page's rows by label (agents by name). A broken `settings.toml` shows its error banner on every wai page and makes them read-only.

**Claude pages** show what Claude Code loads from `~/.claude` and the project. They read it in the background: while they load, each says `Reading ~/.claude…`. A file that does not parse is named in a banner on the pages it feeds (`~/.claude/settings.json does not parse: <error>`), and the toggles that would write it are locked. Groups start open; on a page with more than 20 rows only the first group does. A search opens every group with a match and hides the rest. Each group's **+** is a disabled **Coming soon** button.

- **Plugins.** "What Claude loads from `~/.claude/plugins`: commands, skills, agents, hooks and MCP servers. A change applies to new Claude sessions." The **Learn more about Claude Code plugins** link opens the docs in your browser. **Browse** and **⋯** are Coming soon. There is one group per marketplace. A plugin is listed when it is installed for you or for this project. Each row has its name, a `v<version>` badge, its path, a summary (component counts such as `15 skills · 1 hook`, else its description, else `No components`) and a toggle. The toggle writes `enabledPlugins` in `~/.claude/settings.json` and says `Applies to new Claude sessions`. It is locked, with the reason shown, when a project or local settings file sets that plugin, for a managed install (`Managed by your organization`), or while your settings file is broken. Disabled plugins contribute nothing to the MCP, Skills, Agents and Hooks pages.
- **MCP Servers.** "Servers that give Claude more tools. Check status shows whether each one connects." **Add server…** is Coming soon. Groups: **From plugins** (enabled plugins only), **User**, **This project** (local scope and `.mcp.json`), and **claude.ai connectors** (one Coming soon row). Each row has its name, a badge (`plugin: <name>`, `user`, `local` or `.mcp.json`), its transport (`stdio`, `HTTP`, `SSE` or `WebSocket`), its status and a toggle. The toggle is Coming soon and disabled: wai does not change Claude's MCP servers yet, and it never writes `~/.claude.json`, Claude's own state file. Turn a server off or on with `/mcp` in Claude.
  - **Check status** runs `claude mcp list` in the project folder; it is the only thing that starts these servers, and only when you click it. The button reads `Checking…` meanwhile and the check stops after 60 s. Until then a server's status is `Not checked`. Afterwards it is `Connected`, `Needs sign-in`, `Failed to connect · <reason>`, `Pending approval` or `Off for this project`. Without a `claude` command on your PATH it says `Needs the claude CLI`.
- **Skills.** "Instructions Claude loads on its own when a task fits. They come with plugins; turn the plugin off to drop its skills." Groups **User**, **Project**, then one per enabled plugin. Each row has its name, its description in one line and **Open** (`SKILL.md`).
- **Instructions.** "Files Claude reads at the start of every session (CLAUDE.md and what it includes)." Groups **User** (`~/.claude/CLAUDE.md` and its imports) and **Project** (each `CLAUDE.md`, `.claude/CLAUDE.md` and `CLAUDE.local.md` from the project folder up, and their imports). Each row has its file name, its path, a summary (its first heading, `Included by <file>`, `This project`, `Parent folder` or `Yours, not shared`) and **Open**.
- **Agents.** "Subagents Claude can hand work to." Groups **User**, **Project**, then one per enabled plugin. Each row has its name, its path (`~/…` or `plugin: <name>`), a summary and **Open**.
- **Hooks.** "Commands Claude Code runs on its own at set moments. They run with your permissions." Groups **User** (one row per event in `~/.claude/settings.json`), **Project** (the same, from the project's settings files) and **From plugins** (one row per enabled plugin with hooks, listing its events joined with ` · `). A row's summary is `1 hook` or `N hooks`. **Open** opens the file that declares them.
- Files under a plugin's install folder open read-only. Your user and project files open in the editor, and can be edited there.

**Other agents.** Codex, Copilot and Antigravity each have a page with its title, a **Coming soon** badge, the note `Not in wai yet`, and the text `Today wai shows what Claude uses. <X> comes next; its own settings stay in its CLI meanwhile.` Each page says what it will show: the plugins, MCP servers, skills, instructions, agents and hooks of that agent, like the Claude ones.

**First run.** With no `default_agent`, no agent starts and wai opens Settings on **AI**, saying `Choose an AI to start a session.` Choosing a row writes `default_agent` and starts that agent. wai never picks one for you, even when only one is ready. **⌘N**, **+**, a template without an agent and an empty pane lead to the same place; the palette's **New session with <agent>** still starts a specific agent. A broken `settings.toml` or a missing default agent opens Settings on **AI** the same way; an approval dialog for an unapproved command opens over it.

| Change | Takes effect |
|---|---|
| Theme | at once |
| Processing | at once, in every open session |
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
| `conversation.processing` | `"steps"` | How a chat shows the agent's steps: `"steps"` (one folded line each), `"grouped"` (one folded block per run) or `"hidden"` |
| `updates.check` | `false` | Turns on the update check |
| `updates.repo` | unset | `owner/repo` on GitHub; unset means nothing is checked |

An example:

```toml
default_agent = "claude"
theme = "dark"

[agents.claude]
command = "npx -y @agentclientprotocol/claude-agent-acp@0.103.0"

[agents.my-agent]
command = "MY_AGENT_LOG=debug /opt/my-agent/bin/my-agent --acp"

[terminal]
shell = "/bin/zsh"

[conversation]
processing = "grouped"
```

### Agent commands

The `command` is one of two forms:
- **A command line.** It is split like a POSIX shell line on every platform, so quote arguments with spaces. Backslashes are escapes there, so for Windows paths prefer the JSON form. Leading `NAME=value` words set environment variables for the agent.
- **A JSON object, written as a string:** `command = '{"command": "/path/to/agent", "args": ["--acp"], "env": {"NAME": "value"}}'`.

**The built-in agents.** There are four, and none is the default until you choose it. An entry `[agents.<id>]` with a built-in's id replaces its command; any other id adds an agent, shown with its id as its name.

| Id | Name | Command | If the CLI is missing | If signed out |
|---|---|---|---|---|
| `claude` | Claude | `npx -y @agentclientprotocol/claude-agent-acp@0.103.0` | Install Node.js (it provides npx) | `claude /login` |
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
  command = "PATH=/usr/local/bin:/usr/bin:/bin /usr/local/bin/npx -y @agentclientprotocol/claude-agent-acp@0.103.0"
  ```

  With Homebrew on Apple silicon, use `/opt/homebrew/bin` in both places.
- MCP servers that extensions contribute with a bare `npx` (such as wai-navegador's) have no per-server override, so for those, start wai from a terminal.

### What a project's `.wai/` may set

A repository you open can carry a `.wai/` folder. Because opening a cloned repository must never choose what wai runs, it is limited:

| File | Allowed | Not allowed |
|---|---|---|
| `.wai/settings.toml` | `theme`; `default_agent`, when you have already chosen a default and it names that default, an agent with an `[agents.<id>]` entry in your file, or an agent of an installed extension | `agents` and `terminal` (commands, shells, environment); `models`; `updates`; `conversation`. These are ignored with a notice |
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

The **Extensions** button opens Settings on its **Extensions** page, the list of installed extensions. There you can:
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
