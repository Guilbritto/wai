// wai-bichinho: a wai extension (line-delimited JSON-RPC 2.0 on stdio) that keeps a small pet in
// the prompt box. It hatches from an egg at a rarity, grows on your agents' finished turns, gets
// hungry and lonely with time, and reacts to the focused session.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"math/rand/v2"
	"os"
	"path/filepath"
	"strings"
	"time"
)

const (
	promptPanel = "pet"
	cardPanel   = "card"
	stateFile   = "pet.json"
	frameEvery  = 500 * time.Millisecond
	// A session list every listEvery frames drops the states of closed sessions.
	listEvery = 60
	sleepGap  = 20 * time.Minute
	flashFor  = 4 * time.Second
)

type message struct {
	ID     json.RawMessage `json:"id,omitempty"`
	Method string          `json:"method,omitempty"`
	Params json.RawMessage `json:"params,omitempty"`
	Result json.RawMessage `json:"result,omitempty"`
}

type node = map[string]any

type server struct {
	out io.Writer
	now func() time.Time
	rng *rand.Rand

	path  string // pet.json; empty until initialize
	st    State
	ready bool

	focused   string            // agent\x00session, or "" when no session is focused
	states    map[string]string // agent\x00session → idle, working, needs_you, error
	lastEvent time.Time         // sleeps after sleepGap without one
	flash     struct {          // a short reaction
		mood, text string
		until      time.Time
	}
	frame  int // advances only while a session is focused
	ticks  int
	listID int
	sent   map[string]string // the last tree sent per panel
}

func newServer(out io.Writer, now func() time.Time, rng *rand.Rand) *server {
	return &server{out: out, now: now, rng: rng, st: newState(), states: map[string]string{}, sent: map[string]string{}}
}

func (s *server) write(v any) {
	b, _ := json.Marshal(v)
	fmt.Fprintf(s.out, "%s\n", b)
}

func (s *server) call(method string, params any) {
	s.write(node{"jsonrpc": "2.0", "method": method, "params": params})
}

func key(agent, session *string) string {
	if agent == nil || session == nil {
		return ""
	}
	return *agent + "\x00" + *session
}

func (s *server) load() {
	b, err := os.ReadFile(s.path)
	if err != nil {
		return
	}
	var st State
	if err := json.Unmarshal(b, &st); err != nil {
		fmt.Fprintln(os.Stderr, "wai-bichinho: cannot read", s.path+":", err)
		return
	}
	if st.Seen == nil {
		st.Seen = map[string]bool{}
	}
	// A species this version does not know (a newer one wrote the file) is left out.
	if st.Pet != nil && speciesByID(st.Pet.Species) == nil {
		st.Pet = nil
	}
	past := st.Past[:0]
	for _, p := range st.Past {
		if speciesByID(p.Species) != nil {
			past = append(past, p)
		}
	}
	st.Past = past
	s.st = st
}

// save writes pet.json whole, through a temporary file, so a crash never leaves half of it.
func (s *server) save() {
	if s.path == "" {
		return
	}
	b, _ := json.MarshalIndent(s.st, "", "  ")
	tmp := s.path + ".tmp"
	if err := os.WriteFile(tmp, b, 0o600); err != nil {
		fmt.Fprintln(os.Stderr, "wai-bichinho: cannot save:", err)
		return
	}
	if err := os.Rename(tmp, s.path); err != nil {
		fmt.Fprintln(os.Stderr, "wai-bichinho: cannot save:", err)
	}
}

func (s *server) react(mood, text string) {
	s.flash.mood, s.flash.text, s.flash.until = mood, text, s.now().Add(flashFor)
}

// mood picks the face and the word beside the level, most urgent first.
func (s *server) mood(now time.Time) (string, string) {
	p := s.st.Pet
	if p == nil {
		return "egg", "hatch me"
	}
	if now.Before(s.flash.until) {
		return s.flash.mood, s.flash.text
	}
	for _, state := range s.states {
		if state == "needs_you" {
			return "alert", "needs you"
		}
	}
	switch s.states[s.focused] {
	case "error":
		return "error", "oops"
	case "working":
		return "working", "working"
	}
	switch {
	case p.Hunger(now) >= 60:
		return "hungry", "hungry"
	case p.Affection(now) < 25:
		return "sad", "lonely"
	case now.Sub(s.lastEvent) > sleepGap:
		return "sleeping", "zzz"
	}
	return "idle", p.Name
}

func (s *server) menu() node {
	var items []node
	if p := s.st.Pet; p != nil {
		items = append(items, node{"id": "pet", "label": "Pet " + p.Name}, node{"id": "feed", "label": "Feed"})
	}
	items = append(items, node{"id": "card", "label": "Open card"})
	if s.st.Eggs > 0 {
		items = append(items, s.hatchItem(fmt.Sprintf("Hatch an egg (%d)", s.st.Eggs)))
	}
	return node{"type": "menu", "items": items}
}

// hatchItem asks first when a pet would move out.
func (s *server) hatchItem(label string) node {
	item := node{"id": "hatch", "label": label}
	if p := s.st.Pet; p != nil {
		item["confirm"] = node{"title": "Hatch a new egg?", "text": p.Name + " moves to your past pets.", "confirm_label": "Hatch"}
	}
	return item
}

func (s *server) promptTree(now time.Time) node {
	mood, status := s.mood(now)
	badge := node{"type": "badge", "text": "egg"}
	if p := s.st.Pet; p != nil {
		badge = node{"type": "badge", "text": fmt.Sprintf("Lv %d", level(p.XP)), "tone": rarities[speciesByID(p.Species).Rarity].tone}
	}
	return node{"type": "column", "children": []node{
		{"type": "text", "text": draw(s.st.Pet, mood, s.frame), "mono": true},
		{"type": "row", "children": []node{badge, {"type": "text", "text": status, "muted": true}, {"type": "spacer"}, s.menu()}},
	}}
}

func (s *server) cardTree(now time.Time) node {
	text := func(t string, muted bool) node { return node{"type": "text", "text": t, "muted": muted} }
	kv := func(pairs ...string) node {
		var items []node
		for i := 0; i+1 < len(pairs); i += 2 {
			items = append(items, node{"key": pairs[i], "value": pairs[i+1]})
		}
		return node{"type": "kv", "items": items}
	}
	hatch := node{"type": "button", "id": "hatch", "label": "Hatch an egg", "disabled": s.st.Eggs == 0}
	if item := s.hatchItem(""); item["confirm"] != nil {
		hatch["confirm"] = item["confirm"]
	}
	p := s.st.Pet
	if p == nil {
		return node{"type": "column", "children": []node{
			{"type": "text", "text": draw(nil, "egg", 0), "mono": true},
			text("An egg. Hatch it to meet your pet: it may be common, or very rare.", true),
			hatch,
		}}
	}
	sp := speciesByID(p.Species)
	r := rarities[sp.Rarity]
	head := []node{{"type": "text", "text": p.Name}, {"type": "badge", "text": r.name, "tone": r.tone}}
	if p.Shiny {
		head = append(head, node{"type": "badge", "text": "✦ Shiny", "tone": "warning"})
	}
	lv := level(p.XP)
	next := xpFor(lv + 1)
	toEgg := turnsPerEgg - s.st.Turns%turnsPerEgg
	eggs := fmt.Sprintf("%d (next in %d turns)", s.st.Eggs, toEgg)
	if s.st.Eggs >= maxEggs {
		eggs = fmt.Sprintf("%d (full)", s.st.Eggs)
	}
	var stats []string
	for i, n := range statNames {
		stats = append(stats, n, fmt.Sprintf("%s %d", strings.Repeat("●", p.Stats[i])+strings.Repeat("○", 10-p.Stats[i]), p.Stats[i]))
	}
	var dex []node
	seen := 0
	for _, sp := range species {
		name := "???"
		if shiny, ok := s.st.Seen[sp.ID]; ok {
			seen++
			name = sp.Name
			if shiny {
				name += " ✦"
			}
		}
		dex = append(dex, node{"cells": []any{name, node{"type": "badge", "text": rarities[sp.Rarity].name, "tone": rarities[sp.Rarity].tone}}})
	}
	var past []string
	for _, q := range s.st.Past {
		past = append(past, q.Title(), fmt.Sprintf("%s · Lv %d", speciesByID(q.Species).Name, level(q.XP)))
	}
	kids := []node{
		{"type": "text", "text": draw(p, "idle", 0), "mono": true},
		{"type": "row", "children": head},
		text(fmt.Sprintf("%s · born %s", sp.Name, p.Born.Local().Format("Jan 2")), true),
		kv("Level", fmt.Sprintf("%d (%d/%d xp)", lv, p.XP, next),
			"Hunger", percent(p.Hunger(now)),
			"Love", percent(p.Affection(now)),
			"Eggs", eggs),
		{"type": "progress", "value": float64(p.XP-xpFor(lv)) / float64(next-xpFor(lv))},
		{"type": "row", "children": []node{
			{"type": "button", "id": "pet", "label": "Pet", "style": "primary"},
			{"type": "button", "id": "feed", "label": "Feed"},
			hatch,
		}},
		{"type": "section", "id": "s-stats", "title": "Stats", "children": []node{kv(stats...)}},
		{"type": "section", "id": "s-name", "title": "Name", "open": false, "children": []node{
			{"type": "row", "children": []node{
				{"type": "input", "id": "name", "placeholder": "New name"},
				{"type": "button", "id": "rename", "label": "Rename"},
			}},
		}},
		{"type": "section", "id": "s-dex", "title": "Collection", "badge": fmt.Sprintf("%d/%d", seen, len(species)), "children": []node{
			{"type": "table", "columns": []node{{"title": "Species"}, {"title": "Rarity", "width": 110}}, "rows": dex},
		}},
	}
	if len(past) > 0 {
		kids = append(kids, node{"type": "section", "id": "s-past", "title": "Past pets", "open": false, "children": []node{kv(past...)}})
	}
	return node{"type": "column", "children": kids}
}

// render sends each panel whose tree changed.
func (s *server) render() {
	if !s.ready {
		return
	}
	now := s.now()
	for _, p := range []struct {
		id   string
		tree node
	}{{promptPanel, s.promptTree(now)}, {cardPanel, s.cardTree(now)}} {
		b, _ := json.Marshal(p.tree)
		if s.sent[p.id] == string(b) {
			continue
		}
		s.sent[p.id] = string(b)
		s.call("panel/render", node{"panel": p.id, "tree": json.RawMessage(b)})
	}
}

// tick animates while a session is focused: elsewhere the slot is hidden, or on a draft.
func (s *server) tick() {
	if s.focused != "" {
		s.frame++
	}
	s.ticks++
	if s.ticks%listEvery == 0 && len(s.states) > 0 {
		s.listID++
		s.write(node{"jsonrpc": "2.0", "id": s.listID, "method": "sessions/list"})
	}
	s.render()
}

func (s *server) act(action string, values map[string]any) {
	now := s.now()
	p := s.st.Pet
	switch action {
	case "pet":
		if p != nil {
			p.Pet(now)
			s.react("happy", "♥")
		}
	case "feed":
		if p != nil {
			if p.Feed(now) {
				s.react("happy", "yum")
			} else {
				s.react("idle", "full")
			}
		}
	case "card":
		s.call("panel/open", node{"panel": cardPanel})
	case "hatch":
		if s.st.Eggs > 0 {
			s.st.Hatch(s.rng, now)
			s.react("surprise", "hi!")
		}
	case "rename":
		name, _ := values["name"].(string)
		if name = strings.TrimSpace(name); p != nil && name != "" {
			if r := []rune(name); len(r) > 10 {
				name = string(r[:10])
			}
			p.Name = name
		}
	}
	s.save()
}

func (s *server) handle(m message) {
	switch m.Method {
	case "initialize":
		var p struct {
			DataDir string `json:"data_dir"`
		}
		json.Unmarshal(m.Params, &p)
		s.write(node{"jsonrpc": "2.0", "id": m.ID, "result": node{}})
		dir := p.DataDir
		if dir == "" {
			dir = os.Getenv("WAI_EXT_DATA")
		}
		if dir == "" {
			dir, _ = os.Getwd()
		}
		s.path = filepath.Join(dir, stateFile)
		s.load()
		s.lastEvent = s.now()
		s.ready = true
		s.listID++
		s.write(node{"jsonrpc": "2.0", "id": s.listID, "method": "sessions/list"})
		s.render()
	case "panel/action":
		var p struct {
			Action string         `json:"action_id"`
			Values map[string]any `json:"values"`
		}
		json.Unmarshal(m.Params, &p)
		s.write(node{"jsonrpc": "2.0", "id": m.ID, "result": nil})
		s.lastEvent = s.now()
		s.act(p.Action, p.Values)
		s.render()
	case "event/session_state":
		var p struct {
			Agent, Session, State *string
		}
		json.Unmarshal(m.Params, &p)
		if k := key(p.Agent, p.Session); k != "" && p.State != nil {
			s.states[k] = *p.State
		}
		s.lastEvent = s.now()
		s.render()
	case "event/turn_ended":
		var p struct{ Reason string }
		json.Unmarshal(m.Params, &p)
		levelUp, egg := s.st.TurnEnded(p.Reason)
		switch {
		case levelUp:
			s.react("happy", "level up!")
		case egg:
			s.react("surprise", "an egg!")
		case s.st.Pet != nil && p.Reason == "end_turn":
			s.react("happy", fmt.Sprintf("+%d xp", xpPerTurn))
		}
		s.save()
		s.lastEvent = s.now()
		s.render()
	case "event/session_focused":
		var p struct{ Agent, Session *string }
		json.Unmarshal(m.Params, &p)
		s.focused = key(p.Agent, p.Session)
		s.lastEvent = s.now()
		s.render()
	case "":
		// The answer to our sessions/list: the open sessions' states, closed ones dropped.
		var list []struct{ Agent, Session, State *string }
		if json.Unmarshal(m.Result, &list) == nil && m.Result != nil {
			states := map[string]string{}
			for _, l := range list {
				if k := key(l.Agent, l.Session); k != "" && l.State != nil {
					states[k] = *l.State
				}
			}
			s.states = states
			s.render()
		}
	default:
		if m.ID != nil {
			s.write(node{"jsonrpc": "2.0", "id": m.ID, "error": node{"code": -32601, "message": "Method not found"}})
		}
	}
}

// serve handles wai's messages and the animation frames on one goroutine, until stdin closes.
func serve(s *server, in io.Reader, frames <-chan time.Time) {
	msgs := make(chan message)
	go func() {
		defer close(msgs)
		r := bufio.NewReaderSize(in, 1<<20)
		for {
			line, err := r.ReadBytes('\n')
			var m message
			if len(line) > 0 && json.Unmarshal(line, &m) == nil {
				msgs <- m
			}
			if err != nil {
				return
			}
		}
	}()
	for {
		select {
		case m, ok := <-msgs:
			if !ok {
				return
			}
			s.handle(m)
		case <-frames:
			s.tick()
		}
	}
}

func main() {
	s := newServer(os.Stdout, time.Now, rand.New(rand.NewPCG(uint64(time.Now().UnixNano()), 0)))
	serve(s, os.Stdin, time.NewTicker(frameEvery).C)
}
