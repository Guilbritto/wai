package main

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"
)

func petWith(stats [4]int, xp int) *Pet {
	return &Pet{Name: "Bolota", Species: "tatu", Born: t0, XP: xp, Stats: stats}
}

func TestPersonaFollowsStats(t *testing.T) {
	p := persona(petWith([4]int{9, 2, 8, 5}, 0))
	for _, want := range []string{"terse", "dramatic", "mischievous"} {
		if !strings.Contains(p, want) {
			t.Errorf("persona lacks %q:\n%s", want, p)
		}
	}
	for _, no := range []string{"affectionate", "grumpy", "distracted", "soothing", "orderly"} {
		if strings.Contains(p, no) {
			t.Errorf("persona has %q:\n%s", no, p)
		}
	}
	if !strings.Contains(p, "baby talk") || strings.Contains(p, "diary") {
		t.Errorf("level 1:\n%s", p)
	}
	if p6 := persona(petWith([4]int{5, 5, 5, 5}, 625)); !strings.Contains(p6, "diary") || strings.Contains(p6, "baby talk") {
		t.Errorf("level 6:\n%s", p6)
	}
	leg := petWith([4]int{5, 5, 5, 5}, 0)
	leg.Species = "boitata"
	if !strings.Contains(persona(leg), "legend") || strings.Contains(persona(leg), "shine") {
		t.Errorf("legendary:\n%s", persona(leg))
	}
	leg.Shiny = true
	if !strings.Contains(persona(leg), "shine") {
		t.Errorf("shiny:\n%s", persona(leg))
	}
}

func TestRequestHasNoWorkData(t *testing.T) {
	h := start(t, t.TempDir())
	h.s.prs = map[string]string{"https://github.com/secret/repo/pull/7": "approved"}
	h.s.states["claude\x00session-secret"] = "working"
	diary := []Note{{At: t0, Text: "reached level 3"}}
	r := request(petWith([4]int{1, 2, 3, 4}, 100), "idle", diary)
	for _, want := range []string{"Bolota", "level 3", "reached level 3", "pet", "needs_you"} {
		if !strings.Contains(r, want) {
			t.Errorf("request lacks %q:\n%s", want, r)
		}
	}
	for _, no := range []string{"secret", "github", "session-"} {
		if strings.Contains(r, no) {
			t.Errorf("request has %q:\n%s", no, r)
		}
	}
}

func TestParseLines(t *testing.T) {
	long := strings.Repeat("x", 81)
	got, err := parseLines([]byte(`{"structured_output": {"pet": ["a", ""], "feed": ["` + long + `"]}}`))
	if err != nil || !reflect.DeepEqual(got, map[string][]string{"pet": {"a"}}) {
		t.Errorf("structured_output: %v %v", got, err)
	}
	got, err = parseLines([]byte(`{"result": "{\"pet\":[\"b\"],\"nope\":[\"c\"]}"}`))
	if err != nil || !reflect.DeepEqual(got, map[string][]string{"pet": {"b"}}) {
		t.Errorf("result: %v %v", got, err)
	}
}

func allKinds() map[string][]string {
	m := map[string][]string{}
	for _, k := range kinds {
		m[k.Kind] = []string{"da IA " + k.Kind}
	}
	return m
}

// withPet starts a harness with a hatched pet and a runner stub that counts its calls.
func withPet(t *testing.T, out string) (*harness, *int) {
	h := start(t, t.TempDir())
	h.s.st.Hatch(seeded(), h.now)
	calls := new(int)
	h.s.run = func(ctx context.Context, system, prompt, schema string) ([]byte, error) {
		*calls++
		return []byte(out), nil
	}
	return h, calls
}

func TestBadClaudeOutputKeepsFallback(t *testing.T) {
	h, calls := withPet(t, `{"type":"result","is_error":true,"result":"Not logged in"}`)
	s := h.s
	s.maybeGenerate(h.now)
	s.gotPools(<-s.gen, h.now)
	if *calls != 1 || len(s.st.Lines) != 0 || !s.genFailed.Equal(h.now) || s.genBusy {
		t.Fatalf("calls %d lines %v failed %v busy %v", *calls, s.st.Lines, s.genFailed, s.genBusy)
	}
	if got := s.pick("pet"); got != fallback["pet"][0] {
		t.Errorf("pick = %q", got)
	}
	s.maybeGenerate(h.now.Add(10 * time.Minute))
	if *calls != 1 || s.genBusy {
		t.Errorf("ran again inside the backoff: %d", *calls)
	}
	s.maybeGenerate(h.now.Add(31 * time.Minute))
	if !s.genBusy {
		t.Error("did not retry after the backoff")
	}
	<-s.gen
}

func TestPoolsRefreshOnLevelAndAge(t *testing.T) {
	b, _ := json.Marshal(map[string]any{"structured_output": allKinds()})
	h, calls := withPet(t, string(b))
	s := h.s
	s.maybeGenerate(h.now)
	s.maybeGenerate(h.now) // busy
	s.gotPools(<-s.gen, h.now)
	if *calls != 1 {
		t.Fatalf("calls %d", *calls)
	}
	if len(s.st.Lines) != len(kinds) || !s.st.LinesBorn.Equal(s.st.Pet.Born) || s.st.LinesLevel != 1 {
		t.Fatalf("lines %d born %v level %d", len(s.st.Lines), s.st.LinesBorn, s.st.LinesLevel)
	}
	if got := s.pick("pet"); got != "da IA pet" {
		t.Errorf("pick = %q", got)
	}
	s.maybeGenerate(h.now.Add(time.Hour))
	if s.genBusy {
		t.Fatal("ran with fresh pools")
	}
	s.st.Pet.XP = 25 // level 2
	s.maybeGenerate(h.now.Add(time.Hour))
	if !s.genBusy {
		t.Fatal("no run after a level up")
	}
	s.gotPools(<-s.gen, h.now.Add(time.Hour))
	s.maybeGenerate(h.now.Add(2 * time.Hour))
	if s.genBusy {
		t.Fatal("ran with fresh pools after the level up")
	}
	s.maybeGenerate(h.now.Add(8 * time.Hour))
	if !s.genBusy {
		t.Fatal("no run after the pools aged")
	}
	<-s.gen
}

func TestPoolsForAnotherPetAreDropped(t *testing.T) {
	b, _ := json.Marshal(map[string]any{"structured_output": allKinds()})
	h, _ := withPet(t, string(b))
	s := h.s
	s.maybeGenerate(h.now)
	s.st.Eggs = 1
	s.st.Hatch(seeded(), h.now.Add(time.Minute))
	s.gotPools(<-s.gen, h.now)
	if len(s.st.Lines) != 0 || s.genBusy {
		t.Errorf("lines %v busy %v", s.st.Lines, s.genBusy)
	}
}

func TestHatchKeepsDiaryClearsLines(t *testing.T) {
	st := newState()
	st.Hatch(seeded(), t0)
	st.Lines = allKinds()
	st.Diary = []Note{{At: t0, Text: "x"}}
	st.Eggs = 1
	st.Hatch(seeded(), t0.Add(time.Hour))
	if st.Lines != nil || len(st.Diary) != 1 {
		t.Errorf("lines %v diary %v", st.Lines, st.Diary)
	}
}

func TestOldStateLoads(t *testing.T) {
	for _, body := range []string{
		`{"pet":{"name":"Bolota","species":"tatu","born":"2026-11-01T00:00:00Z"},"eggs":1,"turns":3,"seen":{"tatu":false}}`,
		`{"pet":{"name":"Bolota","species":"tatu","born":"2026-11-01T00:00:00Z"},"eggs":1,"turns":3,"seen":{},"lines":{"unknown_kind":["x"]}}`,
	} {
		dir := t.TempDir()
		os.WriteFile(filepath.Join(dir, stateFile), []byte(body), 0o600)
		h := start(t, dir)
		if h.s.st.Pet == nil {
			t.Fatalf("pet lost: %s", body)
		}
		for _, k := range kinds {
			if h.s.pick(k.Kind) == "" {
				t.Errorf("no line for %s", k.Kind)
			}
		}
	}
}

func TestDiaryCapped(t *testing.T) {
	h := start(t, t.TempDir())
	for i := range 40 {
		h.s.note(strings.Repeat("n", i+1))
	}
	d := h.s.st.Diary
	if len(d) != maxDiary || len(d[0].Text) != 11 || len(d[len(d)-1].Text) != 40 {
		t.Errorf("diary %d first %d", len(d), len(d[0].Text))
	}
}

func TestEventsWriteDiary(t *testing.T) {
	h := start(t, t.TempDir())
	h.send(`{"jsonrpc":"2.0","id":1,"method":"panel/action","params":{"panel":"pet","action_id":"hatch","values":{}}}`)
	if d := h.s.st.Diary; len(d) != 1 || !strings.HasPrefix(d[0].Text, "hatched "+h.s.st.Pet.Name+", a ") {
		t.Fatalf("diary %v", d)
	}
	h.s.st.Pet.XP = 20
	h.send(`{"jsonrpc":"2.0","method":"event/turn_ended","params":{"reason":"end_turn"}}`)
	if d := h.s.st.Diary; d[len(d)-1].Text != "reached level 2" {
		t.Errorf("diary %v", d)
	}
}
