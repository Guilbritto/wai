# wai "Campo": one visual language for every control

Status: approved by the owner on 2026-11-16 ("segue com a implementação até o final"). Mockups, in the order they were decided, are in
`.superpowers/brainstorm/33691-1794880346/content/`:

- `input-select-redesign.html` picked "A · Campo";
- `all-components-redesign.html` covers every extension-panel node;
- `app-wide-redesign.html` covers switch, radio, number, dialog, Settings, the chat question, and light
  mode;
- `path-picker-redesign.html` covers the path field and attachments.

The mockups are the visual reference. Where this document and a mockup disagree, this document wins.

## Intent

It started with the owner not liking the look of input and select in extension panels. They picked a
recessed field, "A · Campo". Then they asked for every panel component to be redesigned in the same
line, and then for the same language across the whole app: "temos que fazer toda a aplicação estar com
os mesmos estilos".

Today wai mixes two looks:

- gpui-component defaults: a cyan focus ring, 10 px corners, a solid red danger button, controls of
  different heights;
- aurora's own pieces: the composer, cards, tabs.

The goal is that every control looks like it belongs to one app.

## Goals

- Every control in wai and in extension panels follows the rules below.
- The look lives in two places, the theme (`crates/wai/assets/theme/aurora.json`) and helpers in
  `crates/wai/src/aurora.rs`. Every call site goes through them, so a later change is one edit.
- Dark and light mode both follow the rules.

## Non-goals

- No change to behavior. The only new pieces are the path field, used for Settings → Shell, and the
  extension `path` node (API 1.6) that offers it to extensions.
- No new layout for any screen. Settings, the sidebar, the chat and the panels keep their structure;
  only the controls and section headers change.
- The composer's `@` mentions and images in order are a separate project; see
  `docs/NOTES-composer-mentions.md`.
- No patch to gpui-component.

## Anti-goals

- A half-migrated app where some screens have the old look. Every call site moves in the same release.
- A light mode that is unreadable or still shows the dark surfaces.
- A regression in focus, keyboard navigation, disabled states or accessibility labels.

## Constraints

- gpui_kit 0.9.0 (gpui-component 0.9.0) is used as published. The styling hooks available:
  - `Styled` on `Button`, `Input`, `Textarea` and `Checkbox`;
  - `.appearance(false)` on `Select`, `Textarea`, `Popover` and `HoverCard`;
  - the `appearance`, `bordered` and `focus_bordered` options on `Input`;
  - `ButtonCustomVariant` and `aurora::filled`;
  - `Tag::custom(color, foreground, border)`.
- Light mode today is gpui-component's "Default Light". aurora.json holds only the dark theme, which
  `aurora::register` installs as `dark_theme`. aurora.rs already branches on `theme.is_dark()` in about
  50 places.
- CI runs `cargo fmt --check`, `clippy -D warnings`, and the tests on macOS and Windows. One test
  asserts `theme.primary == aurora::boreal()`; primary stays cyan.
- The dev-only storybook extension (`docs/storybook`) renders every panel node type. It is how panels
  get checked.

## Approaches

1. **Theme plus aurora helpers. Chosen by the owner ("vamos de 1").**
   - Colors and focus that gpui-component reads from the theme change in aurora.json.
   - Shapes the theme can't express go into aurora.rs helpers: height, halo, tinted tones, the select
     chevron, the recessed off state.
   - Every call site uses the helpers.
2. **Patch or fork gpui-component's styles.** Rejected: every gpui_kit upgrade becomes a merge.
3. **Helpers only, no theme change.** Rejected: gpui-component paints focus rings, popovers, list
   selection and the caret from theme tokens, so cyan rings and the old popovers would stay.

## The rules

Values are for dark mode; light mode is described further down.

| Rule | Value |
|---|---|
| Height of every clickable control in a row (field, select, button, menu trigger) | 28 px |
| Control corners | 8 px |
| **Recessed** surface: input, multiline, select, number, path field, log, code; off checkbox, radio, switch | bg `#00000033` (log and code `#00000040`), border `#ffffff1a` |
| **Glass** surface: cards, list rows, table rows, default button | bg `#ffffff0b`, border `#ffffff1a`; hover `#ffffff17` |
| Focus, everywhere | border `#8d72ff73` plus a 3 px halo `#8d72ff33` |
| Chosen or on: select check, checkbox, radio, switch | cyan `#36e2c0` with a dark mark `#0b1024` |
| Filled: only the primary button | cyan `#36e2c0`, text `#0b1024` |
| Tones (badge, alert, danger button, failed attachment) | tinted: tone at about 15% as bg, the tone as text; alerts and danger also get a tone border at about 35–40% |
| Small labels: table headers, section headers | 10–10.5 px, weight 700, uppercase, letter-spacing .06em (dropped if GPUI can't space letters); table headers dim `#626c98`, section headers muted `#8f99c2` |
| Disabled | opacity .42 |
| Popover frame (select, menu, bar hover card) | bg `#0d1230` (the theme's `popover.background`), radius 9, padding 4, 26 px items |

Tone colors are the theme's: danger `#ff6b8b`, warning `#ffc15a`, success `#46e39a`, info `#5bb8ff`.

## Components

**Fields**
- **Input and multiline:** recessed, 28 px (multiline grows), radius 8, full width, placeholder dim
  `#626c98`, focus violet with halo.
- **Select:** a field showing the value, with a chevron on the right. Its menu is as wide as the field,
  in the popover frame, with a cyan check on the right of the chosen item.
- **Number:** the field with `−` and `+` segments joined on the right, each 26 px wide and set off by a
  hairline.
- **Input with an icon or shortcut** (sidebar filter, search): a leading icon in dim and a trailing
  `kbd` chip (10 px mono, border, radius 5).
- **Path field** (new): see below.

**Buttons**
- Default: glass, 12 px, weight 600.
- Primary: filled cyan.
- Danger: tinted red (`#ff6b8b26` bg, red text, `#ff6b8b55` border), no longer solid.
- Ghost: no bg or border.
- Icon: 28 px square.

**Toggles**
- **Checkbox:** 16 px, radius 5. Off: recessed with an inset `#ffffff38` ring. On: cyan with a dark
  check.
- **Radio:** 16 px circle, same family. On: cyan with a 6 px dark dot.
- **Switch:** 30×18 track (24×14 for the small one in History). Off: recessed with the inset ring and a
  muted knob. On: cyan with a dark knob.
- **Radio with a description** (the chat question form): each option is a loose glass row, fully
  clickable, and the chosen one turns violet (`#8d72ff29` bg, `#8d72ff73` border). An option without a
  description stays a bare radio and label.

**Tabs:** underline style, 28 px. The active tab has fg text and a 2 px violet bottom border; the row
has a hairline below.

**Lists and tables:** loose rows.
- Each row is a separate glass row with 4 px between rows, 30 px for table rows and 42 px for list
  rows, radius 9.
- Hover makes a row violet: bg `#8d72ff29`, border `#8d72ff73`.
- Table headers are small labels.

**Cards**
- Glass, radius 12.
- A clickable card gets a violet border and a right chevron on hover.
- The card icon sits in a 28 px recessed chip.

**Section**
- A 24 px header: chevron, UPPERCASE title, count chip, and a hairline to the edge.
- The body is not indented.
- Settings' group headers ("General", "Agents") use the same uppercase label.

**Alerts:** tinted box, radius 10, with the icon in the tone color, the title in fg and the text in
fg2.

**Badges:** tinted, no border, 10.5 px weight 700, radius 6.

**Indicators**
- Progress: a 6 px recessed track with a violet → cyan gradient fill.
- Spinner: a violet arc.

**Link:** blue, with an external ↗ icon and an underline on hover.

**Log**
- Recessed, border, radius 8, mono 11.
- Copy is a 22 px button in the corner.

**Menu**
- The trigger is a ghost 28 px icon button.
- Items are 26 px in the popover frame; a danger item is red.

**Key/value:** the key is a little smaller and muted.

**Unsupported panel element:** a dashed chip reading "Unsupported element" with a `?` icon.

**Dialog**
- Radius 14, popover bg, 28 px footer buttons.
- The confirm button gets the initial focus, showing the halo.

**Attachments in the composer**
- 28 px glass chips: a 20 px recessed thumbnail (file or folder icon, or the image itself), then the
  name, then a 20 px `×` inside the chip instead of hanging off the corner.
- Pending: dashed border.
- Failed: tinted red.

**Composer:** keeps its own design; only its focus changes from cyan to the violet border with the
halo.

**New chat → project folder:** stays a select in the Campo style.
- The field shows a folder icon and the path in mono with `~`.
- The menu lists recent folders with a folder icon, then a hairline, then "Open folder…" and "Clone
  repository…" with icons, in muted text.

### Path field

Picked by the owner: "A · Digita ou escolhe". It is used for Settings → Shell (a file). The owner then
liked both mocked forms and asked that extensions can use both ("tanto fora do input, quanto dentro"),
read here as: form A, a typeable field with the picker button inside it, and form B, a pick-only field
that is itself a button.

- It is an ordinary recessed text field you can type into or paste into. A leading icon shows a folder
  or a file.
- A 30 px folder button is joined on the right, like the number's segments. It opens the system picker
  (`prompt_for_paths`), here for files.
- The path is shown in mono 11.5 px, with the home folder shortened to `~` and an ellipsis when it is
  too long. A `×` clears it when there is a value.
- States:
  - focus: violet with the halo;
  - dragging from Finder: dashed violet border, violet tint and halo, with the text "Drop to use this
    folder" or "file";
  - error: red border and a red line below, "This file no longer exists.";
  - disabled.

### The extension `path` node (API 1.6)

A new panel node, a form field like `input`:

| Field | |
|---|---|
| `id` | required; joins the form ids |
| `pick` | required: `"file"` or `"folder"`; what the system picker chooses and which icon leads |
| `placeholder`? | one line; default `Choose a file…` or `Choose a folder…` |
| `value`? | an initial path, an initial value like `input`'s |
| `editable`? | bool, default `true`: form A. `false` is form B: the field is a button that opens the picker, its right segment reads `Choose…` or `Change…`, and a `×` clears it |

- The value goes in `values` under the id as a string, `""` when empty. A leading `~` the user typed is
  expanded to the home folder before it is sent; the field shows the home folder as `~`.
- Dropping a file or folder from Finder onto the field sets it (the drag state above).
- One path only; no `multiple`. No existence check: an extension that cares shows an `alert`.
- `manifest::API` goes to `"1.6"`; docs/extensions.md and the storybook get the node.

## Light mode

The same shapes and rules apply. Light mode gets its own entry in aurora.json, "wai Aurora Light",
installed as `light_theme`, in place of gpui-component's Default Light. Starting values, from the
mockup:

| | Value |
|---|---|
| Recessed | `#14193a0a` |
| Glass | `#ffffff` |
| Border | `#14193a1f` |
| Background | `#f5f6fb` |
| Text: fg / fg2 / muted / dim | `#1b1f36` / `#3b4263` / `#5d6587` / `#8a91ad` |
| Focus | `#7b5cf080` with halo `#7b5cf02e` |
| Chosen and primary | `#13b896`, with a white mark on toggles |
| Tones, darker for contrast | danger `#d43a5c`, warning `#a96b00`, success `#16894f`, info `#1f6fd1`; tinted at about 12% |
| Off toggles | inset ring `#14193a40` |

## Theme changes (dark, aurora.json)

- `ring`: `#36e2c0` → `#8d72ff73`, so gpui-component's own focus borders turn violet.
- `radius`: 10 → 8, and `radius.lg`: 16 → 14. The builder checks which widgets read these and fixes any
  that should not move.
- Everything else stays, including `primary` (cyan), `popover.background` (`#0d1230`), `caret`, the
  list and tab tokens, and the tone colors.

## Helpers (aurora.rs)

Names and signatures are the builder's call. The set they cover:

- the field style (height, radius, recessed bg, border, focus border and halo) for input, multiline,
  number and path;
- the halo itself;
- button variants: default glass, primary, tinted danger, ghost, 28 px icon;
- the select: `.appearance(false)`, the value and a right chevron in the field style, the menu as wide
  as the field, the cyan check;
- checkbox, radio, switch. If the gpui-component widgets can't be styled this far, wai draws its own,
  keeping keyboard, focus and accessibility labels;
- a tone → (bg, fg, border) mapping for badges, alerts, the danger button and failed attachments;
- the small uppercase label;
- the loose row for lists and tables (`sidebar::row` and `ext_panel::row` included);
- the popover frame;
- the path field component;
- shortening a path to `~`. `wai_settings::shown` does not do this today.

## Call sites

Measured on 2026-11-16, approximate:

| What | Where |
|---|---|
| `Button::new` | about 90, in 19 files |
| `Input::new` | 13 |
| `Textarea` | 3 |
| `Select::new` | 5: settings_view, chat/draft, chat/input |
| `Switch` | 6: extensions_view, sidebar, settings_view, settings_view/claude, chat/question |
| `Radio` | 5: settings_view default agent, chat/question `RadioGroup` |
| `Checkbox` | 2 |
| `Tag` | 12: ext_panel, chat/edits |
| `Alert`, `Progress`, `Spinner`, `Link` | ext_panel only |
| `dropdown_menu` | ext_panel and topbar |
| Dialogs (`DialogFooter`) | across the app |
| Attachment chips | chat/input and chat/draft |
| Number input | chat/question |

The composer (chat/input, chat/draft) already uses `.appearance(false)`; only its focus changes.

## Testing

- The existing tests keep passing. `primary == boreal()` stays true.
- Visual checks:
  - panels: the storybook extension;
  - the rest: an isolated debug build (`HOME=/tmp/wai-dev target/debug/wai`) on Settings, the sidebar,
    the chat question, dialogs, the new chat and the composer with attachments;
  - both modes.
- Small unit tests for logic that is not plain styling: path shortening to `~`, and the tone mapping
  if it computes colors.

## Left to the builder

- Helper names, signatures and file layout inside `aurora.rs`.
- How each gpui-component widget reaches the look: styling, `appearance(false)`, or wai's own drawing.
- The order of the migration and how it is split into commits.
- Light-mode tuning beyond the starting values, as long as the contrast holds.
- Shell field validation timing (while typing, on blur, or on save), as long as the error state shows a
  missing file.

## Calls made in the brainstorm that the owner did not pick explicitly

- Every clickable control in a row is 28 px.
- The danger button becomes tinted instead of solid red.
- Radio and switch use cyan for "on", like the checkbox.
- An option with a description becomes a loose clickable row in the chat question.
- One popover bg, the theme's `#0d1230`.
- The composer keeps its design; only the focus changes.
- Light mode gets its own theme entry with the values above, replacing Default Light.
- The extension node is named `path`, with `pick` and `editable` as above; values are sent with `~`
  expanded; no `multiple` and no existence check.
- The new chat folder picker stays a select.
- `radius` 10 → 8 and `radius.lg` 16 → 14.
