# Pet Art Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every wai-bichinho form reads as what it is at dock size. The solid pen fills bodies, all 40 forms get a new drawing, and five niche lines become Kobold, Beholder, Fada, Tiamat and Goblin.

**Architecture:** The art stays in `art.go` as Go drawing functions over a 36×32 grid of three values. The pen changes once, globally: a part's inside becomes dim, and `cut` makes holes. Then the forms are redrawn rarity by rarity. The golden file is regenerated from Go and pins every form. A test-only `-preview` flag writes an HTML page for the owner's review.

**Tech Stack:** Go (standard library only), `go test`, the brainstorming visual companion for the owner's review.

**Spec:** `docs/superpowers/specs/2026-11-23-pet-art-redesign-design.md`

## Global Constraints

- All work is in `extensions/wai-bichinho/`, and every `go` command runs from there. Do not touch wai (`crates/`). The extension API stays `"1.9"`.
- Branch `feat/pet-art-redesign` from `development`. Commits are `chore:`, have no scope and no `Co-Authored-By`, and use the imperative mood. Another session commits in this repo (for example `crates/wai/src/chat/*`), so stage only the files your task names.
- The grid is 36×32. Forms draw through `pen{&g, 2, bob}`, so creature x 0..33 is grid x 2..35. Cell values are 0 (empty), 1 (dim, the theme's foreground at 40%) and 2 (full). No new shades.
- **Solid style.** A body is a `part` (full edge, dim inside). Details are full lines or holes (`cut`, or a dim stroke over a dim body). No hollow outlines, and no form that is another form scaled down.
- Each form exaggerates its defining trait from the art brief below. It must read at 2 px cells without its name.
- Keep grid (29..35, 0..9) clear enough that `fx`'s hearts, z's and hunger marks and the dock's `!` stay readable.
- The bar: the four JS samples in `/Users/guilherme.brito/Project/Guilherme/wai/.superpowers/brainstorm/73005-1795442593/lab/lab.js` (`C2.dragao_c2`, `C2.caozinho_c`, `C2.mimico_c2`, `C2.beholder_c`; the pen there is `spen`, the eyes `beye`). Port them as written: JS `p.cut` is Go `p.cut`, JS `beye(p,x,y,mood,f)` is Go `eyes(p, xs(x), y, mood, f)`, and the small eye is `seye`.

### Art brief: the trait each form exaggerates

| Form | Trait |
|---|---|
| gotinha | Small drop-shaped slime, big eyes, a shine highlight |
| gosma | Dome slime with drips at its base and a shine highlight |
| gosmao | Huge slime with something swallowed showing inside (a bone, a coin) and drips |
| larvinha | Caterpillar: round segments, antennae, a row of tiny legs |
| bug | Beetle: shell with a centre split, six legs, antennae |
| megabug | Rhinoceros beetle: a big horn, wing cases opened over wings |
| fagulha | A small flame with a face |
| fogo | Will-o'-wisp: a floating ghostly flame with a trailing tail |
| bit | A tiny cube robot with an antenna |
| bot | A robot with a screen face, an antenna and arms |
| alevino | A small fish: big eye, tail fin |
| abissal | Anglerfish: a glowing lure on a stalk, a jaw of teeth |
| bauzinho | A small chest, lid ajar, a few teeth peeking |
| mimico | Port `C2.mimico_c2` |
| goblin | Big pointed ears, long nose, toothy grin, a small dagger |
| grifinho | Griffin chick: eagle head and beak, stubby wings, lion tail tuft |
| grifo | Eagle head, wings spread, lion body with a tufted tail |
| pedrinha | A small rock with cracks and a sprout |
| golem | A blocky stone giant: huge fists, a rune on the chest |
| corvinho | A small crow: big beak, folded wing |
| corvo | Raven on a branch: big beak, a wing half open |
| drone | Quadcopter: four rotor bars on arms, a camera eye (must not read as a crab) |
| ursinhocoruja | Owlbear cub: owl face (beak, ear tufts) on a round bear body |
| ursocoruja | Owlbear: big bear body and claws, owl face with a beak |
| kobold | Small reptile humanoid: lizard snout, horn nubs, tail, a spear |
| dragaozinho | Baby dragon: big head, horn nubs, small bat wings, tail |
| dragao | Port `C2.dragao_c2` |
| caozinho | Port `C2.caozinho_c` |
| cerbero | Three dog heads with snouts and noses, a spiked collar |
| cavalinho | Seahorse: upright S, tube snout, crown spikes, tightly curled tail |
| hipocampo | Horse front with a mane, fish tail, waves |
| cubo | Gelatinous cube: a see-through cube with a skull and bones inside |
| fenix | Bird with flame wings and long flame tail feathers |
| beholder | Port `C2.beholder_c` |
| mecha | Giant robot: cockpit head, shoulder cannons |
| fada | Tiny person with big butterfly wings and a wand with a star |
| kraken | Octopus: big head, curling tentacles with suckers |
| jormungandr | World serpent biting its own tail (a ring) |
| hidra | Wingless serpent body, several snake heads on long necks |
| tiamat | Five horned dragon heads on long necks, bat wings, one body |

## Review Focus

- **Light theme.** There, full cells are dark and holes are light, so a pupil or mouth drawn as a hole turns light. The preview shows a light-theme row, and the owner reviews both themes (Tasks 2 and 8).
- **Moods on unusual faces.** Happy and sleep must change the form's own face, not only `fx`'s hearts and z's. `TestEveryFaceShowsTheMood` pins this (Task 7). Until then, check the preview's mood row in each art task.
- **Stored pets of removed lines.** A shiny, high-level pet of a removed line, a stored one, a `Seen` entry and a 0.3.1 `curupira` must all carry over with their shiny flag (Task 3).
- **Clipping.** `pen.set` silently drops cells outside the grid, so a drawing that runs past creature x 33 or y 31 loses part of itself with no error. Check this on the zoomed preview in every art task.
- **Mirroring.** The stage flips the sprite when the pet walks left, so a side-view form must read facing either way. Check it on the preview's flipped column (Task 2).

---

### Task 1: The solid pen

**Files:**
- Modify: `art.go` (`drawer`, `pen.set`, `pen.part`, `pen.fill`, `pen.glyph`, `scaledPen`, `EYES`, `SEYES`)
- Test: `art_test.go`, `testdata/forms.golden`

**Interfaces:**
- Produces: `drawer.cut(m mask)`, which sets the cells of `m` to 0. `pen.put(x, y float64, v uint8)` is a raw clipped write. `pen.set` now turns a dim cell drawn over a dim cell into 0. `part` writes 2 on the edge and 1 inside. `EYES`/`SEYES` take the new shapes.

- [ ] **Step 1: Write the failing tests** in `art_test.go`:

```go
func TestSolidPartFillsTheInside(t *testing.T) {
	var g grid
	pen{&g, 0, 0}.part(rrect(2, 2, 6, 6, 0))
	if g[2][4] != 2 || g[4][2] != 2 || g[4][4] != 1 || g[3][3] != 1 {
		t.Errorf("edge/inside: %v %v %v %v", g[2][4], g[4][2], g[4][4], g[3][3])
	}
}

func TestDimOverDimIsAHole(t *testing.T) {
	var g grid
	p := pen{&g, 0, 0}
	p.part(rrect(2, 2, 8, 8, 0))
	p.line(4, 5, 6, 5, 1)
	p.set(12, 12, 1)
	if g[5][4] != 0 || g[5][6] != 0 || g[12][12] != 1 {
		t.Errorf("hole %v %v, dim on empty %v", g[5][4], g[5][6], g[12][12])
	}
}

func TestCutClears(t *testing.T) {
	var g grid
	p := pen{&g, 0, 0}
	p.part(rrect(2, 2, 8, 8, 0))
	p.cut(rrect(4, 4, 5, 5, 0))
	if g[4][4] != 0 || g[5][5] != 0 || g[6][6] != 1 {
		t.Errorf("cut: %v %v %v", g[4][4], g[5][5], g[6][6])
	}
}
```

- [ ] **Step 2: Run them.** `go test -run 'TestSolid|TestDimOver|TestCut' ./...` should FAIL to compile, because `pen.cut` is undefined.

- [ ] **Step 3: Implement it in `art.go`.**
  - Add `cut(m mask)` to `drawer`.
  - On `pen`: add `put` (today's `set` body), make `set` call `put` with 0 when `v == 1` and the cell is already 1, have `part` and `fill` write through `put` (a part's edge 2, its inside 1; the inner-staircase rule goes), add `cut(m)` as `fill(m, 0)`, and make `glyph` write `#` and `o` through `put` and `+` through `set`.
  - On `scaledPen`: add `cut(m) { d.p.cut(d.T(m)) }`.
  - New eyes, matching the samples: `EYES = {idle: {"###", "#o#", "###"}, blink: {"...", "###", "..."}, happy: {".#.", "#.#", "..."}}` and `SEYES = {idle: {"##", "#o"}, blink: {"..", "##"}, happy: {".#", "#."}}`.

- [ ] **Step 4: Regenerate the golden.** `go test -run TestFormsMatchTheMockup -update ./...`, then `go test ./...` should PASS.

- [ ] **Step 5: Commit.**

```bash
git add art.go art_test.go testdata/forms.golden
git commit -m "chore: fill the pet's parts and let it cut holes"
```

### Task 2: A golden that pins every form, and a preview page

**Files:**
- Modify: `art_test.go` (`TestFormsMatchTheMockup` → `TestFormsMatchTheGolden`, plus a new `TestPreview`)
- Test: `testdata/forms.golden`

**Interfaces:**
- Consumes: `dex`, `forms`, `sprite`, `grid.rows()`.
- Produces: `go test -run TestFormsMatchTheGolden -update ./...` rewrites the golden from `goldenCases()`. `go test -run TestPreview -preview <file> ./...` writes the review page. Later tasks run both.

- [ ] **Step 1: Write the failing test.** Add `goldenCases() []goldenBlock`, which lists every form in `dex` order with the poses `idle 0`, `happy 0`, `talk 1`, `hungry 1`, `sleep 2`, then `ovo idle 0` and `ovo idle 1`, with empty `rows`. Rename the test to `TestFormsMatchTheGolden`. In update mode it writes exactly `goldenCases()` (header `form mood f`, the 32 rows, a blank line). In check mode it fails when the golden's headers differ from `goldenCases()` (count or order), with the message `golden has %d blocks, want %d` or `block %d is %s %s %d, want %s %s %d`. Rows are compared as today.

- [ ] **Step 2: Run it.** `go test -run TestFormsMatchTheGolden ./...` should PASS. Today's golden already holds exactly these 202 blocks (40 forms × 5 poses, then the egg's 2) in this order. The header check starts to matter in Task 3, when the roster changes. To see it fail, temporarily swap two lines in `dex`: it should report `block 0 is gotinha idle 0, want …`. Then undo the swap.

- [ ] **Step 3: Verify update mode.** `go test -run TestFormsMatchTheGolden -update ./... && git diff --stat testdata/forms.golden` should show no change.

- [ ] **Step 4: Add `TestPreview`.** A `-preview` string flag, and the test skips when it is empty. It writes an HTML fragment (no `<html>`; the visual companion wraps it) with one section per rarity in `dex` order. Each form shows:
  - its `#NNN Name`;
  - idle frame 0 as inline SVG at 2 px and at 5 px, plus the same at 2 px mirrored (the walking-left case);
  - a row of `happy 0`, `talk 1`, `hungry 1`, `sleep 2` at 2 px;
  - idle at 2 px on a light card.

  Draw it like `lab/gen.py`: one `<rect>` per run of equal cells, `shape-rendering="crispEdges"`, dim cells with `fill-opacity=".4"`. Use foreground `#dcdce0` on card `#1e1e22` for dark, and `#1e1e22` on `#ffffff` for light. The egg comes first.

- [ ] **Step 5: Check it.** `go test -run TestPreview -preview /tmp/pets.html ./...` should PASS and write a file with 41 entries. Open the page with the companion and check it shows the current art. Start the companion if it is not running: `bash /Users/guilherme.brito/.claude/plugins/cache/claude-plugins-official/superpowers/7.0.2/skills/brainstorming/scripts/start-server.sh --project-dir /Users/guilherme.brito/Project/Guilherme/wai`, then copy the file into the `screen_dir` it prints, as `pets-<n>.html`.

- [ ] **Step 6: Commit.**

```bash
git add art_test.go testdata/forms.golden
git commit -m "chore: pin every pet form in the golden and preview them"
```

### Task 3: The new roster and the migration

**Files:**
- Modify: `pet.go` (`dex`, `oldSpecies`), `art.go` (remove `C["ferrugem"]`, `C["sleipnir"]`, `C["verme"]`, `C["singularidade"]`, `C["tarrasque"]`; add `C["kobold"]`, `C["beholder"]`, `C["fada"]`, `C["tiamat"]`, `C["goblin"]`), `README.md`
- Test: `main_test.go`, `testdata/forms.golden`

**Interfaces:**
- Consumes: the solid pen (Task 1), golden/preview (Task 2).
- Produces: `dex` with 7/6/4/4/4 lines per rarity.

- [ ] **Step 1: Write the failing tests** in `main_test.go`. Rename `TestDexMatchesTheMockup` to `TestDexShape`, and change its per-rarity want to `[]int{7, 6, 4, 4, 4}`. Add:

```go
func TestLoadMigratesReplacedLines(t *testing.T) {
	dir := t.TempDir()
	body := `{"pet":{"name":"A","species":"verme","xp":5000,"shiny":true},` +
		`"past":[{"name":"B","species":"ferrugem"},{"name":"C","species":"sleipnir"},{"name":"D","species":"tarrasque"},{"name":"E","species":"singularidade"},{"name":"F","species":"curupira"}],` +
		`"eggs":0,"seen":{"verme":true,"ferrugem":false,"singularidade":false}}`
	if err := os.WriteFile(filepath.Join(dir, stateFile), []byte(body), 0o600); err != nil {
		t.Fatal(err)
	}
	st := start(t, dir).s.st
	if st.Pet.Species != "beholder" {
		t.Errorf("pet: %q", st.Pet.Species)
	}
	var past []string
	for _, p := range st.Past {
		past = append(past, p.Species)
	}
	if want := []string{"kobold", "fada", "tiamat", "goblin", "fada"}; !reflect.DeepEqual(past, want) {
		t.Errorf("past: %v, want %v", past, want)
	}
	if want := map[string]bool{"beholder": true, "kobold": false, "goblin": false, "fada": false, "tiamat": false}; !reflect.DeepEqual(st.Seen, want) {
		t.Errorf("seen: %v, want %v", st.Seen, want)
	}
}
```

- [ ] **Step 2: Run them.** `go test -run 'TestDexShape|TestLoadMigratesReplacedLines' ./...` should FAIL: `Common: 6 lines, want 7`, and the pet is dropped. Once the roster changes, `TestFormsMatchTheGolden` also fails, on the first replaced block, until Step 6 regenerates the golden.

- [ ] **Step 3: Change the roster in `pet.go`.**
  - Common: append `ln(Common, fm("goblin", "Goblin", 1))` right after the `bauzinho`/`mimico` line.
  - Replace `ferrugem` with `ln(Uncommon, fm("kobold", "Kobold", 1))`, `sleipnir` with `ln(Epic, fm("fada", "Fada", 1))`, `verme` with `ln(Epic, fm("beholder", "Beholder", 1))`, and `tarrasque` with `ln(Legendary, fm("tiamat", "Tiamat", 1))`. Delete `singularidade`.
  - In `oldSpecies`, add `"ferrugem": "kobold", "verme": "beholder", "sleipnir": "fada", "tarrasque": "tiamat", "singularidade": "goblin"`, and change `"curupira"` to `"fada"`.

- [ ] **Step 4: Draw the five new forms in `art.go`** following the art brief, and delete the five removed drawings. Port `beholder` from `C2.beholder_c`. Each of the five needs a face that `eyes`/`seye`/`mouth` change with the mood. For the Fada: a body under 12 px wide and wings that carry the silhouette.

- [ ] **Step 5: Update `README.md`.**
  - The Forms bullet becomes: `25 lines and 40 forms, drawn in pixels: 7 common, 6 uncommon, 4 rare, 4 epic and 4 legendary lines.`
  - At the end of the "Pets from 0.3.x" paragraph, add: `Pets of the lines 0.5.0 replaced become their successors: Monstro da ferrugem to Kobold, Verme púrpura to Beholder, Sleipnir to Fada, Tarrasque to Tiamat and Singularidade to Goblin.`

- [ ] **Step 6: Regenerate and verify.** Run `go test -run TestFormsMatchTheGolden -update ./...`, then `go test ./...` should PASS. Run `go test -run TestPreview -preview /tmp/pets.html ./...` and check the five new forms at 2 px, in both themes and mirrored, with nothing clipped.

- [ ] **Step 7: Commit.**

```bash
git add pet.go art.go main_test.go README.md testdata/forms.golden
git commit -m "chore: swap the pet's niche lines for kobold, beholder, fada, tiamat and goblin"
```

### Tasks 4–7: Redraw the forms, rarity by rarity

Each of these tasks follows the same steps; only the forms change.

| Task | Forms (art brief above) | Commit message |
|---|---|---|
| 4 | Common: gotinha, gosma, gosmao, larvinha, bug, megabug, fagulha, fogo, bit, bot, alevino, abissal, bauzinho, mimico (port) | `chore: redraw the common pets` |
| 5 | Uncommon: grifinho, grifo, pedrinha, golem, corvinho, corvo, drone, ursinhocoruja, ursocoruja | `chore: redraw the uncommon pets` |
| 6 | Rare: dragaozinho, dragao (port), caozinho (port), cerbero, cavalinho, hipocampo, cubo | `chore: redraw the rare pets` |
| 7 | Epic and legendary: fenix, mecha, kraken, jormungandr, hidra; and check that the egg (`ovo`) reads filled. Before Step 1, add `TestEveryFaceShowsTheMood` (below) to `art_test.go`; today it FAILs with `mecha: happy or sleep looks like idle` | `chore: redraw the epic and legendary pets` |

The guard test for Task 7:
```go
// Happy and sleep change the form's own drawing, not only fx's hearts and z's.
func TestEveryFaceShowsTheMood(t *testing.T) {
	draw := func(id, mood string) grid { var g grid; forms[id](pen{&g, 2, 0}, mood, 0); return g }
	for id := range forms {
		if id == "ovo" {
			continue
		}
		idle := draw(id, "idle")
		if draw(id, "happy") == idle || draw(id, "sleep") == idle {
			t.Errorf("%s: happy or sleep looks like idle", id)
		}
	}
}
```

**Files:**
- Modify: `art.go` (the listed `C["…"]` entries)
- Test: `testdata/forms.golden` (and `art_test.go` in Task 7)

**Interfaces:**
- Consumes: `drawer` (`part`, `cut`, `ring`, `fill`, `pix`, `line`, `glyph`, `set`), `eyes`, `seye`, `mouth`, `ell`, `rrect`, `poly`, `tube`, `curve`, `and`, `or`, `not`.
- Produces: in Task 5, `baby`, `scaled`, `scaledPen`, `drawer.scale()` and `sw` are deleted, because Tasks 4 and 5 replace every `baby(...)` form. If only `pen` implements `drawer` after that, forms take `p pen` and `drawer` goes. Calls to `sw(p, w)` become plain ints.

- [ ] **Step 1: Redraw each listed form** to its art-brief trait, in the solid style. Each baby form (gotinha, alevino, bauzinho, grifinho, corvinho, ursinhocoruja) gets its own drawing, with a big head and a short body, not `baby(...)`. Mouths and eyes go through `eyes`/`seye`/`mouth`, so the moods keep working.

- [ ] **Step 2: Regenerate the golden.** `go test -run TestFormsMatchTheGolden -update ./...`

- [ ] **Step 3: Run the tests.** `go test ./...` should PASS, including `TestEveryFormDraws` (and `TestEveryFaceShowsTheMood` from Task 7 on).

- [ ] **Step 4: Check the preview.** `go test -run TestPreview -preview /tmp/pets.html ./...`, then open it in the companion. Every redrawn form must:
  - name itself at 2 px, judged against the art brief;
  - read in the light theme and mirrored;
  - have nothing clipped;
  - leave the top-right fx corner legible.

  Fix and repeat until all of them pass.

- [ ] **Step 5: Commit.**

```bash
git add art.go testdata/forms.golden   # plus art_test.go in Task 7
git commit -m "<the task's commit message>"
```

### Task 8: Owner review and release 0.5.0

**Files:**
- Modify: `wai-extension.toml` (`version = "0.5.0"`), `../index.toml` (after the release)

- [ ] **Step 1: Show the owner.** Run `go test -run TestPreview -preview /tmp/pets.html ./...`, put the page in the companion, and send the owner the URL. **Stop until the owner approves.** Each form the owner calls out goes back through Tasks 4–7's steps 1–5, then the page is shown again.

- [ ] **Step 2: Bump and verify.** Set `version = "0.5.0"` in `wai-extension.toml`. Then `go vet ./... && go test ./...` should PASS, and `go build -o bin/wai-bichinho .` should succeed.

- [ ] **Step 3: Commit.**

```bash
git add wai-extension.toml
git commit -m "chore: release the redrawn pets as wai-bichinho 0.5.0"
```

- [ ] **Step 4: Release, with the owner's go-ahead.** Pushing is outward-facing, so ask the owner first. Then merge into `development`; after `git log --oneline -5 development`, fast-forward `main` with `git branch -f main development` (only when `main` is an ancestor of `development`). Push both, then tag and push `wai-bichinho-v0.5.0`. `.github/workflows/extensions.yml` builds the packages.

- [ ] **Step 5: List it.** Replace the `wai-bichinho` entry in `extensions/index.toml` with the release's `index-entry.toml`. Check with `cargo run -q -p wai-ext --bin wai-index-check -- extensions/index.toml` from the repo root, then commit `chore: list wai-bichinho 0.5.0 in the official index`, as for 0.4.0.
