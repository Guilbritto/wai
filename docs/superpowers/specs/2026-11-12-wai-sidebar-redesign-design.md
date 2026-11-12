# wai: sidebar redesign (activity bar + subagents by the prompt)

> 2026-11-12. The owner asked for a redesign of the sessions sidebar. Of three directions they picked **B** (a VS Code-style icon bar, rows that say what each session is doing) **without** B's automatic Needs you / Working / Stopped sections: "eu que vou organizar como fica a visualização". They liked C's per-session detail but wants it the way Claude Code does it: **the session's agents listed under the prompt**, navigable, each one openable to see what it is doing. For opening a subagent they picked the **drawer above the prompt** over replacing the transcript. They approved the mockups of every sidebar tab and asked to follow them strictly ("vamos seguir à risca o design").
>
> Mockups (committed, open in a browser): `docs/superpowers/specs/2026-11-12-wai-sidebar-redesign-mockups/`
> - `sidebar-v2.html`: the whole window with the main session, then the two ways to open a subagent (**the drawer is the chosen one**).
> - `sidebar-tabs.html`: the window with each bar icon selected (Sessions, Files, Search, Agents, an extension panel).
>
> The mockups use flat panels; the build keeps Aurora's glass cards and backdrop. Spacing, type sizes and colours in the mockups come from `aurora.json`, so they carry over.

## Intent

Today the sidebar is a row of text tabs (Sessions | History | Files | Search | Agents | one per extension panel) over one-line session rows: a dot, the title and, at most, `Needs you`. You cannot tell what a session is doing, how long it has been at it, or what its subagents are up to without opening it, and subagents are visible only as step cards in the transcript or as lines in the Agents tab.

The owner wants (1) navigation that scales and frees width, like VS Code's activity bar; (2) session rows that answer "what is it doing now?" at a glance, in the order and groups they choose; (3) Claude Code's agent list under the prompt, to see and open each subagent of the session in view.

## Goals

1. **Activity bar.** A vertical bar of icons replaces the text tabs.
2. **Sessions panel.** Two-line rows (title + what it is doing, and how long), in the user's order and groups; History folds into this panel as a collapsible section; a filter field on top.
3. **Agents under the prompt.** Under each session's prompt box, a list with `main` and each subagent of the session: name, task, time, tokens when known. Keyboard and mouse navigable.
4. **Subagent drawer.** Opening a subagent shows its tool calls and answer in a glass drawer above the prompt box, over the transcript, while the user can still type to `main`.
5. Files, Search, Agents and extension panels move under the bar, with the mockup's touches (see "The other panels"); their behaviour stays as it is.

## Non-goals

- Automatic sections by state (Needs you / Working / Stopped). Explicitly rejected by the owner.
- Sending messages to a subagent. Only `main` takes prompts.
- Collapsing the sidebar by clicking the selected icon, or resizing the sidebar. Not asked for.
- A manifest `icon` field for extension panels. Initials are enough for now (see "Extension panels").
- Changing what Files, Search and Agents do, or anything an extension draws. Only the visual touches listed under "The other panels" change.
- Showing a subagent's own text or checklist while it runs (wai drops them today; see `subagents.rs`). Its final answer is shown, as today.
- Translating the UI. Strings stay in English, like the rest of wai; the Portuguese in the mockups (`Sessões`, `Abertas`, `Histórico`, `Rascunho`, `Permissão`, `Filtrar sessões`…) becomes English (`Sessions`, `Open`, `History`, `Draft`, `Permission`, `Filter sessions`…).

## Anti-goals (a failure even if it works)

- **Reordering the user's sessions behind their back.** The open list keeps exactly the order and groups the user set (drag, groups, relaunch), whatever the sessions' states.
- Losing a feature on the way: double click to start a session, drag a row onto a pane, right-click menus (Rename, Fork, groups), rename by double click, Open to the side, ×, All projects, history Fork, moving Agents to the right, extension panels, the `Needs you` count.
- Breaking existing test ids (`tab-*`, `open-row`, `open`, `open-side`, `open-close`, `draft-row`, `new-group`, `history`, `all-projects`, `sidebar-new`, `sidebar-templates`, …) without updating their tests in the same change.
- Raising idle CPU above the < 0.5 % target. Times in rows and in the agent list tick at most once a second, and only while something is running.
- The drawer hiding a permission card or question. Waiting cards stay visible and answerable.

## Decided by the owner

- Direction B's bar and panel, **without** state sections; the user organises the list.
- Subagents listed under the prompt, as in Claude Code; opening one uses the **drawer above the prompt** (the main transcript stays behind it, the prompt keeps working for `main`).
- **History becomes a collapsible section of the Sessions panel**, not its own tab (this reverses the 2026-11-11 draft spec's "History stays its own sidebar tab"). Its **All projects** switch becomes an `All` toggle on the section header.
- **Extension panels get their own icon in the bar**, drawn as the initials of the panel title (`My cards` → `MC`), full title in the tooltip.
- Extensions and Settings live at the foot of the bar and open Settings as today; they leave the top bar.
- Follow the mockups strictly.

## Behavior

### The activity bar

A 44 px column at the window's left edge, under the top bar, outside the cards (like VS Code's activity bar), over the Aurora backdrop. Icons are 32 px squares with 9 px corners, 6 px apart.

Top group, in order:

| Icon | Opens | Notes |
|---|---|---|
| Sessions (chat bubble) | Sessions panel | Amber badge with the number of sessions that need you (hidden at 0). Double click starts a new session, as the Sessions tab does today |
| Files (folder) | Files panel | |
| Search (magnifier) | Search panel | Focuses the query, as today |
| Agents (sparkles) | Agents panel | Hidden while Agents is on the right, as the tab is today |
| One per extension panel | that panel | Initials of the title in a 20 px rounded square (first letter of the first two words, else the first two letters; uppercase). Tooltip: the full title |

Foot group: **Extensions** (puzzle) opens Settings on its Extensions page; **Settings** (gear) opens Settings. The top bar loses its Settings and Extensions buttons; it keeps templates, palette, terminal, Agents (when on the right) and New session.

The selected icon has the active background (`list.active.background`), its border (`list.active.border`) and a 2 px violet bar on its left edge. Each icon keeps today's tab semantics: role tab, aria label = the label, selected state, focus ring, Enter/Space activate, tooltip with the label.

Icons not in gpui-kit's default set (chat bubble, sparkles, puzzle) are added through `ExtraIcons` in `app.rs`.

### The panel

The sidebar card now holds only the panel of the selected icon. Every panel opens with a **title row**: the panel name in small caps (11 px, bold, 0.07 em tracking, muted), then right-aligned icon buttons for that panel:

| Panel | Title actions |
|---|---|
| Sessions | **+** New session, **New group**, **Collapse all** (folds every group) |
| Files | Collapse all folders |
| Search | Collapse all files |
| Agents | Move to the right (today's button) |
| Extension | **no title row**: the extension already draws its own heading (`My cards` + `4 open`), which is what the mockup shows |

### The Sessions panel

From top to bottom:

1. **Title row** (above).
2. **Filter field**, placeholder `Filter sessions`. Typing narrows the Open rows and History rows to titles containing the text (case-insensitive); groups with no match hide; Esc clears. With text in it, groups show expanded. The mockup's `⌘K` hint is shown only if ⌘K is free in wai's keymap (it is today) and then focuses the field.
3. **Open** section header (collapsible, chevron, count of open sessions and drafts).
   - Draft rows and session rows outside any group, in the user's order, then each group: header (chevron, a colour swatch, name, count), then its rows indented 10 px. A **collapsed** group whose sessions include one that needs you shows an amber `● N` at the header's right end.
   - Dropping a row on the Open header or the ungrouped area takes it out of its group, as today.
4. **History** section header (collapsible): `History · this project`, and at its right an `All` toggle that is today's **All projects** switch. Its rows are today's history rows, one line, with ↺, the title and a relative date (`yesterday`, `2d`). It keeps today's "Choose an AI in Settings to see its history." hint and lazy list. The section takes the panel's remaining height and scrolls on its own; when Open is long, Open scrolls too. Both sections start expanded; their folded state is kept on relaunch.

**Session row** (two lines, 9 px radius, 5 × 8 px padding):

- Line 1: the status dot (as today, same colours and glow), the title (truncates), and at the right a short time (`30s`, `4m`, `1h`, `2d`): how long the session has been in its current state (for `Working`, since the turn started). The time is not saved: a session restored at launch shows no time until its state first changes.
- Line 2 (11 px, muted) says what it is doing:

  | State | Line 2 |
  |---|---|
  | Working | the status line's text without the trailing `…` duplication: the running tool (`cargo test -p wai…`, `Reading Cargo.toml…`) or `Thinking…` |
  | Needs you | `Permission:` (amber) + the top waiting card's title; for a question, `Question:` + its title |
  | Idle, last turn ended normally | `✓ Done` (green) |
  | Idle otherwise | `Idle` |
  | Error | `Agent exited` (red) `· Restart` |
  | Draft | `Draft` |

  When the session has live subagents, ` · N subagents` is appended (`1 subagent`).
- The focused pane's session has the active background and border, as today.
- On hover, a small glass chip at the row's top-right holds today's **Open to the side** and **×** buttons; they are not shown otherwise.
- Click, double click (rename), drag, right-click menu: as today. While renaming, line 1 becomes the text box and line 2 stays.

**Group colour.** Each group gets a swatch colour when created, the next one from a fixed cycle of the theme's accents (violet, cyan, blue, magenta, amber, green). It is stored with the group and kept on relaunch. No UI to change it in this change.

### Agents under the prompt

Under every session's prompt box (not drafts), a list in the mono font, 12 px:

```
●  main                                   1m 12s · ↓ 82k tokens
○  general-purpose   List my open GitHub PRs        9s · ↓ 46.9k tokens
✓  Explore           Find callers of send_limit     done · 32s
```

- **`main`** is always the first row: bold name; at the right, the running turn's elapsed time (else nothing) and the session's context used (`↓ <used> tokens`, from the usage the agent reports; hidden when unknown).
- **One row per subagent** of the session, in the order announced: a mark (`○` cyan while running, amber while it needs you, `✓` green when done, `✗` red when stopped or failed), its name (the subagent's kind, e.g. `general-purpose`, `Explore`), its task (the subagent's title), and at the right its elapsed time (live while running, `done · 32s` after) and its tokens when the agent reports them.
- **Which subagents:** the ones of the current turn plus any still running from earlier (background ones). When a new turn starts, finished subagents of earlier turns leave the list.
- **Hidden** when the session has no subagents to show: then the list is not drawn at all (no lone `main` row).
- **Selection:** the row whose drawer is open is selected (active background and border); with no drawer, `main` is.
- **Mouse:** click a subagent row to open its drawer; click `main` (or the selected subagent again) to close it.
- **Keyboard:** with the prompt box empty, ↓ from the prompt moves into the list, ↑/↓ move, Enter opens the selected row's drawer (`main` closes it), Esc closes the drawer and returns to the prompt. Under the list, a hint line: `↑↓ navigate · Enter open · Esc back to main`, shown only while the list has focus.

### The subagent drawer

Opening a subagent lays a glass drawer over the bottom of the transcript, just above the prompt box, the way the waiting cards do today (`render_waiting`), about 46 % of the pane's height, with the violet active border and a shadow. The transcript behind it dims.

- **Header:** state mark, the subagent's name (bold), its task (muted), at the right its time · tokens and a **×** that closes the drawer.
- **Body**, in the mono font, scrolling, following new lines while scrolled to the bottom: each of the subagent's tool calls in order as one line (`✓`/`✗`/spinner, then its title, `$ ` before commands). A click on a line unfolds its output under it, as step lines do in the transcript. When the subagent answers, its answer follows, rendered as Markdown.
- **The transcript shows each subagent as one card** (mockup v2): state mark, its kind, its task, a `Running` / `Done` / `Failed` pill. **Its tool calls leave the main transcript** and show only in its drawer, as in Claude Code. A click on the card opens its drawer. (Update `subagent_shows_in_the_chat` and siblings accordingly.)
- **The prompt box stays live** for `main`. Waiting permission cards and questions stay above the drawer and answerable.
- The drawer follows the subagent's state; it does not close by itself when the subagent finishes.
- Closing: **×**, Esc, clicking `main` or the subagent's row again. Switching sessions in the pane closes it.
- **Esc with the drawer open only closes the drawer.** It does not stop the turn; the next Esc does, as today. (Esc today stops the turn and, with it, every subagent.)

### The other panels

Behaviour unchanged; these visual touches from `sidebar-tabs.html` are in scope:

- **Files:** the project folder's name as a collapsible section header over the tree; a small file-type mark before each file (folder icon for folders; for files, the extension's short form in a colour per language, e.g. `rs` orange, `M↓` blue for Markdown, `≡` muted for the rest); the file shown in the editor's active tab is marked with the active background. Title action **Collapse all** folds every folder.
- **Search:** results summary line (`9 results in 3 files`); each file is a collapsible header (chevron, file mark, name, its folder muted, a count pill); match lines in mono with the line number muted and the matched text highlighted (amber at 25 % behind it); the last clicked result is marked. Title action **Collapse all** folds every file. The query field holds `Aa` and `.*` inside it at the right, the active one with the active background.
- **Agents:** each card gains a thin context bar (cyan→violet gradient, `used / size`) above its existing `used / size (pct%)` line. Everything else on the card stays.
- **Extension panels:** no change (see the title-row table).

## What the code has today, and what is missing

- Subagents: `ChatModel::subagents` holds `id`, `title`, `state`; tool calls carry `subagent: Some(id)`; `SubagentReply` delivers the answer; `subagent_tool(id)` gives the running tool. **Missing:** the subagent's kind/name as distinct from its title, when it started and finished, its tokens, and which turn it belongs to.
- Tokens: `SessionEvent::Usage` updates the session's `usage` (context `used`/`size`). A subagent's usage update is dropped in `subagents.rs` (`_ => None`). **To do:** route it to the subagent. Whether `claude-agent-acp` sends usage for subagent sessions must be checked against the real agent; if it does not, the tokens column stays hidden for subagents (time only).
- Turn time: `ChatSession::turn` holds the turn's start `Instant`. **Missing:** a per-session "state changed at" time for row times when idle/needs you/error.
- History: `render_history` with `uniform_list`, `all_projects`. Moves into the Sessions panel.
- Groups: `SessionGroup` (name, sessions, collapsed) is persisted. **To add:** the swatch colour.
- Top bar: `topbar.rs` (`sidebar-extensions`, `topbar-settings` move to the bar).

## Left to the builder

- Exact Lucide icons (e.g. `message-square` vs `message-circle`), spinner glyph, and the drawer's open/close animation (none is fine).
- How Open and History share height when both are long (a fixed split, or Open sized to content up to a cap); it must never push History off-screen entirely.
- Where the subagent's kind comes from (ACP subagent update metadata, else the tool call's `kind`/title); if none is available, show the title as the name and leave the task empty.
- Persistence keys for the folded Open/History sections.
- The relative-date format of History rows.
- Tests: follow the existing visual-test style in `sidebar.rs` and `chat/view.rs` (fake agent with `--subagent`).

## Calls made without the owner (check while reading)

- **A subagent's tool calls move out of the main transcript** into its drawer; the transcript keeps one card per subagent, which opens the drawer.
- **Esc closes the drawer first**; only the next Esc stops the turn.
- Restored sessions show no row time until their state changes.
- Extension panels get no wai title row (the extension draws its own).

- The bar sits outside the cards at the window's edge; the sidebar card holds only the panel.
- Group swatch colours come from a fixed cycle, stored per group, with no picker yet.
- The filter also narrows History.
- The agent list hides entirely when there are no subagents, and drops finished subagents of earlier turns.
- ↓ from an empty prompt enters the agent list.
- Subagent tokens show only if the agent reports them.
