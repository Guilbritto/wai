# wai: new-session drafts ("What are we building?")

> 2026-11-11. The owner looked at VS Code's agent start screen (folder, Branch/New Worktree, branch and agent pickers above the prompt) and asked how it would look in wai. His idea: **New session opens the screen but does not start a session; the session starts when the first message is sent.** He approved the clickable mockup ("ficou ótimo"), with one fix: History is its own sidebar tab (as wai already has it), not a section with a toggle under Open.
>
> Mockup (local, git-ignored): `.superpowers/brainstorm/44632-1794399351/content/new-session-draft-v2.html`. Open it in a browser; the chips open their menus and Enter "sends".

## Intent

Today ⌘N, the top bar's New session, a double click on the Sessions tab and the palette's "New session with X" all call `Workspace::start_session` at once: the pane shows `Preparing <agent>…`, the agent process spawns and `session/new` runs in `self.project`, before the user has typed anything. The only choice offered is the agent, and only through the palette.

The owner wants to choose **where** (folder, local checkout or a new worktree, branch) and **with whom** (agent, mode, model) first, type the request, and only then have anything run. A draft costs nothing: no process, no `session/new`, no worktree, no history entry.

## Goals

1. Every "new session" entry point opens a **draft** in the focused pane instead of starting a session.
2. The draft screen: a "What are we building?" title, a chip strip on top of the prompt box (folder, Local/New worktree, branch, agent), and the prompt box with attach (+), mode and model chips and a round send button. The mockup is the reference for layout and copy.
3. **Sending the first message starts the session** with what the draft chose: checks out the branch or creates the worktree, starts the agent in that folder, applies mode and model, sends the message (with its attachments).
4. A draft shows in the Open list as "New session · Draft" with a ×, so the user can leave it and come back.
5. An agent whose command is not installed is flagged in the draft before sending, with how to install it.

## Non-goals

- "This Computer" / remote machines. wai only runs locally; the chip is dropped.
- VS Code's "Chat" (a session with no folder). ACP needs a working directory.
- Removing worktrees. A worktree wai creates stays after its session closes; the user removes it with git.
- Keeping drafts between runs. Like the editor and settings panes, drafts are not saved in `state.json`.
- Changing how extensions start sessions (`sessions/open` with `send`): that API keeps starting a session at once.
- Translating the UI: strings stay in English, like the rest of wai.

## Anti-goals (a failure even if it works)

- **Losing what the user typed.** If anything fails on send (checkout, worktree, agent start, sign-in), the draft comes back with its text, attachments and choices intact and the error shown in it.
- **Touching uncommitted work.** wai never stashes, resets or forces a checkout. If git refuses the checkout, that refusal is the error.
- A draft spawning an agent process or calling `session/new` before send.
- Breaking existing test ids (`new-session`, `send`, `stop`, `tab-*`, `sidebar-*`) or the idle CPU target (< 0.5%).
- The workspace (Files, Search, terminal, window title) switching folders while the user is only picking in a draft.

## Decided by the owner

- The session starts on the first message, not on New session.
- Layout and pickers as in the mockup: folder, Local/New worktree, branch, agent above the prompt; attach, mode, model and send inside it.
- **New session while a draft is already open opens a second draft** (it does not jump back to the first). Each click is a new draft; several drafts can sit in the Open list.
- History stays its own sidebar tab, as it is today (Sessions | History | Files | Search | Agents). The Sessions tab only has Open.

## Behavior

### Entry points (all open a new draft in the focused pane, via `session_pane()`)

| Entry point | Draft starts with |
|---|---|
| ⌘N, top bar New session, double click on the Sessions tab, palette "New session" | the default agent (Settings → AI); no default chosen → Settings opens as today (`CHOOSE_AI`) |
| palette "New session with X" | agent X |
| a template, a prompt-only extension command | the template's agent (or the default) and its prompt pre-filled, not sent |

A draft replaces what the focused pane showed, the way a new session does today. The replaced session stays in the Open list.

### The draft screen

Pane header: "New session" and a dashed pill "Draft · starts when you send". Body, centered: the title "What are we building?" (display font), a muted line "Pick where and with whom. Nothing starts until you send.", the chip strip, the prompt box, and a hint row (`⏎` start session, `⇧⏎` new line, `⌘N` another draft).

**Chips (strip above the prompt box):**

1. **Folder**: the draft's working folder; starts as the workspace's project. Menu: a search field, then known folders (see Decisions), each with its parent path; the current one checked; then "Open folder…" (the existing folder dialog) and "Clone repository…" (asks for a URL and a parent folder, clones with git, selects the result). Changing the folder resets the branch to that folder's current branch.
2. **Local / New worktree**: "Local: works in the folder already on this computer" or "New worktree: creates a separate copy for this session, on send". Hidden when the folder is not a git repository.
3. **Branch**: hidden when the folder is not a git repository.
   - Local: shows the current branch, with an amber count of uncommitted files when there are any. Menu: filter field; the current branch ("Current · N uncommitted files"); "Default branch"; then the other local branches. Footer: "Picking another branch checks it out when the session starts."
   - New worktree: shows "from <base>" (default: the default branch). Same menu without the current-branch row. Footer: "The new worktree starts from this branch."
4. **Agent**: the agents the registry knows (settings first, then extensions), each with its icon; ones whose command is not installed say "Not installed". Footer: "Default agent comes from Settings → Agents." Changing the agent resets mode and model to that agent's remembered values.

**Inside the prompt box:**

- A notice at the top, only when the chosen agent is not installed: "<Agent> is not installed", "wai runs <Agent> through its CLI and could not find `<program>` on your PATH.", and a "How to install" button that shows `BuiltinAgent::install` (opens it when it is a URL). Send is disabled while it shows.
- The text field: the same input as the chat (paste images/files, attach), placeholder "Pitch your idea".
- **+**: attach, as in the chat.
- **Mode** (shield icon): the agent's modes, the remembered one selected.
- **Model** (right side): the agent's models, the remembered one selected. Footer: "Last used for <Agent>. The list refreshes when the session starts."
- **Send**: round, primary color; disabled while the text is empty or the agent is not installed. Enter sends, Shift+Enter is a new line.

Menus close on Esc, on a pick and on a click outside; they never grow past the window (they scroll).

### Sending: the session is born

On send, in order, with the pane showing the chat at once (the user's bubble, a `✦ <Agent>` card with one closed step line per action, and the loading line above the input: spinner, current action, elapsed time, "Esc to stop"):

1. **New worktree**: `git fetch origin <base>`, then `git worktree add -b wai/<slug> <repo>/.worktrees/<slug> <origin/base or base>`. `<slug>` is the first words of the message, lower-case, hyphenated; a taken branch or path gets `-2`, `-3`… Step line: "Created worktree `<path>` on branch `wai/<slug>` from `<base>`".
   **Local on another branch**: `git checkout <branch>` in the folder. Step line: "Checked out `<branch>`".
2. Start the agent and `session/new` in that folder (the worktree path for a worktree), with the MCP servers as today. Step line: "Started <Agent> in `<folder>` · <mode> · <model>".
3. If the chosen mode or model differs from what the session started with, set it (the calls `ChatSession::set_mode` and the model config option already make).
4. Send the message and its attachments.
5. The workspace follows the session's folder, as it does today when a session in another folder is focused (`follow_session`).

The Open row turns from Draft into the session (working dot); the top bar's status pill counts it. Until the agent gives a title, the row and the pane header show the first line of the first message.

From then on the pane header shows, after the title, three pills with what the draft chose: the folder, the branch (`wai/<slug>` with the worktree icon for a worktree), and `<Agent> · <model>`. They stay for the life of the session, restored sessions included (folder and branch read from the session's folder; the model from the session). The mode picker stays where it is today, on the right of the header.

### Failure

The draft ends when `session/new` succeeds. Before that (git step, agent spawn, `session/new` itself), any failure puts the pane back on the draft, unchanged (text, attachments, chips), with the error in the notice area of the prompt box; an agent that needs sign-in shows the existing sign-in guidance there. After that, it is an ordinary chat: a mode/model call that fails or a turn that ends in an error shows in the chat as it does today, and the draft is gone.

Retrying must not create a second worktree: a worktree already created for this draft is reused, and the draft's chips then point at it (Local, its folder, its branch).

Esc (or the stop button) before `session/new` succeeds cancels back to the draft, the same way a failure does; a worktree already created stays and is reused. After that, Esc stops the turn as it does today.

### Open list

A draft row: dashed dot, "New session", "Draft" tag, ×. The × discards the draft (no confirmation when the text is empty; with text, the existing close-confirmation style asks "Discard this draft?"). Clicking the row shows the draft in the focused pane. Drafts can be dragged between panes like session rows. Drafts are not in History and are not saved between runs.

## Decisions (builder's calls; the owner has not weighed in)

- **Known folders** = the current project, the folders of open sessions and of History sessions (all projects), most recent first, deduplicated after `canonical`. No new "recent folders" list in `state.json`.
- **Worktree location**: `<repo>/.worktrees/<slug>`, the convention the owner already uses (`~/Project/waycarbon-ecosystem-local` has about ten worktrees there). When wai creates the first one in a repository and `.worktrees/` is not ignored, it adds `.worktrees/` to that repository's `.git/info/exclude` (local, never committed).
- **Worktree branch prefix**: `wai/`.
- **Worktree base**: the branch the user picks; the first time per repository, the default branch. The last base used is remembered per repository (new `AppState` map, repository root → branch), so a repo whose work starts from `development` is not re-picked each time. Before `worktree add`, wai runs `git fetch origin <base>` and starts from `origin/<base>` when it exists; if the fetch fails (offline, needs credentials), a step line says so and the local `<base>` is used. The fetch failing is not a send failure.
- **Local on another branch, guarded**: switching branch in Local is refused (error in the notice, suggesting New worktree) when the folder has uncommitted files or another open session works in the same folder. git's own refusals are errors too. wai never runs a checkout that would carry someone's in-progress edits to another branch.
- **What a new draft starts with**: the model is the last one picked in a draft for that agent (new `AppState` map, agent → model), else `settings.models[agent]`, else the last one the agent reported; a pick never changes Settings. The **mode is not remembered**: every draft starts at the mode the agent last reported as its current one, so a permission-loosening mode ("Bypass permissions", "Full access") is never preselected. The mode list comes from a new `modes_seen` in `AppState`, filled like `models_seen` from each `session/new` answer; before an agent ever ran, its mode chip is hidden.
- **"Not installed"** = the first word of the agent's command is not found on PATH (after `shell_path::import`). Checked when the draft opens and when the agent changes, off the UI thread. An agent with its program present but signed out is not flagged; that still surfaces after send.
- **Git data** (is it a repo, current branch, default branch, branches, uncommitted count) is read off the UI thread when the draft opens and when the folder changes. Default branch = `origin/HEAD`, else `main`, else `master`. Branches are sorted by most recent commit.
- **The empty pane** (`Pane::Empty`) stays as it is, with its start hint. wai does not open a draft at launch on its own.
- **Title until the agent names the session**: the first line of the first message, not "New session".

## Left to the builder

- The model for a draft: likely `Pane::Draft(Entity<DraftView>)` plus a list of drafts in the workspace for the Open list. `Pane::Preparing` may be replaced by the chat-with-steps view or kept underneath.
- How the chip strip and menus are built in GPUI (gpui-component popovers/lists are fine), keeping the Aurora tokens from `2026-11-09-wai-visual-aurora-design.md`.
- The exact `git` invocations and their error text. Run git with the user's PATH and no prompts (`GIT_TERMINAL_PROMPT=0`).
- Test ids for the new elements (suggested: `draft-folder`, `draft-workspace`, `draft-branch`, `draft-agent`, `draft-mode`, `draft-model`, `draft-row-<n>`, and keep `send`).

## What tests must show

- No entry point spawns an agent or calls `new_session` before send (fake agent: zero sessions until send).
- ⌘N twice gives two drafts in the Open list.
- Send in Local with the current branch starts one session in the folder and sends the text; with another branch, it checks out first; with uncommitted files or another session in that folder, it refuses and keeps the draft.
- Send in New worktree creates `<repo>/.worktrees/<slug>` on `wai/<slug>` from the base (after a fetch; an offline fetch still works from the local base) and starts the session there; a name clash gets `-2`.
- A failed start (unknown agent, agent exits, git refuses) returns to the draft with the text and the choices intact; a retry after a created worktree does not create another.
- An agent whose program is missing shows the notice and disables send.
- Folder not a git repository: no Local/worktree or branch chip; send works.
- Drafts are not written to `state.json`, and the existing session-restore tests still pass.
