package main

import (
	"math"
	"strings"
)

// Pixel art for the pets: each form draws itself with a pen on a 36×32 grid.

const W, H = 36, 32

type grid [H][W]uint8 // 0 empty, 1 dim, 2 full

type mask func(x, y float64) bool

type pts = [][2]float64

// jsRound is Math.round: halves go toward +infinity.
func jsRound(v float64) float64 { return math.Floor(v + 0.5) }

// sq keeps the product rounded, so arm64 cannot fuse it into a later add and drift from JS.
func sq(v float64) float64 { return float64(v * v) }

func iff(c bool, a, b uint8) uint8 {
	if c {
		return a
	}
	return b
}

func ell(cx, cy, rx, ry float64) mask {
	return func(x, y float64) bool { return sq((x+.5-cx)/rx)+sq((y+.5-cy)/ry) <= 1 }
}

func rrect(x0, y0, x1, y1, r float64) mask {
	return func(x, y float64) bool {
		if x < x0 || x > x1 || y < y0 || y > y1 {
			return false
		}
		px, py := x+.5, y+.5
		qx := math.Min(math.Max(px, x0+r), x1+1-r)
		qy := math.Min(math.Max(py, y0+r), y1+1-r)
		return sq(px-qx)+sq(py-qy) <= r*r
	}
}

func poly(p pts) mask {
	return func(x, y float64) bool {
		px, py := x+.5, y+.5
		c := false
		for i, j := 0, len(p)-1; i < len(p); j, i = i, i+1 {
			xi, yi, xj, yj := p[i][0], p[i][1], p[j][0], p[j][1]
			if (yi > py) != (yj > py) && px < float64(float64((xj-xi)*(py-yi))/(yj-yi))+xi {
				c = !c
			}
		}
		return c
	}
}

func and(m, f mask) mask { return func(x, y float64) bool { return m(x, y) && f(x, y) } }

func or(ms ...mask) mask {
	return func(x, y float64) bool {
		for _, m := range ms {
			if m(x, y) {
				return true
			}
		}
		return false
	}
}

func not(m mask) mask { return func(x, y float64) bool { return !m(x, y) } }

// tube is a chain of discs along p, the radius going from r0 to r1.
func tube(p pts, r0, r1 float64) mask {
	return func(x, y float64) bool {
		for i, pt := range p {
			r := r0 + float64(float64((r1-r0)*float64(i))/float64(len(p)-1))
			if sq(x+.5-pt[0])+sq(y+.5-pt[1]) <= r*r {
				return true
			}
		}
		return false
	}
}

func curve(a, b, c [2]float64, n int) pts {
	out := make(pts, n+1)
	for i := range out {
		t := float64(i) / float64(n)
		u := 1 - t
		out[i] = [2]float64{
			float64(float64(u*u)*a[0]) + float64(float64(2*u*t)*b[0]) + float64(float64(t*t)*c[0]),
			float64(float64(u*u)*a[1]) + float64(float64(2*u*t)*b[1]) + float64(float64(t*t)*c[1]),
		}
	}
	return out
}

func grey(rows []string) []string {
	out := make([]string, len(rows))
	for i, r := range rows {
		out[i] = strings.ReplaceAll(r, "#", "+")
	}
	return out
}

// hrow is n pixels in a row starting at x.
func hrow(x, y float64, n int) pts {
	out := make(pts, n)
	for i := range out {
		out[i] = [2]float64{x + float64(i), y}
	}
	return out
}

// pen draws in creature coordinates, shifted by (ox, oy) on the grid.
type pen struct {
	g      *grid
	ox, oy int
}

// put writes a cell as is; cells off the grid are dropped.
func (p pen) put(x, y float64, v uint8) {
	xi, yi := int(jsRound(x))+p.ox, int(jsRound(y))+p.oy
	if xi >= 0 && yi >= 0 && xi < W && yi < H {
		p.g[yi][xi] = v
	}
}

// set is put, except that a dim detail over a dim body becomes a hole, so it still shows.
func (p pen) set(x, y float64, v uint8) {
	xi, yi := int(jsRound(x))+p.ox, int(jsRound(y))+p.oy
	if v == 1 && xi >= 0 && yi >= 0 && xi < W && yi < H && p.g[yi][xi] == 1 {
		v = 0
	}
	p.put(x, y, v)
}

func edge(m mask) mask {
	return func(x, y float64) bool {
		return m(x, y) && (!m(x-1, y) || !m(x+1, y) || !m(x, y-1) || !m(x, y+1))
	}
}

// part hides what is under it: its edge is full, its inside dim.
func (p pen) part(m mask) {
	e := edge(m)
	for y := -2.0; y < H; y++ {
		for x := -2.0; x < W; x++ {
			if m(x, y) {
				p.put(x, y, iff(e(x, y), 2, 1))
			}
		}
	}
}

func (p pen) ring(m mask, v uint8) {
	e := edge(m)
	for y := -2.0; y < H; y++ {
		for x := -2.0; x < W; x++ {
			if e(x, y) {
				p.set(x, y, v)
			}
		}
	}
}

func (p pen) fill(m mask, v uint8) {
	for y := -2.0; y < H; y++ {
		for x := -2.0; x < W; x++ {
			if m(x, y) {
				p.put(x, y, v)
			}
		}
	}
}

// cut makes a hole: mouths, pupils, an open maw.
func (p pen) cut(m mask) { p.fill(m, 0) }

func (p pen) pix(list pts, v uint8) {
	for _, pt := range list {
		p.set(pt[0], pt[1], v)
	}
}

func (p pen) line(x0, y0, x1, y1 float64, v uint8) {
	n := math.Max(math.Abs(x1-x0), math.Abs(y1-y0))
	if n == 0 {
		n = 1
	}
	for i := 0.0; i <= n; i++ {
		p.set(jsRound(x0+float64(float64((x1-x0)*i)/n)), jsRound(y0+float64(float64((y1-y0)*i)/n)), v)
	}
}

func (p pen) glyph(x, y float64, rows []string) {
	for j, r := range rows {
		for i, c := range []rune(r) {
			switch c {
			case '#':
				p.put(x+float64(i), y+float64(j), 2)
			case '+':
				p.set(x+float64(i), y+float64(j), 1)
			case 'o':
				p.put(x+float64(i), y+float64(j), 0)
			}
		}
	}
}

// Eyes are full with a hole for the pupil.
var EYES = map[string][]string{
	"idle":  {"###", "#o#", "###"},
	"blink": {"...", "###", "..."},
	"happy": {".#.", "#.#", "..."},
}
var SEYES = map[string][]string{
	"idle":  {"##", "#o"},
	"blink": {"..", "##"},
	"happy": {".#", "#."},
}

func look(mood string, f int) string {
	switch {
	case mood == "sleep":
		return "blink"
	case mood == "happy":
		return "happy"
	case f%8 == 7:
		return "blink"
	}
	return "idle"
}

func eyes(p pen, xs []float64, y float64, mood string, f int) {
	for _, x := range xs {
		p.glyph(x, y, EYES[look(mood, f)])
	}
}

func seye(p pen, x, y float64, mood string, f int) { p.glyph(x, y, SEYES[look(mood, f)]) }

func mouth(p pen, cx, y float64, w int, mood string, f int) {
	x := jsRound(cx - float64(w)/2)
	mid := func(n string) string { return strings.Repeat(n, max(0, w-2)) }
	switch {
	case mood == "happy":
		p.glyph(x, y, []string{"#" + mid(".") + "#", "." + mid("#") + "."})
	case mood == "talk" && f%2 != 0:
		p.glyph(x, y-1, []string{"." + mid("#") + ".", "#" + mid(".") + "#", "." + mid("#") + "."})
	case mood == "hungry":
		p.glyph(x, y-1, []string{"." + mid("#") + ".", "#" + mid("+") + "#", "." + mid("#") + "."})
	default:
		p.glyph(x, y, []string{strings.Repeat("#", w)})
	}
}

func fx(p pen, mood string, f int, shiny bool) {
	if mood == "happy" && f%2 == 0 {
		p.glyph(29, 2, []string{".#.#.", "#####", "#####", ".###.", "..#.."})
	}
	if mood == "sleep" {
		z := []string{"###", ".#.", "###"}
		p.glyph(29, float64(4-f%2), z)
		if f%4 > 1 {
			p.glyph(32, 0, z)
		}
	}
	if mood == "hungry" && f%2 != 0 {
		p.glyph(29, 3, []string{".+.+.", "+.+.+"})
	}
	if shiny {
		for i, pt := range [][2]float64{{0, 2}, {30, 9}, {1, 18}, {31, 24}} {
			switch (f + i) % 3 {
			case 0:
				p.glyph(pt[0], pt[1], []string{".#.", "###", ".#."})
			case 1:
				p.set(pt[0]+1, pt[1]+1, 1)
			}
		}
	}
}

func wave(p pen, y float64, f int) {
	for x := 0.0; x < 32; x++ {
		dy := 1.0
		if math.Sin((x+float64(f))/1.6) > 0 {
			dy = 0
		}
		p.set(x, y+dy, 1)
	}
}

// forms maps a form id to its drawing; "ovo" is the egg.
var forms map[string]func(d pen, mood string, f int)

// sprite draws form at (2+wobble, bob), then fx at (2, 0).
func sprite(form, mood string, f int, shiny bool, bob int) grid {
	var g grid
	ox := 2
	if forms[form] == nil {
		form = "ovo"
	}
	if form == "ovo" {
		ox += [4]int{0, 1, 0, -1}[f%4]
	}
	forms[form](pen{&g, ox, bob}, mood, f)
	fx(pen{&g, 2, 0}, mood, f, shiny)
	return g
}

// rows is the grid as H strings of W chars: '.', '+', '#'.
func (g *grid) rows() []string {
	out := make([]string, H)
	for y := range g {
		b := make([]byte, W)
		for x, v := range g[y] {
			b[x] = ".+#"[v]
		}
		out[y] = string(b)
	}
	return out
}

func xs(v ...float64) []float64 { return v }

func init() {
	forms = map[string]func(d pen, mood string, f int){}
	C := forms
	type pt = [2]float64
	C["ovo"] = func(p pen, mood string, f int) {
		p.part(ell(16, 17, 8.5, 11.5))
		p.glyph(9, 15, []string{"#..#...#..#..#..", ".##.#.#.##.#.#.", ".....#........."})
		p.pix(pts{{12, 9}, {13, 9}, {19, 22}, {20, 22}, {18, 23}}, 1)
	}

	// ── Comum ──
	C["gosma"] = func(p pen, mood string, f int) {
		w := 0.0
		if f%2 != 0 {
			w = .6
		}
		p.part(ell(5, 28, 2, 2))
		p.part(ell(27.5, 28.5, 1.6, 1.6))
		p.part(and(ell(16, 24, 12.5+w, 11-w), func(x, y float64) bool { return y <= 29 }))
		p.fill(ell(9, 18, 1.2, 2.2), 2)
		eyes(p, xs(11, 18), 19, mood, f)
		mouth(p, 16, 25, 4, mood, f)
	}
	C["gotinha"] = func(p pen, mood string, f int) {
		w := float64(f % 2)
		p.part(or(and(ell(16, 24, 7+w*.5, 6-w*.5), func(x, y float64) bool { return y <= 29 }), poly(pts{{10, 22}, {16, 12 + w}, {22, 22}})))
		p.fill(ell(12.5, 20, 1, 1.6), 2)
		eyes(p, xs(12, 17), 21, mood, f)
		mouth(p, 16, 26, 2, mood, f)
	}
	C["gosmao"] = func(p pen, mood string, f int) {
		w := 0.0
		if f%2 != 0 {
			w = .7
		}
		p.part(ell(2, 28.5, 2, 2))
		p.part(ell(31, 28, 1.8, 2))
		p.part(and(ell(16, 22, 15.5+w, 15-w), func(x, y float64) bool { return y <= 29 }))
		p.fill(ell(6, 13, 1.3, 2.6), 2)
		p.line(19, 25, 25, 25, 2)
		p.pix(pts{{18, 24}, {18, 26}, {26, 24}, {26, 26}}, 2)
		p.ring(ell(9, 25, 1.8, 1.8), 2)
		eyes(p, xs(10, 19), 14, mood, f)
		mouth(p, 16, 20, 6, mood, f)
	}

	C["larvinha"] = func(p pen, mood string, f int) {
		for i, x := range []float64{4, 9, 14, 19} {
			dy := float64((i + f) % 2)
			p.part(ell(x, 26-dy, 3, 3))
			p.pix(pts{{x - 1, 30}, {x + 1, 30}}, 2)
		}
		p.line(24, 18, 22, 12, 2)
		p.line(28, 18, 30, 12, 2)
		p.pix(pts{{21, 11}, {22, 11}, {30, 11}, {31, 11}}, 2)
		p.part(ell(26, 22, 5, 5))
		eyes(p, xs(23, 27), 19, mood, f)
		mouth(p, 26, 25, 2, mood, f)
		p.pix(pts{{24, 28}, {28, 28}}, 2)
	}
	C["bug"] = func(p pen, mood string, f int) {
		k := float64(f % 2)
		for _, s := range []float64{-1, 1} {
			p.line(16+s*8, 18, 16+s*13, 15-k, 2)
			p.line(16+s*9, 22, 16+s*14, 22+k, 2)
			p.line(16+s*8, 26, 16+s*13, 30-k, 2)
		}
		p.part(ell(16, 22, 9, 8))
		p.line(16, 16, 16, 29, 2)
		p.cut(ell(12, 21, 1.4, 1.4))
		p.cut(ell(20, 25, 1.4, 1.4))
		p.cut(ell(12, 26, 1, 1))
		p.line(13, 9, 10, 3, 2)
		p.line(19, 9, 22, 3, 2)
		p.part(ell(16, 12, 5.5, 4.5))
		eyes(p, xs(12, 17), 10, mood, f)
		mouth(p, 16, 14, 2, mood, f)
	}
	C["megabug"] = func(p pen, mood string, f int) {
		fl := float64(f % 2)
		for _, s := range []float64{-1, 1} {
			p.part(poly(pts{{16, 15}, {16 + s*16, 6 - fl}, {16 + s*14, 19}}))
			p.line(16+s*2, 15, 16+s*14, 8-fl, 1)
			p.part(poly(pts{{16 + s*1, 16}, {16 + s*11, 12}, {16 + s*9, 24}}))
			p.line(16+s*7, 23, 16+s*13, 27, 2)
			p.line(16+s*7, 27, 16+s*11, 31, 2)
		}
		p.part(ell(16, 24, 7.5, 6.5))
		p.line(16, 19, 16, 30, 2)
		p.part(tube(curve(pt{16, 11}, pt{17, 4}, pt{12, 1}, 24), 1.7, .8))
		p.part(ell(16, 14, 5, 4))
		eyes(p, xs(12, 17), 12, mood, f)
		mouth(p, 16, 16, 2, mood, f)
	}

	C["fagulha"] = func(p pen, mood string, f int) {
		k := float64(f % 2)
		p.part(poly(pts{{10, 28}, {9, 23}, {11, 18 + k}, {13, 21}, {16, 12 - k}, {19, 20}, {21, 16 + k}, {23, 23}, {22, 28}, {16, 30}}))
		p.pix(pts{{7, 13 - k}, {25, 11 + k}, {21, 8 - k}}, 1)
		eyes(p, xs(12, 17), 21, mood, f)
		mouth(p, 16, 26, 2, mood, f)
	}
	C["fogo"] = func(p pen, mood string, f int) {
		fl := float64(f % 2)
		p.line(10, 31, 22, 31, 1)
		p.part(tube(curve(pt{16, 22}, pt{10, 29}, pt{3, 24}, 24), 4, .7))
		p.part(or(ell(17, 16, 8, 7), poly(pts{{9, 15}, {10, 5 - fl}, {14, 10}, {17, 1 + fl}, {20, 9}, {24, 3 - fl}, {25, 15}})))
		p.ring(ell(17, 17, 5, 4.5), 1)
		eyes(p, xs(13, 18), 14, mood, f)
		mouth(p, 17, 19, 3, mood, f)
	}

	C["bit"] = func(p pen, mood string, f int) {
		p.line(16, 17, 16, 13, 2)
		p.fill(ell(16, 12, 1.6, 1.6), 2)
		p.part(rrect(11, 29, 13, 30, 0))
		p.part(rrect(19, 29, 21, 30, 0))
		p.part(rrect(10, 17, 22, 29, 1.5))
		p.ring(rrect(12, 19, 20, 26, 1), 2)
		p.pix(pts{{9, 23}, {23, 23}}, 2)
		seye(p, 13, 21, mood, f)
		seye(p, 17, 21, mood, f)
		mouth(p, 16, 24, 2, mood, f)
	}
	C["bot"] = func(p pen, mood string, f int) {
		a := float64(f % 2)
		p.line(16, 5, 16, 1, 2)
		p.fill(ell(16, 1, 1.4, 1.4), 2)
		p.part(rrect(3, 18+a, 6, 26+a, 1))
		p.part(rrect(26, 18-a, 29, 26-a, 1))
		p.part(rrect(10, 26, 14, 30, 1))
		p.part(rrect(18, 26, 22, 30, 1))
		p.part(rrect(8, 16, 24, 27, 2))
		p.ring(rrect(12, 19, 20, 24, 1), 2)
		p.fill(rrect(15, 21, 17, 22, 0), 2)
		p.part(rrect(9, 5, 23, 15, 2))
		p.ring(rrect(11, 7, 21, 13, 1), 2)
		eyes(p, xs(12, 17), 8, mood, f)
		mouth(p, 16, 12, 4, mood, f)
	}

	C["abissal"] = func(p pen, mood string, f int) {
		t := float64(f % 2)
		p.part(tube(curve(pt{18, 12}, pt{22, 0}, pt{29, 4}, 24), .6, .6))
		p.fill(ell(29, 5, 2, 2), 2)
		if f%2 == 0 {
			p.pix(pts{{32, 5}, {29, 8}, {26, 5}}, 1)
		}
		p.part(poly(pts{{8, 20}, {1, 13 + t}, {3, 20}, {1, 27 - t}}))
		p.part(ell(17, 20, 10, 8.5))
		p.part(poly(pts{{14, 13}, {16, 9}, {19, 12}}))
		p.cut(and(ell(27, 23, 6, 3.5), func(x, y float64) bool { return x > 20 }))
		for x := 21.0; x <= 29; x += 2.5 {
			p.fill(poly(pts{{x, 19.5}, {x + 1.8, 19.5}, {x + .9, 22}}), 2)
			p.fill(poly(pts{{x + 1, 26.5}, {x + 2.8, 26.5}, {x + 1.9, 24}}), 2)
		}
		p.line(12, 24, 15, 28, 1)
		eyes(p, xs(18), 14, mood, f)
	}
	C["alevino"] = func(p pen, mood string, f int) {
		t := float64(f % 2)
		p.ring(ell(24, 13-t, 1.5, 1.5), 2)
		p.ring(ell(27, 8-t, 1, 1), 2)
		p.part(poly(pts{{11, 23}, {5, 18 + t}, {7, 23}, {5, 28 - t}}))
		p.part(poly(pts{{13, 20}, {16, 15}, {19, 20}}))
		p.part(ell(16, 23, 7, 5))
		p.line(12, 20, 12, 26, 1)
		eyes(p, xs(17, 17), 20, mood, f)
		mouth(p, 22, 25, 2, mood, f)
	}

	C["bauzinho"] = func(p pen, mood string, f int) {
		g := 0.0
		if mood != "sleep" {
			g = float64(f % 2)
		}
		p.part(rrect(8, 21, 24, 30, 1))
		p.fill(rrect(8, 21, 9, 30, 0), 2)
		p.fill(rrect(23, 21, 24, 30, 0), 2)
		p.part(rrect(14, 23, 18, 26, .5))
		p.set(16, 24, 0)
		if mood != "sleep" {
			p.cut(rrect(9, 18-g, 23, 21, 0))
			for _, x := range []float64{11, 15, 19} {
				p.fill(poly(pts{{x, 18 - g}, {x + 2.5, 18 - g}, {x + 1.2, 21}}), 2)
			}
		}
		p.part(rrect(8, 13-g, 24, 18-g, 2))
		p.fill(rrect(8, 13-g, 9, 18-g, 0), 2)
		p.fill(rrect(23, 13-g, 24, 18-g, 0), 2)
		seye(p, 12, 14.5-g, mood, f)
		seye(p, 18, 14.5-g, mood, f)
	}
	C["mimico"] = func(p pen, mood string, f int) {
		open := mood != "sleep"
		top := 13.0
		if open {
			top = 4 - float64(f%2)
			if mood == "talk" && f%2 != 0 {
				top = 2
			}
		}
		p.part(rrect(3, 19, 29, 30, 1))
		p.fill(rrect(3, 19, 5, 30, 0), 2)
		p.fill(rrect(27, 19, 29, 30, 0), 2)
		p.line(3, 25, 29, 25, 2)
		if open {
			m := rrect(5, top+5, 27, 20, 0)
			p.cut(m)
			p.ring(m, 2)
			for x := 6.0; x <= 24; x += 3 {
				p.fill(poly(pts{{x, top + 5}, {x + 3, top + 5}, {x + 1.5, top + 9}}), 2)
				p.fill(poly(pts{{x + 1.5, 20}, {x + 4.5, 20}, {x + 3, 16}}), 2)
			}
			p.part(tube(curve(pt{15, 18}, pt{13, 24}, pt{16, 28}, 24), 2.4, 1.8))
		}
		p.part(rrect(3, top, 29, top+5, 2.2))
		p.fill(rrect(3, top, 5, top+5, 1), 2)
		p.fill(rrect(27, top, 29, top+5, 1), 2)
		eyes(p, xs(9, 20), top+1, mood, f)
		if !open {
			p.part(rrect(14, 21, 18, 25, .5))
			p.set(16, 23, 0)
		}
	}

	// ── Incomum ──
	C["grifo"] = func(p pen, mood string, f int) {
		fl := float64(f % 2)
		p.part(tube(curve(pt{7, 23}, pt{1, 25}, pt{1, 17}, 24), 1, .7))
		p.fill(ell(1, 15.5, 1.6, 2.2), 2)
		p.part(rrect(7, 25, 9, 30, 1))
		p.part(rrect(10, 25, 12, 30, 1))
		p.part(rrect(18, 25, 20, 30, 1))
		p.pix(pts{{21, 30}, {17, 30}}, 2)
		p.part(ell(14, 23, 8, 5))
		p.part(poly(pts{{9, 20}, {3, 4 - fl}, {8, 8 - fl}, {11, 1 - fl}, {14, 7 - fl}, {18, 4 - fl}, {18, 19}}))
		p.line(14, 19, 4, 5-fl, 2)
		p.line(14, 19, 11, 2-fl, 2)
		p.line(15, 19, 17, 5-fl, 2)
		p.part(tube(curve(pt{19, 21}, pt{23, 18}, pt{24, 13}, 24), 3, 2.4))
		p.part(poly(pts{{22, 9}, {18, 6}, {22, 7.5}}))
		p.part(ell(25, 11, 4, 3.5))
		p.fill(poly(pts{{28, 9.5}, {33, 11.5}, {32, 13.5}, {28, 13}}), 2)
		seye(p, 25, 9, mood, f)
	}
	C["grifinho"] = func(p pen, mood string, f int) {
		w := float64(f % 2)
		p.part(tube(curve(pt{10, 25}, pt{5, 27}, pt{6, 21}, 24), .8, .8))
		p.fill(ell(6, 20, 1.4, 1.7), 2)
		p.line(13, 28, 13, 30, 2)
		p.line(18, 28, 18, 30, 2)
		p.pix(pts{{12, 30}, {17, 30}, {19, 30}}, 2)
		p.part(ell(15, 25, 6, 4.5))
		p.part(ell(13, 23-w, 4, 2.6))
		p.line(10, 23-w, 16, 24-w, 2)
		p.part(poly(pts{{17, 11}, {16, 6}, {19, 10}}))
		p.part(poly(pts{{20, 10}, {22, 6}, {22, 11}}))
		p.part(ell(19, 16, 6, 5.5))
		p.fill(poly(pts{{24, 14.5}, {29, 16.5}, {28, 18}, {24, 18.5}}), 2)
		eyes(p, xs(18), 13, mood, f)
	}

	C["pedrinha"] = func(p pen, mood string, f int) {
		k := float64(f % 2)
		p.line(17, 16, 17, 11, 2)
		p.part(poly(pts{{17, 13}, {12, 9 - k}, {11, 11 - k}, {15, 14}}))
		p.part(poly(pts{{17, 12}, {22, 7 + k}, {23, 10 + k}, {19, 13}}))
		p.part(poly(pts{{6, 29}, {7, 22}, {11, 17}, {19, 15.5}, {25, 19}, {27, 25}, {26, 29}}))
		p.line(9, 21, 11, 24, 2)
		p.line(11, 24, 10, 26, 2)
		p.line(24, 21, 23, 23, 2)
		eyes(p, xs(12, 18), 20, mood, f)
		mouth(p, 16.5, 25, 2, mood, f)
	}
	C["golem"] = func(p pen, mood string, f int) {
		a := float64(f % 2)
		p.part(rrect(10, 26, 14, 30, 1))
		p.part(rrect(18, 26, 22, 30, 1))
		p.part(rrect(3, 12+a, 8, 21+a, 1))
		p.part(rrect(24, 12+a, 29, 21+a, 1))
		p.part(rrect(8, 12, 24, 27, 2))
		p.line(10, 25, 13, 22, 2)
		p.fill(poly(pts{{16, 15}, {19, 19}, {16, 23}, {13, 19}}), 2)
		p.set(16, 19, 0)
		p.part(rrect(1, 20+a, 9, 28+a, 2))
		p.part(rrect(23, 20+a, 31, 28+a, 2))
		p.line(3, 24+a, 7, 24+a, 2)
		p.line(25, 24+a, 29, 24+a, 2)
		p.part(rrect(11, 3, 21, 12, 1))
		p.line(19, 4, 18, 7, 2)
		seye(p, 13, 6, mood, f)
		seye(p, 17, 6, mood, f)
		mouth(p, 16, 10, 4, mood, f)
	}

	C["corvo"] = func(p pen, mood string, f int) {
		fl := float64(f % 2)
		p.part(rrect(0, 28, 33, 30, 1))
		p.line(26, 28, 30, 24, 2)
		p.line(16, 26, 15, 28, 2)
		p.line(19, 26, 19, 28, 2)
		p.part(poly(pts{{10, 22}, {1, 26}, {3, 27.5}, {11, 25}}))
		p.part(ell(15, 21, 7.5, 5.5))
		p.part(poly(pts{{12, 18}, {3, 11 - fl}, {6, 15}, {2, 17 - fl}, {6, 19}, {4, 22}, {14, 24}}))
		p.line(13, 19, 4, 12-fl, 2)
		p.line(13, 21, 4, 18-fl, 2)
		p.part(ell(22, 13, 4.5, 4))
		p.fill(poly(pts{{25, 11.5}, {33, 13.5}, {31, 15}, {25, 16}}), 2)
		p.line(26, 14, 31, 14, 1)
		seye(p, 21, 11, mood, f)
	}
	C["corvinho"] = func(p pen, mood string, f int) {
		p.part(poly(pts{{11, 25}, {5, 27.5}, {8, 29}, {12, 27}}))
		p.line(14, 28, 14, 30, 2)
		p.line(17, 28, 17, 30, 2)
		p.part(ell(15, 24, 5.5, 4.5))
		p.part(ell(14, 24, 3.5, 2.5))
		p.line(11, 24, 16, 25, 2)
		p.part(ell(19, 17.5, 4.5, 4))
		p.fill(poly(pts{{22.5, 16}, {28, 17.5}, {27, 19}, {22.5, 19.5}}), 2)
		eyes(p, xs(17), 15, mood, f)
	}

	C["drone"] = func(p pen, mood string, f int) {
		r := float64(f % 2)
		for _, s := range []float64{-1, 1} {
			p.line(16+s*5, 17, 16+s*11, 17, 2)
			p.part(rrect(16+s*12-2, 15, 16+s*12+2, 18, 1))
			p.line(16+s*12, 14, 16+s*12, 13, 2)
			p.line(16+s*12-5+r*2, 12, 16+s*12+5-r*2, 12, 2)
			p.line(16+s*3, 23, 16+s*6, 30, 2)
			p.line(16+s*5, 30, 16+s*8, 30, 2)
		}
		p.part(ell(16, 25, 2.6, 2.2))
		p.cut(ell(16, 25.5, 1, 1))
		p.part(rrect(10, 15, 22, 23, 2.5))
		seye(p, 13, 18, mood, f)
		seye(p, 17, 18, mood, f)
	}

	C["ursocoruja"] = func(p pen, mood string, f int) {
		a := float64(f % 2)
		p.part(rrect(8, 26, 13, 30, 1.5))
		p.part(rrect(19, 26, 24, 30, 1.5))
		p.part(ell(5, 21+a, 3.2, 4.5))
		p.part(ell(27, 21-a, 3.2, 4.5))
		p.pix(pts{{3, 26 + a}, {5, 26.5 + a}, {7, 26 + a}, {25, 26 - a}, {27, 26.5 - a}, {29, 26 - a}}, 2)
		p.part(ell(16, 22, 9.5, 7))
		for _, x := range []float64{12, 16, 20} {
			p.line(x-2, 19, x, 21, 1)
			p.line(x, 21, x+2, 19, 1)
		}
		p.part(poly(pts{{9, 7}, {6, 0}, {12, 4}}))
		p.part(poly(pts{{22, 7}, {25, 0}, {19, 4}}))
		p.part(ell(15.5, 10, 8, 6.5))
		p.ring(ell(12, 10, 3.4, 3.4), 2)
		p.ring(ell(19, 10, 3.4, 3.4), 2)
		eyes(p, xs(11, 18), 9, mood, f)
		p.fill(poly(pts{{14, 12}, {17, 12}, {15.5, 16}}), 2)
	}
	C["ursinhocoruja"] = func(p pen, mood string, f int) {
		a := float64(f % 2)
		p.part(rrect(11, 28, 14, 30, 1))
		p.part(rrect(18, 28, 21, 30, 1))
		p.part(ell(9, 24+a, 2, 3))
		p.part(ell(23, 24-a, 2, 3))
		p.part(ell(16, 25, 7, 5))
		p.line(14, 24, 16, 26, 1)
		p.line(16, 26, 18, 24, 1)
		p.part(poly(pts{{11, 11}, {9, 6}, {13, 9}}))
		p.part(poly(pts{{21, 11}, {23, 6}, {19, 9}}))
		p.part(ell(16, 15, 7, 6))
		p.ring(ell(13, 15, 2.8, 2.8), 2)
		p.ring(ell(18, 15, 2.8, 2.8), 2)
		eyes(p, xs(12, 17), 14, mood, f)
		p.fill(poly(pts{{14.5, 17}, {16.5, 17}, {15.5, 19.5}}), 2)
	}

}

func init() {
	C := forms
	type pt = [2]float64
	const mid = 24 // the JS curve default n

	// ── Raro ──
	C["dragaozinho"] = func(p pen, mood string, f int) {
		fl := float64(f % 2)
		p.part(tube(curve(pt{9, 26}, pt{3, 29.5}, pt{3, 23}, 24), 1.4, .8))
		p.part(poly(pts{{1, 23.5}, {3, 19}, {5, 23.5}, {3, 22.5}}))
		p.part(rrect(10, 27, 12, 30, 1))
		p.part(rrect(16, 27, 18, 30, 1))
		p.part(ell(14, 25, 6, 4.5))
		p.part(poly(pts{{11, 22}, {4, 13 - fl}, {7, 16 - fl}, {9, 12 - fl}, {12, 17}, {15, 21}}))
		p.line(12, 21, 5, 14-fl, 2)
		p.line(12, 21, 9, 13-fl, 2)
		p.part(poly(pts{{17, 10.5}, {14.5, 6}, {19, 9}}))
		p.part(poly(pts{{21, 9.5}, {21.5, 5}, {23.5, 9.5}}))
		p.part(rrect(22, 14, 29, 19, 2))
		p.part(ell(19.5, 14.5, 6.5, 6))
		p.set(27, 15, 0)
		eyes(p, xs(18), 12, mood, f)
		mouth(p, 25.5, 18, 3, mood, f)
	}
	C["dragao"] = func(p pen, mood string, f int) {
		t := float64(f % 2)
		p.part(tube(curve(pt{10, 26}, pt{2, 29}, pt{1.5, 21}, 24), 1.8, 1))
		p.part(poly(pts{{-1, 22.5}, {1.5, 16}, {4, 22.5}, {1.5, 21}}))
		p.part(rrect(9, 25, 12, 30, 1))
		p.part(rrect(18, 25, 21, 30, 1))
		p.part(ell(14.5, 23, 7.5, 5))
		p.part(tube(curve(pt{18, 22}, pt{23, 20}, pt{24, 13}, 24), 2.8, 2.2))
		p.part(poly(pts{{24, 8.5}, {19, 3}, {26, 7}}))
		p.part(poly(pts{{21, 12}, {23, 8}, {27, 7}, {33, 9.5}, {33, 12.5}, {26, 14}}))
		p.line(27, 12, 32, 12, 2)
		p.set(31, 10, 2)
		p.part(poly(pts{{10, 20}, {1, 5 + t}, {6, 9.5}, {8, 1 + t}, {11.5, 9}, {16, 3 + t}, {17, 19}}))
		p.line(14, 19, 1, 5+t, 2)
		p.line(14, 19, 8, 1+t, 2)
		p.line(14, 19, 16, 3+t, 2)
		seye(p, 25, 8.5, mood, f)
	}
	C["caozinho"] = func(p pen, mood string, f int) {
		wag := float64(f % 2)
		p.part(tube(curve(pt{22, 24}, pt{28, 22}, pt{27 + wag*2, 15}, 24), 1.5, 1.1))
		p.part(ell(16, 25, 7.5, 5))
		p.part(rrect(10, 27, 14, 30, 1.5))
		p.part(rrect(18, 27, 22, 30, 1.5))
		p.set(12, 29, 1)
		p.set(20, 29, 1)
		p.part(ell(16, 14, 8.5, 7))
		for _, s := range []float64{-1, 1} {
			drop := 0.0
			if s > 0 {
				drop = wag
			}
			p.fill(tube(curve(pt{16 + s*6, 8}, pt{16 + s*10.5, 10}, pt{16 + s*9.5, 19 + drop}, 24), 2.6, 2), 2)
		}
		p.part(ell(16, 17.5, 4.5, 3))
		p.fill(poly(pts{{14, 15.5}, {18, 15.5}, {16, 17.5}}), 2)
		p.line(16, 17, 16, 18.5, 2)
		if mood != "sleep" {
			p.part(rrect(15, 19.5, 17, 22, 1))
			p.set(16, 20.5, 2)
		}
		eyes(p, xs(11, 18), 11, mood, f)
	}
	C["cerbero"] = func(p pen, mood string, f int) {
		wag := float64(f % 2)
		p.part(rrect(9, 27, 12, 30, 1))
		p.part(rrect(20, 27, 23, 30, 1))
		p.part(ell(16, 26, 9, 4))
		// side heads in profile, facing out: snout, nose, a floppy ear
		side := func(hx, hy, d float64) {
			p.part(rrect(math.Min(hx, hx+d*5), hy-.5, math.Max(hx, hx+d*5), hy+3, 1.2))
			p.part(ell(hx, hy, 4, 3.6))
			p.fill(rrect(math.Min(hx+d*4, hx+d*5.5), hy-1, math.Max(hx+d*4, hx+d*5.5), hy+.5, .5), 2)
			p.fill(tube(curve(pt{hx - d*.5, hy - 3}, pt{hx - d*3.5, hy - 1}, pt{hx - d*3, hy + 3.5}, 24), 1.4, 1.2), 2)
			seye(p, hx-1+d*.5, hy-1.5, mood, f)
		}
		side(4.5, 20+wag, -1)
		side(27.5, 21-wag, 1)
		p.fill(rrect(11, 19, 21, 20, 0), 2)
		p.pix(pts{{12, 21}, {15, 21}, {18, 21}, {21, 21}}, 2)
		// the middle head is the Cãozinho's
		p.part(ell(16, 11, 8, 6.5))
		for _, s := range []float64{-1, 1} {
			p.fill(tube(curve(pt{16 + s*5.5, 5}, pt{16 + s*10, 7}, pt{16 + s*9, 15}, 24), 2.4, 1.9), 2)
		}
		p.part(ell(16, 14.5, 4.5, 3))
		p.fill(poly(pts{{14, 12.5}, {18, 12.5}, {16, 14.5}}), 2)
		p.line(16, 14, 16, 15.5, 2)
		if mood != "sleep" {
			p.part(rrect(15, 16.5, 17, 19, 1))
			p.set(16, 17.5, 2)
		}
		eyes(p, xs(11, 18), 8, mood, f)
	}

	C["cavalinho"] = func(p pen, mood string, f int) {
		fl := float64(f % 2)
		p.ring(ell(28, 6-fl, 1.3, 1.3), 2)
		p.ring(ell(30, 2-fl, .9, .9), 2)
		p.part(tube(curve(pt{14, 21}, pt{20, 29}, pt{14, 29.5}, 24), 2.2, 1.4))
		p.part(tube(curve(pt{14, 29.5}, pt{10, 28}, pt{13, 25.5}, 24), 1.4, 1))
		p.part(poly(pts{{11, 13}, {5, 15 - fl}, {6, 19 + fl}, {11, 20}}))
		p.line(10, 15, 6, 16-fl, 2)
		p.line(10, 18, 6, 18+fl, 2)
		p.part(ell(15, 17, 5, 6.5))
		for y := 13.0; y <= 21; y += 2 {
			p.line(17, y, 19, y, 1)
		}
		p.part(rrect(20, 8, 28, 10.5, 1))
		p.part(ell(17.5, 8, 4.2, 3.6))
		p.line(15, 4, 14, 2, 2)
		p.line(17.5, 4, 17.5, 1.5, 2)
		p.line(20, 5, 21, 2.5, 2)
		seye(p, 18, 7, mood, f)
	}
	C["hipocampo"] = func(p pen, mood string, f int) {
		fl := float64(f % 2)
		wave(p, 29, f)
		p.part(tube(curve(pt{12, 22}, pt{4, 28}, pt{3, 18}, 24), 3, 1.2))
		p.part(poly(pts{{0, 18}, {2, 11 + fl}, {4, 16}, {7, 12 - fl}, {5, 19}}))
		for _, q := range (pts{{6, 25}, {9, 26}, {4, 22}}) {
			p.set(q[0], q[1], 1)
		}
		p.part(tube(curve(pt{20, 23}, pt{22, 26}, pt{21, 29}, 24), 1.3, 1.1))
		p.part(tube(curve(pt{22, 22}, pt{26, 22 + fl}, pt{27, 25 + fl}, 24), 1.3, 1.1))
		p.part(ell(16, 20, 7.5, 5))
		p.part(tube(curve(pt{19, 18}, pt{22, 13}, pt{23, 8}, 24), 3, 2.4))
		p.part(poly(pts{{22, 4}, {22, 0}, {24.5, 3}}))
		p.part(poly(pts{{20, 6}, {25, 3}, {32, 7}, {32, 10.5}, {27, 11}, {22, 9.5}}))
		p.set(30, 8, 0)
		p.line(27, 10, 31, 10, 2)
		for _, q := range (pts{{20, 6}, {19, 9}, {18, 12}, {17, 15}}) {
			p.line(q[0], q[1], q[0]-3, q[1]+1.5, 2)
		}
		seye(p, 25, 5, mood, f)
	}

	C["cubo"] = func(p pen, mood string, f int) {
		w := float64(f % 2)
		p.part(poly(pts{{4, 10}, {10, 4}, {31, 4}, {25, 10}}))
		p.part(poly(pts{{25, 10}, {31, 4}, {31, 24}, {25, 30}}))
		p.part(rrect(4, 10, 25, 30, 0))
		p.part(ell(8, 30.5, 1.5, 1.2+w*.5))
		p.part(ell(19, 30.5, 1.2, 1+w*.5))
		p.fill(ell(17, 22.5, 3.5, 3), 2)
		p.fill(rrect(15, 24, 19, 27, .5), 2)
		p.cut(rrect(15, 22, 16, 23, 0))
		p.cut(rrect(18, 22, 19, 23, 0))
		p.pix(pts{{16, 26}, {18, 26}}, 0)
		p.line(7, 27, 11, 24, 2)
		p.pix(pts{{6, 27}, {7, 28}, {11, 23}, {12, 24}}, 2)
		eyes(p, xs(8, 15), 13, mood, f)
		mouth(p, 13, 18, 4, mood, f)
	}

	// ── Épico ──
	C["fenix"] = func(p pen, mood string, f int) {
		fl := float64(f % 2)
		p.line(15, 23, 11, 29, 2)
		p.line(16, 23, 16, 30, 2)
		p.line(17, 23, 21, 29, 2)
		for _, x := range []float64{10, 16, 22} {
			p.part(ell(x, 29.5, 1.6, 1.3))
		}
		for _, s := range []float64{-1, 1} {
			p.part(poly(pts{{16 + s*3, 14}, {16 + s*14, 5 - fl}, {16 + s*12, 9}, {16 + s*15, 11 - fl}, {16 + s*12, 14}, {16 + s*14, 17 - fl}, {16 + s*4, 20}}))
			p.line(16+s*5, 15, 16+s*11, 9-fl, 1)
			p.line(16+s*5, 17, 16+s*11, 15-fl, 1)
		}
		p.part(ell(16, 17, 4.5, 6))
		wig := [4]float64{0, 1, 0, -1}
		for _, xi := range [][2]int{{14, 0}, {16, 1}, {18, 2}} {
			for y := 0; y < 4; y++ {
				p.set(float64(xi[0])+wig[(y+f+xi[1])%4], float64(y+1), iff(y == 0, 1, 2))
			}
		}
		p.part(ell(16, 8, 4, 3.5))
		seye(p, 13, 7, mood, f)
		seye(p, 17, 7, mood, f)
		p.pix(pts{{15, 9}, {16, 9}, {15, 10}, {16, 10}}, 2)
		c := [4][2]float64{{6, 22}, {26, 24}, {9, 26}, {24, 27}}[f%4]
		p.set(c[0], c[1], 1)
	}
	C["mecha"] = func(p pen, mood string, f int) {
		p.part(rrect(8, 27, 15, 30, 1))
		p.part(rrect(17, 27, 24, 30, 1))
		p.part(rrect(10, 20, 14, 27, 1))
		p.part(rrect(18, 20, 22, 27, 1))
		p.part(rrect(3, 14, 8, 22, 2))
		p.part(rrect(24, 14, 29, 22, 2))
		p.part(rrect(2, 21, 8, 25, 2))
		p.part(rrect(24, 21, 30, 25, 2))
		p.part(poly(pts{{8, 11}, {24, 11}, {22, 21}, {10, 21}}))
		p.part(ell(16, 15.5, 2.5, 2.5))
		p.fill(ell(16, 15.5, 1.2, 1.2), iff(f%2 != 0, 2, 1))
		p.part(rrect(1, 9, 9, 15, 2))
		p.part(rrect(23, 9, 31, 15, 2))
		p.line(19, 2, 21, 0, 2)
		p.part(rrect(11, 2, 21, 10, 3))
		p.fill(rrect(12, 5, 20, 7, 0), 1)
		if mood != "sleep" {
			x := 12 + float64([12]int{0, 1, 2, 3, 4, 5, 6, 5, 4, 3, 2, 1}[f%12])
			p.pix(pts{{x, 6}, {x + 1, 6}}, 2)
		}
		p.pix(pts{{14, 9}, {16, 9}, {18, 9}}, 1)
	}

	// ── Lendário ──
	C["kraken"] = func(p pen, mood string, f int) {
		arms := [][3]pt{
			{{10, 17}, {2, 16}, {3, 5}}, {{22, 17}, {30, 16}, {29, 5}},
			{{11, 19}, {6, 26}, {1, 27}}, {{21, 19}, {26, 26}, {31, 27}},
			{{13, 20}, {11, 27}, {7, 30}}, {{19, 20}, {21, 27}, {25, 30}},
		}
		for i, arm := range arms {
			a, b, c := arm[0], arm[1], arm[2]
			d := -1.0
			if (f+i)%4 < 2 {
				d = 1
			}
			cv := curve(a, pt{b[0] + d, b[1]}, pt{c[0] + d, c[1]}, mid)
			p.part(tube(cv, 2, 1))
			for j, q := range cv {
				if j%5 == 3 {
					p.set(math.Floor(q[0]), math.Floor(q[1]), 1)
				}
			}
		}
		p.part(ell(16, 10.5, 8.5, 9.5))
		p.pix(pts{{13, 3}, {19, 4}, {10, 6}, {22, 7}, {16, 2}}, 1)
		eyes(p, xs(11, 18), 10, mood, f)
		mouth(p, 16, 16, 2, mood, f)
	}
	C["jormungandr"] = func(p pen, mood string, f int) {
		cx, cy := 16.0, 16.5
		p.ring(ell(cx, cy, 5, 4.5), 1)
		p.pix(pts{{14, 15}, {15, 15}, {15, 16}, {18, 17}, {18, 18}, {17, 18}, {13, 18}}, 1)
		p.part(and(ell(cx, cy, 14.5, 13.5), not(ell(cx, cy, 9, 8))))
		for i := 0; i < 18; i++ {
			a := float64(float64(float64(i)/18+float64(f)/72) * 2 * math.Pi)
			p.set(jsRound(cx-.5+float64(math.Cos(a)*11.8)), jsRound(cy-.5+float64(math.Sin(a)*10.8)), 1)
		}
		p.part(poly(pts{{23, 2}, {30, 3.5}, {29, 6}, {23, 7}}))
		p.part(ell(21, 4.5, 5, 3.8))
		p.pix(pts{{16, 7}, {17, 8}, {19, 8}, {20, 9}}, 2)
		eyes(p, xs(19), 2, mood, f)
	}
	C["hidra"] = func(p pen, mood string, f int) {
		p.part(tube(curve(pt{24, 26}, pt{32, 27}, pt{30, 19}, mid), 1.8, .8))
		h3 := [][3]pt{{{10, 22}, {4, 16}, {6, 9}}, {{16, 21}, {14, 12}, {16, 6}}, {{22, 22}, {28, 16}, {26, 9}}}
		var heads []pt
		for i, h := range h3 {
			a, b, c := h[0], h[1], h[2]
			d := [4]float64{0, 1, 0, -1}[(f+i)%4]
			e := pt{c[0] + d, c[1]}
			p.part(tube(curve(a, pt{b[0] + d, b[1]}, e, mid), 2.2, 1.6))
			heads = append(heads, e)
		}
		p.part(ell(16, 25.5, 10, 5))
		p.pix(pts{{9, 23}, {12, 22}, {16, 22}, {20, 22}, {23, 23}, {11, 27}, {16, 28}, {21, 27}}, 1)
		for _, h := range heads {
			hx, hy := h[0], h[1]
			p.part(ell(hx, hy-1, 4.5, 3.6))
			seye(p, hx-3, hy-2, mood, f)
			seye(p, hx+1, hy-2, mood, f)
			p.pix(pts{{hx - 1, hy + 1}, {hx, hy + 1}}, 2)
		}
	}

	// ── D&D ──
}

// The lines 0.5.0 brought in, drawn solid from the start.
func init() {
	C := forms
	type pt = [2]float64
	const n = 24 // curve steps

	C["beholder"] = func(p pen, mood string, f int) {
		p.line(9, 31, 23, 31, 1)
		for i, s := range []pt{{4, 6}, {8, 1}, {16, 0}, {24, 1}, {28, 6}} {
			x, y := s[0], s[1]
			w := -1.0
			if (f+i)%2 != 0 {
				w = 1
			}
			p.part(tube(curve(pt{16 + (x-16)*.45, 12}, pt{x + w, (y + 12) / 2}, pt{x, y + 2}, n), 1, 1))
			p.part(ell(x, y+2, 2.2, 2.2))
			dx := 0.0
			if i < 2 {
				dx = -.5
			} else if i > 2 {
				dx = .5
			}
			p.set(x+dx, y+2, iff(mood == "sleep", 2, 0))
		}
		p.part(ell(16, 18, 11.5, 10.5))
		if mood == "sleep" {
			p.line(10, 15, 22, 15, 2)
		} else {
			p.fill(ell(16, 15, 6.5, 4.5), 2)
			p.cut(rrect(15, 12, 17, 18, .8))
			if mood == "happy" {
				p.cut(and(ell(16, 15, 6.5, 4.5), func(x, y float64) bool { return y > 15 }))
			}
		}
		p.cut(and(ell(16, 23, 7, 4), func(x, y float64) bool { return y >= 22 }))
		for x := 10.0; x <= 21; x += 2.5 {
			p.fill(poly(pts{{x, 22}, {x + 2, 22}, {x + 1, 24.5}}), 2)
		}
		if mood == "talk" && f%2 != 0 {
			p.cut(rrect(11, 25, 21, 26, 0))
		}
	}

	C["goblin"] = func(p pen, mood string, f int) {
		p.part(rrect(12, 26, 14, 30, 1))
		p.part(rrect(18, 26, 20, 30, 1))
		p.part(tube(curve(pt{11, 21}, pt{8, 23}, pt{7, 26}, n), 1.4, 1.4))
		p.part(poly(pts{{24, 26}, {28, 17}, {29.5, 18}, {26, 27}}))
		p.line(23, 25, 27, 27, 2)
		p.part(tube(curve(pt{21, 21}, pt{24, 23}, pt{25, 26}, n), 1.4, 1.4))
		p.part(rrect(11, 19, 21, 27, 3))
		p.line(11, 23, 21, 23, 2)
		for _, s := range []float64{-1, 1} {
			p.part(poly(pts{{16 + s*5, 9}, {16 + s*16, 4}, {16 + s*6, 15}}))
			p.line(16+s*7, 10, 16+s*13, 6, 1)
		}
		p.part(ell(16, 12, 7, 6.5))
		eyes(p, xs(11, 18), 9, mood, f)
		p.fill(poly(pts{{14.5, 12}, {17.5, 12}, {16, 16}}), 2)
		mouth(p, 16, 17, 6, mood, f)
		p.pix(pts{{14, 18}, {18, 18}}, 2)
	}

	C["kobold"] = func(p pen, mood string, f int) {
		p.part(tube(curve(pt{12, 26}, pt{3, 30}, pt{2, 22}, n), 2, 1))
		p.line(29, 2, 29, 30, 2)
		p.fill(poly(pts{{27, 8}, {29, 1}, {31, 8}}), 2)
		p.part(rrect(12, 25, 14, 30, 1))
		p.part(rrect(18, 25, 20, 30, 1))
		p.part(ell(16, 21, 5.5, 6))
		for y := 18.0; y <= 24; y += 2 {
			p.line(15, y, 18, y, 1)
		}
		p.part(tube(curve(pt{19, 19}, pt{24, 20}, pt{28, 18}, n), 1.3, 1.3))
		p.part(poly(pts{{12, 9}, {6, 5}, {13, 7}}))
		p.part(poly(pts{{14, 8}, {11, 2}, {16, 7}}))
		p.part(ell(15.5, 11, 5.5, 4.5))
		p.part(rrect(18, 10, 26, 14, 2))
		p.set(25, 11, 0)
		seye(p, 16, 9, mood, f)
		mouth(p, 22, 13, 5, mood, f)
	}

	C["fada"] = func(p pen, mood string, f int) {
		fl := float64(f % 2)
		for _, s := range []float64{-1, 1} {
			p.part(poly(pts{{16 + s*1, 13}, {16 + s*13, 2 + fl}, {16 + s*15, 9}, {16 + s*10, 15}}))
			p.cut(ell(16+s*11, 8, 1.6, 1.6))
			p.part(poly(pts{{16 + s*1, 15}, {16 + s*10, 17}, {16 + s*11, 24 - fl}, {16 + s*5, 22}}))
			p.cut(ell(16+s*8, 20, 1.2, 1.2))
		}
		p.line(15, 23, 14, 28, 2)
		p.line(17, 23, 18, 28, 2)
		p.fill(poly(pts{{12, 24}, {16, 12}, {20, 24}}), 2)
		p.line(18, 16, 24, 13, 2)
		p.line(24, 13, 26, 6, 2)
		p.glyph(25, 2, []string{".#.", "###", ".#."})
		p.part(ell(16, 9.5, 3.8, 3.8))
		p.fill(and(ell(16, 9.5, 3.8, 3.8), func(x, y float64) bool { return y < 8 }), 2)
		seye(p, 13.5, 9, mood, f)
		seye(p, 16.5, 9, mood, f)
		p.pix(pts{{3, 27}, {30, 26}, {7, 30}}, 1)
	}

	C["tiamat"] = func(p pen, mood string, f int) {
		fl := float64(f % 2)
		for _, s := range []float64{-1, 1} {
			p.part(poly(pts{{16 + s*6, 21}, {16 + s*16, 18 - fl}, {16 + s*14, 22}, {16 + s*16, 26 - fl}, {16 + s*9, 27}}))
			p.line(16+s*7, 22, 16+s*15.5, 18.5-fl, 2)
		}
		p.part(tube(curve(pt{23, 27}, pt{30, 31}, pt{33, 27}, n), 1.6, 1))
		p.part(rrect(9, 27, 12, 30, 1))
		p.part(rrect(20, 27, 23, 30, 1))
		p.part(ell(16, 24.5, 8.5, 5))
		// side heads: a wedge facing d on a short neck, a horn swept back.
		side := func(hx, hy, d, nx float64) {
			p.part(tube(curve(pt{nx, 21}, pt{hx - d*1, (hy + 21) / 2}, pt{hx - d*1, hy + 3}, n), 1.8, 1.5))
			p.part(poly(pts{{hx - d*.5, hy + .5}, {hx - d*3, hy - 2}, {hx + d*.5, hy}}))
			p.part(poly(pts{{hx - d*2.5, hy}, {hx + d*1, hy - .5}, {hx + d*5, hy + 1.5}, {hx + d*5, hy + 3}, {hx - d*2.5, hy + 4.5}}))
			seye(p, hx-.5, hy+.5, mood, f)
		}
		side(4, 13, -1, 10)
		side(8, 6, -1, 13)
		side(24, 6, 1, 19)
		side(28, 13, 1, 22)
		// the middle head faces us
		p.part(tube(pts{{16, 21}, {16, 14}, {16, 8}}, 1.9, 1.6))
		p.line(13, 4, 11, 0, 2)
		p.line(19, 4, 21, 0, 2)
		p.part(ell(16, 5, 3.2, 3))
		p.part(rrect(14, 6, 18, 10, 1.2))
		p.pix(pts{{15, 9}, {17, 9}}, 2)
		seye(p, 13.5, 4, mood, f)
		seye(p, 16.5, 4, mood, f)
	}
}
