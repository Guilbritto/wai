package main

import (
	"errors"
	"os"
	"reflect"
	"strings"
	"testing"
	"time"
)

var (
	loc = time.FixedZone("BRT", -3*3600)
	now = time.Date(2026, 11, 13, 14, 5, 0, 0, loc)
)

const zone = "America/Sao_Paulo"

func at(h, m int) time.Time    { return time.Date(2026, 11, 13, h, m, 0, 0, loc) }
func tomorrow(h int) time.Time { return time.Date(2026, 11, 14, h, 0, 0, 0, loc) }

func mustParse(t *testing.T, body string) []window {
	t.Helper()
	ws, err := parseUsage([]byte(body))
	if err != nil {
		t.Fatal(err)
	}
	return ws
}

func children(t *testing.T, n node) []node {
	t.Helper()
	c, ok := n["card"].(node)
	if !ok || c["type"] != "column" {
		t.Fatalf("card is not a column: %v", n["card"])
	}
	return c["children"].([]node)
}

func TestParseUsage(t *testing.T) {
	body, err := os.ReadFile("testdata/usage.json")
	if err != nil {
		t.Fatal(err)
	}
	ws, err := parseUsage(body)
	if err != nil {
		t.Fatal(err)
	}
	want := []window{
		{"Current session", 32, time.Date(2026, 11, 14, 3, 19, 59, 652662000, time.UTC)},
		{"Current week (all models)", 61, time.Date(2026, 11, 14, 1, 59, 59, 652680000, time.UTC)},
		{"Current week (Fable)", 0, time.Date(2026, 11, 14, 2, 0, 0, 0, time.UTC)},
	}
	if len(ws) != len(want) {
		t.Fatalf("got %v", ws)
	}
	for i := range want {
		if ws[i].Label != want[i].Label || ws[i].Used != want[i].Used || !ws[i].Resets.Equal(want[i].Resets) {
			t.Errorf("window %d: got %v want %v", i, ws[i], want[i])
		}
	}
	for _, bad := range []string{"[]", "null", "nope", `"x"`} {
		if _, err := parseUsage([]byte(bad)); !errors.Is(err, errUnreadable) {
			t.Errorf("%s: err = %v", bad, err)
		}
	}
}

func TestParseLimitsFilters(t *testing.T) {
	ws := mustParse(t, `{"five_hour":{"utilization":1},"limits":[
	 {"kind":"weekly_scoped","percent":5,"resets_at":null,"scope":{"model":{"display_name":"A"},"surface":null}},
	 {"kind":"weekly_scoped","percent":6,"scope":{"model":{"display_name":"B"},"surface":"cli"}},
	 {"kind":"weekly_scoped","percent":7,"scope":{"model":{"display_name":""},"surface":null}},
	 {"kind":"weekly_scoped","percent":8,"scope":null},
	 {"kind":"other","percent":9},
	 {"kind":"weekly_all","percent":null},
	 {"kind":"weekly_all","percent":50},
	 {"kind":"session","percent":10}]}`)
	var got []string
	for _, w := range ws {
		got = append(got, w.Label)
	}
	want := []string{"Current session", "Current week (all models)", "Current week (A)"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("got %v", got)
	}
	if !ws[2].Resets.IsZero() {
		t.Error("null resets_at should be zero")
	}
}

func TestParseFallback(t *testing.T) {
	ws := mustParse(t, `{"five_hour":{"utilization":10,"resets_at":"2026-11-13T21:00:00Z"},
	 "seven_day":{"utilization":20},"seven_day_opus":{"utilization":30},
	 "seven_day_sonnet":null,"seven_day_oauth_apps":{"utilization":1},
	 "seven_day_cowork":{"utilization":1},"seven_day_omelette":{"utilization":1},
	 "seven_day_overage_x":{"utilization":1},"seven_day_haiku":{"utilization":null}}`)
	var got []string
	for _, w := range ws {
		got = append(got, w.Label)
	}
	want := []string{"Current session", "Current week (all models)", "Current week (Opus)"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("got %v", got)
	}
	if ws[0].Used != 10 || ws[0].Resets.IsZero() {
		t.Errorf("session = %v", ws[0])
	}
}

func TestOddNumbers(t *testing.T) {
	ws := mustParse(t, `{"five_hour":{"utilization":104,"resets_at":"soon"},"seven_day":{"utilization":-3,"resets_at":null},"seven_day_opus":{"utilization":null}}`)
	if len(ws) != 2 {
		t.Fatalf("got %v", ws)
	}
	if !ws[0].Resets.IsZero() || !ws[1].Resets.IsZero() {
		t.Error("resets should be zero")
	}
	n := view{windows: ws, updated: now}.update(now, loc, zone)
	if n["text"] != "104% · -3%" || n["tone"] != "danger" {
		t.Errorf("text %q tone %q", n["text"], n["tone"])
	}
	for _, c := range children(t, n) {
		if s, _ := c["text"].(string); strings.HasPrefix(s, "Resets") {
			t.Errorf("unexpected %q", s)
		}
	}
}

func TestItemText(t *testing.T) {
	s := window{"Current session", 16, at(0, 20)}
	w := window{"Current week (all models)", 59, at(23, 0)}
	o := window{"Current week (Opus)", 0, time.Time{}}
	n := view{windows: []window{s, w, o}, updated: now}.update(now, loc, zone)
	if n["item"] != "usage" || n["text"] != "16% · 59%" || n["icon"] != "gauge" || n["tone"] != "neutral" ||
		n["tooltip"] != "Session 16% · Week 59% · resets 00:20" {
		t.Errorf("got %v", n)
	}
	n = view{windows: []window{w}, updated: now}.update(now, loc, zone)
	if n["text"] != "59%" || n["tooltip"] != "Week 59%" {
		t.Errorf("got %v", n)
	}
	o.Used = 85
	if n = (view{windows: []window{s, w, o}, updated: now}).update(now, loc, zone); n["tone"] != "warning" {
		t.Errorf("tone %v", n["tone"])
	}
	o.Used = 100
	if n = (view{windows: []window{s, w, o}, updated: now}).update(now, loc, zone); n["tone"] != "danger" {
		t.Errorf("tone %v", n["tone"])
	}
}

func TestCard(t *testing.T) {
	s := window{"Current session", 16, time.Date(2026, 11, 13, 21, 19, 59, 650000000, time.UTC)} // 18:20 BRT after rounding
	w := window{"Current week (all models)", 59, tomorrow(23)}
	n := view{windows: []window{s, w}, updated: now}.update(now, loc, zone)
	want := []node{
		{"type": "row", "children": []node{txt("Current session", false), {"type": "spacer"}, txt("16% used", false)}},
		{"type": "progress", "value": 0.16},
		txt("Resets 18:20 (America/Sao_Paulo)", true),
		{"type": "row", "children": []node{txt("Current week (all models)", false), {"type": "spacer"}, txt("59% used", false)}},
		{"type": "progress", "value": 0.59},
		txt("Resets Nov 14 at 23:00 (America/Sao_Paulo)", true),
		txt("Updated 14:05", true),
	}
	if got := children(t, n); !reflect.DeepEqual(got, want) {
		t.Errorf("got  %v\nwant %v", got, want)
	}
	got := children(t, view{windows: []window{s}, updated: now}.update(now, loc, ""))
	if got[2]["text"] != "Resets 18:20" {
		t.Errorf("got %v", got[2])
	}
	if n["tooltip"] != "Session 16% · Week 59% · resets 18:20" {
		t.Errorf("tooltip %v", n["tooltip"])
	}
}

func TestCardClampsProgress(t *testing.T) {
	n := view{windows: []window{{"Current session", 104, time.Time{}}, {"Current week (all models)", -3, time.Time{}}}, updated: now}.update(now, loc, zone)
	c := children(t, n)
	if c[1]["value"] != 1.0 || c[3]["value"] != 0.0 {
		t.Errorf("got %v %v", c[1], c[3])
	}
}

func TestStates(t *testing.T) {
	old := []window{{"Current session", 40, time.Time{}}}
	cases := []struct {
		name       string
		v          view
		text, tone string
		last       string
	}{
		{"loading", view{}, "Usage …", "neutral", "Loading usage…"},
		{"nologin", view{err: errNoLogin, windows: old}, "Usage", "neutral", "Sign in to Claude Code to see your usage."},
		{"expired", view{err: errExpired, windows: old}, "Usage", "warning", "Your Claude login expired. Use Claude Code once to refresh it."},
		{"stale", view{windows: old, updated: now, err: errors.New("timeout"), failed: at(14, 10)}, "40%", "neutral", "Couldn't update at 14:10: timeout"},
		{"empty failure", view{err: errors.New("timeout\nsecond line")}, "Usage", "warning", "Couldn't load usage: timeout"},
		{"unreadable", view{err: errUnreadable, windows: old}, "Usage ?", "neutral", "Unexpected response from Claude; the extension may need an update."},
	}
	for _, c := range cases {
		n := c.v.update(now, loc, zone)
		if n["text"] != c.text || n["tone"] != c.tone || n["icon"] != "gauge" {
			t.Errorf("%s: got %v", c.name, n)
		}
		ch := children(t, n)
		if got := ch[len(ch)-1]["text"]; got != c.last {
			t.Errorf("%s: last = %v", c.name, got)
		}
	}
	long := view{err: errors.New(strings.Repeat("x", 300))}.update(now, loc, zone)
	ch := children(t, long)
	if got := ch[0]["text"].(string); len(got) != len("Couldn't load usage: ")+120 {
		t.Errorf("reason not cut: %d", len(got))
	}
}
