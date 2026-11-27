package main

import (
	"strings"
	"testing"

	"wai-escritorio/office"
)

var tileNames = []string{"floor", "carpet", "wall", "desk", "chair", "plant", "table", "chair_carpet", "plant_carpet"}

func TestArtValid(t *testing.T) {
	for name, rows := range artPatterns {
		if len(rows) != 16 {
			t.Errorf("%s: %d rows, want 16", name, len(rows))
			continue
		}
		for y, row := range rows {
			if n := len([]rune(row)); n != 16 {
				t.Errorf("%s row %d: %d columns, want 16", name, y, n)
			}
			for _, r := range row {
				if _, ok := artPalette[string(r)]; !ok && r != ' ' {
					t.Errorf("%s row %d: %q is not a palette key", name, y, r)
				}
			}
		}
	}
	for k, v := range artPalette {
		if len(k) != 1 || len(v) != 7 || v[0] != '#' {
			t.Errorf("palette %q: %q", k, v)
		}
	}
	if len(artPalette) > 64 || len(artPatterns) > 64 {
		t.Errorf("limits: %d palette keys, %d patterns", len(artPalette), len(artPatterns))
	}

	// Every character of the default map has a tile pattern.
	for _, row := range office.DefaultMap().Rows {
		for _, r := range row {
			if _, ok := artPatterns[artLegend[string(r)]]; !ok {
				t.Errorf("map character %q has no pattern", r)
			}
		}
	}

	// Tiles are fully opaque: nothing shows the panel through the map.
	for _, name := range tileNames {
		if _, ok := artPatterns[name]; !ok {
			t.Errorf("missing tile %s", name)
		}
		if strings.Contains(strings.Join(artPatterns[name], ""), " ") {
			t.Errorf("tile %s has a transparent pixel", name)
		}
	}

	// Eight avatars, transparent around the figure, and all different.
	seen := map[string]int{}
	for i := 0; i < office.Avatars; i++ {
		rows, ok := artPatterns[avatarPattern(i)]
		if !ok {
			t.Fatalf("avatar %d missing", i)
		}
		all := strings.Join(rows, "\n")
		if !strings.Contains(all, " ") {
			t.Errorf("avatar %d has no transparency", i)
		}
		if j, dup := seen[all]; dup {
			t.Errorf("avatars %d and %d look the same", j, i)
		}
		seen[all] = i
		if avatarNames[i] == "" {
			t.Errorf("avatar %d has no name", i)
		}
	}

	// Furniture on carpet stands on carpet colours, not wood.
	for _, name := range []string{"chair_carpet", "plant_carpet"} {
		for _, y := range []int{0, 15} {
			for _, x := range []int{0, 15} {
				if !strings.ContainsRune("def", rune(artPatterns[name][y][x])) {
					t.Errorf("%s corner %d,%d is not carpet", name, x, y)
				}
			}
		}
	}
}
