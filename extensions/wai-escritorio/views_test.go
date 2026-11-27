package main

import (
	"encoding/json"
	"strings"
	"testing"

	"wai-escritorio/office"
)

// walk calls f on every node of a tree (maps in "children").
func walk(n any, f func(node)) {
	m, ok := n.(node)
	if !ok {
		return
	}
	f(m)
	if kids, ok := m["children"].([]any); ok {
		for _, k := range kids {
			walk(k, f)
		}
	}
}

func find(n any, pred func(node) bool) node {
	var got node
	walk(n, func(m node) {
		if got == nil && pred(m) {
			got = m
		}
	})
	return got
}

func ofType(n any, typ string) []node {
	var out []node
	walk(n, func(m node) {
		if m["type"] == typ {
			out = append(out, m)
		}
	})
	return out
}

func js(t *testing.T, v any) string {
	t.Helper()
	b, err := json.Marshal(v)
	if err != nil {
		t.Fatal(err)
	}
	return string(b)
}

func onlineView() view {
	m := office.DefaultMap()
	return view{
		Setup:  &Setup{Name: "Ana", Server: "wss://x.example", Token: strings.Repeat("s", 32)},
		Online: true, Rows: m.Rows, You: "u1",
		People: map[string]office.Person{
			"u1": {ID: "u1", Name: "Ana", Avatar: 0, X: 10, Y: 16},
			"u2": {ID: "u2", Name: "bruno", Avatar: 1, X: 13, Y: 19},
			"u3": {ID: "u3", Name: "Carla", Avatar: 2, X: 14, Y: 16},
		},
	}
}

func TestNoSetupShowsForm(t *testing.T) {
	tree := officeTree(view{})
	for _, id := range []string{"name", "server", "token", "avatar", "save"} {
		if find(tree, func(m node) bool { return m["id"] == id }) == nil {
			t.Errorf("form has no %q", id)
		}
	}
	sel := find(tree, func(m node) bool { return m["type"] == "select" })
	if opts, _ := sel["options"].([]string); len(opts) != 8 {
		t.Errorf("avatar options: %v", sel["options"])
	}
	if len(ofType(tree, "scene")) != 0 {
		t.Error("no scene before setup")
	}
}

func TestOfficeTreeShowsEveryone(t *testing.T) {
	v := onlineView()
	scenes := ofType(officeTree(v), "scene")
	if len(scenes) != 1 {
		t.Fatalf("scenes: %d", len(scenes))
	}
	sc := scenes[0]
	if sc["follow"] != "u1" {
		t.Errorf("follow = %v", sc["follow"])
	}
	sprites := sc["sprites"].([]any)
	if len(sprites) != 3 {
		t.Fatalf("sprites: %d", len(sprites))
	}
	want := map[string][3]any{"u1": {"Ana", 10, 16}, "u2": {"bruno", 13, 19}, "u3": {"Carla", 14, 16}}
	for _, s := range sprites {
		sp := s.(node)
		w := want[sp["id"].(string)]
		if sp["label"] != w[0] || sp["x"] != w[1] || sp["y"] != w[2] {
			t.Errorf("sprite %v, want %v", sp, w)
		}
		if _, ok := artPatterns[sp["pattern"].(string)]; !ok {
			t.Errorf("sprite pattern %v unknown", sp["pattern"])
		}
	}
	if len(sc["map"].([]string)) != 30 {
		t.Errorf("map rows %d", len(sc["map"].([]string)))
	}
}

func TestPresenceListsNearby(t *testing.T) {
	tree := presenceTree(onlineView())
	items := ofType(tree, "list")[0]["items"].([]any)
	if len(items) != 3 {
		t.Fatalf("items: %d", len(items))
	}
	var names []string
	badge := map[string]any{}
	for _, i := range items {
		it := i.(node)
		names = append(names, it["title"].(string))
		badge[it["title"].(string)] = it["badge"]
	}
	if strings.Join(names, ",") != "Ana,bruno,Carla" {
		t.Errorf("order %v", names)
	}
	// Carla is 4 tiles away in x: not nearby; move her to 3 away. bruno is 3 away: nearby.
	if badge["bruno"] != "Nearby" || badge["Carla"] != nil || badge["Ana"] != nil {
		t.Errorf("badges %v", badge)
	}
	v := onlineView()
	p := v.People["u3"]
	p.X = 13
	v.People["u3"] = p
	items = ofType(presenceTree(v), "list")[0]["items"].([]any)
	for _, i := range items {
		if it := i.(node); it["title"] == "Carla" && it["badge"] != "Nearby" {
			t.Error("Carla at 3 tiles should be Nearby")
		}
	}
	if find(tree, func(m node) bool { return m["id"] == "enter" && m["label"] == "Enter office" }) == nil {
		t.Error("no Enter office button")
	}
	if find(tree, func(m node) bool { return m["id"] == "change" && m["label"] == "Change setup" }) == nil {
		t.Error("no Change setup button")
	}
}

func TestPresenceShowsSetupForm(t *testing.T) {
	tree := presenceTree(view{})
	if find(tree, func(m node) bool { return m["id"] == "token" }) == nil || find(tree, func(m node) bool { return m["id"] == "save" }) == nil {
		t.Errorf("no form: %s", js(t, tree))
	}
}

func TestPresenceOfflineReasons(t *testing.T) {
	base := onlineView()
	base.Online = false
	for _, reason := range []string{reasonUnreachable("wss://x.example"), reasonElsewhere, reasonFull} {
		v := base
		v.Reason = reason
		s := js(t, presenceTree(v))
		if !strings.Contains(s, `"Offline"`) || !strings.Contains(s, reason) {
			t.Errorf("%q: %s", reason, s)
		}
	}
	if reasonUnreachable("wss://x.example") != "Cannot reach wss://x.example" || reasonElsewhere != "Signed in elsewhere" || reasonFull != "The office is full" {
		t.Error("reason texts")
	}
	// A refused token goes back to the form with the message.
	v := base
	v.Editing, v.FormErr = true, reasonToken
	s := js(t, presenceTree(v))
	if !strings.Contains(s, "The server refused the team token") || !strings.Contains(s, `"id":"token"`) {
		t.Errorf("token refusal: %s", s)
	}
}

func TestPresenceAlone(t *testing.T) {
	v := onlineView()
	v.People = map[string]office.Person{"u1": v.People["u1"]}
	if !strings.Contains(js(t, presenceTree(v)), "Nobody else is in the office") {
		t.Error("alone message missing")
	}
	if strings.Contains(js(t, presenceTree(onlineView())), "Nobody else") {
		t.Error("alone message shown with company")
	}
}

func TestStatusBadges(t *testing.T) {
	v := onlineView()
	set := func(s office.Status) any {
		p := v.People["u1"]
		p.Status = s
		v.People["u1"] = p
		for _, sp := range ofType(officeTree(v), "scene")[0]["sprites"].([]any) {
			if m := sp.(node); m["id"] == "u1" {
				return m["badge"]
			}
		}
		return "missing"
	}
	if got := set(office.StatusFocused); got != "Focused" {
		t.Errorf("focused: %v", got)
	}
	if got := set(office.StatusNeedsYou); got != "Needs you" {
		t.Errorf("needs you: %v", got)
	}
	if got := set(office.StatusNone); got != nil {
		t.Errorf("none: %v", got)
	}
}

func TestFormNeverDrawsToken(t *testing.T) {
	v := view{Setup: &Setup{Token: "supersecrettoken"}, Editing: true, Form: Setup{Name: "Ana", Server: "wss://x", Token: "supersecrettoken", Avatar: 2}}
	s := js(t, presenceTree(v))
	if strings.Contains(s, "supersecrettoken") {
		t.Error("token drawn")
	}
	if !strings.Contains(s, `"value":"Ana"`) || !strings.Contains(s, `"value":"wss://x"`) || !strings.Contains(s, `"value":"Green"`) {
		t.Errorf("prefill: %s", s)
	}
}
