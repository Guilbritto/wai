# wai: personalization (shortcuts, fonts, themes)

> 2026-11-12. Item 3.8 of `docs/superpowers/plans/2026-11-12-wai-execucao-pendencias.md`, widened by the owner from "customizable shortcuts" to shortcuts, fonts and themes. Choices marked **(owner)** are the owner's; **(call)** are mine, listed again under "My calls".

## Intent

Owner, verbatim: *"A IDE tem que ter a possibilidade de trocar os atalhos para melhor adequar ao seu usuário, é uma questão de customização. Outra coisa que hoje não temos é a parte de customização de fonte, tamanho, família etc."* And: *"eu consigo criar um tema e instalar como se fosse uma extensão?"*

For the owner and his Mac colleagues. Rule the owner set for these choices: *look at how other IDEs do it and do the same*, Zed first (same UI engine and agent protocol as wai), then VS Code.

## Goals

1. **Shortcuts:** any wai action can be rebound or unbound in `~/.config/wai/keymap.toml`; the change applies on save, without a restart. Settings lists the shortcuts in effect and has **Edit shortcuts**, which opens that file in wai's editor. **(owner: "file + list on screen", how Zed started)**
2. **Fonts, as Zed:** **(owner)**
   - interface: family and size (sidebar, menus, Settings, top bar);
   - code: family and size (editor, code blocks and diffs in the chat);
   - terminal: family and size, each falling back to the code one when unset;
   - chat: its own size (Zed's agent panel font size), interface family;
   - ⌘+ / ⌘- / ⌘0 make the text bigger, smaller, back to default.
3. **Themes:** a theme is a JSON file in wai's theme format, the one `crates/wai/assets/theme/aurora.json` uses (gpui-component theme set). An extension ships it through `contributes.themes`, and it shows up in Settings > Appearance. **(owner: wai JSON, no VS Code import)**
4. **Theme mode, as Zed and VS Code:** the user picks a light theme, a dark theme and a mode (System, Light, Dark). System follows macOS live when its appearance changes. **(owner)**
5. **A theme recolors the whole window.** Today ~170 call sites paint with fixed Aurora colors (`aurora::boreal()`, `aurora::noite()`, …) against ~112 that read the theme; those fixed colors become theme colors. **(call, needed for goal 3 to mean anything)**

## Non-goals

- A screen that records a key press (VS Code's and Zed's current keymap editor). Later, if the file proves clumsy.
- Importing VS Code (or Zed) themes.
- Shortcuts or fonts from a project's `.wai/settings.toml`: they are personal. A project may still pick the theme, as it can today (item 3.10).
- Extensions contributing shortcuts, icon themes, per-language fonts, font ligature switches.
- A theme editor.

## Anti-goals (a failure even if it works)

- Idle CPU above 0.5 %: no polling of `keymap.toml`, of fonts or of the system appearance. The user folder is already watched (`workspace.rs` `watch_settings`); macOS appearance changes arrive as an event.
- A broken `keymap.toml`, theme or font name breaking wai or dropping the default shortcuts. A bad entry is skipped with a notice; everything else applies.
- After the color conversion, wai Aurora looking different from today.
- A theme that leaves parts of the window in Aurora colors.
- Text that ignores the size settings (a fixed `px(13.)` left in the chat, say).

## Constraints

- Settings stay TOML (`~/.config/wai/settings.toml`), applied live through `apply_settings` (`workspace.rs`).
- Key syntax and action names are the ones GPUI already uses (`secondary-shift-p`, `wai::NewSession`, contexts `Workspace`, `Terminal`, `ChatSession`, …); `bind_keys` in `crates/wai/src/app.rs` is today's default keymap.
- Off macOS, `terminal_ctrl_keymap` hides ctrl keys from the app inside the terminal. User bindings must not undo that silently.
- An extension that only ships a theme is manifest-only: no `process`, no permission.

## Approaches

| | What | Trade-off |
|---|---|---|
| **A (picked)** | Two tracks. **Shortcuts** alone (keymap file, Settings list, reload, conflicts). **Appearance** in one pass: theme colors, fonts and sizes, theme mode, themes from extensions. | Colors and sizes live in the same ~270 render call sites, so one pass touches each once. Shortcuts share nothing with them. |
| B | Three tracks: shortcuts, fonts, themes. | Fonts and themes edit the same lines twice; more conflicts in `workspace.rs`, `chat/render.rs`, `sidebar.rs`. |
| C | Ship extension themes first without converting the fixed colors. | Breaks the "whole window" anti-goal. Rejected. |

Order: shortcuts first (small, independent), then appearance. One track at a time (owner's global rule). **(call; the owner left the how to me)**

## Current state (facts to build on)

- `app.rs:92` `bind_keys`: ~19 bindings plus `terminal_ctrl_keymap` off macOS, loaded with `KeyBinding::load` (which accepts `secondary-`). Actions are declared with `actions!` in `app.rs`; other views declare their own (chat, sidebar, palette).
- `docs/user-guide.md` "Keyboard shortcuts" is the hand-written table of the defaults.
- Settings: `ThemeChoice { Dark, Light }` (`wai-settings/src/lib.rs:132`); `app.rs` `theme_mode` maps it to gpui-component `ThemeMode`; dark is "wai Aurora" from `aurora.json`, light is gpui-component's Default Light.
- gpui-component's theme already has `font_family`, `font_size`, `mono_font_family`, `mono_font_size` (`gpui-component-0.9.0/src/theme/mod.rs:170-187`) and its JSON schema ignores unknown keys (`aurora.json` carries `_ref`).
- `ThemeRegistry::load_themes_from_str` loads a theme set from a string (used by `aurora::register`).
- `wai-ext` manifest already parses `contributes.themes: Vec<String>`; `docs/extensions.md` says "Accepted, but not used in this version".
- Nunito Bold is bundled as `DISPLAY_FONT` for the brand and titles.
- Fixed text sizes: ~104 call sites (`text_size(px(..))`, `text_sm()`…), e.g. `chat/render.rs:228` `px(13.)`.

## My calls (not confirmed by the owner; check them while reading)

1. **`keymap.toml` uses Zed's structure in TOML:** a list of sections, each with an optional `context` and a `bindings` table of `key = "action"`. An empty string unbinds (Zed uses `null`, TOML has none).
   ```toml
   [[section]]
   bindings = { "secondary-shift-p" = "wai::OpenPalette" }

   [[section]]
   context = "Terminal"
   bindings = { "secondary-k" = "" }
   ```
   User sections load after the defaults, so they win (GPUI: later bindings win).
2. **Edit shortcuts** creates `keymap.toml` when missing, with a commented header listing every action name and context, then opens it in wai's editor.
3. **Settings > Keyboard shortcuts** is a new section: action, keys, where (context), with the ones the user changed marked. Read-only.
4. **Conflicts:** two bindings for the same keys in the same context, or an unknown action or key, give one notice naming the line; the rest applies.
5. **Settings keys for fonts:**
   ```toml
   [fonts]
   ui_family = "…"      # unset: today's interface font
   ui_size = 14
   code_family = "…"    # unset: today's mono font
   code_size = 13
   chat_size = 14
   [terminal]
   font_family = "…"    # unset: code_family
   font_size = 13       # unset: code_size
   ```
   Defaults reproduce today's sizes. Each size is clamped to 6–100 px, as Zed and VS Code do. Settings > Appearance gets a family picker (installed families) and a size field for each.
6. **⌘+ / ⌘- / ⌘0 are a zoom level, as VS Code's `window.zoomLevel`:** `[fonts] zoom` (integer steps, default 0, clamped to −4…+8) adds one pixel per step to every size at once. ⌘+/⌘- move it and write it to `settings.toml`, so it survives a restart; ⌘0 sets it back to 0 and leaves the four configured sizes alone. The write skips the re-apply its own change would trigger from the watcher.
7. **Every fixed text size becomes relative to one of the four sizes** (interface, chat, code, terminal), keeping today's proportions: a site that is `px(13.)` in the chat today becomes `chat_size × 13/14`, rounded. Rows and boxes whose height comes from their text grow with it; icon sizes and fixed chrome (title bar height, pane dividers) do not.
8. **Settings keys for the theme, as Zed:**
   ```toml
   [theme]
   mode = "system"          # system | light | dark
   light = "wai Light"
   dark = "wai Aurora"
   ```
   Today's `theme = "dark" | "light"` keeps working: it means `mode` when `[theme] mode` is absent, and is ignored when both are present. A project's `.wai/settings.toml` may set the same keys, merged key by key over the user's like every other setting (a project that sets only `dark` changes the dark theme, not the mode).
9. **wai's own colors** (the Aurora ones gpui-component has no slot for) live in each theme object of the same JSON, under a `"wai"` object next to `"colors"` (gpui-component ignores unknown keys, as it does `_ref`). wai reads them with a second parse of the file, keyed by theme name. A theme that leaves one out gets a fallback derived from its base colors; a file that is not valid JSON or not a theme set is refused with a notice and its themes do not appear.
10. **The light theme becomes "wai Light"**, a bundled theme with values for wai's own colors, starting from Default Light.
11. **Theme files:** `contributes.themes` lists paths relative to the extension folder. Each file is a theme set; all its themes show in the pickers. They load at startup and on install/enable from the manifests of enabled extensions; no process is involved. A theme with the same name as a bundled one is refused with a notice. When the selected theme disappears (extension disabled or removed), wai uses the bundled one for that mode and says so once.
12. **Aurora unchanged means the same values:** each fixed color becomes a theme key whose value in `aurora.json` is today's exact color and alpha (`boreal().alpha(0.30)` keeps 0.30, as its own key or as base key plus alpha at the call site). A unit test compares every converted key against today's constants; before/after screenshots of the chat, sidebar and Settings are compared by eye.
13. **Off macOS**, user bindings apply as written, also in the `Terminal` context; `terminal_ctrl_keymap` stays the default under them. The `keymap.toml` header and the user guide say that binding a ctrl key in `Terminal` takes it from the shell.

## Left to the builder

- The exact names of wai's extra color keys and how the Aurora glow and glass map onto them.
- Font family picker details (search, preview).
- How the shortcut list reads action names for display.
- Tests: keymap parsing and conflict rules; settings parsing of `[fonts]` and `[theme]`; a screenshot comparison proving Aurora is unchanged after the color conversion.
- Docs: `docs/user-guide.md` (Keyboard shortcuts, Appearance), `docs/extensions.md` (`contributes.themes` and a theme-only example).
