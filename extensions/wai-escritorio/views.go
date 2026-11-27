package main

import (
	"sort"
	"strings"

	"wai-escritorio/office"
)

type node = map[string]any

// view is everything the panels draw: the saved setup, the form state and the office as the
// server last told it.
type view struct {
	Setup   *Setup // saved setup; nil when there is none
	Editing bool   // the form is shown although a setup is saved
	Form    Setup  // what the form shows; its token is never drawn
	FormErr string
	Online  bool
	Reason  string // why it is offline; "" while connecting
	Rows    []string
	You     string
	People  map[string]office.Person
}

const (
	reasonToken     = "The server refused the team token"
	reasonElsewhere = "Signed in elsewhere"
	reasonFull      = "The office is full"
)

func reasonUnreachable(addr string) string { return "Cannot reach " + addr }

func (v view) showForm() bool { return v.Setup == nil || v.Editing }

func statusLabel(s office.Status) string {
	switch s {
	case office.StatusFocused:
		return "Focused"
	case office.StatusNeedsYou:
		return "Needs you"
	}
	return ""
}

func statusTone(s office.Status) string {
	if s == office.StatusNeedsYou {
		return "warning"
	}
	return "info"
}

// setupTree is the form. The saved token is never drawn: an empty one keeps it.
func setupTree(form Setup, errMsg string) node {
	avatar := form.Avatar
	if avatar < 0 || avatar >= office.Avatars {
		avatar = 0
	}
	kids := []any{
		node{"type": "text", "text": "Join the office", "style": "heading"},
	}
	if errMsg != "" {
		kids = append(kids, node{"type": "alert", "tone": "danger", "text": errMsg})
	}
	kids = append(kids,
		node{"type": "input", "id": "name", "placeholder": "Your name", "value": form.Name},
		node{"type": "input", "id": "server", "placeholder": "Server address (wss://…)", "value": form.Server},
		node{"type": "input", "id": "token", "placeholder": "Team token"},
		node{"type": "select", "id": "avatar", "options": avatarNames[:], "value": avatarNames[avatar]},
		node{"type": "button", "id": "save", "label": "Save and connect", "style": "primary"},
	)
	return node{"type": "column", "children": kids}
}

func (v view) people() []office.Person {
	ps := make([]office.Person, 0, len(v.People))
	for _, p := range v.People {
		ps = append(ps, p)
	}
	sort.Slice(ps, func(i, j int) bool {
		a, b := strings.ToLower(ps[i].Name), strings.ToLower(ps[j].Name)
		if a != b {
			return a < b
		}
		return ps[i].ID < ps[j].ID
	})
	return ps
}

func (v view) offline() []any {
	text := "Connecting…"
	if v.Reason != "" {
		return []any{
			node{"type": "row", "children": []any{
				node{"type": "badge", "text": "Offline", "tone": "danger"},
			}},
			node{"type": "text", "text": v.Reason, "muted": true},
			node{"type": "button", "id": "change", "label": "Change setup"},
		}
	}
	return []any{
		node{"type": "text", "text": text, "muted": true},
		node{"type": "button", "id": "change", "label": "Change setup"},
	}
}

// presenceTree is the sidebar panel.
func presenceTree(v view) node {
	if v.showForm() {
		return setupTree(v.Form, v.FormErr)
	}
	if !v.Online {
		return node{"type": "column", "children": v.offline()}
	}
	me := v.People[v.You]
	items := []any{}
	for _, p := range v.people() {
		it := node{"id": "person:" + p.ID, "title": p.Name}
		if s := statusLabel(p.Status); s != "" {
			it["subtitle"] = s
			it["tone"] = statusTone(p.Status)
		}
		if p.ID == v.You {
			it["note"] = "you"
		} else if office.Nearby(office.Point{X: me.X, Y: me.Y}, office.Point{X: p.X, Y: p.Y}) {
			it["badge"] = "Nearby"
			it["tone"] = "info"
		}
		items = append(items, it)
	}
	kids := []any{}
	if len(v.People) <= 1 {
		kids = append(kids, node{"type": "text", "text": "Nobody else is in the office", "muted": true})
	}
	kids = append(kids,
		node{"type": "list", "items": items, "plain": true},
		node{"type": "row", "children": []any{
			node{"type": "button", "id": "enter", "label": "Enter office", "style": "primary"},
			node{"type": "button", "id": "change", "label": "Change setup"},
		}},
	)
	return node{"type": "column", "children": kids}
}

// officeTree is the main pane: the map with everyone on it.
func officeTree(v view) node {
	if v.showForm() {
		return setupTree(v.Form, v.FormErr)
	}
	if !v.Online {
		return node{"type": "column", "children": v.offline()}
	}
	sprites := []any{}
	for _, p := range v.people() {
		sp := node{"id": p.ID, "x": p.X, "y": p.Y, "pattern": avatarPattern(p.Avatar), "label": p.Name}
		if s := statusLabel(p.Status); s != "" {
			sp["badge"] = s
			sp["tone"] = statusTone(p.Status)
		}
		sprites = append(sprites, sp)
	}
	scene := node{
		"type": "scene", "id": "office", "cell": cell, "scale": 2,
		"palette": artPalette, "patterns": artPatterns, "legend": artLegend,
		"map": v.Rows, "sprites": sprites,
	}
	if _, ok := v.People[v.You]; ok {
		scene["follow"] = v.You
	}
	return node{"type": "column", "children": []any{scene}}
}
