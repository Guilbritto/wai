package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"math"
	"math/rand/v2"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"testing"
	"time"
)

func seeded() *rand.Rand { return rand.New(rand.NewPCG(1, 2)) }

func TestEggsFollowTheRarityOdds(t *testing.T) {
	const n = 200_000
	rng := seeded()
	st := newState()
	tiers := map[Rarity]int{}
	shiny := 0
	for range n {
		st.Eggs, st.Past = 1, nil
		p := st.Hatch(rng, time.Now())
		tiers[lineByID(p.Species).Rarity]++
		if p.Shiny {
			shiny++
		}
		for _, v := range p.Stats {
			if v < rarities[lineByID(p.Species).Rarity].floor || v > 10 {
				t.Fatalf("stat %d out of range for %s", v, p.Species)
			}
		}
	}
	for i, r := range rarities {
		got := float64(tiers[Rarity(i)]) / n * 100
		if math.Abs(got-float64(r.weight)) > 0.5 {
			t.Errorf("%s: %.2f%% of eggs, want about %d%%", r.name, got, r.weight)
		}
	}
	if got := float64(shiny) / n; math.Abs(got-1.0/shinyOdds) > 0.003 {
		t.Errorf("shiny: %.4f of eggs, want about %.4f", got, 1.0/shinyOdds)
	}
	if len(st.Seen) != len(dex) {
		t.Errorf("hatched %d of %d lines", len(st.Seen), len(dex))
	}
}

// The ⋯ menu's row does not change while the pet animates, so the menu never wraps under it.
func TestMenuStaysPutWhileAnimating(t *testing.T) {
	now := time.Now()
	s := newServer(&bytes.Buffer{}, func() time.Time { return now }, seeded())
	s.st.Pet = &Pet{Species: "jormungandr", Shiny: true, FedAt: now, Love: 100, LovedAt: now}
	s.focused = "a\x00s"
	s.states[s.focused] = "working"
	var first string
	for s.frame = range 16 {
		var row string
		for _, kid := range s.dockTree(now)["children"].([]node) {
			b, _ := json.Marshal(kid)
			if strings.Contains(string(b), `"type":"menu"`) {
				row = string(b)
			}
		}
		if s.frame == 0 {
			first = row
		} else if row != first {
			t.Fatalf("frame %d: the menu's row changed:\n%s\n%s", s.frame, first, row)
		}
	}
}

func TestHungerLoveAndLevels(t *testing.T) {
	t0 := time.Date(2026, 11, 16, 12, 0, 0, 0, time.UTC)
	p := &Pet{FedAt: t0, Love: 60, LovedAt: t0}
	if h := p.Hunger(t0.Add(hungerHours / 2 * time.Hour)); h != 50 {
		t.Errorf("hunger halfway: %v", h)
	}
	if h := p.Hunger(t0.Add(100 * time.Hour)); h != 100 {
		t.Errorf("hunger caps at 100: %v", h)
	}
	if p.Feed(t0.Add(time.Minute)) {
		t.Error("fed a full pet")
	}
	if !p.Feed(t0.Add(4*time.Hour)) || p.Hunger(t0.Add(4*time.Hour)) != 0 {
		t.Error("feeding a hungry pet fills it")
	}
	if a := p.Affection(t0.Add(loveHours / 2 * time.Hour)); a != 10 {
		t.Errorf("love after half a day: %v", a)
	}
	p.Pet(t0.Add(48 * time.Hour))
	if a := p.Affection(t0.Add(48 * time.Hour)); a != petLove {
		t.Errorf("petting a forgotten pet: %v", a)
	}
	for xp, want := range map[int]int{0: 1, 24: 1, 25: 2, 99: 2, 100: 3, 225: 4} {
		if got := level(xp); got != want {
			t.Errorf("level(%d) = %d, want %d", xp, got, want)
		}
	}

	st := newState()
	st.Eggs = 0
	eggs := 0
	for range turnsPerEgg * 5 {
		if _, _, egg := st.TurnEnded("end_turn"); egg {
			eggs++
		}
	}
	if eggs != maxEggs || st.Eggs != maxEggs {
		t.Errorf("eggs: %d given, %d held; want %d", eggs, st.Eggs, maxEggs)
	}
	st.Hatch(seeded(), t0)
	up, _, _ := st.TurnEnded("end_turn")
	up2, _, _ := st.TurnEnded("end_turn")
	up3, _, _ := st.TurnEnded("end_turn")
	if st.Pet.XP != 3*xpPerTurn || up || up2 || !up3 {
		t.Errorf("xp %d, level ups %v %v %v", st.Pet.XP, up, up2, up3)
	}
	st.TurnEnded("cancelled")
	if st.Pet.XP != 3*xpPerTurn+2 {
		t.Errorf("a cancelled turn gives 2 xp: %d", st.Pet.XP)
	}
}

type harness struct {
	t    *testing.T
	s    *server
	out  *bytes.Buffer
	now  time.Time
	init []map[string]any // what initialize wrote
}

func start(t *testing.T, dir string) *harness {
	h := &harness{t: t, out: &bytes.Buffer{}, now: time.Date(2026, 11, 16, 12, 0, 0, 0, time.UTC)}
	h.s = newServer(h.out, func() time.Time { return h.now }, seeded())
	h.init = h.send(`{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":1,"data_dir":` + quote(dir) + `}}`)
	return h
}

func quote(s string) string { b, _ := json.Marshal(s); return string(b) }

func (h *harness) send(line string) []map[string]any {
	h.t.Helper()
	var m message
	if err := json.Unmarshal([]byte(line), &m); err != nil {
		h.t.Fatal(err)
	}
	h.s.handle(m)
	var got []map[string]any
	for _, l := range strings.Split(strings.TrimSpace(h.out.String()), "\n") {
		if l == "" {
			continue
		}
		var v map[string]any
		if err := json.Unmarshal([]byte(l), &v); err != nil {
			h.t.Fatalf("bad line %q: %v", l, err)
		}
		got = append(got, v)
	}
	h.out.Reset()
	return got
}

// dockOf is the dock's name (first row) and stage rows, from the last render.
func dockOf(t *testing.T, msgs []map[string]any) (name string, rows []string) {
	t.Helper()
	for _, m := range msgs {
		p, _ := m["params"].(map[string]any)
		if m["method"] != "panel/render" || p["panel"] != petPanel {
			continue
		}
		for _, k := range p["tree"].(map[string]any)["children"].([]any) {
			n := k.(map[string]any)
			switch n["type"] {
			case "row":
				name = n["children"].([]any)[0].(map[string]any)["text"].(string)
			case "pixels":
				rows = nil
				for _, r := range n["rows"].([]any) {
					rows = append(rows, r.(string))
				}
			}
		}
	}
	if rows == nil {
		t.Fatalf("no dock render in %v", msgs)
	}
	return
}

func TestProtocol(t *testing.T) {
	dir := t.TempDir()
	h := start(t, dir)
	if name, _ := dockOf(t, h.init); name != "Egg" || h.s.mood(h.now) != "egg" {
		t.Errorf("first run: %q %q", name, h.s.mood(h.now))
	}
	msgs := h.send(`{"jsonrpc":"2.0","method":"event/file_saved","params":{"path":"/x"}}`)
	if len(msgs) != 0 {
		t.Fatalf("an unknown notification is ignored: %v", msgs)
	}

	msgs = h.send(`{"jsonrpc":"2.0","id":7,"method":"panel/action","params":{"panel":"pet","action_id":"hatch","values":{}}}`)
	if msgs[0]["id"] != 7.0 {
		t.Fatalf("panel/action answered first: %v", msgs[0])
	}
	name, _ := dockOf(t, msgs)
	if name != h.s.st.Pet.Title() || h.s.mood(h.now) != "surprise" {
		t.Errorf("after hatching: %q %q", name, h.s.mood(h.now))
	}
	var saved State
	b, _ := os.ReadFile(filepath.Join(dir, stateFile))
	if json.Unmarshal(b, &saved); saved.Pet == nil || saved.Eggs != 0 {
		t.Fatalf("hatch not saved: %s", b)
	}

	h.send(`{"jsonrpc":"2.0","method":"event/turn_ended","params":{"agent":"claude","session":"s1","reason":"end_turn"}}`)
	if m := h.s.mood(h.now); m != "happy" {
		t.Errorf("after a turn: %q", m)
	}

	h.now = h.now.Add(10 * time.Second)
	h.send(`{"jsonrpc":"2.0","method":"event/session_focused","params":{"agent":"claude","session":"s1"}}`)
	_, art := dockOf(t, h.send(`{"jsonrpc":"2.0","method":"event/session_state","params":{"agent":"claude","session":"s1","state":"working"}}`))
	if m := h.s.mood(h.now); m != "working" {
		t.Errorf("while the focused session works: %q", m)
	}
	h.s.tick()
	if _, moved := dockOf(t, h.send(`{"jsonrpc":"2.0","method":"event/file_saved","params":{"path":"/x"}}`)); reflect.DeepEqual(moved, art) {
		t.Error("a frame does not bob the working pet")
	}
	h.send(`{"jsonrpc":"2.0","method":"event/session_state","params":{"agent":"claude","session":"s2","state":"needs_you"}}`)
	if m := h.s.mood(h.now); m != "alert" {
		t.Errorf("another session needs you: %q", m)
	}
	// The session list drops closed sessions.
	h.send(`{"jsonrpc":"2.0","id":1,"result":[{"agent":"claude","session":"s1","title":null,"state":"idle"}]}`)
	if m := h.s.mood(h.now); m != "idle" {
		t.Errorf("a closed session no longer calls: %q", m)
	}

	h.now = h.now.Add(7 * time.Hour)
	name, _ = dockOf(t, h.send(`{"jsonrpc":"2.0","id":8,"method":"panel/action","params":{"panel":"pet","action_id":"rename","values":{"name":"  Zezinho  "}}}`))
	if m := h.s.mood(h.now); m != "hungry" || name != "Zezinho" {
		t.Errorf("after 7 hours: %q %q", m, name)
	}
	h.send(`{"jsonrpc":"2.0","id":9,"method":"panel/action","params":{"panel":"pet","action_id":"feed","values":{}}}`)
	if m := h.s.mood(h.now); m != "happy" {
		t.Errorf("feeding: %q", m)
	}
	h.send(`{"jsonrpc":"2.0","id":12,"method":"panel/action","params":{"panel":"pet","action_id":"pet","values":{}}}`)
	if !h.s.st.Pet.LovedAt.Equal(h.now) {
		t.Error("clicking the stage does not pet the pet")
	}

	msgs = h.send(`{"jsonrpc":"2.0","id":10,"method":"panel/action","params":{"panel":"pet","action_id":"card","values":{}}}`)
	if msgs[1]["method"] != "panel/open" {
		t.Errorf("Open card opens the card: %v", msgs)
	}
	msgs = h.send(`{"jsonrpc":"2.0","id":11,"method":"nope"}`)
	if msgs[0]["error"].(map[string]any)["code"] != -32601.0 {
		t.Errorf("unknown request: %v", msgs)
	}

	// A restart finds the same pet.
	again := start(t, dir)
	if p := again.s.st.Pet; p == nil || p.Name != "Zezinho" || p.XP != xpPerTurn {
		t.Errorf("reloaded pet: %+v", p)
	}
}

func TestPREvents(t *testing.T) {
	h := start(t, t.TempDir())
	h.send(`{"jsonrpc":"2.0","id":1,"method":"panel/action","params":{"panel":"pet","action_id":"hatch","values":{}}}`)
	h.now = h.now.Add(time.Minute) // past the hatch flash
	prs := func(reasons ...string) string {
		var list []string
		for i, r := range reasons {
			list = append(list, fmt.Sprintf(`{"url":"https://github.com/o/r/pull/%d","reason":%q}`, i+1, r))
		}
		return `{"jsonrpc":"2.0","method":"event/ext","params":{"source":"wai-waycarbon","name":"prs.mine","data":{"prs":[` + strings.Join(list, ",") + `]}}}`
	}
	// a render that would repeat the last tree sends nothing, so read the mood; "!" marks a flash
	word := func(line string) string {
		t.Helper()
		h.send(line)
		if h.now.Before(h.s.flash.until) {
			return h.s.mood(h.now) + "!"
		}
		return h.s.mood(h.now)
	}

	if w := word(prs("approved", "waiting")); w != "happy" {
		t.Errorf("first event, no flash, an approved PR: %q", w)
	}
	if w := word(prs("approved", "ci_failed")); w != "alert!" {
		t.Errorf("a PR that turns red flashes: %q", w)
	}
	h.now = h.now.Add(time.Minute)
	if w := word(prs("approved", "ci_failed", "conflict")); w != "alert!" {
		t.Errorf("a new PR with a conflict flashes: %q", w)
	}
	h.now = h.now.Add(time.Minute)
	if w := word(prs("approved", "ci_failed", "conflict")); w != "alert" {
		t.Errorf("no change, PRs to fix win over approved: %q", w)
	}
	if w := word(prs("approved", "approved")); w != "happy!" {
		t.Errorf("a PR approved: %q", w)
	}
	h.now = h.now.Add(time.Minute)
	if w := word(prs("approved", "approved")); w != "happy" {
		t.Errorf("two approved: %q", w)
	}
	for _, other := range []string{
		`{"jsonrpc":"2.0","method":"event/ext","params":{"source":"someone","name":"prs.mine","data":{"prs":[]}}}`,
		`{"jsonrpc":"2.0","method":"event/ext","params":{"source":"wai-waycarbon","name":"other","data":{"prs":[]}}}`,
		`{"jsonrpc":"2.0","method":"event/ext","params":{"source":"wai-waycarbon","name":"prs.mine","data":"bad"}}`,
	} {
		if msgs := h.send(other); len(msgs) != 0 {
			t.Errorf("ignored %s: %v", other, msgs)
		}
	}
	if w := word(prs()); w == "happy" || w == "happy!" {
		t.Errorf("all merged: %q", w)
	}
}

func TestFirstPRsToFixSpeak(t *testing.T) {
	h := start(t, t.TempDir())
	id, _, _ := bubbleOf(h.action("hatch"))
	h.bubbleDone(id)
	h.now = h.now.Add(time.Minute)
	pr := `{"jsonrpc":"2.0","method":"event/ext","params":{"source":"wai-waycarbon","name":"prs.mine","data":{"prs":[{"url":"https://github.com/o/r/pull/1","reason":"ci_failed"}]}}}`
	id, text, _ := bubbleOf(h.send(pr))
	if text != fallback["pr_fix"][0] {
		t.Fatalf("a PR to fix at launch is said: %q", text)
	}
	if m := h.s.mood(h.now); m != "alert" || h.now.Before(h.s.flash.until) {
		t.Errorf("no flash for an old PR: %q", m)
	}
	h.bubbleDone(id)
	h.now = h.now.Add(time.Minute)
	if _, _, ok := bubbleOf(h.send(pr)); ok {
		t.Error("the same PRs are said again")
	}
}

func TestServeEndsAtEOF(t *testing.T) {
	var out bytes.Buffer
	s := newServer(&out, time.Now, seeded())
	in := `{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":1,"data_dir":` + quote(t.TempDir()) + `}}` + "\n"
	serve(s, strings.NewReader(in), nil)
	if !strings.Contains(out.String(), `"panel":"pet"`) {
		t.Errorf("no render: %s", out.String())
	}
}

// bubbleOf is the bubble node in the last prompt render.
func bubbleOf(msgs []map[string]any) (id, text string, ok bool) {
	for _, m := range msgs {
		p, _ := m["params"].(map[string]any)
		if m["method"] != "panel/render" || p["panel"] != petPanel {
			continue
		}
		id, text, ok = "", "", false
		for _, k := range p["tree"].(map[string]any)["children"].([]any) {
			if n := k.(map[string]any); n["type"] == "bubble" {
				id, _ = n["id"].(string)
				text, _ = n["text"].(string)
				ok = true
			}
		}
	}
	return
}

func sticky(msgs []map[string]any) bool {
	for _, m := range msgs {
		p, _ := m["params"].(map[string]any)
		if m["method"] == "panel/render" && p["panel"] == petPanel {
			for _, k := range p["tree"].(map[string]any)["children"].([]any) {
				if n := k.(map[string]any); n["type"] == "bubble" {
					return n["sticky"] == true
				}
			}
		}
	}
	return false
}

func (h *harness) action(action string) []map[string]any {
	return h.send(`{"jsonrpc":"2.0","id":5,"method":"panel/action","params":{"panel":"pet","action_id":"` + action + `","values":{}}}`)
}

func (h *harness) bubbleDone(id string) []map[string]any {
	return h.send(`{"jsonrpc":"2.0","id":6,"method":"panel/action","params":{"panel":"pet","action_id":"` + id + `","values":{"bubble":"timeout"}}}`)
}

func TestHatchSpeaks(t *testing.T) {
	h := start(t, t.TempDir())
	_, text, ok := bubbleOf(h.action("hatch"))
	if !ok || !strings.Contains(text, h.s.st.Pet.Name) {
		t.Errorf("hatch bubble %q %v", text, ok)
	}
}

func TestNeedsYouIsStickyAndAnswered(t *testing.T) {
	h := start(t, t.TempDir())
	msgs := h.send(`{"jsonrpc":"2.0","method":"event/session_state","params":{"agent":"claude","session":"s2","state":"needs_you"}}`)
	if _, _, ok := bubbleOf(msgs); !ok || !sticky(msgs) {
		t.Fatalf("needs_you bubble: %v", msgs)
	}
	msgs = h.send(`{"jsonrpc":"2.0","method":"event/session_state","params":{"agent":"claude","session":"s2","state":"idle"}}`)
	if _, _, ok := bubbleOf(msgs); ok {
		t.Errorf("the answered bubble stays: %v", msgs)
	}
}

func TestBubbleDoneFreesTheQueue(t *testing.T) {
	h := start(t, t.TempDir())
	id, _, _ := bubbleOf(h.action("hatch"))
	h.bubbleDone(id)
	h.now = h.now.Add(300 * time.Millisecond)
	h.action("pet")
	msgs := h.action("feed")
	id, text, _ := bubbleOf(msgs)
	if text != fallback["pet"][0] {
		t.Fatalf("pet line first: %q", text)
	}
	h.bubbleDone(id)
	h.now = h.now.Add(300 * time.Millisecond)
	h.s.tick()
	_, text, _ = bubbleOf(h.send(`{"jsonrpc":"2.0","method":"event/file_saved","params":{}}`))
	if text != fallback["feed"][0] && text != fallback["full"][0] {
		t.Errorf("feed line next: %q", text)
	}
}

func TestFocusDropsNormalLine(t *testing.T) {
	h := start(t, t.TempDir())
	id, _, _ := bubbleOf(h.action("hatch"))
	h.bubbleDone(id)
	h.now = h.now.Add(300 * time.Millisecond)
	if _, _, ok := bubbleOf(h.action("pet")); !ok {
		t.Fatal("pet speaks")
	}
	if _, _, ok := bubbleOf(h.send(`{"jsonrpc":"2.0","method":"event/session_focused","params":{"agent":"claude","session":"s2"}}`)); ok {
		t.Error("a focus switch leaves the bubble")
	}
}

func TestMouthTalks(t *testing.T) {
	// two frames of one pose and one bob: only the mouth can differ.
	if sprite("gosma", "talk", 0, false, 0) == sprite("gosma", "talk", 1, false, 0) {
		t.Error("the mouth does not move")
	}
	h := start(t, t.TempDir())
	h.action("hatch")
	h.now = h.now.Add(5 * time.Second)
	h.s.lastEvent = h.now
	h.s.st.Pet.Species, h.s.st.Pet.XP = "gotinha", xpFor(5) // gosma, a form whose mouth shows
	h.s.talk.cur = nil
	h.s.say("pet")
	h.s.render()
	if !h.s.talk.talking(h.now) {
		t.Fatal("the pet is not talking")
	}
	h.s.frame = 2 // an odd pose, where the talking mouth is open
	talk := h.s.stageRows(h.now)
	h.s.talk.cur = nil
	if reflect.DeepEqual(talk, h.s.stageRows(h.now)) {
		t.Error("the stage does not change while it talks")
	}
}

func TestDexShape(t *testing.T) {
	if len(dex) != 25 {
		t.Fatalf("%d lines, want 25", len(dex))
	}
	forms40, perRarity := 0, map[Rarity]int{}
	for _, l := range dex {
		perRarity[l.Rarity]++
		var lv []int
		for _, f := range l.Forms {
			forms40++
			lv = append(lv, f.Level)
			if forms[f.ID] == nil {
				t.Errorf("form %s has no art", f.ID)
			}
		}
		want := map[int][]int{1: {1}, 2: {1, 10}, 3: {1, 5, 15}}[len(l.Forms)]
		if !reflect.DeepEqual(lv, want) {
			t.Errorf("%s: levels %v, want %v", l.Forms[0].ID, lv, want)
		}
	}
	if forms40 != 40 {
		t.Errorf("%d forms, want 40", forms40)
	}
	for r, want := range []int{7, 6, 4, 4, 4} {
		if perRarity[Rarity(r)] != want {
			t.Errorf("%s: %d lines, want %d", rarities[r].name, perRarity[Rarity(r)], want)
		}
	}
}

func TestSpriteFallsBackToTheEgg(t *testing.T) {
	if sprite("nope", "idle", 0, false, 0) != sprite("ovo", "idle", 0, false, 0) {
		t.Error("an unknown form does not draw the egg")
	}
}

func TestLoadMigratesOldPets(t *testing.T) {
	dir := t.TempDir()
	body := `{"pet":{"name":"A","species":"tatu","xp":300},"past":[{"name":"B","species":"boitata"},{"name":"C","species":"zzz-unknown"}],"eggs":1,"seen":{"tatu":false,"mico":true,"zzz":true}}`
	if err := os.WriteFile(filepath.Join(dir, stateFile), []byte(body), 0o600); err != nil {
		t.Fatal(err)
	}
	h := start(t, dir)
	st := h.s.st
	if st.Pet.Species != "gotinha" || formOf(st.Pet).ID != "gotinha" {
		t.Errorf("pet: %q, form %q", st.Pet.Species, formOf(st.Pet).ID)
	}
	if len(st.Past) != 1 || st.Past[0].Species != "jormungandr" {
		t.Errorf("past: %+v", st.Past)
	}
	if want := map[string]bool{"gotinha": false, "fenix": true, "jormungandr": false}; !reflect.DeepEqual(st.Seen, want) {
		t.Errorf("seen: %v, want %v", st.Seen, want)
	}

	dir = t.TempDir()
	body = `{"pet":{"name":"A","species":"tatu","xp":5000,"shiny":true},"eggs":0,"seen":{}}`
	if err := os.WriteFile(filepath.Join(dir, stateFile), []byte(body), 0o600); err != nil {
		t.Fatal(err)
	}
	st = start(t, dir).s.st
	if f := formOf(st.Pet); f.ID != "gosmao" || st.Seen["gosmao"] != true || len(st.Seen) != 1 {
		t.Errorf("grown pet: form %q, seen %v", f.ID, st.Seen)
	}
}

func TestEvolvesAtTheLineLevels(t *testing.T) {
	for _, c := range []struct {
		id      string
		lv      int
		to      string
		evolves bool
	}{
		{"gotinha", 5, "gosma", true},
		{"gotinha", 15, "gosmao", true},
		{"grifinho", 5, "grifinho", false},
		{"grifinho", 10, "grifo", true},
		{"drone", 15, "drone", false},
	} {
		st := newState()
		st.Pet = &Pet{Species: c.id, XP: xpFor(c.lv) - 10}
		_, evolved, _ := st.TurnEnded("end_turn")
		_, seen := st.Seen[c.to]
		if evolved != c.evolves || formOf(st.Pet).ID != c.to || seen != c.evolves {
			t.Errorf("%s at Lv %d: evolved %v, form %s, seen %v", c.id, c.lv, evolved, formOf(st.Pet).ID, st.Seen)
		}
	}
	st := newState()
	st.Pet = &Pet{Species: "gotinha", Shiny: true, XP: xpFor(5) - 10}
	st.TurnEnded("end_turn")
	if !st.Seen["gosma"] {
		t.Errorf("a shiny evolving keeps its flag: %v", st.Seen)
	}
}

func TestEvolveSpeaks(t *testing.T) {
	h := start(t, t.TempDir())
	id, _, _ := bubbleOf(h.action("hatch"))
	h.bubbleDone(id)
	h.now = h.now.Add(300 * time.Millisecond)
	h.s.st.Pet.Species, h.s.st.Pet.XP = "gotinha", xpFor(5)-10
	_, text, ok := bubbleOf(h.send(`{"jsonrpc":"2.0","method":"event/turn_ended","params":{"agent":"claude","session":"s1","reason":"end_turn"}}`))
	if !ok || text != fallback["evolve"][0] {
		t.Errorf("bubble %q %v", text, ok)
	}
	found := false
	for _, n := range h.s.st.Diary {
		found = found || n.Text == "evolved into Gosma"
	}
	if !found || h.s.flash.mood != "happy" {
		t.Errorf("diary %v, mood %q", h.s.st.Diary, h.s.flash.mood)
	}
}

func TestCardShowsTheNextEvolution(t *testing.T) {
	h := start(t, t.TempDir())
	h.action("hatch")
	for id, want := range map[string]bool{"gosma": true, "gosmao": false} {
		h.s.st.Pet.Species, h.s.st.Pet.XP = "gotinha", xpFor(map[string]int{"gosma": 5, "gosmao": 15}[id])
		b, _ := json.Marshal(h.s.cardTree(h.now))
		if got := strings.Contains(string(b), "evolves at Lv"); got != want {
			t.Errorf("%s card evolves text: %v", id, got)
		}
	}
}

func TestWalkStaysOnStage(t *testing.T) {
	now := time.Date(2026, 11, 16, 12, 0, 0, 0, time.UTC)
	s := newServer(&bytes.Buffer{}, func() time.Time { return now }, seeded())
	s.ready = true
	s.lastEvent = now
	s.st.Pet = &Pet{Species: "gotinha", FedAt: now, Love: 100, LovedAt: now}
	left, right := false, false
	for i := range 20000 {
		if i%500 == 0 {
			// hungry on odd turns of 500 steps: a day without food, then fed again
			if (i/500)%2 == 1 {
				s.st.Pet.FedAt = now.Add(-24 * time.Hour)
			} else {
				s.st.Pet.Feed(now)
			}
		}
		before, idle := s.walk.x, s.mood(now) == "idle"
		s.tick()
		if x := s.walk.x; x < 0 || x > stageW-W {
			t.Fatalf("step %d: x = %d", i, x)
		} else if x != before && !idle {
			t.Fatalf("step %d: moved while %q", i, s.mood(now))
		} else {
			left, right = left || x == 0, right || x == stageW-W
		}
	}
	if !left || !right {
		t.Errorf("edges reached: left %v right %v", left, right)
	}
}

func TestDockTree(t *testing.T) {
	h := start(t, t.TempDir())
	kids := h.s.dockTree(h.now)["children"].([]node)
	if len(kids) != 2 || kids[0]["children"].([]node)[0]["text"] != "Egg" {
		t.Fatalf("no pet: %v", kids)
	}
	egg := stage(sprite("ovo", "egg", 0, false, 0), h.s.walk.x, false)
	if !reflect.DeepEqual(kids[1]["rows"], egg) {
		t.Error("the stage shows the egg")
	}
	h.action("hatch")
	kids = h.s.dockTree(h.now)["children"].([]node)
	if kids[0]["type"] != "bubble" {
		t.Fatalf("the hatch bubble comes first: %v", kids[0])
	}
	row, px := kids[1], kids[2]
	if c := row["children"].([]node); row["type"] != "row" || c[0]["text"] != h.s.st.Pet.Title() || c[1]["type"] != "spacer" || c[2]["type"] != "menu" {
		t.Errorf("name row: %v", row)
	}
	rows := px["rows"].([]string)
	if px["type"] != "pixels" || px["id"] != "pet" || px["scale"] != 2 || len(rows) != stageH || len(rows[0]) != stageW {
		t.Errorf("stage: %v", px)
	}
}

func TestStageFlipsWhenFacingLeft(t *testing.T) {
	g := sprite("gotinha", "idle", 0, false, 0)
	sp, flipped := g.rows(), stage(g, 10, true)
	for j := range stageH {
		row := []byte(sp[j])
		slices.Reverse(row)
		if got := flipped[j][10 : 10+W]; got != string(row) {
			t.Fatalf("row %d: %q, want %q", j, got, row)
		}
	}
}

func TestWorkingAndAlertGlyphs(t *testing.T) {
	h := start(t, t.TempDir())
	h.action("hatch")
	h.now = h.now.Add(time.Minute)
	h.s.lastEvent = h.now
	rows := func() any { return h.s.stageRows(h.now) }
	idle := rows()
	h.s.focused = "a\x00s"
	h.s.states[h.s.focused] = "working"
	if reflect.DeepEqual(rows(), idle) {
		t.Error("working looks idle")
	}
	h.s.states[h.s.focused] = "idle"
	h.s.states["b\x00s"] = "needs_you"
	alert := rows()
	h.s.states["b\x00s"] = "idle"
	h.s.flash.mood, h.s.flash.until = "talk", h.now.Add(time.Hour)
	if reflect.DeepEqual(alert, rows()) {
		t.Error("alert looks like plain talk")
	}
}

// stageRows is the pixels node's rows: the dock's last child.
func (s *server) stageRows(now time.Time) []string {
	kids := s.dockTree(now)["children"].([]node)
	return kids[len(kids)-1]["rows"].([]string)
}

func hatched(t *testing.T, st *State, at time.Time) *Pet {
	st.Eggs = 1
	return st.Hatch(seeded(), at)
}

func TestUseSwapsAndFreezes(t *testing.T) {
	t0 := time.Date(2026, 11, 1, 12, 0, 0, 0, time.UTC)
	st := newState()
	a := *hatched(t, &st, t0)
	hatched(t, &st, t0.Add(time.Hour))
	b := st.Pet.Born
	if st.Past[0].StoredAt != t0.Add(time.Hour) {
		t.Fatalf("stored at %v", st.Past[0].StoredAt)
	}
	hunger, love := a.Hunger(t0.Add(time.Hour)), a.Affection(t0.Add(time.Hour))
	st.Lines = map[string][]string{"x": {"y"}}
	now := t0.Add(5 * 24 * time.Hour)
	if !st.Use(a.Born.UnixNano(), now) {
		t.Fatal("Use said false")
	}
	if st.Pet.Born != a.Born || !st.Pet.StoredAt.IsZero() {
		t.Errorf("pet: %+v", st.Pet)
	}
	if math.Abs(st.Pet.Hunger(now)-hunger) > 0.01 || math.Abs(st.Pet.Affection(now)-love) > 0.01 {
		t.Errorf("hunger %.2f love %.2f, want %.2f %.2f", st.Pet.Hunger(now), st.Pet.Affection(now), hunger, love)
	}
	if len(st.Past) != 1 || st.Past[0].Born != b || st.Past[0].StoredAt != now {
		t.Errorf("past: %+v", st.Past)
	}
	if st.Lines != nil {
		t.Error("lines kept")
	}
}

func TestUseWithoutStoredAt(t *testing.T) {
	t0 := time.Date(2026, 11, 1, 12, 0, 0, 0, time.UTC)
	old := Pet{Name: "A", Species: "gotinha", Born: t0, FedAt: t0, Love: 50, LovedAt: t0.Add(time.Hour)}
	st := State{Pet: &Pet{Name: "B", Species: "bit", Born: t0.Add(time.Minute)}, Past: []Pet{old}}
	now := t0.Add(72 * time.Hour)
	if !st.Use(old.Born.UnixNano(), now) {
		t.Fatal("Use said false")
	}
	at := t0.Add(time.Hour)
	if got, want := st.Pet.Hunger(now), old.Hunger(at); math.Abs(got-want) > 0.01 {
		t.Errorf("hunger %.2f, want %.2f", got, want)
	}
	if got, want := st.Pet.Affection(now), old.Affection(at); math.Abs(got-want) > 0.01 {
		t.Errorf("love %.2f, want %.2f", got, want)
	}
}

func TestUseIgnoresBadIDs(t *testing.T) {
	h := start(t, t.TempDir())
	h.action("hatch")
	h.s.st.Eggs = 1
	h.now = h.now.Add(time.Hour)
	h.action("hatch")
	snap := func() string { b, _ := json.Marshal(h.s.st); return string(b) }
	before := snap()
	for _, id := range []string{"use-abc", "use-", "use-123", "use-" + fmt.Sprint(h.s.st.Pet.Born.UnixNano())} {
		if msgs := h.action(id); len(msgs) == 0 {
			t.Errorf("%s: no answer", id)
		}
		var a, b State
		json.Unmarshal([]byte(before), &a)
		json.Unmarshal([]byte(snap()), &b)
		if !reflect.DeepEqual(a, b) {
			t.Errorf("%s changed the state", id)
		}
	}
}

func TestHatchKeepsEveryPet(t *testing.T) {
	st := newState()
	for i := range 30 {
		hatched(t, &st, time.Unix(int64(1000+i), 0))
	}
	if len(st.Past) != 29 {
		t.Fatalf("past: %d", len(st.Past))
	}
	for _, q := range st.Past {
		if q.StoredAt.IsZero() {
			t.Error("stored pet without StoredAt")
		}
	}
}

func TestPetsTable(t *testing.T) {
	h := start(t, t.TempDir())
	if strings.Contains(quote(fmt.Sprint(h.s.menu())), "switch") {
		t.Error("switch without past pets")
	}
	h.action("hatch")
	h.s.st.Eggs = 1
	h.now = h.now.Add(time.Hour)
	h.action("hatch")
	old := h.s.st.Past[0]
	b, _ := json.Marshal(h.s.cardTree(h.now))
	for _, want := range []string{`"id":"s-past"`, `"title":"Your pets"`, `{"title":"Pet"},{"title":"Form"},{"title":"Lv","width":40},{"title":"","width":64}`,
		fmt.Sprintf(`{"id":"use-%d","label":"Use","type":"button"}`, old.Born.UnixNano())} {
		if !strings.Contains(string(b), want) {
			t.Errorf("card lacks %s\n%s", want, b)
		}
	}
	mb, _ := json.Marshal(h.s.menu())
	if !strings.Contains(string(mb), `{"id":"switch","label":"Switch pet"}`) {
		t.Errorf("menu: %s", mb)
	}
	h.s.st.Eggs = 1
	want := h.s.st.Pet.Name + " goes to your pets. You can switch back any time."
	if hb, _ := json.Marshal(h.s.hatchItem("x")); !strings.Contains(string(hb), want) {
		t.Errorf("confirm: %s", hb)
	}
}

func TestLoadMigratesReplacedLines(t *testing.T) {
	dir := t.TempDir()
	body := `{"pet":{"name":"A","species":"verme","xp":5000,"shiny":true},` +
		`"past":[{"name":"B","species":"ferrugem"},{"name":"C","species":"sleipnir"},{"name":"D","species":"tarrasque"},{"name":"E","species":"singularidade"},{"name":"F","species":"curupira"}],` +
		`"eggs":0,"seen":{"verme":true,"ferrugem":false,"singularidade":false}}`
	if err := os.WriteFile(filepath.Join(dir, stateFile), []byte(body), 0o600); err != nil {
		t.Fatal(err)
	}
	st := start(t, dir).s.st
	if st.Pet == nil || st.Pet.Species != "beholder" {
		t.Fatalf("pet: %+v", st.Pet)
	}
	var past []string
	for _, p := range st.Past {
		past = append(past, p.Species)
	}
	if want := []string{"kobold", "fada", "tiamat", "goblin", "fada"}; !reflect.DeepEqual(past, want) {
		t.Errorf("past: %v, want %v", past, want)
	}
	if want := map[string]bool{"beholder": true, "kobold": false, "goblin": false, "fada": false, "tiamat": false}; !reflect.DeepEqual(st.Seen, want) {
		t.Errorf("seen: %v, want %v", st.Seen, want)
	}
}

func pull(i int) string { return fmt.Sprintf("https://github.com/o/r/pull/%d", i) }

// prsEvent is wai-waycarbon's prs.mine with one PR per reason, pull/1 first.
func prsEvent(reasons ...string) string {
	var list []string
	for i, r := range reasons {
		list = append(list, fmt.Sprintf(`{"url":%q,"reason":%q}`, pull(i+1), r))
	}
	return `{"jsonrpc":"2.0","method":"event/ext","params":{"source":"wai-waycarbon","name":"prs.mine","data":{"prs":[` + strings.Join(list, ",") + `]}}}`
}

// urlOf is the url of the bubble in the last dock render.
func urlOf(msgs []map[string]any) string {
	url := ""
	for _, m := range msgs {
		p, _ := m["params"].(map[string]any)
		if m["method"] != "panel/render" || p["panel"] != petPanel {
			continue
		}
		url = ""
		for _, k := range p["tree"].(map[string]any)["children"].([]any) {
			if n := k.(map[string]any); n["type"] == "bubble" {
				url, _ = n["url"].(string)
			}
		}
	}
	return url
}

func withHatched(t *testing.T) *harness {
	h := start(t, t.TempDir())
	id, _, _ := bubbleOf(h.action("hatch"))
	h.bubbleDone(id)
	h.now = h.now.Add(time.Minute)
	return h
}

func TestPRBubbleOpensItsPR(t *testing.T) {
	h := withHatched(t)
	msgs := h.send(prsEvent("waiting", "ci_failed"))
	id, text, _ := bubbleOf(msgs)
	if text != fallback["pr_fix"][0] || !sticky(msgs) || urlOf(msgs) != pull(2) {
		t.Fatalf("the PR to fix, sticky: %q %v %q", text, sticky(msgs), urlOf(msgs))
	}
	h.bubbleDone(id)
	h.now = h.now.Add(time.Minute)
	msgs = h.send(prsEvent("approved", "ci_failed"))
	if _, text, _ := bubbleOf(msgs); text != fallback["pr_approved"][0] || !sticky(msgs) || urlOf(msgs) != pull(1) {
		t.Fatalf("the approved PR, sticky: %q %v %q", text, sticky(msgs), urlOf(msgs))
	}
}

func TestUrgentStepsInFrontOfStickyPR(t *testing.T) {
	h := withHatched(t)
	h.send(prsEvent("ci_failed"))
	msgs := h.send(`{"jsonrpc":"2.0","method":"event/session_state","params":{"agent":"claude","session":"s2","state":"needs_you"}}`)
	if _, text, _ := bubbleOf(msgs); text != fallback["needs_you"][0] {
		t.Fatalf("needs_you waits behind a PR: %q", text)
	}
	msgs = h.send(`{"jsonrpc":"2.0","method":"event/session_state","params":{"agent":"claude","session":"s2","state":"idle"}}`)
	if _, text, _ := bubbleOf(msgs); text != fallback["pr_fix"][0] || urlOf(msgs) != pull(1) {
		t.Fatalf("the PR comes back: %q %q", text, urlOf(msgs))
	}
}

func TestPRBubbleLeavesWithItsPR(t *testing.T) {
	h := withHatched(t)
	h.send(prsEvent("ci_failed"))
	if h.s.talk.cur == nil || h.s.talk.cur.URL != pull(1) {
		t.Fatalf("PR bubble: %v", h.s.talk.cur)
	}
	h.send(prsEvent("waiting"))
	if h.s.talk.cur != nil || len(h.s.talk.queue) > 0 {
		t.Fatalf("a fixed PR's bubble stays: %v %v", h.s.talk.cur, h.s.talk.queue)
	}
	h.now = h.now.Add(time.Minute)
	h.send(prsEvent("approved"))
	if h.s.talk.cur == nil || h.s.talk.cur.Kind != "pr_approved" {
		t.Fatalf("approved bubble: %v", h.s.talk.cur)
	}
	h.send(prsEvent())
	if h.s.talk.cur != nil || len(h.s.talk.queue) > 0 {
		t.Fatalf("a merged PR's bubble stays: %v %v", h.s.talk.cur, h.s.talk.queue)
	}
}
