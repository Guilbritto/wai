# wai: the bars (top, left, bottom) and extension items in them

> 2026-11-13. Project 2 ("Places") of the modularity program (`2026-11-12-wai-extension-panels-v2-design.md`). Choices marked **(owner)** are the owner's; **(call)** are mine, listed again under "My calls". The owner accepted my proposals as defaults on 13/11 and reviews them here. API version: **1.3**.

## Intent

Owner, verbatim:
- *"A minha ideia é poder criar extensões que possam adicionar itens no menu, no cabeçalho, que eu consiga mudar o tema, que eu possa adicionar novas funcionalidades, e precisamos preparar o sistema pra isso."* (panels v2 brainstorm)
- *"Vamos pensar em primeiro preparar a IDE para receber as extensões, não vamos trabalhar nas extensões nesse momento"*
- *"Quando eu falo menu eu me refiro aos cabeçalho da aplicação tanto superior quanto inferior quanto lateral. Onde tem a logo o nome da ide a pasta que estamos o botão de new session isso pra mim é a barra de menu superior. Onde temos o ícone de pasta e de pesquisar eu chamo de barra de menu lateral esquerda. E hoje não temos mas o vscode tem uma barra inferior onde tem algumas informações que podemos adicionar aqui dentro do sistema"*
- *"a versão pode ficar nessa barra […] no vscode ficam informações como branch, lint/servidores de lint rodando, encode (UTF8) mas lá é um editor de texto então isso faz sentido, aqui é IA first então dificilmente vamos mexer no código só em última instância"*

So wai gets a bottom bar with what an AI-first IDE has running behind it; extensions can put items in the top and bottom bars; the user hides what they do not want and orders the left bar. No extension is written in this project **(owner)**.

Rule the owner set for these choices: look at how other IDEs do it, Zed first, then VS Code. Zed's extensions cannot add anything to its bars. VS Code's can: a status-bar item is an icon and text, a tooltip, a warning or error background, and a click that runs a command; title-bar and activity-bar entries are hidden with a right click; activity-bar icons are reordered by dragging; all of it is kept in VS Code's state, not in `settings.json`. This design follows VS Code where Zed has nothing.

## Goals

1. A **bottom bar** across the whole window, under the rail and the main area.
2. wai's own bottom-bar items: **MCP servers**, **an extension in trouble**, the **account's usage limit**, the **version** with its update. The version leaves the top bar. **(owner: the version; the other three are calls the owner accepted as defaults)**
3. Extensions **declare items** for the top or the bottom bar, update them live, and receive their clicks.
4. The user **hides** items of the top bar, the bottom bar and the rail with a right click, and **reorders** the rail's icons by dragging. Both survive restarts.
5. **API 1.3**, documented in `docs/extensions.md`. Extensions written for 1.2 behave exactly as before.

## Non-goals

- The API for features (an extension opening a file, typing in the terminal, …): project 4 of the program, its own design later.
- Themes: item 3.8 (`2026-11-12-wai-personalization-design.md`).
- Writing any extension **(owner)**.
- A rail icon from an extension that is not a panel's.
- A menu or picker on an item's click; alignment or priority fields; reordering items inside the top or bottom bar; hiding the whole bottom bar.
- Branch, folder, agent, context use, cost, encoding or cursor position in the bottom bar: the composer pills and each session's header already show the first five, per session, and the last two are an editor's (owner: "IA first").
- A macOS menu bar (File, Edit, …).

## Anti-goals (a failure even if it works)

- An extension item a user cannot tell apart from wai's own, such as a fake "Restart" or update item.
- A broken or chatty extension crashing wai, freezing the UI thread, growing memory without bound or filling a bar.
- Idle CPU above 0.5 %: no repeating timer, no polling, no animation in the bars.
- The top bar no longer fitting a narrow window (`top_bar_fits_narrow_window`), or extension items pushing wai's buttons out of it.
- `claude mcp list` running on its own more than once per launch and per project change.
- An API 1.2 extension rendering or behaving differently.

## Today (from the code)

- **Window:** `Workspace::render` (`crates/wai/src/workspace.rs:3285`) is `v_flex[top_bar, h_flex[rail, main area]]`. The bottom bar becomes its third child.
- **Top bar:** `crates/wai/src/topbar.rs`, `render_top_bar`. Left: brand (logo, "wai", `version()`), project pill (it drags the window), status pill ("2 agents working · 1 needs you"). Right: palette, terminal, agents toggle, New session. On macOS it sits in gpui-component's `TitleBar`, transparent over the aurora in dark mode.
- **Rail:** `crates/wai/src/sidebar.rs`, `render_rail`. The built-in `TABS` (Sessions, Files, Search, Agents; no Agents icon while the agents panel is on the right), then one icon per extension panel (`ext_host.panels()`, each with `ext` and `id`), then the foot: Extensions and Settings.
- **MCP status:** `crates/wai/src/claude_config.rs`, `check_mcp`, runs `claude mcp list` in the project (`run_mcp_list`, 60 s timeout); `wai_settings::claude::parse_mcp_list` gives `BTreeMap<String, McpStatus>` (connected, needs auth, failed with a reason). Only Settings > MCP Servers' Check button runs it today, and the run belongs to the modal (closing it kills `claude`).
- **Usage limit:** `crates/wai-agent/src/convert.rs:59` keeps the ACP usage update's `_claude/rateLimit` meta object. `chat/model.rs` reads only its `status` into `UsageInfo.rate_limit_status`; each session's header shows it after the context use (`usage_label`, `chat/render.rs:80`).
- **Updates:** `Workspace.update: UpdateStatus` (`Idle`, `Checking`, `UpToDate`, `Ready(Staged)`, `Available { release, why }`, `Failed`). The launch notice offers **Restart now** (`install_update`) or **Open release page**.
- **Extensions:** `ExtStatus` in `crates/wai/src/ext_host.rs` (`Starting`, `Ready`, `Failed(String)`, `Crashed(String)`); the Extensions view is `ExtensionsView`.
- **Protocol:** `crates/wai-ext/src/host.rs` coalesces `panel/render` (one per 10 ms per extension, each panel's last tree kept, the last always lands). The manifest refuses unknown fields (`deny_unknown_fields`); `API = "1.2"`; permissions in `manifest.rs:113`, their plain words in `extensions_view.rs:28`. Panel tones: `neutral`, `info`, `success`, `warning`, `danger` (`panel.rs:170`).
- **Kept UI state:** `wai_settings::AppState`, saved to `~/.config/wai/state.json`.

## Approaches

| | Approach | Trade-off |
|---|---|---|
| **A (picked)** | **Declared in the manifest, content by call**, like panels. `contributes.items` declares each item's id, bar and title; `item/update` sends its content; `item/action` reports a click. | The consent dialog and the hide menu know every item before the process runs. Authors already know the pattern from panels. Content is limited to what an item can say (icon, text, tooltip, tone). |
| B | An item is a tiny **panel tree** drawn in the bar. | Reuses the node set and its parser, but a 24 px bar is no layout space, and a free tree can draw something that looks like wai's own controls (anti-goal). |
| C | Items **created at run time**, as VS Code's `createStatusBarItem`. | No manifest change, but the consent dialog cannot say what the extension adds, and the hide menu knows only the items of running extensions. |

The owner handed me the how on 13/11; A is my pick **(call)**.

## Design

### The bottom bar

- Full window width, under the rail and the main area; one line of small text, about 24 px tall **(call)**.
- Same look as the top bar: transparent over the aurora in dark mode, the theme's bar colours in light mode. Only theme tokens and `aurora.rs` helpers; no new hard-coded colour outside `aurora.rs`, so 3.8 can swap them.
- Left to right: **MCP**, **extension in trouble**, the **extension items**, a spacer, **usage limit**, **version**.
- Every item that does something on click is a focusable button (Enter or Space clicks it) with an accessible label and a tooltip. One that does nothing is text with a tooltip.

### wai's items

1. **MCP** (`wai.mcp`)
   - wai runs `claude mcp list` once after launch, in the background, and again when the project changes. "The project" is the workspace's (`Workspace.project`, changed by Open Folder), the same folder Settings' check uses; sessions working in other folders do not change it. One result per workspace. A check from Settings > MCP Servers also updates the item. Nothing else runs it **(call)**. The launch check belongs to the workspace, not to the modal.
   - `claude mcp list` starts each server to check it, as the Check button does today; this now happens once per launch without a click (accepted by the owner, 13/11).
   - Text: `MCP 6` while all are connected; `MCP 5 · 1 failed` (danger) when any failed; `MCP 5 · 1 needs auth` (warning) when any needs a login; failed wins over needs auth. While checking: `MCP …`, no animation. The tooltip names the servers that failed or need a login.
   - If the check fails or times out: `MCP`, warning tone, the error in the tooltip.
   - Hidden when the `claude` CLI is not found or the project has no MCP server.
   - Click: opens Settings > MCP Servers and starts its check.
2. **Extension in trouble** (`wai.extensions`)
   - Shown only while an enabled extension is `Failed` or `Crashed`: `wai-rotinas stopped` for one (its name), `2 extensions stopped` for more, danger tone, a warning icon. The tooltip lists each name with the first line of its error.
   - Click: opens the Extensions view.
3. **Usage limit** (`wai.usage`)
   - The latest `_claude/rateLimit` any session received wins, whatever an earlier one said; the limit is the account's, not a session's. Hidden until one arrives, so agents that do not send it never show it.
   - When a known reset time passes, one wake scheduled for that moment (not a repeating timer) hides the item until the next report, so a `rejected` never outlives the limit **(call)**.
   - `allowed`: `Usage ok`, muted. `allowed_warning`: `Near usage limit · resets 13:30`, warning. `rejected`: `Usage limit reached · resets 13:30`, danger. Another status: the status itself, neutral.
   - The reset time is a local clock time read from the object's reset field. The builder checks what claude-agent-acp sends (today only `status` is read); with no reset field, the "resets" part is left out. A clock time needs no timer.
   - No click **(call)**.
   - Each session's header keeps its context use and cost and drops the rate-limit part, now shown here **(call)**.
4. **Version** (`wai.version`)
   - `wai 0.9.0`; the tooltip shows the full `version()` (version · commit).
   - `UpdateStatus::Ready`: `0.10.0 ready · Restart`, info tone; click = `install_update`, the same as the notice's Restart now.
   - `UpdateStatus::Available`: `0.10.0 available`; click opens the release page.
   - Any other status: click opens Settings > Updates.
   - The top bar's brand keeps the logo and "wai" and drops the version.

### Extension items

**Manifest.** A new contribution, and a new permission **`bar`** **(call)**, in plain words *"Show items in the top and bottom bars"*. `contributes.items` needs `bar`, as `contributes.panels` needs `panels`.

```toml
[[contributes.items]]
id = "prs"               # unique among this extension's items; the same rules as a panel id
bar = "bottom"           # "top" or "bottom"
title = "Pull requests"  # names the item in the consent dialog and in the hide menu
```

- At most **3 items per extension** **(call)**; more makes the manifest invalid. A `bar` other than `top` or `bottom` makes it invalid too.
- `items` without the `bar` permission is an invalid manifest, checked while parsing, as `panels` is (`need(Permission::Panels)`, `manifest.rs:325`).
- A wai that knows API 1.3 accepts `items` and `bar` whatever the manifest's `api` says, as it does the panels v2 fields. Authors set `api = "1.3"` so an older wai refuses the extension with its clear "needs a newer wai" message (`check_api` runs before the strict parse that would trip on the unknown field or permission).

**Extension → host: `item/update`** (needs `bar`)

```json
{"item": "prs", "text": "3 PRs", "icon": "git-pull-request", "tooltip": "2 green, 1 failing", "tone": "warning"}
```

- Result `null`. Each call replaces the item's whole state.
- An item with neither `text` nor `icon` is not drawn: that is how an extension hides its item.
- `text`: one line, cut to 32 characters in the bottom bar and 24 in the top bar. `tooltip`: one line, cut to `LINE_CHARS`. `icon`: a Lucide name; an unknown name draws no icon. `tone`: the panel tones; anything else is `neutral`. Fields the schema does not name are ignored.
- An item id not in the manifest gets `-32602`.
- Rate: it shares `panel/render`'s gap, one per 10 ms per extension; extra updates are coalesced, each item keeping its last state, and the last one always lands.

**Host → extension: `item/action`** (needs `bar`), a request `{"item": "prs"}` when the user clicks the item. wai ignores the answer. The usual reply is a `panel/open` or a `sessions/open`.

**How they show:**
- An item is drawn after its first `item/update` and only while its process runs. When the extension stops, fails, crashes or is disabled, its items go away. Item state belongs to the running process: after a restart, the items wait for a new `item/update`, like a panel waits for a new render. A manifest only changes by reinstalling, which restarts the process.
- Top bar: after the status pill, in the left group, as pills like the project pill. At most **4 extension items in total** are drawn there **(call)**; the rest are not drawn but stay in the hide menu, where hiding others makes room.
- Bottom bar: after wai's left items.
- In both, extensions come in the order the rail lists their panels, and each extension's items in manifest order.
- Tones use the theme's `info`, `success`, `warning` and `danger`; neutral uses the muted foreground.
- **Telling them apart:** every extension item's tooltip ends with *"— <extension name>"*; the hide menu lists extension items under their extension's name; extension items never sit at the bottom bar's right end, where the version and update live.
- A narrow window shrinks extension items first, truncating their text, and wai's own buttons stay in view. How it shrinks is the builder's, within that rule and `top_bar_fits_narrow_window`.

### Hiding and ordering

- **Right click** on the top bar, the bottom bar or the rail opens a menu of that bar's hideable items, each with a check mark; clicking one toggles it. Extension items are grouped under their extension's name. gpui-component's `ContextMenuExt` is already used in `sidebar.rs:953`.
- **What can be hidden:**
  - top bar: the status pill, palette, terminal, agents toggle, New session, extension items. Fixed: brand and project pill **(call: the project pill drags the window)**;
  - bottom bar: all of wai's items and the extension items;
  - rail: Files, Search, Agents and the extension panels' icons. Fixed: Sessions **(call: it is the center of an AI-first IDE, and its double click starts a session)**, Extensions and Settings.
- **Reordering the rail:** drag an icon among the icons above the foot; a line shows where it lands. The foot does not move.
- Hiding the selected sidebar tab switches the sidebar to Sessions. Hiding a main panel's icon leaves its pane open.
- A hidden item still runs: an extension keeps sending updates and wai does not draw them; a hidden item never sends `item/action`. Shortcuts still work (⌘N with New session hidden).
- **Kept** in `state.json` (`AppState`), as VS Code keeps them in its state rather than its settings **(call)**:
  - `hidden: BTreeSet<String>` and `rail_order: Vec<String>`;
  - ids for wai's: top bar `wai.status`, `wai.palette`, `wai.terminal`, `wai.agents-toggle`, `wai.new-session`; rail `wai.files`, `wai.search`, `wai.agents`; bottom bar `wai.mcp`, `wai.extensions`, `wai.usage`, `wai.version`. The top bar's agents toggle and the rail's Agents icon are separate: hiding one leaves the other, and ⌥⌘B still toggles the panel;
  - ids for an extension's: `<extension id>/<panel or item id>`;
  - an id in `rail_order` that no longer exists is skipped; an icon not in it goes after the ordered ones, in the default order;
  - ids of uninstalled extensions stay in the file; they are harmless.

### Docs

- `docs/extensions.md`: API 1.3; `contributes.items`; the `bar` permission; `item/update` and `item/action` with their limits; a short example with one bottom-bar item.
- `docs/user-guide.md`: the bottom bar and its items; hiding; reordering the rail.

## Constraints

- **Idle CPU < 0.5 %:** the bars redraw only when something they show changes; no timer, polling or animation. The MCP check is one process per launch and per project change.
- **Untrusted input:** `item/update` parsing never panics and runs under the same caps as panels; strings are clamped with `wai_ext::panel::clamp`.
- **Light and dark mode**, theme tokens only.
- **Windows:** same layout; the MCP item needs the `claude` CLI; nothing macOS-only.
- Existing tests keep passing (`top_bar_fits_narrow_window`, the rail tests); the `version()` tests move with the version.

## Acceptance

- Tests:
  - the manifest's `items`: limits, a bad `bar`, more than 3, missing `bar` permission;
  - `item/update`: coalescing, clamping, an unknown id, an item with no text and no icon;
  - an extension's items go away when its process stops;
  - each wai item's states: MCP counts, failure and hidden cases; extension trouble with one and with two; usage `allowed`, `allowed_warning` and `rejected`, with and without a reset field; version `Ready` and `Available` click targets;
  - hiding and showing survive a restart through `AppState`; the rail order survives one, and a new icon goes at the end;
  - the top bar fits a narrow window with 4 extension items in it, and a 5th is not drawn;
  - a usage `rejected` with a reset time hides at that time.
- `docs/extensions.md` and `docs/user-guide.md` updated.

## My calls

1. Approach A: items declared in the manifest, content by `item/update`, clicks by `item/action`.
2. The bottom bar is about 24 px and looks like the top bar.
3. The MCP check runs once per launch and per project change, plus the Settings check; its click opens Settings > MCP Servers and checks.
4. The rate-limit status moves from each session's header to the bottom bar.
5. The usage item shows `Usage ok` while known, not only near or at the limit.
6. The usage item does nothing on click.
7. A new permission, `bar`.
8. At most 3 items per extension and 4 extension items drawn in the top bar; text cut to 32 characters (bottom) and 24 (top).
9. No alignment or priority fields: extension items have one fixed spot per bar.
10. Brand, project pill, Sessions, Extensions and Settings cannot be hidden.
11. Hidden items and the rail order live in `state.json`, not in `settings.toml`.
12. The MCP item follows the workspace's project, not each session's folder.
13. The latest usage report wins; the item hides when its reset time passes.

## Left to the builder

- The exact wording and icons of wai's items, within what is written above.
- How the top bar shrinks in a narrow window, within the rule above.
- The drop indicator when reordering the rail.
- Where the launch MCP check lives in the workspace and how it shares its result with the Settings page.
