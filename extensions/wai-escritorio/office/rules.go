package office

import (
	"unicode"
	"unicode/utf8"
)

// Status is what an avatar shows about its owner's agents.
type Status string

const (
	StatusNone     Status = ""
	StatusFocused  Status = "focused"
	StatusNeedsYou Status = "needs_you"
)

// NearbyRadius is the Chebyshev distance that counts as nearby.
const NearbyRadius = 3

// Step returns the tile one step from p for an arrow key or W, A, S, D.
func Step(p Point, key string) (Point, bool) {
	switch key {
	case "up", "w":
		return Point{p.X, p.Y - 1}, true
	case "down", "s":
		return Point{p.X, p.Y + 1}, true
	case "left", "a":
		return Point{p.X - 1, p.Y}, true
	case "right", "d":
		return Point{p.X + 1, p.Y}, true
	}
	return p, false
}

// Adjacent reports whether b is exactly one step from a, no diagonals.
func Adjacent(a, b Point) bool {
	return abs(a.X-b.X)+abs(a.Y-b.Y) == 1
}

// Nearby reports whether b is within NearbyRadius tiles of a (Chebyshev).
func Nearby(a, b Point) bool {
	return max(abs(a.X-b.X), abs(a.Y-b.Y)) <= NearbyRadius
}

// Derive turns session states (session id to wai state) into one status:
// needs_you beats working; anything else shows nothing.
func Derive(states map[string]string) Status {
	s := StatusNone
	for _, st := range states {
		switch st {
		case "needs_you":
			return StatusNeedsYou
		case "working":
			s = StatusFocused
		}
	}
	return s
}

// ValidName reports whether s is 1 to 32 characters with no control characters.
func ValidName(s string) bool {
	n := utf8.RuneCountInString(s)
	if n < 1 || n > 32 || !utf8.ValidString(s) {
		return false
	}
	for _, r := range s {
		if unicode.IsControl(r) {
			return false
		}
	}
	return true
}

func abs(n int) int {
	if n < 0 {
		return -n
	}
	return n
}
