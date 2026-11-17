# wai-waycarbon 0.6: a full-width WayCarbon panel — design

Mockups: `.superpowers/brainstorm/81990-1794877345/content/` (`waycarbon-full-width.html` option C,
`waycarbon-jira-columns.html` collapsed columns, `waycarbon-prs-tab.html` option A,
`waycarbon-prs-search.html`).

## Intent

wai 0.17 lets the sidebar take the whole window (`close_main`). The WayCarbon panel still draws
everything for a 300 px sidebar: one long column of cards per tab, "muito junta, ruim de
manusear". The owner wants the panel rebuilt for a wide space, and a one-click way into it: the
WayCarbon icon in the left rail hides the sessions area so the panel fills the window, and
anything that needs the sessions brings them back.

## Goals

- Clicking the WayCarbon rail icon shows the panel full width; the sessions area is hidden, not
  closed (sessions keep running, the bar item keeps alerting).
- Leaving is one click: the icon again, any other rail icon, or an action that opens a session
  (which then shows, with the sidebar on the Sessions tab).
- Work, Jira and PRs tabs laid out for width: a list or board on the left, the selected item's
  detail on the right, each scrolling on its own.
- Jira: a board in the real Interstellar order, empty columns collapsed, sprint filter, search,
  detail, and a Move to menu with every status.
- PRs: the three groups by Jira card, with a search over every PR of the org.
- The Help tab and everything behind it removed.

## Non-goals

- A narrow layout. The panel only renders wide (it is never shown at 300 px).
- Making other extensions' panels wide: it is opt-in per panel.
- Editing Jira cards (description, fields) or commenting from the panel.
- Showing diffs or posting reviews from the panel: sessions do that.
- Reading the board's columns from Jira: the order is fixed in the extension.

## Anti-goals

- The sessions stop, or a session the extension opens lands in a hidden pane.
- Losing the user's place: a refresh must not reset the selected item, the filter or the search.
- The main refresh gets slower: per-item detail is fetched only when an item is selected.

## Constraints

- Panel trees are JSON over stdio, at most 100 renders/s, 2,000 nodes, 32 levels.
- Today's renderer: a `row` wraps and never grows (R56); only a `fill` list/table/log under columns
  grows. A list/detail split needs a row that fills the height, does not wrap, and whose columns
  scroll on their own.
- gpui cannot rotate text.
- gh and jira are the user's CLIs; the extension never sees tokens.
- Extension releases ship on `wai-waycarbon-v<version>` tags and commit as `chore:`; wai changes are
  `feat:`/`fix:`.

## Approaches

1. **Width signal + two layouts:** wai tells the extension the panel's width; the extension sends a
   narrow or wide tree. Works for every size, but two layouts to keep, and a re-render per resize.
2. **Responsive nodes:** wai lays the same tree out narrow or wide (a split that stacks when narrow).
   One tree, but the renderer grows a responsive layout engine.
3. **`location = "main"` panel:** a pane beside the sessions. Never full width, and the pane fights
   sessions for space. Discarded.
4. **Wide mode on the rail icon (chosen by the owner):** a sidebar panel marked `wide` hides the
   main area while it is shown. The panel only ever renders wide, so the extension keeps one layout,
   and wai needs only a few layout primitives.

## Part 1 — wai (API 1.6)

### 1.1 `wide` panels

- Manifest: `[[contributes.panels]] wide = true` (bool, default false). Only for
  `location = "sidebar"`; with `main` it is a manifest error. `Panel` gets `pub wide: bool`.
- While the sidebar shows a wide panel (`!sidebar_hidden && sidebar_tab == Panel(id)` of a wide
  panel), the main area is not drawn and the sidebar takes its width. This is derived from the
  selected tab, not a new saved flag: `main_shown` becomes
  `!(main_closed && only the empty pane) && !wide_shown`. `sidebar_tab` is not saved, so wai never
  starts wide.
- Rail:
  - Clicking a wide panel's icon selects it (wide), unhiding the sidebar if it was hidden.
  - Clicking it again while selected and shown goes to the Sessions tab (main comes back), instead
    of hiding the sidebar.
  - Clicking any other icon selects that tab, so main comes back by itself.
- ⌘B while wide hides the sidebar, so main shows; ⌘B again shows the wide panel again. No special
  case.
- Opening something in the main area leaves wide mode to the Sessions tab:
  - a session starting (`start_session`, the extension's `sessions/open` included);
  - a main panel opening (`open_panel`).
- `panel/open` of a wide panel (the bar item click) selects it, so it shows wide.
- Width: the render already gives a lone sidebar the main area's width (`main_shown == false`), with
  no resize handle; the sidebar's own width is fixed and nothing is saved, so nothing to restore.

### 1.2 Layout primitives

- **`row` `fill`? (bool):** when every ancestor is a column, the row takes the remaining height
  (`flex_1`, `min_h_0`) and does not wrap. Each child gets the full height, and a `column` child
  scrolls on its own (`overflow_y_scroll`). A `fill` row's descendants do not `fill` (they scroll
  with their column). Elsewhere `fill` is ignored, as for lists.
- **`column` `width`? (number, px):** a fixed width (`flex_none`). Without it, columns in a row share
  the width, as today. Clamped to 24–2,000.
- Scroll offsets must survive re-renders, since the extension repaints on every click. So a fill
  row's columns get ids that do not move when nodes above them come and go: the row's index among
  fill rows, scoped by the selected tab of the enclosing `tabs`, plus the column's index. Not the
  node number. Section open state and input values are already kept by id.
- **`card` `selected`? (bool):** drawn with the selected look (the theme's accent border and
  background). It is display only; the click still sends the card's `id`.
- A `text`'s `\n` breaks lines (check gpui does this; if not, split into lines in `leaf`). That gives
  the collapsed board strips their vertical name, one letter per line.

### 1.3 API version and docs

- `API = "1.6"`. `docs/extensions.md`:
  - `wide` in the manifest table and the Panels section;
  - the three new fields in the Trees tables;
  - "this wai has 1.6", and "set `api = "1.6"` when you use them".
- Tests:
  - manifest parse (`wide`, `wide` with `main` refused);
  - `main_shown` with a wide panel selected;
  - rail click on a selected wide panel goes to Sessions;
  - `start_session` leaves wide mode;
  - fill row grows and scrolls per column (in the style of the R56/R57 tests);
  - `column` width;
  - `card` `selected` parse.

## Part 2 — the extension (`extensions/wai-waycarbon` 0.6.0)

### Manifest

- `version = "0.6.0"`, `api = "1.6"`, the panel gets `wide = true`.
- The description drops "help requests".
- Permissions unchanged: `network` stays (gh and jira reach GitHub and Jira; nothing is enforced).

### Remove Help

- Delete `help.go`, `help_test.go`, `helpview.go`, `helpapi/`, `relay/`.
- Remove from the server and views:
  - `help helpData`;
  - `helpInit`/`helpStart`/`helpAction`;
  - `counts.Help`/`Replies` and their `kv` rows;
  - the Help tab.
- README: three tabs; drop the Help section, the relay token note and `WAI_WAYCARBON_RELAY`.
- Tests: `main_test` stops expecting a `help` id.
- Release-side notes (index, relay deploy) are listed under Owner-only.

### Shared layout

Each tab's tree:

```
column
  row (toolbar: search input?, filter?, spacer, Updated hh:mm, Refresh)
  alerts (gh/jira problems, plugin), as today
  row fill
    column width=W  (list or board)
    column          (detail)
```

- Selection is server state, per tab: `sel map[string]string` (tab → item id).
- A list item is a clickable `card` with `selected` on the chosen one.
- A click sends `sel:<tab>:<id>`; the server stores it and repaints.
- After a refresh, the selection is kept if the item still exists. Otherwise it falls to the first
  item, or to an empty detail ("Select an item").
- Section headers in the list are `section` nodes with a count badge (collapsible, kept open by
  default).

### Work tab (option C: list + detail)

- **List (width 380):** the current groups:
  - Needs you (my PRs by reason);
  - Review requests;
  - Cards to update (merged, no open PR);
  - Waiting.
- Each card:
  - title: `#88 Title`;
  - subtitle: repo, and the card key when the title has one;
  - badge: reason (Conflict, CI failed, Changes, Ready, Waiting, Review).
- **Detail for a PR:** shared with the PRs tab (below).
- **Detail for a card to update:** key, summary, status, the merged PR links, and the **Move to Done**
  action (today's `done:<key>`).
- The plugin alert and install button stay at the top of the Work tab.

### PR detail (Work and PRs)

- **Header:**
  - `#88 Title`;
  - badges: Draft, review decision, CI, mergeable;
  - `repo · branch`;
  - author;
  - Open on GitHub (`link`).
- **Stats:** `+adds −dels · N files` (`kv`).
- **Checks:** one line per check run / status context: name and state, with an icon and tone. Failed
  checks come first.
- **Reviews:** latest reviews (author, state, time, body as markdown, trimmed to 1,000 chars). After
  them, the unresolved review threads' first comments, with path:line.
- **Actions:**
  - the session prompts that exist today, by reason: Fix review, Resolve conflict, Fix CI, Merge,
    Recheck;
  - Review for others' PRs.
  - The primary one comes first.
  - Each opens a session through today's `clickPrompt` ids. Opening the session is what leaves
    wide mode in wai.
- **Data:** fetched when a PR is selected, with one `gh api graphql` by URL:
  - `additions deletions changedFiles author`;
  - `statusCheckRollup.contexts(first: 50)` (CheckRun name/conclusion/status, StatusContext
    context/state);
  - `reviews(last: 10)` with body;
  - `reviewThreads(first: 30)` (isResolved, first comment body/path/line/author).
- Cached per URL until the next refresh. While it loads, the header (known from the snapshot) shows
  with a spinner below; on error, an alert in the detail.

### Jira tab (option B: board + detail)

- **Toolbar:**
  - search input `jira:q` (EC-1234 or text) with Search and Clear, as today;
  - two buttons as a toggle, "Sprint atual" (default) and "Todos os meus" (`jira:scope:sprint`,
    `jira:scope:mine`). The chosen one is `primary`, and a click switches and reloads the board (a
    `select` sends no action, so it would need an extra Apply);
  - Updated time and Refresh.
- **Board** (the left column has no fixed width; the board takes the space left of the detail):
  - Columns in order: Tech Refinement, To Do, Ready For Dev, Development, Development Done, Code
    Review, Code Review Done, Ready For Test, Test, Test Done.
  - A status not in the list gets its own column after these, in the order first seen.
  - A column with cards is a `column` with a header (name and count) and the cards (key, summary,
    PR count badge), and shares the width.
  - An empty column is a `column` `width=28` holding its count and its name with one letter per line
    (`text` with `\n`).
- **Search results** replace the board with one list ("N cards for X", Clear), selectable the same
  way.
- **Detail (column width 420):**
  - key, summary, status badge;
  - meta `kv`: type, priority, parent (key and summary), assignee;
  - description (markdown, trimmed to 4,000 chars);
  - sub-tasks (key, summary, status);
  - PRs: the card's PRs from the org search, each a line with repo, number, state badge, link.
- **Detail actions:**
  - **Solve card** (`card:<key>`, today's prompt);
  - **Move to ▾** (`menu` with every board status but the current one, item id `move:<key>:<status>`,
    with a confirm);
  - **Open in Jira** (`link` to `https://<site>/browse/<key>`, the site taken from the `server:` line
    of `~/.config/.jira/.config.yml`, or `$JIRA_CONFIG_FILE`; without it, no link).
- **Data:**
  - Board, "Sprint atual": `jira issue list -a <me> -s~Done -q "sprint in openSprints()" --plain …`.
    "Todos os meus" is today's `myCards`.
  - Detail: `jira issue view <key> --raw` (JSON) for issuetype, priority, parent, subtasks,
    assignee, status, description. A string description shows as is. An ADF description is turned
    into text: paragraphs, headings, list items and code blocks become lines; text nodes are joined.
  - The PRs come from the same org search as the PRs tab (all states, not the snapshot), by key.
  - Fetched on selection, both in parallel, and cached per key until the next refresh.
- **Move:** `jira issue move <key> <status>`, then refresh. The menu lists every board status, not
  just the transitions the workflow allows from the current one: jira-cli has no non-interactive
  way to list transitions. A failure shows as an alert in the
  detail. `moveDone` becomes a call of this with `WAI_WAYCARBON_DONE`.

### PRs tab (option A: list + detail, with search)

- **Toolbar:** search input `prs:q`, Search, Clear, Updated, Refresh.
- **List (width 420):** sections Mine, Requested from you, Team queue.
  - Each section groups PRs by Jira card, as `groupByCard` does today. A group is a card whose title
    is `EC-1234 summary` (or the repo for PRs without a key), with one line per PR inside as
    `#88 repo` and a badge.
  - Selecting a PR selects `pr:<url>`; selecting the group selects `card:<key>`.
- **Detail:**
  - for a PR, the shared PR detail;
  - for a group, its PRs (as lines, with badges and links) and **Review all** (today's
    `reviewall:<key>`) when some are others' and open.
- **Search:**
  - `gh api graphql` search `<q> in:title is:pr org:<org>` (all states), first 50;
  - grouped by card key, newest first;
  - replaces the list, with "N cards, M PRs for X" and Clear;
  - merged and closed PRs show with their state badge.
  - Implemented like the Jira search: off the lock; a newer search or Clear drops an older answer.

### Bar item

Unchanged, without the Help and Replies counts.

## Testing

- **Go:** table tests on the tree builders, in the style of `work_test.go`:
  - board columns order, collapse and unknown status;
  - selection kept, lost and defaulted;
  - PR detail sections from a fixture GraphQL answer;
  - ADF to text;
  - search grouping;
  - the action ids (`sel:`, `move:`).
  - `main_test` end-to-end over stdio: the tree has the three tabs and `wide` rows; a `sel:` action
    repaints with `selected`.
- **Rust:** as in Part 1.
- **Manual:** install a local build (CLAUDE.md), install the extension from the folder, and check:
  - wide mode in, out by each path;
  - a session opened from the panel shows;
  - the board scrolls per column.

## Order

1. wai: manifest `wide`, layout primitives, workspace behavior, docs, tests (`feat:`).
2. Extension: remove Help (`chore:`).
3. Extension: new data, views, tests, manifest 0.6.0 (`chore:`).
4. Local build and manual check.

## Left to the builder

- gpui details of the scrolling columns (element ids, `min_h_0`), the selected card look.
- Exact GraphQL field names (check against `gh api graphql` on a real PR), and how checks map to
  tone.
- Trimming lengths, widths (380/420/28), and wording.

## Owner-only

- Merging into `development` and releasing:
  - wai first, since API 1.6 is needed;
  - then the extension tag;
  - then `extensions/index.toml`.
- Shutting down the relay deployment, if one runs.

## My calls, for review

- `wide` is derived from the selected tab, not saved; wai always starts on Sessions.
- Clicking the selected wide icon goes to Sessions rather than hiding the sidebar.
- Opening a session or a main panel while wide switches to the Sessions tab.
- Collapsed columns show the name one letter per line, since gpui cannot rotate text.
- The board order is fixed in the extension; unknown statuses go at the end.
- The sprint filter uses JQL `sprint in openSprints()` on my open cards, as a two-button toggle.
- PR and card details are fetched on selection and cached until the next refresh.
- Move to lists every status; a transition the workflow refuses shows jira's error in the detail.
- `network` permission kept.
