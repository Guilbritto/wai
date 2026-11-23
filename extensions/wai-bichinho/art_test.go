package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"slices"
	"strings"
	"testing"
)

var update = flag.Bool("update", false, "rewrite testdata/forms.golden from Go")

type goldenBlock struct {
	form, mood string
	f          int
	rows       []string
}

func readGolden(t *testing.T) []goldenBlock {
	fh, err := os.Open("testdata/forms.golden")
	if err != nil {
		t.Fatal(err)
	}
	defer fh.Close()
	var out []goldenBlock
	sc := bufio.NewScanner(fh)
	for sc.Scan() {
		line := sc.Text()
		if line == "" {
			continue
		}
		var b goldenBlock
		if _, err := fmt.Sscanf(line, "%s %s %d", &b.form, &b.mood, &b.f); err != nil {
			t.Fatalf("bad header %q: %v", line, err)
		}
		for i := 0; i < H; i++ {
			if !sc.Scan() {
				t.Fatalf("truncated block %q", line)
			}
			b.rows = append(b.rows, sc.Text())
		}
		out = append(out, b)
	}
	return out
}

// goldenCases is every form in dex order, in the poses the golden pins, then the egg.
func goldenCases() []goldenBlock {
	poses := []struct {
		mood string
		f    int
	}{{"idle", 0}, {"happy", 0}, {"talk", 1}, {"hungry", 1}, {"sleep", 2}}
	var out []goldenBlock
	for _, l := range dex {
		for _, fm := range l.Forms {
			for _, ps := range poses {
				out = append(out, goldenBlock{form: fm.ID, mood: ps.mood, f: ps.f})
			}
		}
	}
	return append(out, goldenBlock{form: "ovo", mood: "idle", f: 0}, goldenBlock{form: "ovo", mood: "idle", f: 1})
}

func TestFormsMatchTheGolden(t *testing.T) {
	cases := goldenCases()
	if *update {
		var sb strings.Builder
		for _, c := range cases {
			g := sprite(c.form, c.mood, c.f, false, 0)
			fmt.Fprintf(&sb, "%s %s %d\n%s\n\n", c.form, c.mood, c.f, strings.Join(g.rows(), "\n"))
		}
		if err := os.WriteFile("testdata/forms.golden", []byte(sb.String()), 0o644); err != nil {
			t.Fatal(err)
		}
		return
	}
	blocks := readGolden(t)
	if len(blocks) != len(cases) {
		t.Fatalf("golden has %d blocks, want %d", len(blocks), len(cases))
	}
	for i, b := range blocks {
		c := cases[i]
		if b.form != c.form || b.mood != c.mood || b.f != c.f {
			t.Fatalf("block %d is %s %s %d, want %s %s %d", i, b.form, b.mood, b.f, c.form, c.mood, c.f)
		}
		g := sprite(b.form, b.mood, b.f, false, 0)
		got := g.rows()
		for y := range b.rows {
			if got[y] != b.rows[y] {
				t.Errorf("%s %s %d differs at row %d\nwant:\n%s\ngot:\n%s", b.form, b.mood, b.f, y,
					strings.Join(b.rows, "\n"), strings.Join(got, "\n"))
				break
			}
		}
	}
}

func TestEggWobbles(t *testing.T) {
	if sprite("ovo", "idle", 0, false, 0) == sprite("ovo", "idle", 1, false, 0) {
		t.Error("the egg frames are identical")
	}
}

func TestEveryFormDraws(t *testing.T) {
	for name := range forms {
		for _, mood := range []string{"idle", "talk", "happy", "sleep", "hungry"} {
			for f := 0; f < 8; f++ {
				n := 0
				for _, row := range sprite(name, mood, f, true, 1) {
					for _, v := range row {
						if v != 0 {
							n++
						}
					}
				}
				if n < 20 {
					t.Errorf("%s %s %d draws only %d cells", name, mood, f, n)
				}
			}
		}
	}
}

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

var preview = flag.String("preview", "", "write an HTML page of every form to this file")

// svgOf draws rows as crisp cells: '#' in fg, '+' in fg at 40%, like wai's pixels node.
func svgOf(rows []string, scale int, fg string) string {
	var b strings.Builder
	fmt.Fprintf(&b, `<svg width="%d" height="%d" viewBox="0 0 %d %d" shape-rendering="crispEdges" fill="%s">`, W*scale, H*scale, W, H, fg)
	for y, row := range rows {
		for x := 0; x < len(row); {
			n := 1
			for x+n < len(row) && row[x+n] == row[x] {
				n++
			}
			switch row[x] {
			case '#':
				fmt.Fprintf(&b, `<rect x="%d" y="%d" width="%d" height="1"/>`, x, y, n)
			case '+':
				fmt.Fprintf(&b, `<rect x="%d" y="%d" width="%d" height="1" fill-opacity=".4"/>`, x, y, n)
			}
			x += n
		}
	}
	return b.String() + "</svg>"
}

func mirrored(rows []string) []string {
	out := make([]string, len(rows))
	for i, r := range rows {
		b := []byte(r)
		slices.Reverse(b)
		out[i] = string(b)
	}
	return out
}

// TestPreview writes the owner's review page: a fragment the brainstorming companion wraps.
func TestPreview(t *testing.T) {
	if *preview == "" {
		t.Skip("pass -preview <file> to write the page")
	}
	const dark, light = "#dcdce0", "#1e1e22"
	rows := func(id, mood string, f int) []string { g := sprite(id, mood, f, false, 0); return g.rows() }
	var b strings.Builder
	b.WriteString(`<style>
.dex{display:flex;flex-wrap:wrap;gap:10px;margin-bottom:18px}
.pet{border-radius:10px;padding:8px;background:#f0f0f2;font-size:12px}
.pet b{display:block;margin-bottom:4px}
.d{background:#1e1e22;border-radius:6px;padding:6px;display:flex;gap:6px;align-items:flex-end;flex-wrap:wrap;max-width:320px}
.l{background:#fff;border-radius:6px;padding:6px;margin-top:4px;display:inline-block;border:1px solid #ddd}
</style>
<h2>Bichinhos: todas as formas</h2>
<p class="subtitle">Cada cartão: parado no tamanho real (2px), espelhado e ampliado; os humores (feliz, falando, com fome, dormindo); e o tema claro.</p>
`)
	card := func(title, id string) {
		fmt.Fprintf(&b, `<div class="pet"><b>%s</b><div class="d">%s%s%s</div><div class="d" style="margin-top:4px">`,
			title, svgOf(rows(id, "idle", 0), 2, dark), svgOf(mirrored(rows(id, "idle", 0)), 2, dark), svgOf(rows(id, "idle", 0), 5, dark))
		for _, ps := range []struct {
			mood string
			f    int
		}{{"happy", 0}, {"talk", 1}, {"hungry", 1}, {"sleep", 2}} {
			b.WriteString(svgOf(rows(id, ps.mood, ps.f), 2, dark))
		}
		fmt.Fprintf(&b, `</div><div class="l">%s</div></div>`, svgOf(rows(id, "idle", 0), 2, light))
	}
	b.WriteString(`<div class="dex">`)
	card("Ovo", "ovo")
	b.WriteString(`</div>`)
	for r := range rarities {
		fmt.Fprintf(&b, `<h3>%s</h3><div class="dex">`, rarities[r].name)
		no := 0
		for _, l := range dex {
			for _, fm := range l.Forms {
				no++
				if l.Rarity == Rarity(r) {
					card(fmt.Sprintf("#%03d %s", no, fm.Name), fm.ID)
				}
			}
		}
		b.WriteString(`</div>`)
	}
	if err := os.WriteFile(*preview, []byte(b.String()), 0o644); err != nil {
		t.Fatal(err)
	}
}

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

// Every form fits its box at x 2..35: nothing is cut at the grid's edges, at rest or bobbing a row down.
func TestFormsStayOnTheGrid(t *testing.T) {
	draw := func(id, mood string, f, ox, oy int) grid { var g grid; forms[id](pen{&g, ox, oy}, mood, f); return g }
	used := func(cells ...uint8) bool { return slices.ContainsFunc(cells, func(v uint8) bool { return v != 0 }) }
	col := func(g grid, x int) []uint8 {
		out := make([]uint8, H)
		for y := range g {
			out[y] = g[y][x]
		}
		return out
	}
	for id := range forms {
		for _, mood := range []string{"idle", "talk", "happy", "sleep", "hungry"} {
			for f := 0; f < 8; f++ {
				var edges []string
				if used(col(draw(id, mood, f, 3, 0), 0)...) {
					edges = append(edges, "left")
				}
				if used(col(draw(id, mood, f, 1, 0), W-1)...) {
					edges = append(edges, "right")
				}
				if g := draw(id, mood, f, 2, 1); used(g[0][:]...) {
					edges = append(edges, "top")
				}
				if g := draw(id, mood, f, 2, 0); used(g[H-1][:]...) {
					edges = append(edges, "bottom")
				}
				if edges != nil {
					t.Errorf("%s %s %d is cut at the %v", id, mood, f, edges)
					break
				}
			}
		}
	}
}
