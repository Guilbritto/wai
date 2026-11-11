# wai: Settings in a modal, complete (master plan 1.9)

> 2026-11-11. Master plan row 1.9 (`docs/superpowers/plans/2026-11-11-wai-master-plan.md`). The owner
> approved the clickable mockup `.superpowers/brainstorm/91501-1794403714/content/settings-modal-v2.html`
> (local, git-ignored; its data is this machine's real `~/.claude`). Reference: VS Code's
> "Customizations" modal.

## Intent

Settings today is a pane (`Pane::Settings`) that competes with sessions for one of four slots, and it
only knows wai's own settings. The owner wants one big modal over the window that shows everything
that shapes an agent session: wai's settings, wai's extensions, and what Claude Code loads from
`~/.claude` (plugins, MCP servers, skills, instructions, agents, hooks). What Claude already provides
is shown and works (lists, state, plugin and MCP toggles, Open in the editor). What wai cannot do yet
is visible and marked **Coming soon**, so the screen is complete from day one and honest about it.

## Goals

1. ⌘, the top bar's ⚙, the palette's **Settings**, and every place that opens Settings today open a
   modal over the window instead of a pane. Esc, a click outside it and its × close it; a maximize
   button grows it to the window. Title: `Settings` plus a context `(<default agent> · <project>)`.
2. Left nav in three groups with counts: **wai** (AI, Appearance, Conversation, Terminal, Updates,
   Extensions), **✳ Claude** (Plugins, MCP Servers, Skills, Instructions, Agents, Hooks), **Other
   agents** (Codex, Copilot, Antigravity, each "Soon").
3. Each page: title, description, a search over the page, and (Claude pages) collapsible groups with
   a count and a `+`, rows with name, badge, path, summary, status, toggle and `⋯`.
4. The wai pages keep today's behaviour exactly (same controls, same writes to `settings.toml`, same
   approval flow, same read-only mode on a broken file).
5. Extensions moves from the sidebar into the modal, same behaviour (install, consent, enable,
   uninstall, Claude plugin suggestions).
6. Claude pages read `~/.claude` off the UI thread; the modal opens at once and fills in. Plugin and
   MCP toggles write Claude's own files safely. **Open** opens the file in wai's editor.

## Non-goals

- Installing, updating, uninstalling or browsing plugins; adding MCP servers; creating skills, agents,
  hooks or instructions; claude.ai connectors; Codex/Copilot/Antigravity settings. All shown as
  **Coming soon**.
- Claude's own slash commands (`~/.claude/commands`, plugin `commands/`) as a page. They are only
  counted in a plugin's summary.
- `.claude/rules/`, `AGENTS.md`, nested `CLAUDE.md` in subfolders, managed (organization) settings and
  `CLAUDE_CONFIG_DIR`. wai reads `~/.claude` and `~/.claude.json` under the home dir.
- Marketplace-entry component merging (`strict`), `.mcpb` bundles, `lspServers` details: a plugin
  without `plugin.json` reads only its marketplace entry's description and LSP count.
- Live MCP status without asking: status comes from an explicit **Check status** (see Decisions).
- Restyling the wai pages' controls to the mockup's segmented buttons, or a "System default" theme:
  the owner asked for today's sections with the same behaviour.
- Keeping the modal open across relaunches, or its last page.

## Anti-goals (a failure even if it works)

- **Corrupting or clobbering Claude's files.** `~/.claude/settings.json` and `~/.claude.json` are
  written only by the two toggles, only the one key they own changes, every other key keeps its value
  and order, the file mode and symlinks survive, the write is atomic, and nothing is written when the
  file changed since it was shown or does not parse.
- **Showing or keeping a secret.** No `env`, `headers`, `args`, `command`, `url` or hook `command`
  value is held in wai's data structs, shown, or logged. MCP rows show the server name, its source and
  its transport only.
- Blocking the UI thread on `~/.claude` (reads and writes run on the background executor), polling, or
  anything that runs while the modal is closed. Idle CPU stays < 0.5%.
- Running `claude` (or any Claude process) without an explicit click on **Check status**.
- Tests reading the developer's real `~/.claude`, `~/.claude.json` or running the real `claude` CLI.
- Breaking existing test ids (`tab-*`, `sidebar-*` including `sidebar-extensions`, `new-session`,
  `send`, `stop`, approval/permission/question ids, `settings-*` and `ext-*` ids the tests use).
- A Coming soon control that looks like it works: each one is disabled and carries the badge or a
  "coming soon" tooltip.

## Decided by the owner

- Modal over the window replaces `Pane::Settings`; ⚙ and ⌘, open it; Esc / click outside / × close
  it; maximize button. Title `Settings (Claude · <project>)`.
- Three nav groups exactly as listed in Goal 2; Extensions moves in from the sidebar.
- Do it complete: what Claude already provides is shown and works; what wai does not do yet carries
  "Coming soon".
- Plugins come from `~/.claude/plugins/installed_plugins.json` + `enabledPlugins` in
  `~/.claude/settings.json`; the toggle writes `enabledPlugins` and applies to new Claude sessions.
  Preserve every other key; atomic write; never write if the file changed since read.
- MCP servers from the plugins' `.mcp.json` and `~/.claude.json`, with status and an on/off toggle
  **if Claude Code has a real place for them**; where it has none, that piece is Coming soon.
- Skills (plugin skills dirs, `~/.claude/skills`, project `.claude/skills`), Instructions
  (`~/.claude/CLAUDE.md`, the project's `CLAUDE.md` and the files they `@include`), Agents (plugin
  agents, `~/.claude/agents`, project `.claude/agents`), Hooks (`~/.claude/settings.json` hooks and
  the plugins' `hooks/hooks.json`) are lists whose **Open** opens the file in wai's editor.
- Coming soon: Browse/install plugin, plugin update/uninstall (`⋯`), Add server, every `+` that would
  create a skill/agent/hook/instruction, claude.ai connectors, the Other agents pages.
- Reading `~/.claude` off the UI thread, never blocking the modal; no polling (refresh when the modal
  opens and after a toggle). Never read or display secrets.

## What Claude Code actually stores (checked 2026-11-11)

Sources: this machine's files (Claude Code 2.1.322, `claude-agent-acp` 0.103.0), the strings of the
`claude` binary, and code.claude.com docs (`settings`, `mcp`, `memory`, `plugins-reference`).

**Plugins.**
- `~/.claude/plugins/installed_plugins.json`: `{"version": 2, "plugins": {"<name>@<marketplace>": [install, …]}}`.
  Each install has `scope` (`user`, `project`, `local`, `managed`), `installPath`, `version`, and
  `projectPath` for project/local scope. One id can have several installs (one per project).
- Enabled state: `enabledPlugins` (`{"<id>": true|false}`) in user `~/.claude/settings.json`, project
  `.claude/settings.json`, local `.claude/settings.local.json`; precedence local > project > user.
  A plugin with no entry is on unless its manifest says `defaultEnabled: false` (default `true`).
  `claude-agent-acp` starts Claude with `settingSources: ["user","project","local"]`, so wai's Claude
  sessions honour it. Settings files are strict JSON; Claude watches them.
- Plugin layout under `installPath`: optional `.claude-plugin/plugin.json`; `skills/<name>/SKILL.md`;
  `agents/**/*.md`; `commands/*.md`; `hooks/hooks.json` (`{"hooks": {event: [...]}}`); `.mcp.json`;
  `.lsp.json`. Manifest keys: `skills` (path or paths, adds to `skills/`), `agents` and `commands`
  (replace the default dir), `hooks` and `mcpServers` (path, inline object or array; merge with the
  default file). Real `.mcp.json` files come in two shapes: `{"mcpServers": {...}}` and a bare
  `{name: config}` map (the official `github` plugin); both load.
- A plugin with no `plugin.json` (the LSP ones) takes its description and `lspServers` from its
  marketplace entry: `~/.claude/plugins/known_marketplaces.json` → `installLocation` →
  `.claude-plugin/marketplace.json` → `plugins[]` by `name`.

**MCP servers.**
- Scopes: user `~/.claude.json` `mcpServers`; local `~/.claude.json` `projects[<key>].mcpServers`;
  project `<project>/.mcp.json`; plugin servers registered as `plugin:<plugin>:<server>`; claude.ai
  connectors live in the account.
- **On/off has a real place:** `/mcp`'s toggle writes `projects[<key>].disabledMcpServers` (an
  opt-out list of server names, per project) in `~/.claude.json`. It covers user, local, project,
  plugin servers and connectors (`enabledMcpServers` is only for built-ins that default off). It is
  unrelated to `enabledMcpjsonServers`/`disabledMcpjsonServers` (approval of `.mcp.json` servers).
- `<key>` is the git root of the working directory, else its real path (Windows: forward slashes).
- **Status has a real source:** `claude mcp list` (no `--json`) health-checks each server and prints
  `<name>: <command or url>[ (HTTP)] - <glyph> <status>`, e.g. `✔ Connected`,
  `! Needs authentication`, `✘ Failed to connect — <reason>`, `⏸ Pending approval …`,
  `⊘ Disabled for this project …`. It took 8 s here and starts the stdio servers to check them.
  ACP gives no status for settings-defined servers (`claude-agent-acp` reports only client servers).
- `~/.claude.json` is Claude's own state file (167 KB here, mode 0600): account info
  (`oauthAccount`), per-project state, and for API-key users possibly `primaryApiKey`. Claude writes
  it often, under a `proper-lockfile` style lock (the binary bundles it; `<file>.lock` directory).

**Skills, agents, instructions, hooks.**
- Skills: `~/.claude/skills/<name>/SKILL.md`, `<project>/.claude/skills/<name>/SKILL.md`, enabled
  plugins' skills. `~/.claude/skills/synced/…` (claude.ai sync) has no `SKILL.md` at depth 1 and is
  not listed. Frontmatter `name`, `description`.
- Agents: `~/.claude/agents/*.md`, `<project>/.claude/agents/*.md`, enabled plugins' agents
  (subfolders allowed). Frontmatter `name`, `description`.
- Instructions: `~/.claude/CLAUDE.md`; `CLAUDE.md`, `.claude/CLAUDE.md`, `CLAUDE.local.md` in the
  project folder and every folder above it. `@path` imports: relative to the importing file, `~/`
  and absolute allowed, up to four hops deep, ignored inside code spans and fenced blocks, `\ ` for
  spaces.
- Hooks: `hooks` in each settings file (`{event: [{matcher?, hooks: [{type, command, …}]}]}`) and in
  plugins (`hooks/hooks.json` plus the manifest's `hooks`).

## Behaviour

### The modal

- Opens with `window.open_dialog` (gpui-component), `entrance(DialogEntrance::None)`, no built-in
  close button, padding 0, centered. Size: 88% × 86% of the viewport; maximized: viewport − 24 px
  each way (the dialog's own margin clamp applies). The maximize button toggles; the size is not kept.
- The view is created on open and owned by the dialog's builder; the workspace keeps a
  `WeakEntity`. Closing (Esc, outside click, ×) drops it. Opening while open shows the asked page and
  refreshes its data instead of opening a second one.
- Header: `Settings` (display font) and the context in muted text: `(<agent label> · <folder>)` with
  the default agent, `(<folder>)` without one. Then maximize and ×.
- Body: nav (230 px, scrolls) and the content card (scrolls). Nav rows show their count on the right
  (AI: agents; Extensions: installed; Claude pages: rows, once loaded); Other agents rows show "Soon".
- Content header: title (26 px display font), description, page actions on the right. Then the
  search field (`Search <title in lowercase>…`), cleared on every page change, then the page.
- Launch: a broken `settings.toml` or no default agent opens the modal on AI as today; the approval
  dialog for unapproved commands opens after it, so it is on top.
- **Open**, **Open settings.toml**, the error banner's **Open** and an extension's **Install in
  terminal** close the modal first, then act.
- Nested dialogs (approval, extension consent) open above the modal and close back to it.

### wai pages (today's behaviour)

| Page | Description | Rows |
|---|---|---|
| AI | Which AI starts new sessions, and how wai runs each one. | Today's agent rows (default radio, status, Test, Default model, Advanced command); the Choose-an-AI notice and "This project uses X." stay; an **Advanced** group with `settings.toml` · "Everything above, as a file" · **Open** (`settings-open-file`). |
| Appearance | How wai looks. | Theme select. |
| Conversation | What a conversation shows while the agent works. Permission cards, questions and file edits always show. | Processing select. |
| Terminal | The terminal under the panes. | Shell input. |
| Updates | wai checks GitHub releases once a day, only when you turn it on. | Check switch, Repository input. |
| Extensions | Extensions add panels, commands, agents and MCP servers to wai. They run with your permissions. | Install from folder…, git URL + Install; group **Installed** with each extension's row: name, version badge, permissions, problem, Claude plugin suggestions with **Install in terminal**, Enabled switch, Uninstall. |

The search filters the current page's rows by label (agents by name). The `settings.toml` error
banner shows on every wai page and makes them read-only as today.

### Claude pages

All rows come from one `ClaudeData` snapshot. While it loads the pages say `Reading ~/.claude…`. A file
that does not parse is named in a banner on the pages it feeds (`~/.claude/settings.json does not
parse: <error>`), and the toggles that would write it are locked. Groups start open; on a page with
more than 20 rows only the first group starts open. A search opens every group with a match and hides
the rest. Every group `+` on a Claude page is a disabled Coming soon button.

- **Plugins.** Description: "What Claude loads from `~/.claude/plugins`: commands, skills, agents, hooks
  and MCP servers. A change applies to new Claude sessions." Link **Learn more about Claude Code
  plugins** (`https://code.claude.com/docs/en/plugins`, opened with the system browser). Actions:
  **Browse** and `⋯` (Coming soon). One group per marketplace, in file order. A plugin is listed when it
  has a `user`/`managed` install or a `project`/`local` install for this project (that one wins). Row:
  name; badge `v<version>` (semver) or the raw version (commit); path (`~/…` install path); summary
  (component counts, `15 skills · 1 hook`, else the description, else `No components`); toggle; `⋯`
  disabled ("Update, uninstall: coming soon"). The toggle is locked, with the reason as its status,
  when the project or local settings set that plugin ("Set in this project's .claude/settings.json"),
  for a managed install ("Managed by your organization"), or while the user settings file is broken.
  Toggling writes `enabledPlugins["<id>"] = true|false` in `~/.claude/settings.json` and says
  "Applies to new Claude sessions".
- **MCP Servers.** Description: "Servers that give Claude more tools. Turning one off applies to this
  project, in new Claude sessions, and keeps its configuration." Actions: **Check status**, **Add
  server…** (Coming soon). Groups: **From plugins** (enabled plugins only), **User**, **This
  project** (local scope and `.mcp.json`), **claude.ai connectors** (one Coming soon row: "Connectors
  from your claude.ai account" · "They live in your Claude account, not in a local file"). Row: server
  name; badge `plugin: <name>` / `user` / `local` / `.mcp.json`; summary transport (`stdio`, `HTTP`,
  `SSE`, `WebSocket`); status; toggle. Status is "Not checked" until **Check status** runs
  `claude mcp list` in the project folder (button reads "Checking…" meanwhile; 60 s timeout), then
  Connected / Needs sign-in / Failed to connect · reason / Pending approval / Off for this project;
  without a `claude` CLI on PATH it says "Needs the claude CLI". The toggle writes
  `~/.claude.json` `projects["<key>"].disabledMcpServers` (adds the name to turn off, removes it to
  turn on) and says "Applies to new Claude sessions in this project". It is locked ("Start a Claude
  session in this project first") when `~/.claude.json` has no entry for this project, or while that
  file is broken.
- **Skills.** Description from the mockup. Groups **User**, **Project**, then one per enabled plugin.
  Row: name, summary (description, one line), **Open** (`SKILL.md`).
- **Instructions.** "Files Claude reads at the start of every session (CLAUDE.md and what it
  includes)." Groups **User** (`~/.claude/CLAUDE.md` and its imports) and **Project** (each
  `CLAUDE.md` / `.claude/CLAUDE.md` / `CLAUDE.local.md` from the project folder up, and their imports).
  Row: file name, path, summary (its first heading, or "Included by <file>", or "This project" /
  "Parent folder" / "Yours, not shared"), **Open**.
- **Agents.** "Subagents Claude can hand work to." Groups **User**, **Project**, one per enabled plugin.
  Row: name, path (`~/…` or `plugin: <name>`), summary, **Open**.
- **Hooks.** "Commands Claude Code runs on its own at set moments. They run with your permissions."
  Groups **User** (one row per event in `~/.claude/settings.json`, summary `1 hook` / `N hooks`),
  **Project** (same, from the project's settings files), **From plugins** (one row per enabled
  plugin with hooks, summary its events joined with ` · `). **Open** opens the file that declares them.
- Files under a plugin's install path open read-only; user and project files open editable.

### Other agents

Codex, Copilot, Antigravity: the title, "Plugins, MCP servers, skills, instructions, agents and hooks
of <X>, like the Claude ones.", a Coming soon badge, "Not in wai yet" and "Today wai shows what Claude
uses. <X> comes next; its own settings stay in its CLI meanwhile."

### Data and threading

- `wai-settings` gains `claude.rs`: pure, std + `serde_json` (with `preserve_order`, already in the
  lockfile through `indexmap`). `read_claude(home, project) -> ClaudeData`, `parse_mcp_list`,
  `set_plugin_enabled`, `set_mcp_enabled`. Unit-tested with temp dirs.
- The workspace holds `claude_home` (the home dir; tests point it at a temp dir) and
  `claude_program` (`None` = find `claude` on PATH; tests point it at a script or a missing path).
- Opening the modal (and every toggle's completion) spawns `read_claude` on the background executor;
  a newer read replaces the one in flight. The view shows the wai pages at once.
- The check runs `claude mcp list` with `smol::process` (`kill_on_drop`), raced against a 60 s timer,
  only on the button.

### Safe writes (both toggles)

1. The read keeps a `Stamp` (hash of the bytes, or "missing") of each file it parsed.
2. The write resolves symlinks (the target is written, the link stays), takes a lock compatible with
   `proper-lockfile` (`<target>.lock` directory; one younger than 10 s means busy: retry for 1 s, then
   give up; an older one is stale and replaced), reads the bytes, compares their stamp with the
   shown one (different: no write, "changed"), parses, changes only its key, serializes keeping key
   order, the file's indent and its trailing newline, writes `<target>.wai.tmp` with the original's
   permissions, checks the stamp once more, renames over the target, drops the lock.
3. Any failure leaves the file as it was. The UI refreshes after every attempt.

## Decisions that are my call (cost if wrong)

1. **gpui-component's dialog, not a custom overlay.** Free Esc, outside click, focus trap and
   nesting with the approval/consent dialogs. Cost: if its surface fights the two-column layout or the
   maximize resize, fall back to an absolute overlay in `Workspace::render` (half a day).
2. **MCP status on a button, not on open.** `claude mcp list` takes seconds and starts every stdio
   server; doing it on each open would spend CPU and spawn processes just for looking. Cost: the owner
   wanted live status on open; then run it the first time the MCP page shows in each opening (an hour).
3. **MCP toggle per project via `disabledMcpServers`.** That is what `/mcp` does; there is no global
   switch. Cost: none functional; copy says "this project".
4. **Only writing an existing project entry in `~/.claude.json`.** Creating a partial entry could trip
   Claude's own reader. Cost: a project never opened in Claude shows the MCP toggles locked.
5. **A plugin toggle is locked when a project/local file sets it.** A user-level write would not change
   what Claude does there. Cost: the owner may want wai to write the project file; that is a scope
   decision (shared file) better taken later.
6. **Disabled plugins contribute nothing** to MCP, Skills, Agents, Hooks (that is what Claude loads).
   Cost: the owner may want to browse a disabled plugin's skills; then list them greyed (an hour).
7. **Search per page**, not across pages (the mockup). Cost: today's cross-section search goes away for
   the wai pages; restoring it is a filter over all wai rows (an hour).
8. **Extensions keeps its controls** (same ids), laid out as rows; the top bar's Extensions button
   stays (`sidebar-extensions`) and opens the modal on Extensions. Cost: none; remove the button later
   if the owner wants the mockup's bare top bar.
9. **Open closes the modal**, so the editor is visible. Cost: the owner may want to keep it open (one
   line).
10. **Context in the title is the default agent's label**, not always "Claude". Cost: one string.
11. **The lock file is best effort**: Claude bundles `proper-lockfile`, but I did not confirm its config
    writes use `~/.claude.json.lock`. The stamp check is the guarantee; the lock narrows the race.
    Cost: none if unused.

## Owner question (default chosen, not blocking)

The MCP toggle rewrites `~/.claude.json`, Claude's own state file, which holds account info and, for
API-key users, possibly an API key. wai never shows, logs or changes those values and keeps them byte
for byte, but it does read and rewrite the file. The global rule says "the core never touches user
credentials". **Default: go ahead** (the owner asked for the toggle "if Claude has a real place", and
this is that place). If the owner says no, the MCP toggles become Coming soon and Task 7's writer is
dropped; everything else stands.
