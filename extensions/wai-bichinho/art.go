package main

import (
	"math"
	"strings"
)

// Pixel art for the pets, ported from the pet-dex mockup. Names follow the JS
// so the two can be read side by side.

const W, H = 36, 32

type grid [H][W]uint8 // 0 empty, 1 dim, 2 full

type mask func(x, y float64) bool

type drawer interface {
	set(x, y float64, v uint8)
	part(m mask)
	ring(m mask, v uint8)
	fill(m mask, v uint8)
	pix(pts [][2]float64, v uint8)
	line(x0, y0, x1, y1 float64, v uint8)
	glyph(x, y float64, rows []string)
	cut(m mask)
	scale() float64 // the JS `p.s`
}

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

func (p pen) scale() float64 { return 1 }

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

// scaledPen draws the same shapes smaller: they shrink toward the feet, eyes and mouths keep their size.
type scaledPen struct {
	p drawer
	s float64
}

const scaleCX, scaleBY = 16.0, 30.5

func scaled(p drawer, s float64) drawer { return scaledPen{p, s} }

func (d scaledPen) scale() float64 { return d.s }

func (d scaledPen) T(m mask) mask {
	return func(x, y float64) bool {
		return m(scaleCX+float64((x+.5-scaleCX)/d.s)-.5, scaleBY+float64((y+.5-scaleBY)/d.s)-.5)
	}
}

func (d scaledPen) P(x, y float64) (float64, float64) {
	return jsRound(scaleCX + float64((x+.5-scaleCX)*d.s) - .5), jsRound(scaleBY + float64((y+.5-scaleBY)*d.s) - .5)
}

func (d scaledPen) set(x, y float64, v uint8) {
	px, py := d.P(x, y)
	d.p.set(px, py, v)
}
func (d scaledPen) part(m mask)          { d.p.part(d.T(m)) }
func (d scaledPen) ring(m mask, v uint8) { d.p.ring(d.T(m), v) }
func (d scaledPen) fill(m mask, v uint8) { d.p.fill(d.T(m), v) }
func (d scaledPen) cut(m mask)           { d.p.cut(d.T(m)) }
func (d scaledPen) pix(list pts, v uint8) {
	out := make(pts, len(list))
	for i, pt := range list {
		out[i][0], out[i][1] = d.P(pt[0], pt[1])
	}
	d.p.pix(out, v)
}
func (d scaledPen) line(x0, y0, x1, y1 float64, v uint8) {
	ax, ay := d.P(x0, y0)
	bx, by := d.P(x1, y1)
	d.p.line(ax, ay, bx, by, v)
}
func (d scaledPen) glyph(x, y float64, rows []string) {
	w, h := float64(len(rows[0])), float64(len(rows))
	d.p.glyph(
		jsRound(scaleCX+float64((x+w/2-scaleCX)*d.s)-w/2),
		jsRound(scaleBY+float64((y+h/2-scaleBY)*d.s)-h/2),
		rows)
}

func sw(p drawer, w float64) int { return int(math.Max(2, jsRound(w*p.scale()))) }

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

func eyes(p drawer, xs []float64, y float64, mood string, f int) {
	for _, x := range xs {
		p.glyph(x, y, EYES[look(mood, f)])
	}
}

func seye(p drawer, x, y float64, mood string, f int) { p.glyph(x, y, SEYES[look(mood, f)]) }

func mouth(p drawer, cx, y float64, w int, mood string, f int) {
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

func fx(p drawer, mood string, f int, shiny bool) {
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

func wave(p drawer, y float64, f int) {
	for x := 0.0; x < 32; x++ {
		dy := 1.0
		if math.Sin((x+float64(f))/1.6) > 0 {
			dy = 0
		}
		p.set(x, y+dy, 1)
	}
}

// forms maps a form id to its drawing; "ovo" is the egg.
var forms map[string]func(d drawer, mood string, f int)

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
	forms = map[string]func(d drawer, mood string, f int){}
	C := forms
	baby := func(of string, s float64) func(drawer, string, int) {
		return func(p drawer, m string, f int) { C[of](scaled(p, s), m, f) }
	}

	C["ovo"] = func(p drawer, mood string, f int) {
		p.part(ell(16, 17, 8.5, 11.5))
		p.glyph(9, 15, []string{"#..#...#..#..#..", ".##.#.#.##.#.#.", ".....#........."})
		p.pix(pts{{12, 9}, {13, 9}, {19, 22}, {20, 22}, {18, 23}}, 1)
	}

	// ── Comum ──
	C["gosma"] = func(p drawer, mood string, f int) {
		w := 0.0
		if f%2 != 0 {
			w = .6
		}
		p.part(and(ell(16, 24, 12.5+w, 11-w), func(x, y float64) bool { return y <= 29 }))
		p.ring(ell(22, 26.5, 1.4, 1.4), 1)
		p.pix(pts{{9, 16}, {8, 17}, {8, 18}}, 1)
		eyes(p, xs(12, 18), 18, mood, f)
		mouth(p, 16, 24, sw(p, 4), mood, f)
	}
	C["gotinha"] = baby("gosma", .6)
	C["gosmao"] = func(p drawer, mood string, f int) {
		w := 0.0
		if f%2 != 0 {
			w = .7
		}
		p.part(and(ell(16, 22, 15.5+w, 15-w), func(x, y float64) bool { return y <= 29 }))
		p.glyph(18, 23, grey([]string{".##.....", "#..#####", ".##..#.#"}))
		p.ring(ell(7, 24, 1.4, 1.4), 1)
		p.ring(ell(24, 13, 1.2, 1.2), 1)
		p.set(10, 27, 1)
		p.pix(pts{{6, 13}, {5, 14}, {5, 15}}, 1)
		eyes(p, xs(11, 18), 13, mood, f)
		mouth(p, 15.5, 19, 6, mood, f)
	}

	C["larvinha"] = func(p drawer, mood string, f int) {
		segs := pts{{5, 26}, {9, 25.5}, {13, 26}, {17, 25.5}}
		for i, sg := range segs {
			x, y := sg[0], sg[1]
			p.set(x-1, 29, 0)
			p.set(x+1, 29, 0)
			dy := 0.0
			if (i+f)%2 != 0 {
				dy = .6
			}
			p.part(ell(x, y-dy, 3, 3))
			p.set(x, 25, iff((i+f)%3 != 0, 1, 2))
		}
		p.line(21, 16, 19, 12, 2)
		p.line(25, 16, 27, 12, 2)
		p.set(19, 11, iff(f%2 != 0, 2, 1))
		p.set(27, 11, iff(f%2 != 0, 1, 2))
		p.part(ell(23, 21, 5, 5))
		eyes(p, xs(19, 24), 19, mood, f)
		mouth(p, 23, 24, 2, mood, f)
	}
	C["bug"] = func(p drawer, mood string, f int) {
		for _, s := range []float64{-1, 1} {
			p.line(16+s*8, 19, 16+s*12, 16, 2)
			p.line(16+s*9, 23, 16+s*13, 23, 2)
			p.line(16+s*8, 27, 16+s*12, 30, 2)
			p.line(16+s*3, 8, 16+s*6, 3, 2)
			p.set(16+s*7, 2, iff(f%2 != 0, 2, 1))
		}
		p.part(ell(16, 22, 9, 7.5))
		p.line(16, 17, 16, 29, 2)
		for _, s := range []float64{-1, 1} {
			p.line(16+s*3, 20, 16+s*6, 20, 1)
			p.line(16+s*6, 20, 16+s*6, 25, 1)
			p.set(16+s*6, 26, 0)
			p.line(16+s*2, 24, 16+s*4, 24, 1)
		}
		p.part(ell(16, 12.5, 6.5, 5))
		eyes(p, xs(12, 18), 10, mood, f)
		mouth(p, 16, 15, 2, mood, f)
	}
	C["megabug"] = func(p drawer, mood string, f int) {
		fl := float64(f % 2)
		for _, s := range []float64{-1, 1} {
			p.ring(ell(16+s*11, 13-fl, 4, 8), 1)
			p.line(16+s*10, 19, 16+s*15, 15, 2)
			p.line(16+s*11, 23, 16+s*16, 24, 2)
			p.line(16+s*10, 27, 16+s*14, 30, 2)
		}
		p.part(ell(16, 22, 11.5, 8))
		p.line(16, 17, 16, 20, 2)
		p.part(rrect(13, 21, 18, 25, 1))
		p.set(15, 23, iff(f%2 != 0, 2, 1))
		p.set(16, 23, iff(f%2 != 0, 1, 2))
		for _, s := range []float64{-1, 1} {
			p.line(16+s*4, 19, 16+s*8, 19, 1)
			p.line(16+s*8, 19, 16+s*8, 26, 1)
			p.set(16+s*8, 27, 0)
			p.line(16+s*4, 26, 16+s*5, 27, 1)
			p.set(16+s*10, 22, 1)
		}
		p.part(ell(16, 12, 7, 5))
		p.part(poly(pts{{14, 8}, {18, 8}, {19, 4}, {22, 0.5}, {16.5, 2.5}}))
		eyes(p, xs(12, 18), 10, mood, f)
		mouth(p, 16, 14.5, 2, mood, f)
	}

	C["fagulha"] = func(p drawer, mood string, f int) {
		t := float64([4]int{0, 1, 0, -1}[f%4])
		p.part(poly(pts{{16 + t, 8}, {20, 14}, {22, 20}, {21, 25}, {16, 28}, {11, 25}, {10, 20}, {12, 14}}))
		s := pts{{7, 13}, {24, 10}, {25, 21}, {6, 23}}[f%4]
		p.set(s[0], s[1], 1)
		eyes(p, xs(12, 18), 18, mood, f)
		mouth(p, 16, 23.5, 2, mood, f)
	}
	C["fogo"] = func(p drawer, mood string, f int) {
		a := float64(f % 2)
		flame := pts{{8, 17}, {7, 8}, {11, 12}, {14, 2}, {16, 10}, {20, 3}, {21, 11}, {25, 8}, {24, 17}}
		if a != 0 {
			flame = pts{{8, 17}, {9, 9}, {11, 13}, {13, 3}, {16, 11}, {19, 1}, {21, 11}, {24, 6}, {24, 17}}
		}
		p.part(or(
			ell(16, 18, 9, 7.5),
			poly(flame),
			poly(pts{{10, 22}, {22, 22}, {21, 26}, {25, 29 - a}, {17, 28.5}, {12, 26}}),
		))
		s := pts{{11, 23}, {20, 12}, {12, 12}, {21, 23}}[f%4]
		p.set(s[0], s[1], 1)
		s = pts{{4, 8}, {28, 14}, {3, 20}, {29, 24}}[f%4]
		p.set(s[0], s[1], 1)
		eyes(p, xs(12, 18), 15, mood, f)
		mouth(p, 16, 20.5, 2, mood, f)
	}

	C["bit"] = func(p drawer, mood string, f int) {
		p.line(16, 12, 16, 10, 2)
		p.fill(ell(16, 8.5, 1.6, 1.6), iff(f%2 != 0, 2, 1))
		p.pix(pts{{7, 19}, {7, 20}, {24, 19}, {24, 20}}, 2)
		p.part(rrect(10, 28, 13, 30, 0))
		p.part(rrect(18, 28, 21, 30, 0))
		p.part(rrect(8, 13, 23, 28, 3))
		p.ring(rrect(10, 15, 21, 25, 2), 1)
		eyes(p, xs(11, 17), 17, mood, f)
		mouth(p, 15.5, 22.5, 2, mood, f)
	}
	C["bot"] = func(p drawer, mood string, f int) {
		p.line(16, 3, 16, 1, 2)
		p.pix(pts{{15, 0}, {16, 0}}, iff(f%2 != 0, 2, 1))
		p.part(rrect(5, 17, 8, 24, 1))
		p.part(rrect(23, 17, 26, 24, 1))
		p.part(rrect(8, 25, 23, 30, 2))
		for i := 0; i < 4; i++ {
			p.set(float64(10+i*3+f%3), 27, 1)
		}
		p.part(rrect(10, 16, 21, 25, 2))
		for i := 0; i < 3; i++ {
			p.set(float64(12+i*3), 20, iff((f+i)%3 != 0, 1, 2))
		}
		p.pix(pts{{15, 16}, {16, 16}}, 2)
		p.part(rrect(6, 7, 7, 11, 0))
		p.part(rrect(24, 7, 25, 11, 0))
		p.part(rrect(7, 3, 24, 15, 3))
		p.ring(rrect(9, 5, 22, 13, 2), 1)
		eyes(p, xs(11, 17), 6, mood, f)
		mouth(p, 15.5, 11, 2, mood, f)
	}

	C["abissal"] = func(p drawer, mood string, f int) {
		fl := float64(f % 2)
		n := 9
		if p.scale() < 1 {
			n = 5
		}
		p.part(poly(pts{{7, 19}, {0, 13 - fl}, {2, 19}, {0, 26 + fl}}))
		p.part(poly(pts{{9, 14}, {11, 8}, {15, 12}}))
		p.part(ell(15, 19, 10, 8.5))
		p.pix(pts{{9, 17}, {11, 22}, {8, 21}, {13, 25}, {10, 25}}, 1)
		p.line(20, 11, 21, 5, 2)
		p.line(21, 5, 25, 4, 2)
		p.fill(ell(26.5, 5.5, 1.8, 1.8), iff(f%2 != 0, 2, 1))
		eyes(p, xs(17), 13, mood, f)
		open := (mood == "talk" && f%2 != 0) || mood == "hungry"
		teeth := strings.Repeat(".#", n)[:n]
		full, none := strings.Repeat("#", n), strings.Repeat(".", n)
		if open {
			p.glyph(float64(24-n), 20, []string{full, teeth, none, teeth, full})
		} else {
			p.glyph(float64(24-n), 20, []string{full, teeth})
		}
		p.set(4, float64([4]int{9, 7, 5, 3}[f%4]), 1)
	}
	C["alevino"] = baby("abissal", .62)

	C["bauzinho"] = baby("mimico", .62)
	C["mimico"] = func(p drawer, mood string, f int) {
		open := (mood == "talk" && f%2 != 0) || mood == "hungry"
		top := float64(8 - f%2)
		if mood == "sleep" {
			top = 13
		} else if open {
			top = 5
		}
		bot := top + 5
		p.part(rrect(5, 19, 27, 29, 1))
		if p.scale() == 1 {
			p.line(9, 21, 9, 28, 1)
			p.line(23, 21, 23, 28, 1)
		}
		top1 := []float64{7, 11, 19, 24}
		if p.scale() < 1 {
			top1 = []float64{7, 24}
		}
		for _, x := range top1 {
			p.pix(pts{{x, 18}, {x + 1, 18}, {x, 17}}, 2)
		}
		p.part(rrect(5, top, 27, bot, 2))
		p.line(9, top+1, 9, bot-1, 1)
		p.line(23, top+1, 23, bot-1, 1)
		if mood != "sleep" {
			bots := []float64{8, 13, 18, 22}
			if p.scale() < 1 {
				bots = []float64{8, 22}
			}
			for _, x := range bots {
				p.pix(pts{{x, bot + 1}, {x + 1, bot + 1}, {x + 1, bot + 2}}, 2)
			}
			if p.scale() == 1 {
				p.part(tube(curve([2]float64{15, 17}, [2]float64{14, 25}, [2]float64{18, 27}, 24), 1.7, 1.2))
				p.line(15, 19, 15, 23, 1)
			}
		}
		eyes(p, xs(11, 19), top+1, mood, f)
	}

	// ── Incomum ──
	C["grifo"] = func(p drawer, mood string, f int) {
		fl := float64(f % 2)
		p.line(6, 21, 3, 16, 2)
		p.line(3, 16, 3, 13, 2)
		p.part(ell(3, 11.5, 1.8, 2.2))
		p.part(rrect(7, 23, 9, 30, 1))
		p.part(rrect(11, 24, 13, 30, 1))
		p.part(rrect(18, 24, 20, 30, 1))
		p.part(rrect(22, 23, 24, 30, 1))
		p.pix(pts{{17, 30}, {21, 30}, {25, 30}}, 2)
		p.part(ell(15, 21, 9.5, 5))
		p.part(poly(pts{{8, 19}, {4, 7 - fl}, {8, 9 - fl}, {10, 4 - fl}, {13, 8 - fl}, {16, 6 - fl}, {17, 17}}))
		p.line(11, 17, 8, 10-fl, 1)
		p.line(14, 17, 13, 9-fl, 1)
		p.part(poly(pts{{26, 10}, {31, 12}, {30, 14}, {27, 15}}))
		p.part(poly(pts{{20, 9}, {19, 4}, {22, 8}}))
		p.part(ell(23, 12, 4.5, 4.5))
		p.pix(pts{{21, 16}, {23, 17}, {25, 16}}, 1)
		seye(p, 23, 11, mood, f)
	}
	C["grifinho"] = baby("grifo", .62)

	C["pedrinha"] = func(p drawer, mood string, f int) {
		p.line(16, 15, 16, 12, 2)
		p.part(ell(18, 11.5, 2, 1.2))
		p.part(ell(14, 12, 1.6, 1))
		p.part(poly(pts{{7, 29}, {6, 23}, {9, 18}, {15, 15.5}, {22, 16.5}, {26, 21}, {26, 29}}))
		p.line(23, 18, 24, 21, 1)
		p.line(24, 21, 23, 23, 1)
		p.pix(pts{{8, 22}, {9, 21}}, 1)
		eyes(p, xs(11, 18), 20, mood, f)
		mouth(p, 15.5, 26, 2, mood, f)
	}
	C["golem"] = func(p drawer, mood string, f int) {
		p.part(rrect(9, 23, 14, 30, 1))
		p.part(rrect(18, 23, 23, 30, 1))
		p.part(rrect(7, 10, 25, 24, 3))
		R := []string{"..#..", ".#.#.", "#...#", ".#.#.", "#...#"}
		if f%4 < 2 {
			p.glyph(14, 14, R)
		} else {
			p.glyph(14, 14, grey(R))
		}
		p.line(9, 13, 11, 16, 1)
		p.line(22, 18, 20, 21, 1)
		p.part(rrect(2, 10, 8, 21, 2))
		p.part(rrect(24, 10, 30, 21, 2))
		p.part(rrect(1, 20, 8, 26, 2))
		p.part(rrect(24, 20, 31, 26, 2))
		p.pix(pts{{3, 10}, {4, 9}, {5, 10}, {27, 9}, {28, 10}}, 1)
		p.part(rrect(11, 2, 21, 11, 2))
		seye(p, 13, 5, mood, f)
		seye(p, 17, 5, mood, f)
		mouth(p, 16, 8.5, 2, mood, f)
	}

	C["corvo"] = func(p drawer, mood string, f int) {
		p.part(poly(pts{{9, 21}, {2, 28}, {5, 29.5}, {13, 24}}))
		p.part(rrect(0, 27, 31, 28, 0))
		p.part(ell(14, 19, 6.5, 7.5))
		p.glyph(10, 18, grey([]string{"#.#.#", ".###.", "..#..", "..#.."}))
		p.part(poly(pts{{22, 8}, {30, 10.5}, {22, 12.5}}))
		p.part(ell(19, 10, 4.5, 4.2))
		p.pix(pts{{16, 6}, {17, 5}, {15, 7}}, 1)
		seye(p, 19, 9, mood, f)
		p.pix(pts{{13, 26}, {16, 26}}, 2)
	}
	C["corvinho"] = baby("corvo", .62)

	C["drone"] = func(p drawer, mood string, f int) {
		r := f%2 != 0
		for _, xsn := range [][2]float64{{5, -1}, {27, 1}} {
			x, s := xsn[0], xsn[1]
			p.line(x, 13, 16+s*5, 16, 2)
			p.line(x, 11, x, 13, 2)
			if r {
				p.line(x-4, 10, x+4, 10, 2)
			} else {
				p.line(x-2, 10, x+2, 10, 1)
			}
		}
		p.line(12, 21, 11, 25, 2)
		p.line(20, 21, 21, 25, 2)
		p.part(ell(16, 22.5, 3, 2.2))
		p.pix(pts{{15, 22}, {16, 22}}, iff(f%4 != 0, 2, 1))
		p.part(rrect(10, 13, 22, 21, 3))
		eyes(p, xs(12, 18), 14, mood, f)
		p.pix(hrow(13, 30, 7), 1)
	}

	C["ursocoruja"] = func(p drawer, mood string, f int) {
		p.part(ell(11, 29.5, 3.2, 1.5))
		p.part(ell(21, 29.5, 3.2, 1.5))
		for _, s := range []float64{-1, 1} {
			p.part(ell(16+s*10, 22, 3, 5))
			p.pix(pts{{16 + s*9 - 1, 27}, {16 + s*10, 28}, {16 + s*11, 27}}, 2)
		}
		p.part(ell(16, 22, 9, 7.5))
		for _, y := range []float64{19, 22, 25} {
			p.line(13, y, 15, y+1, 1)
			p.line(16, y+1, 18, y, 1)
		}
		p.part(poly(pts{{9, 7}, {7, 0}, {12, 4}}))
		p.part(poly(pts{{23, 7}, {25, 0}, {20, 4}}))
		p.part(ell(16, 10, 8, 7))
		p.ring(ell(12.5, 10, 3.5, 3.5), 1)
		p.ring(ell(19.5, 10, 3.5, 3.5), 1)
		eyes(p, xs(11, 18), 8, mood, f)
		p.pix(pts{{15, 13}, {16, 13}, {15, 14}, {16, 14}}, 2)
	}
	C["ursinhocoruja"] = baby("ursocoruja", .62)

}

func init() {
	C := forms
	type pt = [2]float64
	const mid = 24 // the JS curve default n

	// ── Raro ──
	C["dragaozinho"] = func(p drawer, mood string, f int) {
		fl := float64(f % 2)
		p.part(poly(pts{{21, 26}, {28, 24}, {31, 19}, {30, 25}, {23, 29}}))
		for _, s := range []float64{-1, 1} {
			p.part(poly(pts{{16 + s*5, 19}, {16 + s*12, 14 - fl}, {16 + s*11, 19}, {16 + s*9, 23}}))
		}
		p.part(ell(16, 24, 7.5, 6))
		p.ring(ell(16, 24, 3.5, 3.5), 1)
		p.part(and(ell(16, 27, 11, 4.5), func(x, y float64) bool { return y >= 27 }))
		for x := 6; x <= 25; x++ {
			p.set(float64(x), float64(26+x%2), 0)
		}
		p.part(poly(pts{{10, 9}, {8, 2}, {13, 7}}))
		p.part(poly(pts{{22, 9}, {24, 2}, {19, 7}}))
		p.part(ell(16, 13, 8, 6.5))
		p.pix(pts{{14, 16}, {17, 16}}, 1)
		eyes(p, xs(11, 18), 10, mood, f)
		mouth(p, 16, 17.5, 2, mood, f)
	}
	C["dragao"] = func(p drawer, mood string, f int) {
		fl := float64(f % 2)
		p.part(tube(curve(pt{9, 24}, pt{1, 28}, pt{2, 19}, mid), 1.6, .9))
		p.part(poly(pts{{0, 19}, {2, 14}, {4.5, 19}}))
		p.part(poly(pts{{8, 18}, {3, 6 - fl}, {7, 8 - fl}, {9, 3 - fl}, {12, 8 - fl}, {15, 5 - fl}, {16, 17}}))
		p.line(11, 16, 9, 6-fl, 1)
		p.line(13, 16, 14, 8-fl, 1)
		p.part(ell(9, 26.5, 4, 3.8))
		p.part(rrect(19, 24, 21, 30, 1))
		p.part(ell(14, 21, 8, 6.5))
		for y := 18.0; y <= 25; y += 2 {
			p.line(18, y, 20, y, 1)
		}
		p.part(poly(pts{{18, 17}, {22, 9}, {26, 10}, {23, 20}}))
		p.part(rrect(26, 8, 32, 12, 2))
		p.part(poly(pts{{22, 6}, {19, 0}, {25, 4}}))
		p.part(ell(24, 8.5, 4, 3.5))
		p.set(31, 9, 0)
		seye(p, 24, 7, mood, f)
		if mood != "sleep" && f%4 < 2 {
			p.pix(pts{{33, 7 - fl}, {33, 5 - fl}}, 1)
		}
	}
	C["caozinho"] = func(p drawer, mood string, f int) {
		p.part(rrect(11, 26, 13, 30, 1))
		p.part(rrect(19, 26, 21, 30, 1))
		p.part(ell(16, 25, 7, 4.5))
		for _, s := range []float64{-1, 1} {
			p.part(poly(pts{{16 + s*9, 17}, {16 + s*10, 13}, {16 + s*7, 16}}))
			p.part(ell(16+s*8, 19, 3.2, 2.8))
			p.pix(pts{{16 + s*9 - 1, 19}, {16 + s*9, 19}}, 2)
		}
		p.part(poly(pts{{10, 12}, {9, 5}, {14, 9}}))
		p.part(poly(pts{{22, 12}, {23, 5}, {18, 9}}))
		p.part(ell(16, 14, 6.5, 5.5))
		eyes(p, xs(11, 18), 11, mood, f)
		p.pix(pts{{15, 15}, {16, 15}}, 2)
		mouth(p, 16, 17, 2, mood, f)
	}
	C["cerbero"] = func(p drawer, mood string, f int) {
		p.part(rrect(9, 24, 12, 30, 1))
		p.part(rrect(20, 24, 23, 30, 1))
		p.part(tube(curve(pt{25, 22}, pt{31, 21}, pt{29, 15}, mid), 1.2, .8))
		p.part(ell(16, 23, 9.5, 5.5))
		head := func(cx, cy float64, big bool) {
			rx, ry := 4.5, 4.0
			if big {
				rx, ry = 5.5, 4.8
			}
			p.part(poly(pts{{cx - rx + 1, cy - 1}, {cx - rx, cy - ry - 4}, {cx - 1, cy - ry + .5}}))
			p.part(poly(pts{{cx + rx - 1, cy - 1}, {cx + rx, cy - ry - 4}, {cx + 1, cy - ry + .5}}))
			p.part(ell(cx, cy, rx, ry))
			seye(p, cx-3, cy-1, mood, f)
			seye(p, cx+1, cy-1, mood, f)
			p.pix(pts{{cx - 1, cy + 2}, {cx, cy + 2}}, 2)
		}
		head(6, float64(14+f%2), false)
		head(26, float64(14+(f+1)%2), false)
		head(16, 10, true)
		mouth(p, 15.5, 13.5, 2, mood, f)
		p.pix(pts{{10, 19}, {13, 20}, {16, 20}, {19, 20}, {22, 19}}, 1)
	}

	C["cavalinho"] = func(p drawer, mood string, f int) {
		p.part(tube(curve(pt{15, 22}, pt{18, 30}, pt{11, 29}, mid), 1.8, 1.2))
		p.part(tube(curve(pt{11, 29}, pt{8, 27}, pt{11, 25}, mid), 1.2, 1))
		p.part(poly(pts{{11, 14}, {6, float64(17 - f%2)}, {11, 21}}))
		p.part(ell(16, 18, 5, 6.5))
		for y := 14.0; y <= 22; y += 2 {
			p.line(18, y, 19, y, 1)
		}
		p.part(rrect(19, 9, 26, 11, 1))
		p.part(ell(17, 9, 4, 3.5))
		p.pix(pts{{15, 5}, {16, 4}, {17, 5}, {18, 4}}, 2)
		seye(p, 17, 8, mood, f)
		p.set(28, []float64{11, 9, 7, 5}[f%4], 1)
	}
	C["hipocampo"] = func(p drawer, mood string, f int) {
		wave(p, 28, f)
		p.part(poly(pts{{1, 20}, {0, 12}, {3, 16}, {6, 12}, {5, 20}}))
		p.part(tube(curve(pt{12, 22}, pt{3, 27}, pt{3, 19}, mid), 2.2, 1.3))
		for _, c := range (pts{{7, 25}, {10, 24}, {4, 23}}) {
			p.set(c[0], c[1], 1)
		}
		p.part(poly(pts{{19, 8}, {14, 10}, {17, 12}, {13, 15}, {17, 17}}))
		p.part(rrect(18, 22, 20, 27, 1))
		p.part(poly(pts{{16, 27}, {23, 26}, {21, 29}}))
		p.part(ell(15, 21, 7, 5))
		p.part(poly(pts{{15, 18}, {19, 8}, {24, 9}, {21, 20}}))
		p.part(rrect(24, 7, 31, 11, 2))
		p.part(poly(pts{{20, 5}, {21, 0}, {23, 4}}))
		p.part(ell(23, 7, 4, 3.5))
		seye(p, 23, 6, mood, f)
	}

	C["cubo"] = func(p drawer, mood string, f int) {
		j := float64(f % 2)
		p.part(poly(pts{{24, 10}, {30, 4 + j}, {30, 23}, {24, 29}}))
		p.part(poly(pts{{6, 10}, {12, 4 + j}, {30, 4 + j}, {24, 10}}))
		p.part(rrect(6, 10, 24, 29, 0))
		p.glyph(17, 20, grey([]string{".###.", "#.#.#", "#####", ".#.#."}))
		p.line(8, 27, 13, 22, 1)
		p.line(9, 22, 11, 24, 1)
		p.ring(ell(27, 15, 1.4, 1.4), 1)
		p.set(21, 26, 1)
		eyes(p, xs(10, 17), 13, mood, f)
		mouth(p, 15, 18.5, 2, mood, f)
	}

	// ── Épico ──
	C["fenix"] = func(p drawer, mood string, f int) {
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
	C["mecha"] = func(p drawer, mood string, f int) {
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
	C["kraken"] = func(p drawer, mood string, f int) {
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
	C["jormungandr"] = func(p drawer, mood string, f int) {
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
	C["hidra"] = func(p drawer, mood string, f int) {
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

	C["beholder"] = func(p drawer, mood string, f int) {
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

	C["goblin"] = func(p drawer, mood string, f int) {
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
		mouth(p, 16, 17, sw(p, 6), mood, f)
		p.pix(pts{{14, 18}, {18, 18}}, 2)
	}

	C["kobold"] = func(p drawer, mood string, f int) {
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
		mouth(p, 22, 13, sw(p, 5), mood, f)
	}

	C["fada"] = func(p drawer, mood string, f int) {
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

	C["tiamat"] = func(p drawer, mood string, f int) {
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
