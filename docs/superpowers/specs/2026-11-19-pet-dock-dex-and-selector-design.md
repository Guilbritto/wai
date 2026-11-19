# The pet moves to a sidebar dock, gets pixel art, a 40-form dex with evolutions, and a pet selector

## Intent

The bichinho lives beside the prompt box as three lines of ASCII art. The owner wants:

- **A pet you can see everywhere.** It moves into a dock at the foot of the sidebar, visible on every sidebar tab. The dock shows only the pet's name; the rest is a stage where the pet walks, stops, blinks and turns.
- **Real pixel art.** An LCD-like look with no background, drawn in the theme's text colour in two shades.
- **Many creatures to collect.** 25 evolution lines and 40 forms across five rarities, themed on monsters, Norse and Greek myth, dragons, sea monsters, tech and D&D-style fantasy. They are inspired by monster-collecting games, not copied from any.
- **Choosing among the pets you own.** Today a new egg replaces the current pet for good. Instead, every pet you have hatched stays in a table, and you can switch to any of them.

The look was settled with mockups. They are gitignored but present on this machine:

- `/Users/guilherme.brito/Project/Guilherme/wai/.superpowers/brainstorm/pixel-art/content/pet-sidebar-dock.html`: the dock. It has 2 px cells, a name-only header, a ⋯ menu on hover, the bubble over the list, and walking.
- `/Users/guilherme.brito/Project/Guilherme/wai/.superpowers/brainstorm/pixel-art/content/pet-dex.html`: the dex. It has every form, each mood and the shiny effect, and **it holds the source of the art** (the `<script>` block).

## Goals

- A `sidebar-foot` panel location and a `pixels` panel node in wai, released as extension API 1.9.
- wai-bichinho 0.4.0 does the following:
  - It moves its pet panel to `sidebar-foot` and draws the pet and the egg in pixels.
  - It ships the 40 forms with their evolution lines and evolves the pet at the line's levels.
  - It turns "Past pets" into a table where any pet can be put back in the dock.

## Non-goals

- Bubble placement over the pet's position. The bubble stays at the dock's left.
- Pixel colours per node, tones or more than two shades.
- Moving the pet back beside the prompt box. The `prompt` and `prompt-side` locations stay in wai, but the bichinho stops using them.
- Trading pets, releasing a pet or deleting it from the table.

## Anti-goals

- **Copying a game.** No creature, name or look is taken from an existing game. WotC product-identity monsters (Beholder, Mind Flayer) stay out. The D&D-style creatures that remain are generic fantasy.
- **A dock that eats the session list.** It stays about 90 px tall. The bubble floats over the list instead of pushing it.
- **Losing a pet.** Migration keeps the current pet and every past pet. The `maxPast` cap goes.
- **A stored pet that starves.** Hunger and love freeze while a pet is in the table.

## Constraints

- **API version.** `development` has API 1.8 and `v0.19.x` released it, so the new location and node are **1.9** (`crates/wai-ext/src/manifest.rs:96`). The extension's manifest sets `api = "1.9"`, so an older wai refuses it with "needs a newer wai".
- **Untrusted trees.** Extension trees are parsed in `crates/wai-ext/src/panel.rs` and never panic. A broken `pixels` node becomes `Unsupported`.
- **Sidebar width.** The sidebar is fixed at 280 px (`workspace.rs:60`) and sits on a card inset 4 px. The stage must fit about 264 px, so at 2 px per cell it is at most 128 cells wide.
- **Taffy layout.** An absolute child is placed relative to its parent box. That is how the floating bubble is done (below).
- **Animation by re-render.** An extension animates by sending a new tree. The stage is 128×31 cells, about 4 KB as text per frame. wai already dedups nothing, so the extension sends only when the tree changed. That is what it does today (`s.sent`).
- **Commits and releases.** The wai changes are `feat:` commits. The extension changes are `chore:` and ship on their own tag `wai-bichinho-v0.4.0`. That release must wait for a wai release with API 1.9.

## Approaches

**Where the pet lives** (decided with the mockup): a dock at the sidebar's foot (chosen). The alternatives were the prompt box's side, as today, and a status-bar item. The dock is visible on every tab and every pane layout, and it has room for walking.

**How pixels get drawn:**

1. **A host `pixels` node (chosen).** The extension sends rows of `.`, `+` and `#`, and wai paints them as quads in theme colours. This is generic, it is the only way to get crisp cells in the theme's colours, and it is cheap at this size.
2. **Unicode block or braille text.** This was tried first (`pet-braille.html`). Cells are not square and the font decides the look. Dropped.
3. **An SVG or PNG image node.** It needs a new image pipeline and theme recolouring, and the extension would encode images every frame. Dropped.

**Who animates the stage:**

1. **The extension re-renders (chosen).** It sends the full stage each frame, about 4 per second while walking and less when still. This needs no new protocol, and the walk and pause logic stays with the pet.
2. **The host plays frames.** `pixels` would take a list of frames and a rate. That is a smaller payload, but walking, turning and reacting to moods would need a scripting model in the host. Dropped.

**How a hatched egg behaves** (owner's call): the new pet takes the dock at once, and the current one goes to the pets table.

## Part 1: wai, API 1.9

### `sidebar-foot` location

```toml
[[contributes.panels]]
id = "pet"
title = "Pet"
location = "sidebar-foot"
```

- **Where it draws.** At the bottom of the sidebar card, below whatever tab is shown (Sessions, History, Files, Search, Agents or a sidebar panel), on every tab. It has a 1 px top border in the theme's border colour and full width, and its height is its tree's height.
  - It hides with the sidebar and shows whenever the sidebar does. With a `wide` panel the sidebar spans the window, so the foot spans it too, and its content stays left-aligned.
- **No activity-bar button.** Like the prompt panels, it gets no button. `panel/open` on it does nothing.
- **One per location.** At most one per manifest. Across extensions the first one wins, the same rule as the prompt slots.
- **Manifest rules.** `width`, `frame` and `wide` are invalid on it, with the same style of manifest errors as today.
- **Panel behaviour in the foot.** The `ExtPanel` behaves differently there:
  - **No inner scroll and no full size.** The root is not `size_full` and does not scroll; it takes its content's height.
  - **Bubbles float.** A `bubble` node is drawn absolutely, above its parent node (`bottom: 100%`, left aligned), so it covers what is above the dock instead of growing it. The dock is painted after the tab content, so the bubble is on top.
  - **Menus show on hover.** A `menu` node shows only while the mouse is over the panel (opacity 0, and 1 on group hover). It still takes its space, so nothing moves.
- **Code touch points:**
  - `PanelLocation::SidebarFoot` (serde `sidebar-foot`).
  - A `docked()` helper for "not in the activity bar, one per location": prompt, prompt-side and sidebar-foot. Today `in_prompt()` covers this in `ext_host::rail_panels` and in the manifest's one-per-location check. `in_prompt()` stays for `width`.
  - `ext_host::prompt_panel` becomes `panel_at(location)`.
  - `Workspace::render_sidebar` appends the foot.
  - The `panel/open` match in `workspace.rs:1965` gets the new location.
  - `ExtPanel` gets a `foot: bool`, set when the panel is created for that location.

### `pixels` node

```json
{"type": "pixels", "rows": ["..++..", ".####.", "#.##.#"], "scale": 2, "id": "pet"}
```

| Field | Rules |
|---|---|
| `rows` | Required array of strings: at most 64 rows, each cut to 256 characters. `#` is a full cell, `+` a dim cell, and any other character is empty. Shorter rows are padded with empty cells to the longest. An empty array is `Unsupported`. |
| `scale` | Optional, px per cell, 1 to 8 (clamped). The default is 2. |
| `id` | Optional. When given, a click anywhere on the node sends `panel/action` with this id, and the cursor is a pointer. It shares the id set of buttons, fields, sections and bubbles: a repeated id makes the node `Unsupported`. |

- **Size.** The node is exactly `longest row × scale` by `rows × scale` px, with `flex_none`. A container narrower than that clips it.
- **Colours.** `#` is the theme's `foreground`; `+` is the same at 40% opacity. Empty cells are not painted, so there is no background.
- **Painting.** A gpui `canvas`, one quad per horizontal run of equal cells. Parse the rows once into `Arc<[Box<[u8]>]>` (0, 1 or 2 per cell) so a re-render does not re-read strings.
- **Where it may appear.** It counts as one node. It is not a table cell type.
- **Docs.** `docs/extensions.md` gets the following:
  - API 1.9 in the header, the `initialize` example and the `api` guidance (use `"1.9"` for `sidebar-foot` or `pixels`).
  - The `sidebar-foot` bullet under "Where it shows", with the foot rules above.
  - The `pixels` row in the node table.
  - The `bichinho` mention updated.

## Part 2: wai-bichinho 0.4.0

### The dock

The `pet` panel moves to `location = "sidebar-foot"` (manifest `api = "1.9"`, version `0.4.0`, and a description without "beside your prompt box"). Its tree:

```
column
├─ bubble (when speaking; floats above the dock)
├─ row: text name (Title(): "Quindim ✦"), spacer, menu
└─ pixels id="pet" scale=2: the 128×31 stage
```

- **Click.** Clicking the stage sends `pet` (petting it), the same as the menu's Pet.
- **Menu.** Pet, Feed, Open card, Switch pet (only when the table has pets; it opens the card), and Hatch an egg (n) when there are eggs.
- **What leaves the dock.** The level badge and the status word go; level, hunger and love live on the card.
- **No pet yet.** Before the first hatch the name reads "Egg" and the stage shows the wobbling egg.

### The stage and animation

The tick goes from 500 ms to **250 ms**. The pose frame advances every 2 ticks, so creatures still animate at 500 ms; walking moves 1 cell per tick. `listEvery` doubles, so it stays at 30 s.

- **Size.** The sprite is 36×32 cells (the mockup's `W`, `H`), drawn at offset (2, bob) as in the mockup. The stage is 128×31: the sprite's bottom row is cut, as in the dock mockup (`H = 31`).
- **Walking.** Only in the `idle` and `talk` moods.
  - The x position runs from 0 to 92 (128 − 36). It turns at the edges and pauses 12 ticks there, and randomly pauses for 10 to 25 ticks with a 2.5% chance per tick (the mockup's numbers).
  - Facing left mirrors the sprite.
  - Bob is 1 cell on alternate ticks while walking; when still, the mockup's `f % 2` bob.
- **Standing still.** In every other mood the pet stands where it is.
- **Mood mapping.** The mood names do not change. Art moods come from the existing moods:

  | Existing mood | Art mood | Extra |
  |---|---|---|
  | idle | idle | |
  | working | idle | the mockup's working dots glyph |
  | happy, surprise | happy | |
  | talk, alert, error | talk | alert and error add a `!` glyph where the heart goes |
  | hungry, sad | hungry | |
  | sleeping | sleep | |
  | egg | egg | |

- **Effects.** `fx` from the mockup draws the hearts, the z's, the hunger marks and the shiny sparkles.
- **Frame counting.** The frame advances on every tick, whether or not a session is focused, because the dock is always visible.

### The art port

`art.go` is a Go port of the mockup's art script: the `<script>` of `pet-dex.html`, plus `C.ovo`, the egg, from `pet-sidebar-dock.html`.

- **Helpers.**
  - Masks are `func(x, y float64) bool`: `ell`, `rrect`, `poly`, `and`, `or`, `not`, `tube`, `curve`.
  - A drawer interface has `part`, `ring`, `fill`, `pix`, `line`, `glyph`, `set` and `s()`. It has two implementations: the pen (with rounding in `set`) and `scaled`.
  - Faces: `eyes`, `seye`, `mouth`, `look`. Effects: `fx` and `wave`.
- **Forms.** `forms map[string]func(d drawer, mood string, f int)`, one per form, for all 40 forms and the egg.
- **Output.** `sprite(form, mood, frame, shiny, bob) [32][36]uint8` and `stage(...)` give the grids. Rows serialize as `.`, `+` and `#`.
- **Fidelity check.** A golden file `testdata/forms.golden` holds idle frame 0 of every form and the egg, generated **from the mockup's JS** with node. A Go test compares against it; `go test -update` rewrites it from Go after deliberate art changes.

### The dex and evolution

The dex has 25 lines and 40 forms. Ids, names and evolution levels follow `TIERS` in the mockup.

- **Lines.** A line has an id (its first form's id), a rarity and its forms, each form with the level it is reached at. 3-stage lines evolve at Lv 5 and Lv 15, 2-stage lines at Lv 10. Single-form lines never evolve: every Épico and Lendário line, plus drone, ferrugem and cubo. The `TIERS` array in `pet-dex.html` is the source of truth: 6 Comum, 6 Incomum, 4 Raro, 4 Épico and 5 Lendário lines. Its theme tags (Monstro, Tech…) are not used.
- **Hatching.** It rolls the rarity as today, then a line of that rarity uniformly, and starts at the first form.
- **Stored state.**
  - `Pet.Species` holds the **line id**; the form is derived from the level (`formOf(p)`).
  - `Seen` is keyed by **form id**, marked at hatch and at each evolution (true once that form was seen shiny).
- **Evolving.** When a turn levels the pet across a form's level, it evolves:
  - It writes the diary note `evolved into <Form>`, flashes "evolved!" (happy), and says a new `evolve` kind: prio 2, fallback `Evoluí!`, meaning "the pet just evolved into a new form".
  - The Claude pools regenerate, as on level up, so lines use the new form's name.
  - The evolve flash wins over the level-up one.
- **Names in pt-BR.** Form names stay in pt-BR, as in the mockup (Gotinha, Gosmão, Corvo de Odin…). The rarity names stay English, as the card has them today.
- **Persona.** `persona()` and `request()` use the form's name and say "a creature of myth, the sea, tech or fantasy" instead of "Brazilian fauna or folklore".
- **Migration in `load()`.** The old species map to the base form of a line of the same rarity. Old `Seen` keys are mapped the same way.

  | Old | New line |
  |---|---|
  | tatu | gotinha |
  | sapo | larvinha |
  | jabuti | alevino |
  | vagalume | fagulha |
  | quati | grifinho |
  | tucano | corvinho |
  | preguica | pedrinha |
  | lobo-guara | caozinho |
  | boto | cavalinho |
  | mico | fenix |
  | curupira | sleipnir |
  | boitata | jormungandr |

  A migrated pet shows the form for its current level, so an old Lv 12 tatu becomes a Gosma. An id that is neither old nor new is dropped, as today.

### The card

- **The picture.** It is a `pixels` node at scale 3 (108×96 px) of the current form, idle, frame 0, with sparkles when shiny. The egg is shown before the first hatch.
- **The header line.** `<Form> · born <date>`, followed by the form's next evolution: "evolves at Lv 15", or nothing for a final form.
- **Collection.** It counts forms (`n/40`). Its rows are in dex order, as `#001 Gotinha` for seen forms or `#001 ???` for unseen ones, with the rarity badge.
- **Your pets** (was "Past pets"). It is a table and opens by default when it has rows:

  | Pet | Form | Lv | |
  |---|---|---|---|
  | Pipoca ✦ | Gosma | 7 | [Use] |

  - **Use.** The button's id is `use-<Born.UnixNano>`, so a stale list cannot pick the wrong pet. **Use** swaps the pets: the dock pet goes to the top of the table, and the chosen one comes out of it.
  - **Freezing.** Storing sets `StoredAt = now`. Taking a pet out shifts `FedAt` and `LovedAt` forward by `now − StoredAt` and clears `StoredAt`. A stored pet without `StoredAt` (from before this version) counts as stored at `max(FedAt, LovedAt)`.
  - **Effects of a swap.** It clears `Lines` (the pools are per pet), writes the diary note `switched to <Name>`, flashes "hi!" and says `hatch`. The `hatch` line introduces the pet ("Oi! Eu sou {name}.").
- **Hatch.** It works as today, but the current pet is stored with `StoredAt`. The confirm reads "<Name> goes to your pets. You can switch back any time." `maxPast` is removed.

### What else changes in the extension

- **ASCII art.** `draw()`, `moods` and the old `species` table with its ASCII art go.
- **Docs.** The README covers the dock, the 40 forms by rarity, evolution and switching pets, and that it needs wai with API 1.9.
- **Index.** `extensions/index.toml` is bumped when the extension is released, not before.

## Testing

- **wai-ext.**
  - `pixels` parsing: rows and padding, characters, limits, the scale clamp, the id clash and an empty `rows`.
  - The manifest: `sidebar-foot` parses; `width`, `frame` and `wide` are refused on it; a second one is refused.
  - The API constant is 1.9.
- **wai.**
  - The foot panel draws in the sidebar on two different tabs, is absent from the rail, and hides with the sidebar.
  - A `pixels` node with an `id` sends `panel/action` on click.
  - The bubble floats: it does not change the foot's height.
  - Use the existing `ext_host` and `ext_panel` test helpers.
- **Extension.**
  - The golden art.
  - Evolution at Lv 5, 15 and 10, and the evolve flash and note.
  - Migration of an old `pet.json` (pet, past and seen).
  - Swap freezes and unfreezes hunger and love.
  - Hatch stores with `StoredAt` and has no cap.
  - The dock tree has the name and a 128×31 `pixels` node.
  - Walking stays in bounds and stops in non-walking moods.

## Left to the builder

- The exact text sizes in the dock header, within the mockup's spirit (small, semibold, muted bright).
- How the `pixels` canvas batches its runs, and whether `+` uses `foreground.opacity(0.4)` or a theme token that reads better in light themes.
- Test structure, file splits inside `art.go` if it gets long, and helper names.

## Calls made without asking

- The tick goes to 250 ms, about 16 KB/s of JSON while walking, which is negligible on stdio. Sending only on change keeps it low when the pet stands still: only the effect glyphs (z's, hearts) change then.
- The stage is 128 cells, not the mockup's 132, so it fits the 280 px sidebar.
- The egg migration map in the table above.
- "Switch pet" in the menu just opens the card.
- The hover-only ⋯ applies to every menu in any `sidebar-foot` panel. It is a rule of the location, not a node flag.
