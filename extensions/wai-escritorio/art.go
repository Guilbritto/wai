package main

import (
	"fmt"
	"strings"

	"wai-escritorio/office"
)

// Original 16×16 pixel art for the office, drawn in code. Tiles are fully opaque
// (furniture is drawn over floor colour); avatars use a space for transparency.

const cell = 16

// avatarNames name the 8 looks in the setup form, in order.
var avatarNames = [office.Avatars]string{"Red", "Blue", "Green", "Yellow", "Orange", "Violet", "Pink", "Purple"}

var artPalette = map[string]string{
	// floor, carpet, wall
	"a": "#c89f6b", "b": "#a97f4f", "c": "#d8b283",
	"d": "#5f8a8b", "e": "#4f7778", "f": "#74a0a0",
	"g": "#5b4660", "h": "#7a6381", "i": "#3a2c40",
	// desk, monitor, chair
	"j": "#b5ab9b", "k": "#d9d2c5", "l": "#efe9dd",
	"m": "#2b2f3a", "n": "#7fd0e0",
	"o": "#3d4f6b", "p": "#5b78a3",
	// plant, table
	"q": "#a0522d", "r": "#3f8f4f", "s": "#62b36a",
	"t": "#9b6b45", "u": "#b07d55", "v": "#84573a",
	// avatars: outline, eye, skin, hair, shirt, pants
	"O": "#2a1f2d", "E": "#1d1a22",
	"K": "#f1c9a0", "L": "#c68f64", "M": "#8d5b3b", "N": "#ffdfc4",
	"A": "#2b1b14", "B": "#d9a441", "C": "#b5382c", "D": "#8a8a96", "F": "#7c4a9e", "G": "#1f2a44",
	"H": "#d9534f", "I": "#3b82c4", "J": "#4aa860", "R": "#e8c040", "S": "#e0782e", "U": "#8e5cc4", "V": "#e86fa0", "W": "#f2f2f2",
	"X": "#3a3f58", "Y": "#6b5a48",
}

// artLegend maps a map character to the tile pattern that draws it.
var artLegend = map[string]string{
	"#": "wall", ".": "floor", ",": "carpet", "S": "carpet",
	"D": "desk", "C": "chair", "P": "plant", "T": "table",
	"c": "chair_carpet", "p": "plant_carpet",
}

var artPatterns = buildPatterns()

func avatarPattern(i int) string { return fmt.Sprintf("av%d", i) }

type canvas [cell][cell]byte

func newCanvas(fill byte) *canvas {
	c := &canvas{}
	for y := range c {
		for x := range c[y] {
			c[y][x] = fill
		}
	}
	return c
}

func (c *canvas) rect(x0, y0, x1, y1 int, k byte) *canvas {
	for y := y0; y <= y1; y++ {
		for x := x0; x <= x1; x++ {
			c[y][x] = k
		}
	}
	return c
}

func (c *canvas) rows() []string {
	out := make([]string, cell)
	for y := range c {
		out[y] = string(c[y][:])
	}
	return out
}

func floorCanvas() *canvas {
	c := newCanvas('a')
	c.rect(0, 7, 15, 7, 'b').rect(0, 15, 15, 15, 'b')
	c.rect(5, 0, 5, 6, 'b').rect(12, 8, 12, 14, 'b')
	for _, p := range [][2]int{{2, 2}, {3, 2}, {9, 4}, {8, 11}, {9, 11}, {14, 12}, {1, 10}, {14, 3}} {
		c[p[1]][p[0]] = 'c'
	}
	return c
}

func carpetCanvas() *canvas {
	carpet := newCanvas('d')
	for y := 0; y < cell; y++ {
		for x := 0; x < cell; x++ {
			switch {
			case x%4 == 1 && y%4 == 1:
				carpet[y][x] = 'f'
			case x%4 == 3 && y%4 == 3:
				carpet[y][x] = 'e'
			}
		}
	}
	return carpet
}

func drawChair(chair *canvas) *canvas {
	chair.rect(4, 3, 11, 5, 'o').rect(4, 6, 11, 10, 'p').rect(4, 11, 11, 11, 'o')
	chair.rect(5, 12, 5, 13, 'm').rect(10, 12, 10, 13, 'm')
	return chair
}

func drawPlant(plant *canvas) *canvas {
	for y := 0; y < 11; y++ {
		for x := 0; x < cell; x++ {
			dx, dy := float64(x)-7.5, float64(y)-5.5
			switch d := dx*dx + dy*dy; {
			case d <= 9 && (x+y)%3 == 0:
				plant[y][x] = 's'
			case d <= 20:
				plant[y][x] = 'r'
			}
		}
	}
	plant.rect(5, 10, 10, 14, 'q').rect(5, 10, 10, 10, 'v')
	return plant
}

func buildPatterns() map[string][]string {
	p := map[string][]string{}

	p["floor"] = floorCanvas().rows()

	carpet := carpetCanvas()
	p["carpet"] = carpet.rows()

	wall := newCanvas('g')
	for y := 3; y < cell; y += 4 {
		wall.rect(0, y, 15, y, 'i')
	}
	for row := 0; row < 4; row++ {
		y := row * 4
		wall.rect(0, y, 15, y, 'h')
		seams := []int{7, 15}
		if row%2 == 1 {
			seams = []int{3, 11}
		}
		for _, x := range seams {
			wall.rect(x, y, x, y+2, 'i')
		}
	}
	p["wall"] = wall.rows()

	desk := floorCanvas()
	desk.rect(0, 7, 15, 13, 'k').rect(0, 7, 15, 7, 'l').rect(0, 13, 15, 13, 'j')
	desk.rect(5, 1, 10, 5, 'm').rect(6, 2, 9, 4, 'n').rect(7, 6, 8, 6, 'm')
	desk.rect(5, 10, 10, 10, 'o').rect(0, 14, 15, 14, 'b')
	p["desk"] = desk.rows()

	p["chair"] = drawChair(floorCanvas()).rows()
	p["chair_carpet"] = drawChair(carpetCanvas()).rows()

	p["plant"] = drawPlant(floorCanvas()).rows()
	p["plant_carpet"] = drawPlant(carpetCanvas()).rows()

	table := newCanvas('t')
	table.rect(1, 5, 7, 5, 'v').rect(9, 11, 14, 11, 'v')
	for _, q := range [][2]int{{3, 2}, {4, 2}, {11, 3}, {6, 8}, {7, 8}, {13, 8}, {2, 13}, {3, 13}, {10, 14}} {
		table[q[1]][q[0]] = 'u'
	}
	p["table"] = table.rows()

	short := []string{
		"................",
		".....oooooo.....",
		"....ohhhhhho....",
		"....ohhhhhho....",
		"....ohkkkkho....",
		"....okekkeko....",
		"....okkkkkko....",
		".....okkkko.....",
		"....oossssoo....",
		"...osssssssso...",
		"...okssssssko...",
		"...okssssssko...",
		"....osssssso....",
		"....oppppppo....",
		"....oppooppo....",
		"....oooooooo....",
	}
	long := append([]string(nil), short...)
	long[2] = "....ohhhhhho...."
	long[3] = "...ohhhhhhhho..."
	long[4] = "...ohkkkkkkho..."
	long[5] = "...ohkekkekho..."
	long[6] = "...ohkkkkkkho..."
	long[7] = "...ohhkkkkhho..."
	long[8] = "...ohossssoho..."
	long[9] = "...ohssssssho..."
	looks := [office.Avatars]struct {
		long                    bool
		hair, skin, shirt, pant string
	}{
		{false, "A", "K", "H", "X"}, {true, "B", "N", "I", "X"},
		{false, "C", "L", "J", "Y"}, {true, "D", "M", "R", "X"},
		{false, "G", "K", "S", "Y"}, {true, "F", "N", "W", "X"},
		{false, "B", "M", "V", "X"}, {true, "A", "L", "U", "Y"},
	}
	for i, l := range looks {
		tpl := short
		if l.long {
			tpl = long
		}
		r := strings.NewReplacer("h", l.hair, "k", l.skin, "s", l.shirt, "p", l.pant, "o", "O", "e", "E", ".", " ")
		rows := make([]string, len(tpl))
		for y, row := range tpl {
			rows[y] = r.Replace(row)
		}
		p[avatarPattern(i)] = rows
	}
	return p
}
