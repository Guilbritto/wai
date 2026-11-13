# wai: extension panels v2 (new components and the main area)

> 2026-11-12. Asked for by the owner in a brainstorm about making wai modular. First project of the "modularity" program (below). API version: **1.2**.

## Intent

Owner, verbatim:
- *"A minha ideia é poder criar extensões que possam adicionar itens no menu, no cabeçalho, que eu consiga mudar o tema, que eu possa adicionar novas funcionalidades, e precisamos preparar o sistema pra isso."*
- *"Eu sempre quis montar isso, não foi de agora, eu só fui construindo as coisas que eu fui sentindo necessidade e fui evoluindo, e agora chegou a hora de mexer nesse topico"*
- *"Qualquer pessoa pode escrever uma extensão, então temos que deixar as coisas o mais modular possivel"*
- *"Eu penso em uma extensão para fazer por exemplo, pegar uma task direto no jira ver os PRs abertos, fazer integrações com coisas da Way etc... (pegar o que tem por exemplo no devhub e passar para uma extensão etc..)"*
- *"Vamos então de A, prepara já os componentes novos, como card e outros"*

The test case is DevHub (`~/Project/waycarbon-ecosystem-local/devhub`, Wails: Go + React): "Meus PRs" with CI state, staging deploys with a confirmed merge, S3 → Postgres restore with live progress, local services with start/stop, Keycloak sync output. Today an extension panel cannot draw those screens: it lives only in the narrow sidebar and knows `column`, `row`, `text`, `markdown`, `list`, `button`, `input`, `select`, `checkbox`, `badge`, `progress` (`crates/wai-ext/src/panel.rs`). This project gives panels the components those screens need and room to show them.

Anyone may write an extension, so every addition is a public contract: it is versioned, documented in `docs/extensions.md`, and must not break extensions written for API 1.1.

### The modularity program (context, not this project)

1. **This project:** new panel components, panels in the main area, panel icons, API versioning.
2. **Places:** extension items in the top bar (next to the project and agent-status pills, live text updated by the extension), hiding and ordering rail icons and top-bar items by the user.
3. **Themes:** replace the ~150 hard-coded Aurora colours (`aurora::boreal()`, …) by semantic tokens, then load `contributes.themes` (JSON theme sets, the format `gpui-component`'s `ThemeRegistry` already reads, like Zed's).
4. **API for features:** what else an extension may ask wai for (open a file, type in the terminal, …), driven by real extensions.

## Goals

1. New nodes: `card`, `section`, `table`, `log`, `alert`, `link`, `icon`, `spinner`, `divider`, `spacer`, `kv`, `menu`.
2. Richer existing nodes: `button` (`icon`, `disabled`, `confirm`), `badge` (`tone`), `input` (`multiline`), `text` (`mono`).
3. A confirmation dialog drawn by wai, declared on a button or menu item, before the action reaches the extension.
4. A panel can live in the **main area** as a pane, not only in the sidebar.
5. A panel declares an **icon** for the activity bar, instead of initials.
6. Extensions declare the API they need; wai refuses clearly when it is too old.
7. Acceptance: DevHub's "Meus PRs" screen and its restore screen can be written with these nodes alone (two sample trees in the docs, both covered by tests).

## Non-goals

- A webview, HTML or JavaScript (approach B below).
- Top-bar items, hiding/reordering rail icons, themes: later projects of the program.
- A `tabs` node, charts, trees, drag and drop, sortable tables, SVG icons from the extension's folder.
- An incremental update API (`panel/append`, patching part of a tree). Panels keep sending whole trees.
- Saving extension panes between runs.

## Anti-goals (a failure even if it works)

- A hostile or broken tree crashes wai, freezes the UI thread, or grows memory without bound.
- An extension draws a dialog or control that a user cannot tell apart from wai's own.
- An extension written for API 1.1 renders differently or stops working.
- Idle CPU above 0.5 %: a panel that is not visible draws nothing; a `spinner` or `log` animates only while shown.
- A link that runs anything other than opening an `http(s)` page in the browser.

## Constraints (from today's code)

- The tree is untrusted. Parsing never panics, never recurses past `MAX_DEPTH` (32), stops at `MAX_NODES` (2,000) and runs off the UI thread. A broken node, an unknown `type`, or a missing/mistyped required field becomes `Unsupported` ("Unsupported element"); only an oversized tree is refused whole ("Panel too large"). Fields the schema does not name are ignored. New nodes follow all of this.
- Strings outside `text`/`markdown` are one line of at most `LINE_CHARS` (500).
- Form state survives re-renders: an initial value (`value`, `checked`) applies when the field first appears or when the extension sends a different one (`ExtPanel::set_tree`, `crates/wai/src/ext_panel.rs`). New stateful nodes (`section`'s `open`, `input` with `multiline`) follow the same rule.
- Buttons and form fields share one set of unique ids per tree; list item ids may repeat.
- Calls are checked against granted permissions; panels need `panels`. Render rate: 100/s per extension, coalesced.
- The manifest refuses unknown fields at every level (`deny_unknown_fields`, `crates/wai-ext/src/manifest.rs`).
- Look: wai draws Aurora helpers in dark mode (`aurora::card`, `aurora::badge`, `aurora::pill`) and theme tokens in light mode. New nodes use those helpers and `cx.theme()` tokens (`success`, `warning`, `danger`, `info`, …), adding no new hard-coded colour outside `aurora.rs`, so the themes project can swap them later.

## Approaches

- **A. Native, declarative JSON (picked by the owner).** The extension describes the screen; wai draws it with its own components (`gpui-component` already has table, dialog, collapsible, alert, description list, spinner). Theme and look apply to every extension for free, CPU stays low, any language works, and wai keeps control of what is drawn. Cost: porting DevHub means rewriting its screens as trees, and each new component needs a wai release.
- **B. Webview (VS Code's model).** The extension serves HTML/React and wai embeds the system browser. DevHub's React front could be reused almost as is. Rejected: `gpui-component` has no webview, embedding WKWebView/WebView2 into a GPUI window is a project of its own (focus, keyboard, resizing), the page ignores wai's theme, costs more memory and CPU, and widens the attack surface. Kept as an escape hatch if a screen turns out impossible to describe.
- **C. Extensions draw with GPUI directly (Rust/WASM UI code).** Rejected: ties extensions to Rust and to wai's UI crate version, which breaks "anyone, any language".

## Design

### API version

- `initialize` sends `"api": "1.2"`.
- New optional manifest field `api = "1.2"`: the minimum API the extension needs. Absent means 1.0.
  - **Format:** `major.minor`, both decimal numbers. Anything else is an invalid manifest.
  - **Comparison:** numeric, by major then by minor.
  - **When it is checked:** wai reads `api` before the strict parse, so a newer manifest's unknown fields do not hide the reason. The check runs inside manifest parsing, so it applies both at install and when an installed extension is loaded at launch.
  - **When wai is too old:** it refuses with `<name> needs a newer wai (extension API <api>; this wai has <own>)`.
  - `api` is only a minimum. The new fields and nodes work whatever `api` says; declaring it just gets older wais to refuse with a clear message.
- wai ≤ API 1.1 does not know the field and refuses it as unknown. The docs say an extension that uses API 1.2 needs the wai release that ships it (0.9.0).
- New node types degrade to "Unsupported element" on an older wai, as today.

### Panels: icon and location

`[[contributes.panels]]` gains two optional fields:

| Field | Values | Effect |
|---|---|---|
| `icon` | a Lucide icon name, kebab-case (`git-pull-request`, `database`, …), from the set wai ships (`gpui_kit::assets::IconName`) | The activity-bar icon. An unknown name falls back to today's initials; it does not fail the install |
| `location` | `"sidebar"` (default) or `"main"` | Where the panel shows |

- **`sidebar`:** as today.
- **`main`:** the rail icon opens the panel as a pane in the main area, or focuses its pane if it is open. It is never a sidebar tab: the rail icon is a button, shown selected while the pane is on screen.
  - **One view.** The pane shows the same `ExtPanel` entity `ExtHost` already owns, so what the user typed is kept. At most one pane per panel.
  - **Placement:** `Workspace::session_pane`'s rule. That is the focused pane unless it is the editor, then a new pane while there are fewer than `MAX_PANES`, else the next pane. The session it replaces stays open in the sidebar.
  - **Closing** the pane does not stop the extension.
  - **Lifetime:** the pane closes when its panel goes. That happens when the extension stops, fails, crashes or is disabled, the same moment the sidebar tab disappears today. A restart for a project switch keeps the panel, so the pane stays.
  - **Work areas:** an extension pane is not part of any area. Showing an area or a lone session replaces it like any non-editor pane, and the rail icon reopens it. Ceiling: a user who wants it to stick across areas has to reopen it. The upgrade is to carry it like the editor pane.
  - Extension panes are not saved between runs.
- **`panel/open`** (extension → host, needs `panels`): `{"panel": "<id>"}` shows the panel: a sidebar panel becomes the sidebar's tab, a main panel opens or focuses its pane. It never takes the keyboard focus. At most one every 2 seconds per extension (`Too many panel opens; try again later`). Typical use: a palette command → `command/run` → `panel/open`.

### New nodes

All fields without `?` are required.

| `type` | Fields | Behaviour |
|---|---|---|
| `card` | `id`?, `title`?, `subtitle`?, `badge`?, `icon`?, `children`? (nodes) | A bordered card in wai's card style: header (icon, title, subtitle, badge) then its children. With `id`, the whole card is clickable (hover, pointer, focusable, Enter/Space) and sends `panel/action` with `action_id` = `id`; card ids may repeat, like list items |
| `section` | `id`, `title`, `open`? (bool, initial, default `true`), `badge`?, `children` | A collapsible block. The user's open/closed state survives re-renders; a different `open` from the extension replaces it. `id` is in the shared unique-id set |
| `table` | `columns` (array of `{title, width?}`), `rows` (array of `{id?, cells}`), `fill`? | A header row and rows. `width` is in pixels; columns without one share the rest. A cell is a string (one line) or one of these nodes: `text`, `badge`, `link`, `icon`, `button`, `menu`, `spinner`, `progress`. Extra cells are dropped, missing ones are blank. A row with `id` is clickable and sends `panel/action` with that `id` (row ids may repeat); a click on a button, menu or link inside a cell goes to that control only. Without `fill` it shows 12 rows and scrolls; with `fill` it takes the remaining height. At most 20 columns and 1,000 rows; the rest are dropped |
| `log` | `text`, `follow`? (bool, default `true`), `fill`? | Monospace, selectable output. Keeps the **last** 100,000 characters (cut from the start). ANSI escape codes are removed. With `follow`, it stays scrolled to the bottom unless the user scrolled up. Without `fill` it shows 12 lines and scrolls |
| `alert` | `tone` (`info`, `success`, `warning`, `danger`), `title`?, `text` | A coloured notice. `text` up to `TEXT_CHARS`, wraps |
| `link` | `text`, `url` | Opens `url` in the default browser. Only `https://` and `http://`; any other scheme makes it `Unsupported`. The tooltip shows the full URL. Sends no action |
| `icon` | `name`, `tone`? | A Lucide icon (same names as the panel `icon`). An unknown name is `Unsupported` |
| `spinner` | `label`? | A loading indicator; animates only while visible |
| `divider` | — | A horizontal rule |
| `spacer` | — | In a row, takes the free space (pushes later children to the end) |
| `kv` | `items` (array of `{key, value}`) | A key/value list, both one line. At most 200 items |
| `menu` | `id`?, `label`?, `icon`? (default `ellipsis`), `items` (array of `{id, label, danger?, confirm?}`) | A button that opens a dropdown. Picking an item sends `panel/action` with the item's `id` (item ids may repeat). At most 50 items |

`tone` everywhere is one of `neutral` (default), `info`, `success`, `warning`, `danger`, mapped to the theme's tokens.

### Changes to existing nodes

| `type` | New fields |
|---|---|
| `button` | `icon`?, `disabled`? (bool), `confirm`? |
| `badge` | `tone`? |
| `input` | `multiline`? (bool), `rows`? (number, default 4, at most 20): a text area. Its value arrives in `values` like any input |
| `text` | `mono`? (bool) |

### Confirmation

`confirm` on a `button` or a `menu` item: `{"title", "text"?, "confirm_label"?}`.
- Clicking opens a window-level modal drawn by wai (`gpui-component`'s dialog). It shows:
  - the **extension's name** in its header;
  - the title, then the text (`TEXT_CHARS`, wraps);
  - **Cancel**, and the confirm button labelled `confirm_label` (default `Confirm`), in danger style when the button or item is `danger`.
- Only **Confirm** sends `panel/action`. The action id is the one captured at the click. The form values are the ones at the moment of Confirm. Cancel, Esc or a click outside send nothing.
- Re-renders while the modal is open do not touch it. If the panel itself is gone at Confirm time (the extension stopped), nothing is sent.

### Limits

`MAX_DEPTH` and `MAX_NODES` stay as they are.
- **What counts as a node:** every node counts, including a node cell, which sits one level below its table. String cells, table columns, table rows, `kv` items and `menu` items are not nodes; their own caps above bound them, and the 4 MB message cap bounds their total size.
- **Per-node caps** are in the tables above. Nothing in a tree can make wai keep more than these caps allow.

### Nesting, clicks and `fill`

- **Clicks inside clickable things:** a click on a button, menu, link or form field inside a clickable `card` or table row goes to that control only, not to the card or row.
- **Form fields in containers:** `card` and `section` children may hold any node, form fields included. Table cells may not hold form fields (the cell list above).
- **`fill`** (`list`, `table`, `log`) behaves as `list`'s `fill` does today. It is honoured only for a direct child of the panel's root `column` (or the root itself); anywhere else it is ignored.

## Testing and docs

- **Parser** (`crates/wai-ext/src/panel.rs`): per node, a valid tree, each required field missing or mistyped → `Unsupported`, each cap, `link` with `javascript:`/`file:` → `Unsupported`, `log` keeps the tail, ANSI stripped.
- **Rendering** (`crates/wai/src/ext_panel.rs`, existing test patterns): every node renders; card, table row and menu item send their action; a button inside a row sends only its own; confirm shows the modal, Cancel sends nothing, Confirm sends once; section state survives a re-render and yields to a new `open`; a `main` panel opens as a pane and its rail icon focuses it; `panel/open` respects its rate and the `panels` grant.
- **Manifest:** `icon`, `location`, `api` parse; a newer `api` refuses with the message above.
- **Docs:** `docs/extensions.md` moves to API 1.2: every node, the panel fields, `panel/open`, `confirm`, and two complete sample trees, DevHub's "Meus PRs" (table: repo, title as link, CI badge, updated, ⋯ menu; Refresh button) and a restore screen (form, confirmed danger button, progress, log). A test renders both samples with no `Unsupported` node.

## Left to the builder

- Which `gpui-component` pieces back each node (`table`, `collapsible`/`accordion`, `alert`, `description_list`, `spinner`, `dialog`), element ids, sizes and spacing.
- Mapping kebab-case names to `IconName`.
- The pane plumbing (`Pane` in `crates/wai/src/workspace.rs`), including how an extension pane interacts with work areas, following what the editor pane does.
- Whether `log` and large tables need virtualised rendering to stay within frame time at the caps.

## Calls made in the brainstorm (not asked of the owner)

- Lucide icons only; no SVG from the extension's folder (untrusted file parsing; add when someone needs a custom icon).
- An unknown panel icon falls back to initials instead of failing the install.
- The confirmation modal names the extension, so it cannot pass for wai's own.
- `link` opens only `http(s)` and sends no action.
- `log` is re-sent whole, keeping the last 100,000 characters. Ceiling: a fast log re-sends its whole text every render; upgrade path is `panel/append` if it is measured as a problem.
- No sorting in `table`, no `tabs` node: the extension sorts, and switches views with buttons or a `select`.
- The program order: this project first (owner: "prepara já os componentes novos"), then places, themes, and the feature API.
- After the builder check, with the owner away (*"vc termina de executar e itera sobre eles até a entrega da funcionalidade pronta, ai vc vai mesclar em dev e promover a nova versão"*):
  - a `main` panel is never a sidebar tab, and its pane shows the one `ExtPanel` entity;
  - placement follows `session_pane`;
  - the pane closes with its panel and is not part of work areas;
  - the confirm action id is captured at the click;
  - nested controls take their own clicks;
  - `fill` is honoured only at the root;
  - `api` is `major.minor`, checked while parsing the manifest.
