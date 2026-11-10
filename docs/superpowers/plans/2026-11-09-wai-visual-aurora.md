# wai Visual Review "Aurora" Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** wai looks like the Central de IA, natively and still within its performance goals:
- an aurora backdrop;
- dark-glass cards;
- a top bar with a status pill;
- pill tabs, chat bubbles, Central-style permission cards and a pill prompt box.

**Architecture:** One embedded gpui-component theme JSON ("wai Aurora") carries most of the change, because every view already reads theme tokens. A new `aurora.rs` holds:
- the extra palette constants;
- the bundled display font;
- the drawing helpers: backdrop, card, card bar, pill.

A new `topbar.rs` draws the top bar. Each view's render function then adopts the helpers. There is no animation anywhere.

**Tech Stack:** Rust 1.99.1, `gpui-kit =0.9.0` (GPUI + gpui-component 0.9.0: `ThemeRegistry`, `Theme`, `TitleBar`, `BoxShadow`, `linear_gradient`), Nunito Bold TTF (OFL 1.1). No new crates.

**Spec:** `docs/superpowers/specs/2026-11-09-wai-visual-aurora-design.md`, which carries the palette table, decisions, non-goals and anti-goals. Read it first. Visual references: `../../central-ia-waycarbon-claude/docs/img/{escritorio-desktop,chat-celular,permissao-celular}.png`, `../../central-ia-waycarbon-claude/macos-state/lado-desktop.png`; CSS source `../../central-ia-waycarbon-claude/central/www/app.css` (`:root`, `.painel-cartao`, `.painel-barra`, `.janela-aba`, `.btn*`, `.chip`, `.msg*`, `.selo-trabalho`).

## Global Constraints

- **Branch:** `feat/visual-aurora`, created from `development`. Commits use `feat:`, `fix:` or `docs:` with no scope and no `Co-Authored-By` line.
- **Dependencies:**
  - no new crates, and `Cargo.lock` stays unchanged;
  - never run a bare `cargo update`;
  - embed assets with `include_str!`/`include_bytes!`.
- **Performance:**
  - idle CPU < 0.5%, streaming CPU < 10%, window visible < 1 s;
  - no `with_animation` and no repeating timers added for visuals.
- **Element ids:** every existing one keeps working: `tab-sessions|files|search|agents`, `tab-panel`, `sidebar-templates`, `sidebar-extensions`, `sidebar-new`, `new-session`, `send`, `stop`, `restart`, the permission choice ids from `button_id()`, `question-submit|decline|cancel`, `editor-tab-close`, and all `debug_selector`s. The test suite (`cargo test --workspace`, 283 passing today) must stay green after every task.
- **Light mode:** `theme = "light"` keeps gpui-component's "Default Light". Helpers branch on `cx.theme().is_dark()`: in light there is no backdrop, and cards and pills use theme tokens (`background`, `border`, `secondary`), never Aurora constants.
- **Copy:** exactly the following, and nothing else new:
  - "No agent working", "1 agent working", "N agents working";
  - " · 1 needs you", " · N need you";
  - "No messages yet.";
  - "PERMISSION REQUESTED", "QUESTION";
  - "Start a session".
- **Safety:**
  - tests and screenshots never touch the real `~/.config/wai`, and screenshots run the built binary with a scratch `HOME`;
  - never `cargo run` with a fake `HOME`;
  - screenshots show no username, hostname or personal path.
- **Rust:** cargo is at `~/.cargo/bin`. Prefix commands with `export PATH="$HOME/.cargo/bin:$PATH" _ZO_DOCTOR=0;`.
- **Visual check after each UI task:**
  1. `cargo build -p wai --release --features screenshot` and `cargo build --release -p wai-agent --bins`;
  2. run `target/release/wai` with a scratch `HOME` holding a `settings.toml` that points `default_agent` at `target/release/wai-fake-agent`, plus `WAI_SHOT=<scratch png>` and the scripted variables documented above `fn screenshot` in `crates/wai/src/app.rs`;
  3. look at the PNG and compare it with the reference screenshots. Scratch PNGs stay out of git; only Task 5 commits screenshots.

## Review Focus

1. **Light mode** (`theme = "light"`): everything new stays readable. There is no backdrop, and no Aurora constant shows on a light surface. Task 2 adds `light_mode_has_no_backdrop`, and Task 5 takes a light screenshot.
2. **Narrow window and long names:** at 900 px, with a 60-character project name and 4 panes, the top bar truncates the project name and keeps every action button inside the window. Task 2 adds `top_bar_fits_narrow_window`.
3. **macOS title bar:** dragging from the top bar moves the window, double-click zooms, the traffic lights don't overlap the brand, and full screen leaves no gap. Checked by hand in Task 5.
4. **Redraw cost of the backdrop:** big blurred shadows must not push streaming CPU past 10% or idle past 0.5%. Measured in Task 5 against Phase 4.
5. **Windows build:** the non-macOS `cfg` branch (native title bar, plain top-bar row) compiles and has no `TitleBar`. Check it by reading the code, plus `cargo check -p wai --target x86_64-pc-windows-msvc` if that target is installed. CI covers the rest.

---

### Task 1: Aurora theme, display font and terminal palette

**Files:**
- Create: `crates/wai/assets/theme/aurora.json`. This is a gpui-component `ThemeSet` (same shape as `~/.cargo/registry/src/*/gpui-component-0.9.0/src/theme/default-theme.json`) with one theme: `"name": "wai Aurora"`, `"mode": "dark"`, `"radius": 10`, `"radius.lg": 16`.
- Create: `crates/wai/assets/fonts/Nunito-Bold.ttf` and `crates/wai/assets/fonts/OFL.txt`. Take both from the upstream Nunito project (`googlefonts/nunito`, a static `Nunito-Bold.ttf` instance) at a pinned commit. Record the URL and commit in the commit message.
- Create: `crates/wai/src/aurora.rs`.
- Modify: `crates/wai/src/main.rs` or `lib.rs` (`mod aurora;`), `crates/wai/src/app.rs:31-42` (`init`).
- Modify: `crates/wai-terminal/src/lib.rs:97-120` (`DEFAULT_FG`, `DEFAULT_BG`, `ANSI16`).
- Modify: `README.md` "Third-party" or licence note, with one line: Nunito, OFL 1.1, bundled.

**Interfaces:**
- **Produces, in `aurora.rs`:**
  - `pub(crate) const DISPLAY_FONT: &str = "Nunito";`
  - `pub(crate) fn register(cx: &mut App)`. It adds the font bytes with `cx.text_system().add_fonts(...)`, loads `aurora.json` into `ThemeRegistry::global_mut(cx)`, and sets the global theme's `dark_theme` to it. `app::init` calls it right after `gpui_kit::init(cx)` and before `Theme::change`.
  - Palette functions, each returning `Hsla`, with the values from the spec table: `noite()`, `noite_2()`, `noite_3()`, `boreal()`, `lilas()`, `magenta()`, `ceu()`, `brasa()`, `ok()`, `ambar()`, `erro()`, `tinta()`, `tinta_2()`, `nevoa()`, `nevoa_2()`, `vidro()`, `vidro_forte()`, `borda()`, `borda_forte()`, `card_fill()` (`rgba(8,11,28,0.84)`).
- **Theme JSON mapping.** Every other key is the builder's choice, within the palette.

  | Theme key | Value |
  |---|---|
  | `background` | noite |
  | `foreground` | tinta |
  | `muted.foreground` | nevoa |
  | `muted.background` | `#0a0e24` (code blocks) |
  | `border`, `input.border` | borda |
  | `primary.background` | boreal |
  | `primary.foreground` | `#0b1024` |
  | `ring` | boreal |
  | `selection.background` | lilas at 35% |
  | `accent.background`, `list.active.background`, `sidebar.accent.background` | lilas at 16% |
  | `secondary.background` | vidro-forte |
  | `popover.background` | noite-2 |
  | `sidebar.background`, `title_bar.background`, `tab_bar.background` | transparent |
  | `title_bar.border` | transparent |
  | `info.background` | ceu |
  | `success.background` | ok |
  | `warning.background` | ambar |
  | `danger.background` | erro |
  | `base.cyan` | boreal |
  | `link.foreground` | ceu |
  | `caret` | boreal |
  | `drag_border` | lilas |

  The `highlight` section: editor background transparent; syntax colors as in the spec's "Left to the builder".
- **Terminal palette:**
  - `DEFAULT_FG = #d9def5`;
  - `DEFAULT_BG = #0a0e24`;
  - ANSI 0–7: `#1b2147 #ff6b8b #46e39a #ffc15a #5bb8ff #ff6ec7 #36e2c0 #c4cbeb`;
  - ANSI 8–15: `#626c98 #ff8fa6 #7af0b8 #ffd38a #8fd0ff #ff9ad9 #7ff0d8 #eceffe`.

- [ ] **Step 1: Write the failing tests** in `aurora.rs` `#[cfg(test)]`, following the existing `#[gpui_kit::test]` test style:
  - `aurora_is_the_default_dark_theme`: after `app::init(&Settings::default(), cx)`, assert:
    - `cx.theme().theme_name() == "wai Aurora"`;
    - `cx.theme().background` equals `noite()`;
    - `cx.theme().primary` equals `boreal()`.

    Compare colors with a helper `#[cfg(test)] pub(crate) fn hex(c: Hsla) -> u32` in `aurora.rs` (later tasks reuse it) that goes through `Rgba` rounded to u8, because the JSON parse and the constants may differ by float rounding.
  - `light_theme_stays_default`: with `settings.theme = ThemeChoice::Light`, assert the name is `"Default Light"`.
  - `display_font_is_registered`: assert `cx.text_system().all_font_names()` contains `"Nunito"`.
- [ ] **Step 2: Run** `cargo test -p wai aurora`. Expected: FAIL, because the module is empty.
- [ ] **Step 3: Implement** `register`, the palette functions, `aurora.json`, the font files, the `app::init` call and the terminal constants.
- [ ] **Step 4: Run** `cargo test -p wai aurora`, then `cargo test --workspace`. Expected: the 3 new tests pass, plus all existing ones.
- [ ] **Step 5: Visual check**, using the procedure in Global Constraints with the default scripted shot (`WAI_SHOT` alone).
  - The window must be indigo with teal primary buttons, and the terminal navy with the new colors.
  - Then run with `theme = "light"`: it must look as before.
- [ ] **Step 6: Commit:** `feat: add aurora theme, display font and terminal palette`.

### Task 2: Backdrop, glass cards and top bar

**Files:**
- Modify: `crates/wai/src/aurora.rs`, adding the helpers below.
- Create: `crates/wai/src/topbar.rs`, containing `impl Workspace { pub(crate) fn render_top_bar(&self, cx: &mut Context<Workspace>) -> impl IntoElement }` and `status_label`.
- Modify: `crates/wai/src/workspace.rs:1580-1660` (`render`).
- Modify: `crates/wai/src/sidebar.rs:166-243`. The project name row and the three header buttons move to the top bar, with the same ids, tooltips and handlers. The tabs row stays.
- Modify: `crates/wai/src/app.rs:124-131` (`WindowOptions`): on macOS only, `titlebar: Some(TitleBar::title_bar_options())`.

**Interfaces:**
- **Consumes:** Task 1's palette functions and `DISPLAY_FONT`.
- **Produces, in `aurora.rs`:**
  - `pub(crate) fn backdrop(cx: &App) -> Option<AnyElement>`:
    - returns `None` in light mode;
    - in dark mode, an absolutely positioned full-size element with `debug_selector("aurora-backdrop")`;
    - its base is a 180° `linear_gradient` from `#060816` to `#0a0e24`;
    - three blobs, each a div painted only by a large blurred `BoxShadow`:
      - boreal at 30%, top-left;
      - lilas at 34%, top-right;
      - magenta at 20%, bottom-center.

      Positions and sizes follow `.aurora-1..3` in `app.css`. Fall back to one PNG (≤ 300 KB) only if the shadows band or clip.
  - `pub(crate) fn card(focused: bool, cx: &App) -> Div`:
    - in dark mode: `card_fill()` background, radius 16, 1 px `borda()` border, shadows `0 40 80 -40 black .95` and `0 0 60 -30 lilas .45`, `overflow_hidden`;
    - `focused` makes the border `lilas()` at 50% and the glow stronger;
    - in light mode: `theme.background` with `theme.border` and radius 12.
  - `pub(crate) fn card_bar(cx: &App) -> Div`: the header strip. It has the white 5%→1.5% top gradient and a white 6% bottom line, or in light mode `theme.border`.
  - `pub(crate) fn pill(cx: &App) -> Div`: fully rounded, `vidro()` fill, `borda()` border, `px 10`, height 24–30; in light mode `theme.secondary` and `theme.border`.
- **Produces, in `topbar.rs`:**
  - `pub(crate) fn status_label(working: usize, needs_you: usize) -> String`.
  - Debug selectors `topbar`, `topbar-project` and `topbar-status`.
- **Layout:**
  - The root is a relative container: `backdrop` first, then a column with the top bar and the existing `h_resizable` tree.
  - Every resizable panel's content is wrapped in `card(..)`, with a 4 px inset so the cards sit 8 px apart and 8 px from the window edges.
  - A pane's card is `focused` when `split && ix == focused_pane`. This replaces today's `border_t_2` blue line.
  - The resize handle's 1 px line must not show at rest.
- **Top bar** (dark: transparent over the backdrop):
  - the left side holds, in order:
    1. the brand: `crates/wai/assets/icon/icon.png` embedded and drawn at 22 px with radius 6, then "wai" in `DISPLAY_FONT` at 15 px bold;
    2. the project chip: a `pill` with a folder icon and `folder_name(&project)`, truncated;
    3. the status pill: a 7 px dot that is `boreal()` when any session is `Working`, else `nevoa_2()`, with a small glow, then `status_label`. The " · N needs you" part is in `ambar()`.
  - the right side, as ghost or pill buttons:
    1. templates (`sidebar-templates`);
    2. extensions (`sidebar-extensions`);
    3. palette (`topbar-palette`, which dispatches `OpenPalette`);
    4. terminal (`topbar-terminal`, which dispatches `ToggleTerminal`);
    5. "New session" (`sidebar-new`, primary, with a `+` icon).
  - On macOS it is wrapped in `TitleBar::new()`. Elsewhere it is a plain `h_flex` 40 px tall.

- [ ] **Step 1: Write the failing tests:**
  - `status_label_counts`, a unit test in `topbar.rs`:
    - `(0,0)` → `"No agent working"`;
    - `(1,0)` → `"1 agent working"`;
    - `(3,1)` → `"3 agents working · 1 needs you"`;
    - `(0,2)` → `"No agent working · 2 need you"`.
  - `light_mode_has_no_backdrop`, in the workspace tests: open with `ThemeChoice::Light`, then `cx.debug_bounds("aurora-backdrop")` is `None`. With the default settings it is `Some`.
  - `top_bar_fits_narrow_window`:
    1. resize the test window to 900×700;
    2. set the project to a temp directory whose name is 60 characters;
    3. open 4 panes;
    4. assert the `debug_bounds("sidebar-new")` right edge ≤ 900, and the `topbar-project` right edge < the `topbar-status` left edge.
- [ ] **Step 2: Run** `cargo test -p wai -- status_label light_mode_has_no_backdrop top_bar_fits`. Expected: FAIL.
- [ ] **Step 3: Implement** the helpers, `topbar.rs`, the layout, the macOS title bar and the moved buttons.
- [ ] **Step 4: Run** `cargo test --workspace`. Expected: all pass, including the existing sidebar and palette tests that click the moved ids.
- [ ] **Step 5: Visual check** of the default shot and `WAI_SHOT_SIDE=1` with sessions. Look for:
  - the aurora behind the cards;
  - 8 px gaps and no divider lines;
  - the top bar showing brand, project and status;
  - the cards' glow.
- [ ] **Step 6: Commit:** `feat: add aurora backdrop, glass cards and top bar`.

### Task 3: Sidebar, editor tabs and empty pane

**Files:**
- Modify: `crates/wai/src/sidebar.rs`: the tabs row, `open_row`, `history_row`, `heading`, `row`, `status_color`.
- Modify: `crates/wai/src/editor/pane.rs:566-628`, the tab bar `render`.
- Modify: `crates/wai/src/workspace.rs:1541-1564`, `empty_pane`.

**Interfaces:**
- **Consumes:** Task 1's palette functions and Task 2's `card_bar` and `pill`.
- **Produces:** `status_color(&TurnState::Working, theme)` returns `theme.cyan`, which is boreal under Aurora. The other states are unchanged: `warning`, `muted_foreground`, `danger`.
- **Sidebar tabs:** a segmented control inside the sidebar card.
  - The container is radius 12 with a `vidro()` fill, a `borda()` border and 3 px padding.
  - The selected tab gets a lilas 16% fill, a lilas 45% border and `tinta()` text. The others are `nevoa()`, with a `vidro()` hover.
  - Extension panel tabs join it, and it wraps when it runs out of width.
- **Rows:** `row()` gets radius 10. The selected row gets a lilas 16% fill with a lilas 45% border. Status dots are 8 px with a glow in their own color.
- **Headings:** `heading()` ("Open", "History") is uppercase, 11 px, semibold, `nevoa_2()`.
- **Editor tab bar:** uses `card_bar`, with tabs per `.janela-aba`:
  - height 30, radius 10;
  - inactive tabs are `nevoa()` with a `vidro()` hover;
  - the active tab gets a lilas 16% fill, a lilas 45% border and `tinta()`;
  - the close × appears on every tab, as today.
- **Empty pane**, a centered column:
  1. the brand icon at 48 px;
  2. "Start a session" in `DISPLAY_FONT` at 20 px;
  3. the existing `START_HINT` text in `nevoa()`;
  4. the primary `new-session` button;
  5. the agent problem text in `danger`, as today.

- [ ] **Step 1: Write the failing test** `working_dot_is_boreal` in `sidebar.rs`: after `app::init` with the default settings, `status_color(&TurnState::Working, cx.theme())` equals `aurora::boreal()`, compared with Task 1's `hex` helper.
- [ ] **Step 2: Run** `cargo test -p wai working_dot_is_boreal`. Expected: FAIL, because it returns `theme.blue` today.
- [ ] **Step 3: Implement** the styling above.
- [ ] **Step 4: Run** `cargo test --workspace`. Expected: all pass.
- [ ] **Step 5: Visual check** of `WAI_SHOT_SIDE=1`, `WAI_SHOT_FILES=src` and `WAI_SHOT_OPEN=<two files>`, plus a run with no session, which shows the empty pane.
- [ ] **Step 6: Commit:** `feat: restyle sidebar, editor tabs and empty pane`.

### Task 4: Chat

**Files:**
- Modify: `crates/wai/src/chat/render.rs`: `render_header`, `render_item`, `render_tool`, `render`.
- Modify: `crates/wai/src/chat/input.rs:132-170`, `render_prompt`.
- Modify: `crates/wai/src/chat/permission.rs` (`card`), `crates/wai/src/chat/question.rs` (`card`) and the `edits.rs` block frame.

**Interfaces:**
- **Consumes:** Task 1's palette functions and `DISPLAY_FONT`; Task 2's `card_bar` and `pill`.
- **Produces:** `pub(crate) fn starts_reply(items: &[ChatItem], ix: usize) -> bool` in `render.rs`. It is true when `items[ix]` is `ChatItem::Agent` and no other `Agent` item comes between it and the last `User` item before it, or the start of the list.

**What changes:**
- **Header:** uses `card_bar`; the title is in `DISPLAY_FONT` at 14 px; the usage label becomes a `pill`.
- **User message:**
  - aligned right, `max_w(relative(0.88))`;
  - fill is a linear gradient at 160°, lilas 32% to 18%; 1 px border, lilas 35%;
  - radius 16, with the bottom-right corner at 6.
- **Agent text:**
  - full width; `vidro()` fill, `borda()` border, radius 16 with the bottom-left corner at 6, padding 12;
  - when `starts_reply` is true, a header line "✦ {agent}" goes above it: the star in `brasa()`, the name in `#ffc6a3`, 11 px semibold. The agent name is `ChatSession.agent`.
- **Thought:** unchanged, muted.
- **Tool rows and edits blocks:** radius 12, `vidro()` fill, `borda()` border.
- **Notices:** a centered `pill` with 11.5 px `nevoa_2()` text. The error kind uses `erro()` text, an erro 12% fill and an erro 40% border. The restart button is kept.
- **Empty list:** a centered `pill` reading "No messages yet."
- **Permission card:**
  - radius 18, 1 px border boreal 55%, fill gradient from boreal 8% to transparent, glow `0 0 40 -12 boreal .5`;
  - eyebrow "PERMISSION REQUESTED" in `brasa()` at 11 px semibold;
  - the title in `DISPLAY_FONT` at 16 px;
  - the diff or command block on a `#0a0e24` fill with radius 12;
  - each choice is a full-width row with radius 12, a `vidro()` fill and a `borda()` border. Each row starts with a 26 px badge holding the choice's number: radius 8, brasa 18% fill, `#ffb68a` text.
  - the first allow-kind row is highlighted with a lilas 16% fill and a lilas 45% border; reject-kind rows have `erro()` text;
  - rows keep the ids from `button_id()` and stay clickable as before; the answered and cancelled states are kept.
- **Question card:** the same frame with a lilas border and glow, the eyebrow "QUESTION", and `question-submit` as the primary button.
- **Prompt box:**
  - no top border line; a fully rounded container (radius 24) with a `vidro()` fill, a `borda_forte()` border and padding `6 6 6 14`;
  - the `Textarea` is drawn without its own border or fill;
  - send (`send`) is a circular 36 px button with a `boreal()` fill and an arrow-up icon in `#0b1024`. Disabled, it has a `vidro()` fill and a `nevoa()` icon;
  - stop (`stop`) is circular 36 px with an erro 18% fill, an erro 50% border and a stop icon;
  - tooltips "Send" and "Stop"; attachment chips sit above the box as today.

- [ ] **Step 1: Write the failing test** `reply_header_once_per_turn`, a unit test on `starts_reply`:
  - `[User, Agent, Tool, Agent]` → ix 1 true, ix 3 false;
  - `[Agent]` → ix 0 true;
  - `[User, Thought, Agent]` → ix 2 true, ix 1 false;
  - `[User, Agent, User, Agent]` → ix 3 true.
- [ ] **Step 2: Run** `cargo test -p wai reply_header_once_per_turn`. Expected: FAIL.
- [ ] **Step 3: Implement** the styling above and `starts_reply`.
- [ ] **Step 4: Run** `cargo test --workspace`. Expected: all pass. The existing permission, question, send and stop tests must pass unchanged.
- [ ] **Step 5: Visual check** of `WAI_SHOT_PROMPT=…` with the fake agent run with `--ask-permission --reply-file <markdown with a list, code block and table>`, of a question card (the fake agent's question mode, see `fase-2-question.png` in `app.rs`), and of an idle empty session. Compare with `chat-celular.png` and `permissao-celular.png`.
- [ ] **Step 6: Commit:** `feat: restyle chat in the aurora look`.

### Task 5: Sweep, screenshots, measurements and docs

**Files:**
- Modify only if the sweep finds hardcoded looks: `crates/wai/src/{palette,search_view,files,agents_view,extensions_view,ext_panel}.rs`.
- Create: `docs/screens/aurora-*.png`.
- Create: `docs/spikes/aurora-medicoes.md`, in pt-BR.
- Modify: `README.md` (hero screenshot), `docs/user-guide.md` (one paragraph on the look and the light option), `docs/RELATORIO-final.md` (a short "Revisão visual" section and the new rulings).

- [ ] **Step 1: Sweep.** Take screenshots of:
  - the palette (`WAI_SHOT_PALETTE=1`);
  - the consent dialog (`WAI_SHOT_CONSENT`);
  - an extension panel (`WAI_SHOT_PANEL=1` with `wai-waycarbon` and a fake `jira`);
  - the Agents tab (`WAI_SHOT_AGENTS=1`);
  - Search;
  - the template menu (`WAI_SHOT_TEMPLATES=1`);
  - a notification.

  Fix anything that ignores the theme. Rows use `row()`, surfaces use `card`, `pill` or the theme tokens. Commit fixes as `fix: align <view> with aurora theme`.
- [ ] **Step 2: Screenshots.** Commit scratch-free, privacy-checked PNGs:
  - `aurora-chat.png`, with a permission card;
  - `aurora-editor.png`, with two files and the terminal;
  - `aurora-panel.png`, with the "My cards" panel;
  - `aurora-palette.png`;
  - `aurora-light.png`.

  Update the README hero image to `aurora-chat.png`.
- [ ] **Step 3: Measure.** Build release with `demo` and use the Phase 3/4 method (`docs/spikes/fase-4-medicoes.md`):
  - idle CPU over 30 s and RAM, with 1 project, 3 sessions and the terminal;
  - streaming CPU at 40 tokens/s with the fake agent;
  - window-visible time, through the `.app` from `CI=true cargo packager --release`.

  Write `docs/spikes/aurora-medicoes.md` with a table against Phase 4. All targets must hold. If streaming exceeds 10%, see the backdrop fallback in Task 2.
- [ ] **Step 4: Manual title-bar check on macOS** (Review Focus 3): run the `.app` with a scratch `HOME` and check:
  - dragging from the top bar moves the window;
  - double-click zooms;
  - the traffic lights clear the brand;
  - full screen leaves no gap.

  Note the results in the measurements doc.
- [ ] **Step 5: Docs.** Add one user-guide paragraph: "the default dark theme is Aurora; set `theme = "light"` for the light one". Add the RELATORIO "Revisão visual" section (pt-BR), with rulings from R73 on.
- [ ] **Step 6: Run** `cargo test --workspace` and `go test ./...` in both Go extensions. Expected: all pass.
- [ ] **Step 7: Commit:** `docs: add aurora screenshots and measurements`.

### Task 6: Close button on session panes and a collapsible History

Owner request (2026-11-09): "Quando splitamos as sessões não tem como fechar elas, deveria ter como; as sessões na lateral deveria ter uma separação pois fica muito poluído com várias sessões (histórico) — coloque uma aba colapsável chamada histórico e deixa isso dentro dela."

**Files:**
- Modify: `crates/wai/src/chat/render.rs` (`render_header`), `crates/wai/src/workspace.rs` (`close_session`/`drop_pane`, `empty_pane`, and whatever wiring the close needs), `crates/wai/src/sidebar.rs` (`render_sessions`, history section).
- Tests next to the existing ones (workspace/chat view tests, sidebar tests).

**Behaviour:**
1. **Close a session pane.**
   - Every session pane's header (the `card_bar` in `render_header`) gets a ghost xsmall icon button, id `session-close`, `IconName::Close`, tooltip and accessibility label "Close session".
   - Clicking it does exactly what ⌘W (`CloseSession`) does today, for the pane it sits in. That pane need not be focused: the session stops and leaves the Open list (it stays in the agent's history, resumable), and the pane is removed. If it is the only pane, the pane becomes the empty pane.
   - It must close the clicked pane, not the focused one. Test this with 2 panes, clicking × on the unfocused one.
   - An empty pane in a split (more than one pane) gets a small × at its top-right, id `pane-close`, tooltip and accessibility label "Close pane", which drops that pane. A lone empty pane shows no ×.
   - The editor tabs keep their own per-tab ×. No change there.
2. **Collapsible History.**
   - In the Sessions tab, the History section becomes collapsible, and it starts collapsed.
   - Its header row has a chevron (right when collapsed, down when open) and the uppercase "HISTORY" heading followed by the count of history rows, e.g. "HISTORY · 12". The whole row toggles. It is keyboard reachable (tab_index plus role/label like the sidebar tabs).
   - Collapsed: no history rows, and the "All projects" switch is hidden.
   - Expanded: exactly as today (switch plus rows).
   - The Open section is unchanged and always visible.
   - The collapsed/expanded state lives in the Workspace in memory, is not persisted, and starts collapsed at every launch.
   - Keep the history row ids. Tests that click history rows must expand History first; the behaviour change is intentional.

**Copy:** only "Close session", "Close pane" and the count. "History" already exists. The UI stays English.

**Style:** use the existing aurora helpers (`aurora::tab`, `heading`, `card_bar`, `pill`). Light mode uses theme tokens only. No animation.

**Tests (TDD):**
- `close_button_closes_its_own_pane`: 2 session panes, focus pane 0, click `session-close` in pane 1. Expected: 1 pane left, holding pane 0's session, and pane 1's session no longer in the Open list.
- `lone_empty_pane_has_no_close`: no `pane-close` in a single empty pane; after a split with an empty pane, `pane-close` drops it.
- `history_starts_collapsed_and_toggles`: with history rows, collapsed shows no history rows (debug selector or id lookup) and no "All projects" switch; clicking the header shows them.

**Commit:** `feat: close session panes and collapse history`.

### Task 7: Drag sessions to arrange the panes

Owner request (2026-11-09): "poder arrastar as sessões e organizar elas na tela da forma que eu achar melhor, inclusive arrastar ela do menu lateral para a tela de sessão."

**Files:**
- Modify: `crates/wai/src/workspace.rs` (pane layout, drop handling, save_state), `crates/wai/src/sidebar.rs` (drag sources on rows), `crates/wai/src/chat/render.rs` (drag handle on the session header).
- A new small module is allowed for the pure placement logic, e.g. `crates/wai/src/layout.rs`.

**Behaviour:**
1. **Drag sources.** Use GPUI drag and drop (`on_drag` with a typed payload, `on_drop`/`drag_over`). Three kinds:
   - a sidebar **Open** row (an open session, whether shown in a pane or not);
   - a sidebar **History** row (a session not open);
   - a session pane's **header** (the title area of its `card_bar`; buttons such as `session-close` and the mode select keep their clicks).

   The drag preview is a small `aurora::pill` with the session title.
2. **Drop targets.** Each pane has three zones:
   - **left 25%:** insert a new pane to the left of it;
   - **right 25%:** insert a new pane to the right of it;
   - **center:** show the session in this pane.

   While a drag hovers a zone, highlight it: a lilas 16% fill with a lilas 45% border in dark mode, theme `accent` and `ring` in light mode. No animation.
3. **Placement rules.** Keep these in a pure function and unit-test them.
   - **Open session already shown in pane A, dropped on pane B:**
     - center: swap the panes' contents, so B's previous content goes to A;
     - left/right: move it to a new pane beside B, and remove A if A is left empty and more than one pane remains.
   - **Open session not shown in any pane, or a History row:**
     - center: it replaces B's content. A session B was showing stays in the Open list with no pane, never stopped.
     - left/right: a new pane beside B.
     - A History row resumes, exactly like clicking it today, but into the chosen place.
   - **Limits:**
     - With 4 panes already (the current maximum), a left/right drop acts as center.
     - Dropping a session on its own pane's center is a no-op.
     - Editor panes are valid targets; a center drop replaces the editor pane's content only if it has no unsaved tabs. Otherwise act as a right drop (or left if right is impossible).
4. After any change, save the layout (`save_state`) and focus the pane that received the session.
5. The panes stay a single horizontal row (no vertical splits).

**Copy:** none new.

**Tests (TDD):**
- **Unit tests on the pure placement function:** swap, move-to-edge with source removal, the 4-pane cap, a self-drop no-op, a History drop into the center keeping the displaced session open, and an editor with unsaved tabs never replaced.
- **Drag integration:** if GPUI's test context can simulate a drag (mouse down, move with button pressed, mouse up over a target), add `drag_open_row_to_right_edge_splits`. If it can't, say so in the report and rely on the unit tests plus a screenshot.

**Commit:** `feat: drag sessions to arrange panes`.
