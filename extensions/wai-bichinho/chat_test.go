package main

import (
	"encoding/json"
	"fmt"
	"strings"
	"testing"
	"time"
)

func find(msgs []map[string]any, method string) map[string]any {
	for _, m := range msgs {
		if m["method"] == method {
			return m
		}
	}
	return nil
}

func paramsOf(m map[string]any) map[string]any {
	p, _ := m["params"].(map[string]any)
	return p
}

func (h *harness) reply(call map[string]any, result string) []map[string]any {
	h.t.Helper()
	return h.send(fmt.Sprintf(`{"jsonrpc":"2.0","id":%v,"result":%s}`, call["id"], result))
}

func (h *harness) fail(call map[string]any) []map[string]any {
	h.t.Helper()
	return h.send(fmt.Sprintf(`{"jsonrpc":"2.0","id":%v,"error":{"code":-32000,"message":"No open session"}}`, call["id"]))
}

func (h *harness) mention(text string) []map[string]any {
	h.t.Helper()
	return h.send(`{"jsonrpc":"2.0","id":"m1","method":"mention/message","params":{"id":"pet","text":` + quote(text) + `,"agent":null,"session":null}}`)
}

func (h *harness) ended(session, reason string) []map[string]any {
	h.t.Helper()
	return h.send(`{"jsonrpc":"2.0","method":"event/turn_ended","params":{"agent":"claude","session":"` + session + `","reason":"` + reason + `"}}`)
}

func petted(t *testing.T) *harness {
	h := start(t, t.TempDir())
	h.action("hatch")
	return h
}

// opened is a hatched pet with its session s1 open and waiting for the turn.
func opened(t *testing.T) *harness {
	h := petted(t)
	open := find(h.mention("oi"), "sessions/open")
	h.reply(open, `{"agent":"claude","session":"s1"}`)
	return h
}

func (h *harness) answer(items string) []map[string]any {
	h.t.Helper()
	read := find(h.ended("s1", "end_turn"), "sessions/read")
	if read == nil {
		h.t.Fatal("no sessions/read after the turn")
	}
	return h.reply(read, `{"items":`+items+`,"total":2}`)
}

func TestMentionFollowsThePet(t *testing.T) {
	h := start(t, t.TempDir())
	set := find(h.init, "mentions/set")
	if set == nil || len(paramsOf(set)["names"].([]any)) != 0 {
		t.Fatalf("an egg sends no names: %v", h.init)
	}
	h.action("hatch")
	msgs := h.send(`{"jsonrpc":"2.0","id":8,"method":"panel/action","params":{"panel":"pet","action_id":"rename","values":{"name":"Bolota Jr"}}}`)
	b, _ := json.Marshal(paramsOf(find(msgs, "mentions/set")))
	if string(b) != `{"names":[{"id":"pet","name":"BolotaJr"}]}` {
		t.Errorf("names: %s", b)
	}
	if find(h.send(`{"jsonrpc":"2.0","method":"event/file_saved","params":{}}`), "mentions/set") != nil {
		t.Error("an unchanged name is sent again")
	}
}

func TestFirstMessageOpensABackgroundSession(t *testing.T) {
	h := petted(t)
	msgs := h.mention("oi")
	if msgs[0]["id"] != "m1" || msgs[0]["result"] != nil {
		t.Errorf("the mention is answered null first: %v", msgs[0])
	}
	open := find(msgs, "sessions/open")
	p := paramsOf(open)
	prompt, _ := p["prompt"].(string)
	if p["background"] != true || p["send"] != true || !strings.Contains(prompt, h.s.st.Pet.Name) ||
		!strings.Contains(prompt, "Brazilian Portuguese") || !strings.HasSuffix(prompt, "oi") {
		t.Errorf("open: %v", p)
	}
	if _, text, ok := bubbleOf(msgs); !ok || text != "…" || !sticky(msgs) {
		t.Errorf("waiting bubble %q %v", text, ok)
	}
	h.now = h.now.Add(10 * time.Second)
	if m := h.s.mood(h.now); m != "working" {
		t.Errorf("mood while it thinks: %q", m)
	}
}

func TestAnswerBecomesTheBubble(t *testing.T) {
	h := opened(t)
	xp := h.s.st.Pet.XP
	read := find(h.ended("s1", "end_turn"), "sessions/read")
	if p := paramsOf(read); p["from"] != 0.0 || p["agent"] != "claude" || p["session"] != "s1" {
		t.Fatalf("read: %v", p)
	}
	msgs := h.reply(read, `{"items":[{"kind":"user","text":"oi"},{"kind":"agent","text":"Tudo certo.\n\nDetalhes"}],"total":2}`)
	id, text, _ := bubbleOf(msgs)
	if text != "Tudo certo." || h.s.st.Pet.XP != xp {
		t.Errorf("bubble %q, xp %d want %d", text, h.s.st.Pet.XP, xp)
	}
	msgs = h.send(`{"jsonrpc":"2.0","id":6,"method":"panel/action","params":{"panel":"pet","action_id":"` + id + `","values":{"bubble":"click"}}}`)
	show := find(msgs, "sessions/show")
	if p := paramsOf(show); show == nil || p["agent"] != "claude" || p["session"] != "s1" {
		t.Errorf("a click shows the session: %v", msgs)
	}
}

func TestShortAnswerClickJustCloses(t *testing.T) {
	h := opened(t)
	id, text, _ := bubbleOf(h.answer(`[{"kind":"agent","text":"Oi!"}]`))
	if text != "Oi!" {
		t.Fatalf("bubble %q", text)
	}
	msgs := h.send(`{"jsonrpc":"2.0","id":6,"method":"panel/action","params":{"panel":"pet","action_id":"` + id + `","values":{"bubble":"click"}}}`)
	if find(msgs, "sessions/show") != nil {
		t.Error("a whole answer has nothing to show")
	}
}

func TestLongFirstLineIsCut(t *testing.T) {
	text, cut := firstLine(strings.Repeat("a", 200))
	if r := []rune(text); len(r) != 140 || r[139] != '…' || !cut {
		t.Errorf("%d runes, cut %v", len([]rune(text)), cut)
	}
	if text, cut := firstLine("  Oi  \n"); text != "Oi" || cut {
		t.Errorf("%q %v", text, cut)
	}
}

func TestNoAnswerSaysDone(t *testing.T) {
	h := opened(t)
	id, text, _ := bubbleOf(h.answer(`[{"kind":"tool","text":"ls"}]`))
	if text != fallback["chat_done"][0] {
		t.Fatalf("bubble %q", text)
	}
	msgs := h.send(`{"jsonrpc":"2.0","id":6,"method":"panel/action","params":{"panel":"pet","action_id":"` + id + `","values":{"bubble":"click"}}}`)
	if find(msgs, "sessions/show") == nil {
		t.Error("a click shows the session")
	}
}

func TestBusyQueuesUpToFive(t *testing.T) {
	h := opened(t)
	for i := range 5 {
		h.mention(fmt.Sprintf("m%d", i))
	}
	if _, text, _ := bubbleOf(h.mention("m5")); text != "Calma, um de cada vez." {
		t.Errorf("sixth: %q", text)
	}
	read := find(h.answer(`[{"kind":"agent","text":"ok"}]`), "sessions/read")
	if p := paramsOf(read); read == nil || p["limit"] != 1.0 {
		t.Fatalf("the next message probes the session: %v", read)
	}
	prompt := find(h.reply(read, `{"items":[],"total":4}`), "sessions/prompt")
	if p := paramsOf(prompt); prompt == nil || p["text"] != "m0" {
		t.Fatalf("prompt: %v", prompt)
	}
	read = find(h.ended("s1", "end_turn"), "sessions/read")
	if p := paramsOf(read); p["from"] != 4.0 {
		t.Errorf("answer read: %v", p)
	}
}

func TestClosedSessionOpensANewOne(t *testing.T) {
	h := opened(t)
	h.answer(`[{"kind":"agent","text":"ok"}]`)
	probe := find(h.mention("de novo"), "sessions/read")
	if probe == nil {
		t.Fatal("no probe")
	}
	open := find(h.fail(probe), "sessions/open")
	prompt, _ := paramsOf(open)["prompt"].(string)
	if open == nil || !strings.Contains(prompt, "Brazilian Portuguese") || !strings.HasSuffix(prompt, "de novo") {
		t.Errorf("open: %v", open)
	}
}

func TestOwnSessionIsIgnored(t *testing.T) {
	h := opened(t)
	state := func(session string) []map[string]any {
		return h.send(`{"jsonrpc":"2.0","method":"event/session_state","params":{"agent":"claude","session":"` + session + `","state":"needs_you"}}`)
	}
	state("s1")
	if h.s.talk.cur.Text != "…" || len(h.s.states) != 0 {
		t.Errorf("own session spoke: %q %v", h.s.talk.cur.Text, h.s.states)
	}
	if _, text, _ := bubbleOf(state("s2")); text != fallback["needs_you"][0] {
		t.Errorf("another session: %q", text)
	}
}

func TestNeedsYouOnItsSession(t *testing.T) {
	h := opened(t)
	msgs := h.send(`{"jsonrpc":"2.0","method":"event/permission_requested","params":{"agent":"claude","session":"s1","request":"r1","title":"Run gh pr list"}}`)
	if _, text, _ := bubbleOf(msgs); text != "preciso de você: Run gh pr list" {
		t.Fatalf("bubble %q", text)
	}
	msgs = h.send(`{"jsonrpc":"2.0","method":"event/permission_resolved","params":{"agent":"claude","session":"s1","request":"r1"}}`)
	if _, text, _ := bubbleOf(msgs); text != "…" {
		t.Errorf("after resolving: %q", text)
	}
}

func TestFailedTurnSaysError(t *testing.T) {
	h := opened(t)
	h.mention("segunda")
	msgs := h.ended("s1", "cancelled")
	if _, text, _ := bubbleOf(msgs); text != fallback["chat_error"][0] {
		t.Errorf("cancelled: %q", text)
	}
	probe := find(msgs, "sessions/read")
	if probe == nil || paramsOf(probe)["limit"] != 1.0 {
		t.Fatalf("the next message is sent: %v", msgs)
	}

	h = petted(t)
	open := find(h.mention("oi"), "sessions/open")
	msgs = h.fail(open)
	if _, text, _ := bubbleOf(msgs); text != fallback["chat_error"][0] {
		t.Errorf("open error: %q", text)
	}
	if h.s.chat.busy {
		t.Error("still busy after an error")
	}
}

func TestShowRetriesOnce(t *testing.T) {
	h := opened(t)
	id, _, _ := bubbleOf(h.answer(`[{"kind":"tool","text":"x"}]`))
	click := `{"jsonrpc":"2.0","id":6,"method":"panel/action","params":{"panel":"pet","action_id":"` + id + `","values":{"bubble":"click"}}}`
	show := find(h.send(click), "sessions/show")
	h.fail(show)
	h.now = h.now.Add(2 * time.Second)
	h.s.tick()
	retry := find(h.send(`{"jsonrpc":"2.0","method":"event/file_saved","params":{}}`), "sessions/show")
	if retry == nil {
		t.Fatal("no second try")
	}
	h.fail(retry)
	h.now = h.now.Add(2 * time.Second)
	h.s.tick()
	if find(h.send(`{"jsonrpc":"2.0","method":"event/file_saved","params":{}}`), "sessions/show") != nil {
		t.Error("a third try")
	}
}
