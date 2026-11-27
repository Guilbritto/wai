// Package office holds the rules the office server and the extension share.
package office

import (
	_ "embed"
	"errors"
	"fmt"
	"strings"
)

// Point is a tile position.
type Point struct{ X, Y int }

// Map is a parsed office layout.
type Map struct {
	W, H  int
	Rows  []string
	Spawn Point
}

//go:embed default.map
var defaultMap string

// DefaultMap returns the built-in office.
func DefaultMap() Map {
	m, err := ParseMap(defaultMap)
	if err != nil {
		panic("office: bad default.map: " + err.Error())
	}
	return m
}

const (
	walkable = ".,CS"
	blocking = "#DPT"
)

// ParseMap reads rows of tiles: # wall, . floor, , carpet, D desk, C chair,
// P plant, T table, S spawn. The first S is the spawn point.
func ParseMap(text string) (Map, error) {
	text = strings.TrimRight(strings.ReplaceAll(text, "\r\n", "\n"), "\n")
	m := Map{Rows: strings.Split(text, "\n")}
	spawned := false
	for y, row := range m.Rows {
		for x, c := range []rune(row) {
			switch {
			case strings.ContainsRune(walkable+blocking, c):
			default:
				return Map{}, fmt.Errorf("unknown tile '%c' at %d,%d", c, x, y)
			}
			if c == 'S' && !spawned {
				m.Spawn, spawned = Point{x, y}, true
			}
			if x+1 > m.W {
				m.W = x + 1
			}
		}
	}
	m.H = len(m.Rows)
	if !spawned {
		return Map{}, errors.New("map has no spawn point")
	}
	return m, nil
}

// Blocked reports whether p is a wall, furniture or outside the map.
func (m Map) Blocked(p Point) bool {
	if p.Y < 0 || p.Y >= len(m.Rows) || p.X < 0 {
		return true
	}
	row := []rune(m.Rows[p.Y])
	if p.X >= len(row) {
		return true
	}
	return strings.ContainsRune(blocking, row[p.X])
}
