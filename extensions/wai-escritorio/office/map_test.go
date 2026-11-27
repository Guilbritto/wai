package office

import (
	"strings"
	"testing"
)

func TestDefaultMapParses(t *testing.T) {
	m, err := ParseMap(defaultMap)
	if err != nil {
		t.Fatal(err)
	}
	if m.W < 36 || m.W > 44 || m.H < 26 || m.H > 34 {
		t.Fatalf("size %dx%d, want about 40x30", m.W, m.H)
	}
	if m.Blocked(m.Spawn) {
		t.Fatalf("spawn %v is blocked", m.Spawn)
	}
	for _, c := range "#.,DCPTS" {
		if !strings.ContainsRune(defaultMap, c) {
			t.Errorf("default map has no %q tile", c)
		}
	}
	if DefaultMap().Spawn != m.Spawn {
		t.Fatal("DefaultMap differs from ParseMap")
	}
}

func TestBlocked(t *testing.T) {
	m, err := ParseMap("#####\n#.D.#\n#PTS#\n#,C.#\n#####")
	if err != nil {
		t.Fatal(err)
	}
	cases := []struct {
		name string
		p    Point
		want bool
	}{
		{"wall", Point{0, 0}, true},
		{"desk", Point{2, 1}, true},
		{"plant", Point{1, 2}, true},
		{"table", Point{2, 2}, true},
		{"floor", Point{1, 1}, false},
		{"carpet", Point{1, 3}, false},
		{"chair", Point{2, 3}, false},
		{"spawn", Point{3, 2}, false},
		{"outside left", Point{-1, 1}, true},
		{"outside right", Point{5, 1}, true},
		{"outside top", Point{1, -1}, true},
		{"outside bottom", Point{1, 5}, true},
	}
	for _, c := range cases {
		if got := m.Blocked(c.p); got != c.want {
			t.Errorf("%s %v: blocked=%v, want %v", c.name, c.p, got, c.want)
		}
	}
}

func TestMapWithoutSpawnRefused(t *testing.T) {
	_, err := ParseMap("###\n#.#\n###")
	if err == nil || err.Error() != "map has no spawn point" {
		t.Fatalf("err = %v", err)
	}
}

func TestUnknownTileRefused(t *testing.T) {
	_, err := ParseMap("####\n#S.#\n#.x#\n####")
	if err == nil || err.Error() != "unknown tile 'x' at 2,2" {
		t.Fatalf("err = %v", err)
	}
}
