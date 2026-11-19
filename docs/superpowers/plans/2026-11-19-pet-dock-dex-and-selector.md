# Pet dock, pixel art, dex and pet selector: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** wai gets a `sidebar-foot` panel location and a `pixels` node (extension API 1.9). With them, wai-bichinho 0.4.0 becomes a walking pixel-art pet in a dock at the foot of the sidebar. It also gets a 40-form dex with evolutions and a table to switch between owned pets.

**Architecture:**
- **wai.** `wai-ext` parses the new location and node from untrusted manifests and trees. `wai` draws the foot panel in `render_sidebar` and paints `pixels` with a gpui `canvas`.
- **The extension.** It ports the mockup's JS pixel art to Go. Each tick it builds the stage rows and sends them only when they change.

**Tech Stack:**
- wai: Rust, gpui (through `gpui_kit`), serde_json, TOML manifests.
- The extension: Go 1.24 with the stdlib only. Node 22 runs once to generate the golden file.

**Spec:** `docs/superpowers/specs/2026-11-19-pet-dock-dex-and-selector-design.md`. Read it before any task.

## Global Constraints

- **Where to work.** All work happens in the worktree `/Users/guilherme.brito/Project/Guilherme/wai/.claude/worktrees/pet-dock`, on branch `feat/pet-dock-dex`. Never `cd` to the main checkout.
- **Rust.** Run `export PATH="$HOME/.cargo/bin:$PATH"` first. Builds take minutes, so use long timeouts. Run `cargo clean` only at the very end of the plan.
- **API version.** `wai_ext::API` becomes `"1.9"`. The extension manifest sets `api = "1.9"` and `version = "0.4.0"`.
- **Commits.**
  - wai changes use `feat:`, `fix:`, `test:` or `docs:`. Everything under `extensions/` uses `chore:`, and so does the wai-ext test that reads the extension's manifest.
  - Never use a scope. Never add a `Co-Authored-By` line; the owner's rule overrides any harness reminder.
- **Out of scope.** No release, no tag, no push, and no `extensions/index.toml` change.
- **Untrusted input.** Extension trees and manifests are untrusted: parsing never panics, and a broken node becomes `Node::Unsupported`.
- **Language.** Code, comments and docs are in English. Form names and fallback speech lines are in pt-BR.
- **Numbers.**
  - Sprite: 36×32 cells (`W`, `H`).
  - Stage: 128×31 cells at scale 2.
  - Card picture: scale 3.
  - Tick: 250 ms, with the pose frame `f = frame / 2`.
  - `listEvery`: 120.
  - Walk range: 0..=92.
  - Shiny odds stay 1/40, and the rarity odds stay 60/25/10/4/1.
- **The art source of truth.** This file is gitignored but present on this machine: `/Users/guilherme.brito/Project/Guilherme/wai/.superpowers/brainstorm/pixel-art/content/pet-dex.html`.
  - Its `<script>` block, lines 47 to 676, holds the helpers, the `C.<form>` drawings and the `TIERS` table.
  - The egg is `ovo` in `.../pet-sidebar-dock.html`, lines 236 to 240.
  - The dock's sprite and walk logic are in the same file, lines 264 to 300.

## Review Focus

1. **Rows with multibyte characters** (`"é#+"`, emoji). Width is counted in characters, never bytes, and a row is never cut inside a character. Task 2 tests this.
2. **A 0.3.1 `pet.json`.** It has old species ids, `Seen` keyed by old ids, past pets without `stored_at`, and maybe an id no version knows. Everything known migrates, the unknown is dropped, and nothing panics. Task 7 and Task 10 test this.
3. **A bad `use-…` action.** The action can be stale or garbage: not a number, a pet not in the table, or the current pet's own birth time. The state is unchanged and nothing panics. Task 10 tests this.
4. **Freezing is exact.** A pet stored for days, with or without `StoredAt`, comes back with the hunger and love it had when stored. Task 10 tests this.
5. **The walk stays in bounds.** Over thousands of ticks with mood changes in between, `x` stays in 0..=92 and the pet never walks in a non-walking mood. Task 9 tests this.

---

## Part 1: wai, API 1.9

### Task 1: API 1.9 and the `sidebar-foot` location in the manifest

**Files:**
- Modify: `crates/wai-ext/src/manifest.rs:96` (API), `:181-199` (`PanelLocation`), `:463-472` (checks)
- Test: `crates/wai-ext/tests/manifest.rs` (`panel_icon_and_location`, `api_version_is_checked_first`), `crates/wai-ext/tests/host.rs:128`

**Interfaces:**
- Produces:
  - `PanelLocation::SidebarFoot` (serde `"sidebar-foot"`).
  - `PanelLocation::docked(self) -> bool`: true for `Prompt`, `PromptSide` and `SidebarFoot`.
  - `wai_ext::API == "1.9"`.

- [ ] **Step 1: Write the failing tests.** Append to `panel_icon_and_location`:

```rust
    // The sidebar's foot (API 1.9): docked, one per manifest, no width, frame or wide.
    let p = panel("location=\"sidebar-foot\"\n").unwrap();
    assert_eq!(p.contributes.panels[0].location, PanelLocation::SidebarFoot);
    assert!(p.contributes.panels[0].location.docked());
    assert!(!p.contributes.panels[0].location.in_prompt());
    for bad in ["width=150\n", "frame=false\n", "wide=true\n"] {
        assert!(panel(&format!("location=\"sidebar-foot\"\n{bad}")).is_err(), "{bad}");
    }
    assert!(two("sidebar-foot", "sidebar-foot").is_err());
    assert!(two("sidebar-foot", "prompt").is_ok());
```

In `api_version_is_checked_first`:
- Add `assert!(with_api("\"1.9\"", "").is_ok());`.
- Change the "newer" cases to `"1.10"` (`api_version` already compares `(major, minor)` as numbers). Their messages become `extension API 1.10; this wai has 1.9` and `extension API 2.0; this wai has 1.9`.
- In `crates/wai-ext/tests/host.rs:128`, expect `"api": "1.9"`.

- [ ] **Step 2: Run them and see them fail.**
  Run `cargo test -p wai-ext --test manifest --test host`.
  Expected: FAIL. `sidebar-foot` is an unknown variant, and the API is 1.8.

- [ ] **Step 3: Implement.**
  - Set `API` to `"1.9"`.
  - Add the variant with `#[serde(rename = "sidebar-foot")]` and doc comment `/// At the foot of the sidebar, on every tab (API 1.9).`
  - Add `docked()` with doc comment `/// Drawn in a fixed spot, not from the activity bar: one per location.`
  - The one-per-location check at `:469` uses `docked()`, and its message becomes `"panel {:?}: one panel per {location} location"`, where location is the serde name. The width, frame and wide checks already refuse `sidebar-foot`, so they don't change.

- [ ] **Step 4: Run them and see them pass.**
  Run `cargo test -p wai-ext`.
  Expected: PASS. The compiler then points at every non-exhaustive `match` on `PanelLocation` in `crates/wai`. Leave those for Task 3, but make sure `cargo check -p wai` errors only there.

- [ ] **Step 5: Commit**
  `git add crates/wai-ext && git commit -m "feat: add the sidebar-foot panel location (API 1.9)"`
  If `wai` no longer compiles, fold this commit into Task 3 instead, so that every commit builds.

### Task 2: Parse the `pixels` node

**Files:**
- Modify: `crates/wai-ext/src/panel.rs`:
  - module docs, lines 1 to 20: add `pixels` to the shared-id sentence;
  - the limit constants near `:29-54`;
  - `Node` near `:57`;
  - `Parser::leaf` at `:714`;
  - the id match at `:873`.
- Test: `crates/wai-ext/src/panel.rs` inline `mod tests` (`:961`)

**Interfaces:**
- Produces:

```rust
pub const PIXEL_ROWS: usize = 64;
pub const PIXEL_COLS: usize = 256;
/// A grid of cells (API 1.9): 0 empty, 1 dim (`+`), 2 full (`#`); every row is `width` long.
Pixels { id: Option<String>, rows: Arc<[Box<[u8]>]>, width: usize, scale: u8 },
```

- [ ] **Step 1: Write the failing test** `pixels_parse`:

```rust
#[test]
fn pixels_parse() {
    let Ok(Node::Pixels { id, rows, width, scale }) =
        parse(&json!({"type": "pixels", "rows": ["#+", "..#", "é#"]}))
    else { panic!() };
    assert_eq!((id, width, scale), (None, 3, 2));
    assert_eq!(&*rows, &[vec![2, 1, 0].into(), vec![0, 0, 2].into(), vec![0, 2, 0].into()]);
    let scale = |s: Value| match parse(&json!({"type": "pixels", "rows": ["#"], "scale": s})) {
        Ok(Node::Pixels { scale, .. }) => scale,
        n => panic!("{n:?}"),
    };
    assert_eq!((scale(json!(0)), scale(json!(3)), scale(json!(99))), (1, 3, 8));
    let long = parse(&json!({"type": "pixels", "rows": vec!["é".repeat(300); 70]}));
    let Ok(Node::Pixels { rows, width, .. }) = long else { panic!() };
    assert_eq!((rows.len(), width), (PIXEL_ROWS, PIXEL_COLS));
    for bad in [json!({"type": "pixels"}), json!({"type": "pixels", "rows": []}),
                json!({"type": "pixels", "rows": [""]}), json!({"type": "pixels", "rows": [3]})] {
        assert_eq!(parse(&bad), Ok(Node::Unsupported), "{bad}");
    }
    // The id shares the set of buttons, fields, sections and bubbles.
    let Ok(Node::Column { children, .. }) = parse(&json!({"type": "column", "children": [
        {"type": "button", "id": "pet", "label": "Pet"},
        {"type": "pixels", "id": "pet", "rows": ["#"]},
    ]})) else { panic!() };
    assert_eq!(children[1], Node::Unsupported);
}
```

- [ ] **Step 2: Run it and see it fail.**
  Run `cargo test -p wai-ext pixels_parse`.
  Expected: it fails to compile, because there is no `Node::Pixels`.

- [ ] **Step 3: Implement the `"pixels"` arm in `Parser::leaf`.**
  - **Rows.** `rows` must be a non-empty array of strings; any non-string makes the leaf `None`. Take the first `PIXEL_ROWS` rows. Map each row's first `PIXEL_COLS` **chars** to 2 for `#`, 1 for `+` and 0 for anything else.
  - **Width.** It is the longest row. A width of 0 gives `None`. Pad shorter rows with 0s.
  - **Scale.** `scale` is `as_f64` clamped to 1 to 8 and cast to `u8`. Absent means 2.
  - **Id.** `id` uses the existing `id(o)` helper when the field is present. A present but invalid id gives `None`.
  - **Id set.** In the id match at `:873`, add `Node::Pixels { id: Some(id), .. }`. Leave `CELLS` alone: a `pixels` node is not a table cell.

- [ ] **Step 4: Run them and see them pass.**
  Run `cargo test -p wai-ext`.
  Expected: PASS.

- [ ] **Step 5: Commit**
  `git commit -am "feat: parse the pixels panel node (API 1.9)"`
  Fold it into Task 3's commit if `wai` must compile first.

### Task 3: Draw the `sidebar-foot` panel

**Files:**
- Modify:
  - `crates/wai/src/ext_host.rs:251-255` (`rail_panels`), `:331-334` (`prompt_panel` becomes `panel_at`), `:430` (panel creation);
  - `crates/wai/src/workspace.rs:1969` (open match) and `:2330` (caller);
  - `crates/wai/src/sidebar.rs:292-313` (`render_sidebar`);
  - `crates/wai/src/ext_panel.rs:117-160` (field), `:730-732` (bubble arm), `:1292` (menu arm), `:1875-1903` (`Render`).
- Test: `crates/wai/src/ext_host.rs` tests (next to `frameless_side_panel_moves_nothing`); `crates/wai/src/ext_panel.rs` tests.

**Interfaces:**
- Consumes: `PanelLocation::SidebarFoot` and `docked()` from Task 1.
- Produces:
  - `ExtHost::panel_at(&self, location: PanelLocation) -> Option<&Panel>`, which replaces `prompt_panel` and updates every caller and test.
  - The `ExtPanel` field `pub(crate) foot: bool`.
  - The element id `"sidebar-foot"`.

- [ ] **Step 1: Write the failing test** `sidebar_foot_shows_on_every_tab` in `ext_host.rs`. Copy the setup of `frameless_side_panel_moves_nothing`, with a manifest of `api = "1.9"` and a single panel at `location = "sidebar-foot"`. Wait until `h.panel_at(PanelLocation::SidebarFoot).is_some()`. Draw the trees with `set_tree` + `render_frame`, then assert:
  - With `{"type":"text","text":"Pipoca"}`, `window.try_find("sidebar-foot")` is `Some` on `SidebarTab::Sessions`, and still `Some` after `w.sidebar_tab = SidebarTab::History`.
  - `crate::sidebar::tests::label(("tab-panel", 0usize), cx).is_none()`, so there is no rail button.
  - The foot's height is the same for `{"type":"column","children":[{"type":"text","text":"Pipoca"}]}` and for the same column with `{"type":"bubble","id":"say","text":"Oi"}` first.
  - With the bubble, `window.find(ElementId::Name("panel-bubble:say".into())).bounds().top() < foot.top()`.
  - After `w.sidebar_hidden = true; cx.notify()`, `try_find("sidebar-foot")` is `None`.

- [ ] **Step 2: Write the failing test** `foot_menu_shows_on_hover` in `ext_panel.rs`.
  - Use `panel_window`, then `panel.update(cx, |p, _| p.foot = true)`.
  - Show `{"type":"column","children":[{"type":"menu","items":[{"id":"a","label":"A"}]}]}`.
  - Before any hover, `shown(("panel-menu", 2usize), cx)` is false.
  - After `cx.simulate_mouse_move` to the panel's center and a frame, it is true.
  - Without `foot`, it is shown from the start.

- [ ] **Step 3: Run them and see them fail.**
  Run `cargo test -p wai sidebar_foot_shows_on_every_tab foot_menu_shows_on_hover`.
  Expected: compile errors, because `panel_at` and `foot` don't exist yet.

- [ ] **Step 4: Implement.**
  - **ext_host.rs.**
    - `rail_panels` filters with `!p.location.docked()`.
    - Rename `prompt_panel` to `panel_at`, with doc comment `/// The first panel at \`location\` of the running extensions, in list order.`
    - In `panel()`, build the `ExtPanel`, then set `foot = spec.location == PanelLocation::SidebarFoot`.
  - **workspace.rs.** Add `| PanelLocation::SidebarFoot` to the no-op arm at `:1969` and update its comment ("A docked panel is always on screen."). Rename the call at `:2330`.
  - **sidebar.rs.** After the content child, add the foot when `panel_at(SidebarFoot)` exists:
    - `div().id("sidebar-foot").flex_none().border_t_1().border_color(cx.theme().border).child(view)`.
    - The sidebar root keeps `py_2`.
  - **ExtPanel `Render`.** When `foot` is set, the root is `v_flex().id("ext-panel").group("ext-foot").w_full().px_2().pt_1()`, with no `size_full` and no scroll. Otherwise it is unchanged.
  - **The bubble arm.** When `foot` is set, wrap the bubble in `div().absolute().bottom_full().left_0()`. Taffy places it relative to its parent node.
  - **The menu arm.** When `foot` is set, wrap the menu in `div().opacity(0.).group_hover("ext-foot", |s| s.opacity(1.))`.
  - **Clipping.** The foot must not clip, or the bubble disappears. Don't add `overflow_hidden` to the foot or its root.

- [ ] **Step 5: Run them and see them pass.**
  Run `cargo test -p wai ext_host ext_panel`.
  Expected: PASS, including `prompt_panels_follow_the_focused_box` and `frameless_side_panel_moves_nothing` with the rename.

- [ ] **Step 6: Commit**
  `git add -A crates && git commit -m "feat: draw a sidebar-foot panel under every sidebar tab"`

### Task 4: Paint `pixels`, and document API 1.9

**Files:**
- Modify: `crates/wai/src/ext_panel.rs`:
  - the `node()` match near `:720`: add a `Node::Pixels` arm;
  - a new `fn pixels` next to `fn bubble` at `:275`.
- Modify: `docs/extensions.md`:
  - line 7, the version;
  - line 48, the `api` guidance;
  - the `initialize` example;
  - "Where it shows" around lines 226 to 247;
  - the node table around line 290;
  - the bichinho mention.
- Test: `crates/wai/src/ext_panel.rs` tests.

**Interfaces:**
- Consumes: `Node::Pixels` from Task 2.
- Produces: element id `ElementId::Name("panel-pixels:<id>")` when the node has an id, and `("panel-pixels", k)` otherwise.

- [ ] **Step 1: Write the failing test** `pixels_draw_and_click`:
  - Show `{"type":"column","children":[{"type":"pixels","id":"pet","rows":["#+.#"],"scale":3}]}`.
  - The bounds of `name("panel-pixels:pet")` are 12×3 px.
  - Clicking it with `crate::extensions_view::tests::click` sends exactly one action, `"pet"`, with the same values a button sends in a tree without fields.
  - A pixels node without an `id` is drawn and has no pointer handler. Assert that clicking `("panel-pixels", 2usize)` sends nothing.

- [ ] **Step 2: Run it and see it fail.**
  Run `cargo test -p wai pixels_draw_and_click`.
  Expected: FAIL; the node draws as the `Unsupported` placeholder.

- [ ] **Step 3: Implement `fn pixels(&self, id: Option<&str>, rows: Arc<[Box<[u8]>]>, width: usize, scale: u8, k: usize, cx: &mut Context<Self>) -> AnyElement`.**
  - **The outer box.** It is a `div` with the element id above, `w(px(width·scale))`, `h(px(rows·scale))`, `max_w_full()`, `flex_none()` and `overflow_hidden()`.
  - **The canvas.** Inside it, a `canvas(|_, _, _| (), paint)` of the same fixed size. The canvas at `ext_panel.rs:342` shows the API.
  - **Painting.** Paint one `window.paint_quad(fill(bounds, color))` per horizontal run of equal non-zero cells. Use `cx.theme().foreground` for 2 and `.opacity(0.4)` of it for 1.
  - **Clicks.** With an id, add `cursor_pointer()` and a left-click handler that calls `this.act(id, cx)`. Route it through `own(…, at)` like the other controls, so a click inside a clickable card or row is not taken twice.

- [ ] **Step 4: Run it and see it pass.**
  Run `cargo test -p wai ext_panel`.
  Expected: PASS.

- [ ] **Step 5: Document API 1.9 in `docs/extensions.md`.**
  - The header and the `initialize` example say 1.9.
  - The `api` guidance starts: `Set it to "1.9" when you use a sidebar-foot panel or the pixels node, to "1.8" when …`.
  - Add a `sidebar-foot` bullet under "Where it shows", with the rules from the spec's "`sidebar-foot` location" section: on every tab; top border; its natural height; one per manifest and the first extension wins; no rail button; `panel/open` does nothing; no `width`, `frame` or `wide`; no inner scroll; bubbles float above; menus show on hover.
  - Add a `pixels` row to the node table: `rows`, `scale`? and `id`?, with the spec's limits and colours.
  - Update the bichinho mention to "a pet in a dock at the sidebar's foot".

- [ ] **Step 6: Commit**
  `git add -A crates docs/extensions.md && git commit -m "feat: paint the pixels panel node"`

---

## Part 2: wai-bichinho 0.4.0

Run every Go command from `extensions/wai-bichinho`: `go test ./...` and `go vet ./...`.

### Task 5: Art helpers, the egg, the golden harness, and the Comum and Incomum forms

**Files:**
- Create: `extensions/wai-bichinho/art.go`, `extensions/wai-bichinho/art_test.go`, `extensions/wai-bichinho/testdata/forms.golden`
- Throwaway, not committed: `/tmp/pet-golden/gen.mjs`

**Interfaces:**
- Produces:

```go
const W, H = 36, 32
type grid [H][W]uint8                         // 0 empty, 1 dim, 2 full
type mask func(x, y float64) bool
type drawer interface {
	set(x, y float64, v uint8)
	part(m mask)
	ring(m mask, v uint8)
	fill(m mask, v uint8)
	pix(pts [][2]float64, v uint8)
	line(x0, y0, x1, y1 float64, v uint8)
	glyph(x, y float64, rows []string)
	scale() float64                           // the JS `p.s`
}
var forms map[string]func(d drawer, mood string, f int) // form id → drawing; "ovo" is the egg
// sprite draws form at (2+wobble, bob), then fx at (2, 0); art moods: idle happy talk hungry sleep.
func sprite(form, mood string, f int, shiny bool, bob int) grid
func (g *grid) rows() []string                // H strings of W chars: '.', '+', '#'
```

- [ ] **Step 1: Generate the golden file from the mockup.**
  - Write `/tmp/pet-golden/gen.mjs`. It reads `pet-dex.html`, takes the `<script>` text between `const W` and `const all =`, and appends the `ovo` drawing from `pet-sidebar-dock.html:236-240` as `C.ovo = (p, mood, f) => {…}`.
  - Evaluate the result with `vm.runInNewContext` and a stub `document` whose methods return `[]` and `{}`.
  - **What to print.** For every form in `TIERS` order, then `ovo`, print one block per (mood, frame). The pairs are `idle 0`, `happy 0`, `talk 1`, `hungry 1` and `sleep 2`; `ovo` gets `idle 0` and `idle 1` only.
  - **Block format.** A header line `<form> <mood> <frame>`, then 32 rows of 36 chars (`.`, `+`, `#`), then a blank line.
  - **How each grid is drawn.**
    - A fresh zeroed grid.
    - `C[key](pen(g, ox, 0), mood, f)`, where `ox` is 2, or `2 + [0,1,0,-1][f%4]` for `ovo`.
    - Then `fx(pen(g, 2, 0), mood, f, false)`.
  - Run `node /tmp/pet-golden/gen.mjs > extensions/wai-bichinho/testdata/forms.golden`.
  - Check the output: 41 forms, 202 blocks.

- [ ] **Step 2: Write the failing tests in `art_test.go`.**
  - **`TestFormsMatchTheMockup`.** It parses the golden file, and for each block whose form is in `forms`, it compares `sprite(form, mood, f, false, 0).rows()` row by row. On a mismatch it prints both grids. With `-update` (`var update = flag.Bool("update", false, "rewrite testdata/forms.golden from Go")`), it rewrites the file from Go for every form in the golden file.
  - **`TestEggWobbles`.** `sprite("ovo", "idle", 0, false, 0) != sprite("ovo", "idle", 1, false, 0)`.

- [ ] **Step 3: Run them and see them fail.**
  Run `go test -run 'TestForms|TestEgg' ./...`.
  Expected: they fail to compile.

- [ ] **Step 4: Port the helpers to `art.go`.**
  - **What to port.** From `pet-dex.html`: `ell`, `rrect`, `poly`, `and`, `or`, `not`, `tube`, `curve`, `pen`, `scaled`, `sw`, `EYES`, `SEYES`, `look`, `eyes`, `seye`, `mouth`, `fx` and `wave`.
  - **Rounding.** JS `Math.round` rounds .5 up, toward +∞, so port it as `func jsRound(v float64) float64 { return math.Floor(v + 0.5) }`. Never use `math.Round`, which rounds -0.5 to -1.
  - **Defaults.** JS default arguments become explicit: `v = 2` and `curve`'s `n = 24`.
  - **Pens.** `pen` is a struct over `*grid` with `ox`, `oy`, and `scale() == 1`. `scaled` wraps a drawer, with `cx = 16` and `by = 30.5` as in the JS.
  - **Bounds.** Out-of-bounds sets are ignored.
  - **Forms.** Port `ovo`, then the Comum and Incomum forms: `gosma`, `gotinha`, `gosmao`, `larvinha`, `bug`, `megabug`, `fagulha`, `fogo`, `bit`, `bot`, `alevino`, `abissal`, `bauzinho`, `mimico`, `grifinho`, `grifo`, `pedrinha`, `golem`, `corvinho`, `corvo`, `drone`, `ursinhocoruja`, `ursocoruja` and `ferrugem`. Port them as literally as Go allows; a baby is `forms["gosma"](scaled(d, .6), m, f)`, as in the JS.

- [ ] **Step 5: Run them and see them pass.**
  Run `go test ./...`.
  Expected: PASS. The forms not ported yet are skipped by the golden test.

- [ ] **Step 6: Commit**
  `git add extensions/wai-bichinho && git commit -m "chore: port the pet pixel art helpers and the first forms to Go"`

### Task 6: The Raro, Épico and Lendário forms

**Files:**
- Modify: `extensions/wai-bichinho/art.go`, `extensions/wai-bichinho/art_test.go`

**Interfaces:**
- Consumes: the drawer and helpers from Task 5.
- Produces: `forms` holding all 40 forms plus `ovo`.

- [ ] **Step 1: Make the golden test strict.** A golden form missing from `forms` now fails the test. Add **`TestEveryFormDraws`**: for every key of `forms`, every art mood and frames 0 to 7, `sprite(…, true, 1)` doesn't panic and draws at least 20 non-empty cells.
- [ ] **Step 2: Run it and see it fail.**
  Run `go test ./...`.
  Expected: FAIL, listing the 16 missing forms.
- [ ] **Step 3: Port the remaining forms.** They are `dragaozinho`, `dragao`, `caozinho`, `cerbero`, `cavalinho`, `hipocampo`, `cubo`, `fenix`, `sleipnir`, `mecha`, `verme`, `kraken`, `jormungandr`, `hidra`, `singularidade` and `tarrasque`. Port them from `pet-dex.html` lines 343 to 640, the same way as in Task 5.
- [ ] **Step 4: Run it and see it pass.**
  Run `go test ./... && go vet ./...`.
  Expected: PASS.
- [ ] **Step 5: Commit**
  `git commit -am "chore: port the rare, epic and legendary pet forms"`

### Task 7: The dex: lines, hatching, migration, persona and pixels in place of ASCII

**Files:**
- Modify: `extensions/wai-bichinho/pet.go`:
  - replace `Species`, `species`, `eggArt`, `speciesByID`, `mood`, `moods` and `draw`;
  - `Hatch` at `:197`.
- Modify: `extensions/wai-bichinho/main.go`:
  - `load()` at `:90`;
  - `promptTree` at `:247`;
  - `cardTree` at `:276`;
  - the hatch note at `:452`.
- Modify: `extensions/wai-bichinho/agent.go:50-52`, `:84`.
- Test: `main_test.go`, `agent_test.go`, `speech_test.go` (wherever `speciesByID` or `draw` appear).

**Interfaces:**
- Consumes: `sprite` and `grid.rows()`.
- Produces:

```go
type Form struct{ ID, Name string; Level int } // Level: reached at; the first form is 1
type Line struct{ Rarity Rarity; Forms []Form } // its id is Forms[0].ID
var dex []Line                                  // TIERS order: the source of form numbers #001…#040
func lineByID(id string) *Line                  // nil when unknown
func formOf(p *Pet) Form                        // the last form whose Level <= level(p.XP)
var oldSpecies = map[string]string{"tatu": "gotinha", "sapo": "larvinha", "jabuti": "alevino",
	"vagalume": "fagulha", "quati": "grifinho", "tucano": "corvinho", "preguica": "pedrinha",
	"lobo-guara": "caozinho", "boto": "cavalinho", "mico": "fenix", "curupira": "sleipnir",
	"boitata": "jormungandr"}
func pixels(g grid, scale int, id string) node  // {"type":"pixels","rows":…,"scale":…,"id"?}
```

- [ ] **Step 1: Write the failing tests.**
  - **`TestDexMatchesTheMockup`.**
    - `dex` has 25 lines and 40 forms, and every form id has an entry in `forms`.
    - Per rarity there are 6, 6, 4, 4 and 5 lines.
    - 3-form lines have Levels 1, 5 and 15, and 2-form lines have 1 and 10.
  - **`TestEggsFollowTheRarityOdds`.** Use `lineByID(p.Species).Rarity`.
  - **`TestLoadMigratesOldPets`.**
    - Write a 0.3.1-style `pet.json` with:
      - a pet `{"species":"tatu","xp":300}` (Lv 4);
      - past pets `boitata` and `zzz-unknown`;
      - seen `{"tatu":false,"mico":true}`.
    - After `start()`, check that:
      - `Pet.Species == "gotinha"`;
      - `formOf(Pet).ID == "gotinha"`;
      - Past holds one pet, `"jormungandr"`;
      - `Seen` equals `{"gotinha":false,"fenix":true,"jormungandr":false}`.
    - Each owned pet's current form is marked seen.
    - A second pet at `xp 5000`, which is Lv 15, shows form `gosmao` and marks it seen.
  - **`TestPersonaUsesTheForm`.** `persona(&Pet{Species:"gotinha", XP:400})` contains `"Gosma"` and `"myth, the sea, tech or fantasy"`, and not `"Brazilian fauna"`.

- [ ] **Step 2: Run them and see them fail.**
  Run `go test ./...`.
  Expected: they fail to compile.

- [ ] **Step 3: Implement.**
  - **The dex.** `dex` mirrors `TIERS`, with names in pt-BR exactly as the mockup has them. Its rarities map Comum to Common, Incomum to Uncommon, Raro to Rare, Épico to Epic and Lendário to Legendary.
  - **Hatch.** It picks a uniform line of the rolled rarity, sets `Species` to the line id, and marks `Seen[line id]`.
  - **load().**
    - Map ids through `oldSpecies` when `lineByID` is nil, and drop the ones still unknown.
    - Remap `Seen` keys the same way; ORing a shiny flag keeps true.
    - Delete unknown keys.
    - Mark the current form of the pet and of every past pet.
  - **The prompt tree.** It replaces the mono text with `pixels(sprite(formOf(p).ID, artMood, s.frame, p.Shiny, s.frame%2), 2, "pet")`, where `artMood` is the mapping in Task 9. For now, `idle` is fine for every mood.
  - **The card.**
    - `pixels(sprite(formOf(p).ID, "idle", 0, p.Shiny, 0), 3, "")` replaces the ASCII art. With no pet, it shows the egg.
    - The header line is `<Form> · born <date>`.
    - The Collection section has the badge `n/40`, over form ids in dex order. Its rows are `#001 Gotinha` (with ` ✦` when the form was seen shiny) or `#001 ???`, plus the rarity badge.
  - **The hatch note.** It uses the form name.
  - **Persona.** In `persona()`, use `"You are %s, a %s, a %s creature of myth, the sea, tech or fantasy; let its nature show."`, with `formOf(p).Name`. `request()` also uses `formOf(p).Name`.
  - **Deletions.** Delete `draw`, `mood`, `moods`, `eggArt`, `Species`, `species` and `speciesByID`. Delete `TestArtFitsTheSlot`. Make `TestMouthTalks` compare pixels rows between two talk frames.

- [ ] **Step 4: Run them and see them pass.**
  Run `go test ./... && go vet ./...`.
  Expected: PASS.

- [ ] **Step 5: Commit**
  `git commit -am "chore: give the pet a 40-form dex drawn in pixels"`

### Task 8: Evolution

**Files:**
- Modify:
  - `extensions/wai-bichinho/pet.go` (`TurnEnded` at `:230`);
  - `main.go`, the turn_ended handling at `:531-548` and the card header;
  - `lines.go`, adding `evolve` to `fallback` and `kinds`.
- Test: `main_test.go`, `speech_test.go`

**Interfaces:**
- Produces:
  - `func (st *State) TurnEnded(reason string) (levelUp, evolved, egg bool)`, which marks `Seen[newForm]` on evolving;
  - `func nextForm(p *Pet) (Form, bool)`.

- [ ] **Step 1: Write the failing tests.**
  - **`TestEvolvesAtTheLineLevels`.**
    - A `gotinha` pet at `xpFor(5)-10` evolves on the next `end_turn` (`evolved == true`). `formOf` becomes `gosma`, and `Seen["gosma"]` is set.
    - The same holds from `xpFor(15)-10` to `gosmao`.
    - A `grifinho` pet evolves at Lv 10, not at Lv 5.
    - A `drone` pet never evolves.
  - **`TestEvolveSpeaks`.**
    - Through the harness, set the pet's XP to `xpFor(5)-10` and send `event/turn_ended` with `end_turn`.
    - The bubble text is in `fallback["evolve"]`.
    - The diary's last note is `evolved into Gosma`.
    - `s.flash.mood == "happy"`.
  - **The card.** A `gosma` pet's card shows `evolves at Lv 15`; a `gosmao` pet's card shows nothing about evolving.

- [ ] **Step 2: Run them and see them fail.**
  Run `go test ./...`.
  Expected: they fail to compile.

- [ ] **Step 3: Implement.**
  - **The turn handler.**
    - On `evolved`, call `s.note("evolved into " + form.Name)`.
    - The switch checks `evolved` before `levelUp`, and calls `s.react("happy", …)` and `s.say("evolve")`.
    - The Claude pools already regenerate on a level change (`maybeGenerate` compares `LinesLevel`), so nothing else is needed for them.
  - **Speech.** Add `"evolve": {"Evoluí!"}` to `fallback` and `{"evolve", 2, false, false, "the pet just evolved into a new form"}` to `kinds`.
  - **The card header.** It becomes `<Form> · born <date> · evolves at Lv N` when `nextForm` is ok.

- [ ] **Step 4: Run them and see them pass.**
  Run `go test ./...`.
  Expected: PASS.

- [ ] **Step 5: Commit**
  `git commit -am "chore: evolve the pet at its line's levels"`

### Task 9: The `sidebar-foot` dock: walking stage, name row, mood art

**Files:**
- Modify: `extensions/wai-bichinho/wai-extension.toml`:
  - `version = "0.4.0"`, `api = "1.9"`;
  - the pet panel: `location = "sidebar-foot"`, with no width and no frame;
  - the description: "A small pet that walks in a dock at the foot of the sidebar, grows with your agents' finished turns, evolves, and talks in a personality Claude writes from its stats".
- Modify: `extensions/wai-bichinho/main.go`:
  - `frameEvery` becomes 250 ms and `listEvery` becomes 120;
  - `tick()`;
  - `promptTree` becomes `dockTree`;
  - `mood()` and `react()` lose the status text;
  - `menu()`.
- Modify: `crates/wai-ext/tests/manifest.rs:198-216`. The bichinho expectation becomes `(slot.location, slot.width, slot.frame) == (PanelLocation::SidebarFoot, None, None)`, and its comment becomes "a pet in the sidebar's foot".
- Test: `main_test.go`

**Interfaces:**
- Produces:

```go
const stageW, stageH = 128, 31
type walk struct{ x, dir, pause int }      // dir starts at 1
func (w *walk) step(rng *rand.Rand) bool   // the mockup's tick: true when it moved
func artMood(mood string) string           // idle working→idle; happy surprise→happy; talk alert error→talk; hungry sad→hungry; sleeping→sleep; egg→egg
func stage(g grid, x int, flip bool) []string // stageH rows of stageW chars
func (s *server) dockTree(now time.Time) node
func (s *server) mood(now time.Time) string
func (s *server) react(mood string)
```

- [ ] **Step 1: Write the failing tests.**
  - **`TestWalkStaysOnStage`.**
    - Run 20 000 steps with `seeded()`. Every 500 steps, the test's own loop toggles the mood between idle and hungry; `tick` steps only in `idle`.
    - Assert `0 <= x <= 92` throughout.
    - `x` changes only on idle ticks.
    - Both edges are reached.
  - **`TestDockTree`.**
    - After hatching, `dockTree` has, in order, an optional bubble, then a row of text `p.Title()`, a spacer and a menu, then a `pixels` node `id "pet"` at scale 2 with 31 rows of 128 chars.
    - With no pet, the text is `"Egg"` and the stage shows the egg.
  - **`TestMenuStaysPutWhileAnimating`.** Keep it, on `dockTree`'s row.
  - **`TestStageFlipsWhenFacingLeft`.** `stage(g, 10, true)` row j equals the reverse of the sprite's row j inside columns 10 to 45.
  - **`TestWorkingAndAlertGlyphs`.**
    - The working stage differs from the idle stage at the same frame.
    - The alert stage differs from the plain talk stage.
  - **`TestProtocol`.** Rewrite it.
    - Replace `promptOf`'s badge and word with `dockOf(t, msgs) (name string, rows []string)`.
    - Replace each status-word assertion with `h.s.mood(h.now)`. The moods in order are `egg`, `surprise` after hatching, `happy` after the turn, `working`, `alert`, `idle`, `hungry`, then `happy` after feeding.
    - The "a frame does not bob" check compares stage rows.
    - Clicking the stage (`action_id "pet"`) pets the pet.

- [ ] **Step 2: Run them and see them fail.**
  Run `go test ./...`.
  Expected: they fail to compile.

- [ ] **Step 3: Implement.**
  - **tick().**
    - `s.frame++` runs always.
    - When there is a pet and `s.mood(now) == "idle"`, set `s.moved = s.walk.step(s.rng)`; otherwise `s.moved = false`.
    - Then render.
  - **walk.step.** It mirrors `pet-sidebar-dock.html:285-291`, with `rng.Float64() < .025` and `10 + rng.IntN(16)`.
  - **dockTree's sprite.** The pose is `f := s.frame / 2`, and the bob is:
    - `s.frame % 2` when moved;
    - 0 for sleep and the egg;
    - `s.frame % 2` when working;
    - `f % 2` otherwise.
  - **Mood mapping.** The art mood is `artMood(mood)`, with `talk` when idle and talking, as today.
  - **Extra glyphs.** Draw them on the sprite grid through `pen(&g, 2, 0)`:
    - when working, `glyph(27, 6, []string{[]string{"#....", "#.#..", "#.#.#"}[f%3]})`;
    - when alert or error, `glyph(30, 1, []string{"##", "##", "##", "..", "##"})`.
  - **The stage.** Flip it when `walk.dir < 0` and the pet is walking (the mood is idle).
  - **The menu.** It is unchanged here; Task 10 adds "Switch pet".
  - **Deletions.** Drop the `flash.text` field, the status strings and the badge. `maybeGenerate` uses the new `mood()`.

- [ ] **Step 4: Run the tests and see them pass.**
  Run `go test ./... && go vet ./...`, then `cargo test -p wai-ext --test manifest`.
  Expected: PASS.

- [ ] **Step 5: Commit**
  `git add -A extensions/wai-bichinho crates/wai-ext/tests/manifest.rs && git commit -m "chore: move the pet to a walking dock at the sidebar's foot"`

### Task 10: The pet selector, and the README

**Files:**
- Modify: `extensions/wai-bichinho/pet.go`:
  - add `Pet.StoredAt`;
  - `Hatch` stops capping and stamps `StoredAt`;
  - delete `maxPast`.
- Modify: `extensions/wai-bichinho/main.go`:
  - `act()`: add `use-<n>` and `switch`;
  - `menu()`;
  - `hatchItem()` confirm text;
  - `cardTree`: the "Your pets" table.
- Modify: `extensions/wai-bichinho/README.md`.
- Test: `main_test.go`

**Interfaces:**
- Produces:
  - `StoredAt time.Time \`json:"stored_at,omitempty"\``.
  - `func (st *State) Use(born int64, now time.Time) bool`, where `born` is `Born.UnixNano()`. It returns false and changes nothing when no past pet matches.

- [ ] **Step 1: Write the failing tests.**
  - **`TestUseSwapsAndFreezes`.**
    - Hatch A. Then, at t0+1h, hatch B, so A is stored with `StoredAt` = t0+1h. Record A's `Hunger` and `Affection` at that moment.
    - At t0+5 days, call `Use(A.Born.UnixNano(), now)`. It returns true.
    - A is now `st.Pet`, with the same hunger and love within 0.01.
    - B is `Past[0]` with `StoredAt == now`.
    - A no longer has a `StoredAt`.
    - `st.Lines == nil`.
  - **`TestUseWithoutStoredAt`.** A past pet from 0.3.1 has no `StoredAt`. After `Use`, its hunger and love are what they were at `max(FedAt, LovedAt)`.
  - **`TestUseIgnoresBadIDs`.**
    - Send the actions `use-abc`, `use-`, `use-123`, and `use-<the current pet's Born nano>`.
    - Each answers and leaves `st` deep-equal to before (`reflect.DeepEqual` on a JSON round-trip).
  - **`TestHatchKeepsEveryPet`.** After 30 hatches, `len(Past) == 29`, and every stored pet has a `StoredAt`.
  - **`TestPetsTable`.**
    - The card has the section `"Your pets"` (id `s-past`) with a table of the columns `Pet`, `Form` and `Lv`, plus a button column.
    - A row's button has id `use-<Born nano>` and label `Use`.
    - `menu()` has `{"id":"switch","label":"Switch pet"}` only when Past is not empty.
    - The hatch confirm text is `"<Name> goes to your pets. You can switch back any time."`.

- [ ] **Step 2: Run them and see them fail.**
  Run `go test ./...`.
  Expected: they fail to compile.

- [ ] **Step 3: Implement.**
  - **Use.**
    - Find the past pet whose `Born.UnixNano() == born`.
    - Compute `base := StoredAt`, or `max(FedAt, LovedAt)` when `StoredAt` is zero, and `shift := now.Sub(base)`.
    - Add `shift` to `FedAt` and `LovedAt`, and clear `StoredAt`.
    - Remove the pet from Past, then prepend the current pet with `StoredAt = now`.
    - Set `st.Pet` and `st.Lines = nil`.
  - **act().**
    - `"switch"` opens the card, as `"card"` does.
    - In the `default` branch, `strings.CutPrefix(action, "use-")` and `strconv.ParseInt` lead to `Use`. On success, call `s.note("switched to " + name)`, `s.react("surprise")` and `s.say("hatch")`.
  - **Hatch.** It prepends the current pet with `StoredAt = now`. Remove the cap and `maxPast`.
  - **The card.** The `s-past` section is titled "Your pets", with the badge `len(Past)`, open by default. Its table has the columns `Pet`, `Form`, `Lv` (width 40) and an untitled column (width 64). Each row is `[q.Title(), formOf(&q).Name, strconv.Itoa(level(q.XP)), {"type":"button","id":"use-<nano>","label":"Use"}]`.

- [ ] **Step 4: Update the README.** It covers:
  - the dock at the sidebar's foot (click to pet, ⋯ on hover);
  - the 25 lines and 40 forms by rarity, with the odds;
  - evolution at Lv 5 and 15, or at Lv 10;
  - shiny;
  - "Your pets" and Use, and that hunger and love freeze while a pet is stored;
  - "needs wai with extension API 1.9";
  - the migration of 0.3.x pets.

- [ ] **Step 5: Run the tests and see them pass.**
  Run `go test ./... && go vet ./...`.
  Expected: PASS.

- [ ] **Step 6: Commit**
  `git commit -am "chore: let the owner switch between their pets"`

### Task 11: Whole-branch check

- [ ] **Step 1: Run everything.**
  Run `cargo test -p wai-ext && cargo test -p wai && cargo clippy -p wai -p wai-ext -- -D warnings && (cd extensions/wai-bichinho && go test ./... && go vet ./...)`.
  Expected: all pass.
- [ ] **Step 2: Build a local wai, but don't install it.**
  Run `CI=true cargo packager --release` in the background.
  Expected: `target/release/wai.app` is built. Installing it is the owner's call.
- [ ] **Step 3: Clean up.**
  Run `cargo clean` to free the disk.
