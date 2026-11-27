package main

import (
	"strings"
	"testing"
)

// The longest tunnel URL expected: a 63-character machine name (a DNS label's limit) on a
// tailnet, with a port.
var longestTunnel = "https://" + strings.Repeat("m", 63) + ".tail0123ab.ts.net:8443"

const code22 = "AAAAAAAAAAAAAAAAAAAAAA"

// PAIR-08: the QR code fits the pixels node: at most 64 rows of at most 256 characters, square,
// '#' and ' ' only, with a light quiet zone of 4 modules.
func TestQRRowsFitPixelsNode(t *testing.T) {
	payload := pairPayload(longestTunnel, code22)
	want := "wai-remoto:pair?url=https%3A%2F%2F" + strings.Repeat("m", 63) + ".tail0123ab.ts.net%3A8443&code=" + code22
	if payload != want {
		t.Fatalf("payload %q, want %q", payload, want)
	}
	rows, err := qrRows(payload)
	if err != nil {
		t.Fatal(err)
	}
	if len(rows) > 64 || len(rows) < 21+8 {
		t.Fatalf("%d rows", len(rows))
	}
	for y, r := range rows {
		if len(r) != len(rows) || len(r) > 256 || strings.Trim(r, "# ") != "" {
			t.Fatalf("row %d is %q", y, r)
		}
		edge := y < 4 || y >= len(rows)-4
		if (edge && strings.Contains(r, "#")) || strings.Contains(r[:4]+r[len(r)-4:], "#") {
			t.Fatalf("row %d breaks the quiet zone: %q", y, r)
		}
	}

	// Too long for 64 rows: an error, not a code cut by the node.
	if rows, err := qrRows(pairPayload("https://"+strings.Repeat("x", 300)+".ts.net", code22)); err == nil {
		t.Errorf("a 300-character URL drew %d rows", len(rows))
	}
}

// PAIR-08: the rows carry the three finder patterns, top left, top right and bottom left only,
// and a typical tunnel URL fits version 6 (49 rows with the quiet zone), as the design says.
func TestQRRowsHaveFinderPatterns(t *testing.T) {
	rows, err := qrRows(pairPayload("https://my-mac.tail0123ab.ts.net", code22))
	if err != nil {
		t.Fatal(err)
	}
	if len(rows) > 49 {
		t.Errorf("%d rows for a typical URL, want at most 49", len(rows))
	}
	size := len(rows) - 8
	// finder: a 7×7 dark ring, a light ring, a dark 3×3 centre, and a light separator around it.
	finder := func(x0, y0 int) bool {
		for y := -1; y <= 7; y++ {
			for x := -1; x <= 7; x++ {
				ring := max(abs(x-3), abs(y-3))
				dark := ring <= 1 || ring == 3
				if ring == 4 {
					dark = false
				}
				if (rows[4+y0+y][4+x0+x] == '#') != dark {
					return false
				}
			}
		}
		return true
	}
	if !finder(0, 0) || !finder(size-7, 0) || !finder(0, size-7) {
		t.Errorf("missing a finder pattern:\n%s", strings.Join(rows, "\n"))
	}
	if finder(size-7, size-7) {
		t.Error("a finder pattern at the bottom right")
	}
}

func abs(n int) int { return max(n, -n) }
