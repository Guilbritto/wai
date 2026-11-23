# The pets get readable art and an iconic fantasy roster

## Intent

The owner looks at the bichinho and cannot tell what it is: "da forma que está não dá pra saber o que são, o desenho às vezes é confuso." The goal is that every form reads as what it is at dock size (2 px cells, a 72×64 px sprite), without its name next to it.

Two causes, found by rendering all 40 forms at dock size:

1. **The stroke.** Every form is a hollow 1 px outline. At 2 px cells it becomes a thin scribble and the trait that names the creature disappears: the Dragon reads as a duck, the Cãozinho as a cat, the Cavalo-marinho as a duck, the Drone as a crab, the Bauzinho as a cake.
2. **The roster.** Five lines are niche references that no drawing can make recognizable: Monstro da ferrugem, Tarrasque, Verme púrpura, Singularidade and Sleipnir (its eight legs do not fit in 36 px).

## Goals

- Redraw all 40 forms in a **solid** style: a part's inside is the dim shade (`+`), its edge the full shade (`#`).
- Each form exaggerates the trait that names it (the Dragon's bat wing and horns, the dog's floppy ears, the Mimic's teeth and tongue, the Beholder's central eye and eyestalks).
- Baby forms are drawn as babies (big head, short body), not as the adult scaled down. Today 6 forms are `baby(adult, scale)`; each gets its own drawing.
- Swap the five niche lines for iconic fantasy creatures (table below).
- Pets and collection entries of the removed lines carry over to their replacement.

## Non-goals

- Colour, more than two shades, or any change in wai. The `pixels` node and extension API 1.9 stay as they are. The owner chose the one-colour LCD look in the 2026-11-19 spec.
- New mechanics. Odds, levels, xp, care, moods, speech and the card layout do not change.
- Renaming the 35 lines that stay.

## Anti-goals

- A form that still needs its name to be recognized at dock size.
- A stored pet or collection entry that disappears or breaks after the update.
- A mood that stops showing: happy, talk, hungry and sleep must still read on every form.

## Roster

The 35 other lines stay with their ids, names and levels. Each new line has one form, like the line it replaces.

| Out (id) | In (id, name) | Rarity |
|---|---|---|
| Monstro da ferrugem (`ferrugem`) | `kobold`, Kobold | Uncommon |
| Verme púrpura (`verme`) | `beholder`, Beholder | Epic |
| Sleipnir (`sleipnir`) | `fada`, Fada | Epic |
| Tarrasque (`tarrasque`) | `tiamat`, Tiamat (five-headed dragon) | Legendary |
| Singularidade (`singularidade`) | `goblin`, Goblin | **Common** (a legendary goblin makes no sense) |

After the swap: 7 common, 6 uncommon, 4 rare, 4 epic, 4 legendary lines; still 25 lines and 40 forms.

The owner chose the name "Beholder" knowing it comes from D&D (1975).

**Telling multi-headed forms apart.** Tiamat is drawn with wings and dragon heads; the Hidra stays a wingless serpent.

## Approaches

| | Approach | Trade-off | Outcome |
|---|---|---|---|
| A | **Fill only.** Same shapes, `part` paints the inside dim. | One change in the pen, but the shapes stay wrong: the Dragon still reads as a duck. | Rejected after the side-by-side mockup. |
| B | **Redraw.** Solid style plus a new drawing per form, built around its defining trait. | Redraws 40 forms, but fixes both the stroke and the silhouettes. | **Chosen by the owner** in the browser mockup. |
| C | **Colour or more shades.** | The biggest boost to recognition, but needs a new `pixels` format in wai (API 1.10) and reverses the one-colour LCD look. | Out of scope. |

## Decisions

### The solid pen (owner-approved through the mockup)

- `part(m)`: edge cells are `2`, every inside cell is `1`. The current "grey inner staircase corners, empty inside" rule goes.
- A dim detail (`v = 1` in `ring`, `line`, `pix`, or `+` in `glyph`) drawn over a dim cell becomes a hole (`0`), so the old dim details stay visible on a filled body.
- A new `cut(m)` sets cells to `0`: mouths, pupils, the Mimic's open maw.
- Eyes: a full eye with a hole for the pupil (`###`/`#o#`/`###`, small `##`/`#o`). Blink and happy keep their current shapes.

The four sample forms are drawn in JS in `.superpowers/brainstorm/73005-1795442593/lab/lab.js` (`spen`, `cut`, `beye`, `C2.dragao_c2`, `C2.caozinho_c`, `C2.mimico_c2`, `C2.beholder_c`). The owner saw these, and they set the bar. `traco.html` in the same session's `content/` is the side-by-side the owner picked from. `node lab/render.js lab/lab.js <key>[:mood:frame]…` prints the grids; `python3 lab/grid.py out.txt out.png <scale> <cols>` renders them.

### Where the art lives (builder's call, recorded)

The art is drawn directly in Go, in `extensions/wai-bichinho/art.go`. The 2026-11-19 spec made `pet-dex.html`'s JS the source of the art and generated the golden from it. That would mean drawing 40 forms twice, so that rule ends here. `testdata/forms.golden` is rewritten from Go with `go test -update`, and `pet-dex.html` is left as a historical mockup. The four JS samples are ported to Go as the first forms.

### Migration

`oldSpecies` in `pet.go` gains `ferrugem→kobold`, `verme→beholder`, `sleipnir→fada`, `tarrasque→tiamat`, `singularidade→goblin`. The replaced lines have one form each, so their form id equals their line id, and `load()` in `main.go` already maps `Seen` entries and stored pets through `oldSpecies`.

`load()` maps an id through `oldSpecies` only once, so the 0.3.1 entry `curupira→sleipnir` changes to `curupira→fada`. Otherwise a 0.3.1 Curupira would be dropped.

A test loads a `pet.json` holding each removed id as the current pet, as a stored pet and as a `Seen` entry, plus `curupira`.

### Dex order, tests and the golden (builder-check calls)

- **Dex order.** The goblin goes at the end of the Common block in `dex`; the other four take the place of the line they replace. Form numbers (`#001`…) are computed from `dex` order when the card renders (`main.go` `cardTree`) and are not stored, so it is fine that they shift.
- **Dex-shape test.** The test in `main_test.go` that checks the dex has 25 lines and 40 forms also counts lines per rarity; it changes to 7/6/4/4/4. Egg odds and stat floors are per rarity, so they do not change.
- **The solid pen is global.** The new `part` replaces the old one for every form at once, with no per-form flag. Forms are redrawn on the work branch, and the golden is regenerated once they are all done.
- **The golden.** `testdata/forms.golden` is regenerated from the `forms` map (every form, the egg, every mood), not patched. The new forms are covered and the removed ones go. `TestFormsMatchTheMockup` becomes `TestFormsMatchTheGolden`, since Go is now the source.
- **Preview for the owner.** The golden is rendered to an HTML page (reusing `lab/gen.py`'s inline-SVG approach) at 2 px and zoomed, with a mood row. The owner reviews it once with all forms done; the forms the owner calls out get redrawn and shown again.

### Constraints to keep

- The grid is 36×32. Forms draw through `pen{&g, 2, bob}`, so creature x 0..33 lands on grid x 2..35.
- `fx` draws hearts, z's and hunger marks around grid (29..35, 0..9), and `dockTree` draws the `!` there for alert or error. The top-right corner should stay readable when those show.
- The stage mirrors the sprite when the pet walks left, so side-view forms must read both ways.
- Bob moves the sprite 1 px. The egg wobbles sideways and is filled in the solid style too.
- The card shows the idle frame at scale 3; the dock uses scale 2.

### Owner review before release

Before any commit that releases, all 40 forms and the egg are shown in the browser (see "Preview for the owner") for the owner to approve or call out forms.

### Release

Commits are `chore:` (CLAUDE.md: extensions never trigger a wai release). The version goes to 0.5.0 in `wai-extension.toml`. The tag `wai-bichinho-v0.5.0` publishes it through `.github/workflows/extensions.yml`, and then `extensions/index.toml` lists 0.5.0 with the new sha256s, as was done for 0.4.0. The README's dex counts and the rarity line list get updated.

## Left to the builder

- The exact drawing of every form, keeping to the solid style and the bar set by the four samples.
- How each mood shows on forms whose faces are unusual (the Beholder's single eye, the Mimic's lid eyes, the Kraken).
- Whether the shiny sparkles need any tweak to stay visible next to filled bodies.
