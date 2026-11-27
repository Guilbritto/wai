package office

import (
	"strings"
	"testing"
)

func TestStepKeys(t *testing.T) {
	p := Point{5, 5}
	cases := map[string]Point{
		"up": {5, 4}, "w": {5, 4},
		"down": {5, 6}, "s": {5, 6},
		"left": {4, 5}, "a": {4, 5},
		"right": {6, 5}, "d": {6, 5},
	}
	for k, want := range cases {
		got, ok := Step(p, k)
		if !ok || got != want {
			t.Errorf("Step(%q) = %v, %v; want %v, true", k, got, ok, want)
		}
	}
	for _, k := range []string{"space", "enter", "q", "W", "", "escape"} {
		if got, ok := Step(p, k); ok || got != p {
			t.Errorf("Step(%q) = %v, %v; want unchanged, false", k, got, ok)
		}
	}
}

func TestAdjacent(t *testing.T) {
	a := Point{3, 3}
	for _, b := range []Point{{3, 2}, {3, 4}, {2, 3}, {4, 3}} {
		if !Adjacent(a, b) {
			t.Errorf("%v should be adjacent", b)
		}
	}
	for _, b := range []Point{{3, 3}, {4, 4}, {2, 2}, {3, 5}, {5, 3}} {
		if Adjacent(a, b) {
			t.Errorf("%v should not be adjacent", b)
		}
	}
}

func TestNearbyChebyshev(t *testing.T) {
	a := Point{10, 10}
	for _, b := range []Point{{10, 10}, {13, 10}, {7, 13}, {13, 13}, {8, 12}} {
		if !Nearby(a, b) {
			t.Errorf("%v should be nearby", b)
		}
	}
	for _, b := range []Point{{14, 10}, {10, 6}, {14, 14}, {6, 10}} {
		if Nearby(a, b) {
			t.Errorf("%v should not be nearby", b)
		}
	}
}

func TestDeriveFocused(t *testing.T) {
	got := Derive(map[string]string{"a": "working", "b": "idle", "c": "working"})
	if got != StatusFocused {
		t.Fatalf("got %q", got)
	}
}

func TestDeriveNeedsYouWins(t *testing.T) {
	got := Derive(map[string]string{"a": "working", "b": "needs_you", "c": "working"})
	if got != StatusNeedsYou {
		t.Fatalf("got %q", got)
	}
}

func TestDeriveNone(t *testing.T) {
	for _, in := range []map[string]string{nil, {}, {"a": "idle", "b": "done"}} {
		if got := Derive(in); got != StatusNone {
			t.Fatalf("Derive(%v) = %q", in, got)
		}
	}
}

func TestValidName(t *testing.T) {
	good := []string{"Ana", "a", strings.Repeat("x", 32), strings.Repeat("é", 32), "Ana Maria", "日本語"}
	for _, s := range good {
		if !ValidName(s) {
			t.Errorf("%q should be valid", s)
		}
	}
	bad := []string{"", strings.Repeat("x", 33), strings.Repeat("é", 33)}
	for _, s := range bad {
		if ValidName(s) {
			t.Errorf("%q should be invalid", s)
		}
	}
}

func TestNameRejectsControlChars(t *testing.T) {
	for _, s := range []string{"Ana\n", "A\tna", "\x00", "Ana\r\nBob", "a\x1bb", "a\u0085b", "a\x7fb", "a\u2028b", "a\u200bb", "a\u202eb"} {
		if ValidName(s) {
			t.Errorf("%q should be invalid", s)
		}
	}
}
