package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"
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

func TestFormsMatchTheMockup(t *testing.T) {
	blocks := readGolden(t)
	if *update {
		var sb strings.Builder
		for _, b := range blocks {
			if _, ok := forms[b.form]; !ok {
				continue
			}
			g := sprite(b.form, b.mood, b.f, false, 0)
			fmt.Fprintf(&sb, "%s %s %d\n%s\n\n", b.form, b.mood, b.f, strings.Join(g.rows(), "\n"))
		}
		if err := os.WriteFile("testdata/forms.golden", []byte(sb.String()), 0o644); err != nil {
			t.Fatal(err)
		}
		return
	}
	checked := 0
	for _, b := range blocks {
		if _, ok := forms[b.form]; !ok {
			t.Errorf("form %q is in the golden file but not ported", b.form)
			continue
		}
		checked++
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
	if checked == 0 {
		t.Fatal("no golden block matched a ported form")
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
