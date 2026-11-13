package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"sort"
	"strconv"
	"strings"
	"time"
)

type node = map[string]any

type window struct {
	Label  string    // "Current session", "Current week (all models)", "Current week (Opus)"
	Used   float64   // percent, as the API sends it
	Resets time.Time // zero when unknown
}

var (
	errNoLogin    = errors.New("no Claude login")
	errExpired    = errors.New("Claude login expired")
	errUnreadable = errors.New("unexpected response")
)

const (
	labelSession = "Current session"
	labelWeek    = "Current week (all models)"
)

// view is what the item shows: the last good windows, when they were fetched, and the last fetch's error (nil if it worked).
type view struct {
	windows []window
	updated time.Time
	failed  time.Time // when err happened
	err     error
}

type rawWindow struct {
	Percent     *float64 `json:"percent"`
	Utilization *float64 `json:"utilization"`
	ResetsAt    *string  `json:"resets_at"`
	Kind        string   `json:"kind"`
	Scope       *struct {
		Model *struct {
			DisplayName string `json:"display_name"`
		} `json:"model"`
		Surface any `json:"surface"`
	} `json:"scope"`
}

func (r rawWindow) window(label string) (window, bool) {
	used := r.Percent
	if used == nil {
		used = r.Utilization
	}
	if used == nil {
		return window{}, false
	}
	w := window{Label: label, Used: *used}
	if r.ResetsAt != nil {
		if t, err := time.Parse(time.RFC3339Nano, *r.ResetsAt); err == nil {
			w.Resets = t
		}
	}
	return w, true
}

func parseUsage(body []byte) ([]window, error) {
	var top map[string]json.RawMessage
	if json.Unmarshal(body, &top) != nil || top == nil {
		return nil, errUnreadable
	}
	var ws []window
	add := func(raw json.RawMessage, label string) {
		var r rawWindow
		if json.Unmarshal(raw, &r) != nil {
			return
		}
		if w, ok := r.window(label); ok {
			ws = append(ws, w)
		}
	}
	if raw, ok := top["limits"]; ok && string(raw) != "null" {
		var limits []json.RawMessage
		if json.Unmarshal(raw, &limits) != nil {
			return nil, errUnreadable
		}
		var session, week, scoped []window
		for _, l := range limits {
			var r rawWindow
			if json.Unmarshal(l, &r) != nil {
				continue
			}
			label := ""
			switch {
			case r.Kind == "session":
				label = labelSession
			case r.Kind == "weekly_all":
				label = labelWeek
			case r.Kind == "weekly_scoped" && r.Scope != nil && r.Scope.Surface == nil &&
				r.Scope.Model != nil && r.Scope.Model.DisplayName != "":
				label = "Current week (" + r.Scope.Model.DisplayName + ")"
			default:
				continue
			}
			w, ok := r.window(label)
			if !ok {
				continue
			}
			switch r.Kind {
			case "session":
				session = append(session, w)
			case "weekly_all":
				week = append(week, w)
			default:
				scoped = append(scoped, w)
			}
		}
		// ponytail: a repeated session/weekly_all entry is kept, not deduplicated
		ws = append(append(session, week...), scoped...)
		return ws, nil
	}
	if raw, ok := top["five_hour"]; ok {
		add(raw, labelSession)
	}
	if raw, ok := top["seven_day"]; ok {
		add(raw, labelWeek)
	}
	var keys []string
	for k := range top {
		name, ok := strings.CutPrefix(k, "seven_day_")
		if !ok || name == "oauth_apps" || name == "cowork" || name == "omelette" || strings.HasPrefix(name, "overage") {
			continue
		}
		keys = append(keys, k)
	}
	sort.Strings(keys)
	for _, k := range keys {
		name := strings.TrimPrefix(k, "seven_day_")
		add(top[k], "Current week ("+strings.ToUpper(name[:1])+name[1:]+")")
	}
	return ws, nil
}

func pct(f float64) string { return strconv.FormatFloat(f, 'f', 0, 64) + "%" }

// stamp formats t in loc: 15:04 on now's date, Jan 2 at 15:04 otherwise (rounded to the minute).
func stamp(t, now time.Time, loc *time.Location) string {
	t = t.Round(time.Minute).In(loc)
	y, m, d := now.In(loc).Date()
	if ty, tm, td := t.Date(); ty == y && tm == m && td == d {
		return t.Format("15:04")
	}
	return t.Format("Jan 2 at 15:04")
}

func reason(err error) string {
	s, _, _ := strings.Cut(strings.TrimSpace(err.Error()), "\n")
	if r := []rune(s); len(r) > 120 {
		s = string(r[:120])
	}
	return s
}

func item(text, tone, tooltip string, children ...node) node {
	return node{
		"item": "usage", "text": text, "icon": "gauge", "tooltip": tooltip, "tone": tone,
		"card": node{"type": "column", "children": children},
	}
}

func state(text, tone, msg string) node { return item(text, tone, msg, txt(msg, false)) }

func txt(s string, muted bool) node {
	n := node{"type": "text", "text": s}
	if muted {
		n["muted"] = true
	}
	return n
}

func (v view) update(now time.Time, loc *time.Location, zone string) node {
	switch {
	case errors.Is(v.err, errNoLogin):
		return state("Usage", "neutral", "Sign in to Claude Code to see your usage.")
	case errors.Is(v.err, errExpired):
		return state("Usage", "warning", "Your Claude login expired. Use Claude Code once to refresh it.")
	case errors.Is(v.err, errUnreadable):
		return state("Usage ?", "neutral", "Unexpected response from Claude; the extension may need an update.")
	case len(v.windows) == 0 && v.err != nil:
		return state("Usage", "warning", "Couldn't load usage: "+reason(v.err))
	case len(v.windows) == 0 && v.updated.IsZero():
		return state("Usage …", "neutral", "Loading usage…")
	case len(v.windows) == 0:
		return state("Usage", "neutral", "No usage limits reported.")
	}

	suffix := ""
	if zone != "" {
		suffix = " (" + zone + ")"
	}
	var card []node
	var texts, tips []string
	var sessionReset time.Time
	peak := v.windows[0].Used
	for _, w := range v.windows {
		peak = max(peak, w.Used)
		card = append(card,
			node{"type": "row", "children": []node{txt(w.Label, false), {"type": "spacer"}, txt(pct(w.Used)+" used", false)}},
			node{"type": "progress", "value": min(max(w.Used/100, 0), 1)})
		if !w.Resets.IsZero() {
			card = append(card, txt("Resets "+stamp(w.Resets, now, loc)+suffix, true))
		}
		switch w.Label {
		case labelSession:
			texts, tips, sessionReset = append(texts, pct(w.Used)), append(tips, "Session "+pct(w.Used)), w.Resets
		case labelWeek:
			texts, tips = append(texts, pct(w.Used)), append(tips, "Week "+pct(w.Used))
		}
	}
	if len(texts) == 0 { // only per-model weeks
		texts, tips = []string{pct(v.windows[0].Used)}, []string{v.windows[0].Label + " " + pct(v.windows[0].Used)}
	}
	tooltip := strings.Join(tips, " · ")
	if !sessionReset.IsZero() {
		tooltip += " · resets " + stamp(sessionReset, now, loc)
	}
	tone := "neutral"
	switch {
	case peak >= 100:
		tone = "danger"
	case peak >= 80:
		tone = "warning"
	}
	if !v.updated.IsZero() {
		card = append(card, txt("Updated "+stamp(v.updated, now, loc), true))
	}
	if v.err != nil {
		card = append(card, txt(fmt.Sprintf("Couldn't update at %s: %s", stamp(v.failed, now, loc), reason(v.err)), true))
	}
	return item(strings.Join(texts, " · "), tone, tooltip, card...)
}

func zoneName() string {
	if tz := strings.TrimPrefix(os.Getenv("TZ"), ":"); tz != "" {
		return tz
	}
	link, err := os.Readlink("/etc/localtime")
	if err != nil {
		return ""
	}
	_, name, ok := strings.Cut(link, "zoneinfo/")
	if !ok {
		return ""
	}
	return name
}
