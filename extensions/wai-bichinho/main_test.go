package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"math"
	"math/rand/v2"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
	"unicode/utf8"
)

func seeded() *rand.Rand { return rand.New(rand.NewPCG(1, 2)) }

func TestEggsFollowTheRarityOdds(t *testing.T) {
	const n = 200_000
	rng := seeded()
	st := newState()
	tiers := map[Rarity]int{}
	shiny := 0
	for range n {
		st.Eggs = 1
		p := st.Hatch(rng, time.Now())
		tiers[speciesByID(p.Species).Rarity]++
		if p.Shiny {
			shiny++
		}
		for _, v := range p.Stats {
			if v < rarities[speciesByID(p.Species).Rarity].floor || v > 10 {
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
	if len(st.Seen) != len(species) {
		t.Errorf("hatched %d of %d species", len(st.Seen), len(species))
	}
	if len(st.Past) != maxPast {
		t.Errorf("past pets: %d, want the last %d", len(st.Past), maxPast)
	}
}

// 15 characters of text_sm monospace fit the 144 px inside a 160 px slot.
func TestArtFitsTheSlot(t *testing.T) {
	pets := []*Pet{nil}
	for _, sp := range species {
		pets = append(pets, &Pet{Species: sp.ID, Shiny: true})
	}
	for _, p := range pets {
		for m := range moods {
			for frame := range 16 {
				art := draw(p, m, frame)
				lines := strings.Split(art, "\n")
				if len(lines) != 3 {
					t.Fatalf("%v %s: %d lines", p, m, len(lines))
				}
				for _, l := range lines {
					if n := utf8.RuneCountInString(l); n > 15 {
						t.Errorf("%v %s frame %d: %q is %d wide", p, m, frame, l, n)
					}
				}
				if p != nil && strings.ContainsAny(art, "EM") {
					t.Errorf("%s %s: placeholder left in %q", p.Species, m, art)
				}
			}
		}
	}
}

// The ⋯ menu's row does not change while the pet animates, so the menu never wraps under it.
func TestMenuStaysPutWhileAnimating(t *testing.T) {
	now := time.Now()
	s := newServer(&bytes.Buffer{}, func() time.Time { return now }, seeded())
	s.st.Pet = &Pet{Species: "boitata", Shiny: true, FedAt: now, Love: 100, LovedAt: now}
	s.focused = "a\x00s"
	s.states[s.focused] = "working"
	var first string
	for s.frame = range 16 {
		var row string
		for _, kid := range s.promptTree(now)["children"].([]node) {
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
		if _, egg := st.TurnEnded("end_turn"); egg {
			eggs++
		}
	}
	if eggs != maxEggs || st.Eggs != maxEggs {
		t.Errorf("eggs: %d given, %d held; want %d", eggs, st.Eggs, maxEggs)
	}
	st.Hatch(seeded(), t0)
	up, _ := st.TurnEnded("end_turn")
	up2, _ := st.TurnEnded("end_turn")
	up3, _ := st.TurnEnded("end_turn")
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

// promptOf is the prompt panel's badge (first row), art and word, from the last render.
func promptOf(t *testing.T, msgs []map[string]any) (art, badge, word string) {
	t.Helper()
	for _, m := range msgs {
		p, _ := m["params"].(map[string]any)
		if m["method"] != "panel/render" || p["panel"] != promptPanel {
			continue
		}
		for _, k := range p["tree"].(map[string]any)["children"].([]any) {
			n := k.(map[string]any)
			switch {
			case n["type"] == "row":
				badge = n["children"].([]any)[0].(map[string]any)["text"].(string)
			case n["type"] == "text" && n["mono"] == true:
				art = n["text"].(string)
			case n["type"] == "text" && n["muted"] == true:
				word = n["text"].(string)
			}
		}
	}
	if art == "" {
		t.Fatalf("no prompt render in %v", msgs)
	}
	return
}

func TestProtocol(t *testing.T) {
	dir := t.TempDir()
	h := start(t, dir)
	if _, badge, word := promptOf(t, h.init); badge != "egg" || word != "hatch me" {
		t.Errorf("first run: %q %q", badge, word)
	}
	msgs := h.send(`{"jsonrpc":"2.0","method":"event/file_saved","params":{"path":"/x"}}`)
	if len(msgs) != 0 {
		t.Fatalf("an unknown notification is ignored: %v", msgs)
	}

	msgs = h.send(`{"jsonrpc":"2.0","id":7,"method":"panel/action","params":{"panel":"pet","action_id":"hatch","values":{}}}`)
	if msgs[0]["id"] != 7.0 {
		t.Fatalf("panel/action answered first: %v", msgs[0])
	}
	_, badge, word := promptOf(t, msgs)
	if badge != "Lv 1" || word != "hi!" {
		t.Errorf("after hatching: %q %q", badge, word)
	}
	var saved State
	b, _ := os.ReadFile(filepath.Join(dir, stateFile))
	if json.Unmarshal(b, &saved); saved.Pet == nil || saved.Eggs != 0 {
		t.Fatalf("hatch not saved: %s", b)
	}
	name := saved.Pet.Name

	_, _, word = promptOf(t, h.send(`{"jsonrpc":"2.0","method":"event/turn_ended","params":{"agent":"claude","session":"s1","reason":"end_turn"}}`))
	if word != "+10 xp" {
		t.Errorf("after a turn: %q", word)
	}

	h.now = h.now.Add(10 * time.Second)
	h.send(`{"jsonrpc":"2.0","method":"event/session_focused","params":{"agent":"claude","session":"s1"}}`)
	art, _, word := promptOf(t, h.send(`{"jsonrpc":"2.0","method":"event/session_state","params":{"agent":"claude","session":"s1","state":"working"}}`))
	if word != "working" {
		t.Errorf("while the focused session works: %q", word)
	}
	h.s.tick()
	if moved, _, _ := promptOf(t, h.send(`{"jsonrpc":"2.0","method":"event/file_saved","params":{"path":"/x"}}`)); moved == art {
		t.Error("a frame does not bob the working pet")
	}
	_, _, word = promptOf(t, h.send(`{"jsonrpc":"2.0","method":"event/session_state","params":{"agent":"claude","session":"s2","state":"needs_you"}}`))
	if word != "needs you" {
		t.Errorf("another session needs you: %q", word)
	}
	// The session list drops closed sessions.
	_, _, word = promptOf(t, h.send(`{"jsonrpc":"2.0","id":1,"result":[{"agent":"claude","session":"s1","title":null,"state":"idle"}]}`))
	if word != name {
		t.Errorf("idle pet shows its name %q: %q", name, word)
	}

	h.now = h.now.Add(7 * time.Hour)
	_, _, word = promptOf(t, h.send(`{"jsonrpc":"2.0","id":8,"method":"panel/action","params":{"panel":"pet","action_id":"rename","values":{"name":"  Zezinho  "}}}`))
	if word != "hungry" {
		t.Errorf("after 7 hours: %q", word)
	}
	_, _, word = promptOf(t, h.send(`{"jsonrpc":"2.0","id":9,"method":"panel/action","params":{"panel":"pet","action_id":"feed","values":{}}}`))
	if word != "yum" {
		t.Errorf("feeding: %q", word)
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
	word := func(line string) string {
		t.Helper()
		h.send(line) // a render that would repeat the last tree sends nothing, so read the mood
		_, w := h.s.mood(h.now)
		return w
	}

	if w := word(prs("approved", "waiting")); w != "merge PR" {
		t.Errorf("first event, no flash, an approved PR: %q", w)
	}
	if w := word(prs("approved", "ci_failed")); w != "check PR" {
		t.Errorf("a PR that turns red flashes: %q", w)
	}
	h.now = h.now.Add(time.Minute)
	if w := word(prs("approved", "ci_failed", "conflict")); w != "check PR" {
		t.Errorf("a new PR with a conflict flashes: %q", w)
	}
	h.now = h.now.Add(time.Minute)
	if w := word(prs("approved", "ci_failed", "conflict")); w != "fix 2 PRs" {
		t.Errorf("no change, PRs to fix win over approved: %q", w)
	}
	if w := word(prs("approved", "approved")); w != "approved!" {
		t.Errorf("a PR approved: %q", w)
	}
	h.now = h.now.Add(time.Minute)
	if w := word(prs("approved", "approved")); w != "2 to merge" {
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
	if w := word(prs()); w == "2 to merge" || w == "approved!" {
		t.Errorf("all merged: %q", w)
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
		if m["method"] != "panel/render" || p["panel"] != promptPanel {
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
		if m["method"] == "panel/render" && p["panel"] == promptPanel {
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
	h := start(t, t.TempDir())
	h.send(`{"jsonrpc":"2.0","method":"event/session_focused","params":{"agent":"claude","session":"s1"}}`)
	h.action("hatch")
	h.now = h.now.Add(5 * time.Second)
	h.s.st.Pet.Species = "tatu"
	h.s.talk.cur = nil
	h.s.say("pet")
	frame := func() string {
		h.s.tick()
		art, _, _ := promptOf(t, h.send(`{"jsonrpc":"2.0","method":"event/file_saved","params":{}}`))
		return strings.NewReplacer("·", "*").Replace(art)
	}
	if a, b := frame(), frame(); a == b {
		t.Errorf("the mouth does not move:\n%s\n%s", a, b)
	}
}
