package main

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"reflect"
	"strconv"
	"strings"
	"testing"
	"time"
)

// api is an initialized server with one paired device, "Pixel", and the fake wai behind it.
type api struct {
	t      *testing.T
	h      *host
	http   http.Handler
	token  string
	device Device
}

// dataDir is a temporary data folder. Unlike t.TempDir, its removal retries for a while: on
// Windows a state.json just replaced can stay delete-pending for a moment, and the folder then
// refuses to go.
func dataDir(t *testing.T) string {
	t.Helper()
	dir, err := os.MkdirTemp("", "wai-remoto-test")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		for i := 0; ; i++ {
			err := os.RemoveAll(dir)
			if err == nil {
				return
			}
			if i == 50 {
				t.Errorf("cannot remove %s: %v", dir, err)
				return
			}
			time.Sleep(20 * time.Millisecond)
		}
	})
	return dir
}

func startAPI(t *testing.T) *api {
	t.Helper()
	h := startHost(t)
	h.initialize(dataDir(t))
	h.next() // initialize's answer
	h.next() // the first render
	token, d, err := h.s.store.add("Pixel")
	if err != nil {
		t.Fatal(err)
	}
	return &api{t: t, h: h, http: h.s.handler(), token: token, device: d}
}

// req is a request from the paired device: loopback Host, its Bearer token, no Origin.
func (a *api) req(method, path, body string) *http.Request {
	r := httptest.NewRequest(method, path, strings.NewReader(body))
	r.Host = "127.0.0.1:7717"
	r.Header.Set("Authorization", "Bearer "+a.token)
	return r
}

// do serves r on its own goroutine, so the test can answer wai's calls meanwhile.
func (a *api) do(r *http.Request) <-chan *httptest.ResponseRecorder {
	ch := make(chan *httptest.ResponseRecorder, 1)
	go func() {
		w := httptest.NewRecorder()
		a.http.ServeHTTP(w, r)
		ch <- w
	}()
	return ch
}

func (a *api) wait(ch <-chan *httptest.ResponseRecorder) *httptest.ResponseRecorder {
	a.t.Helper()
	select {
	case w := <-ch:
		return w
	case <-time.After(5 * time.Second):
		a.t.Fatal("no HTTP answer")
	}
	return nil
}

// status checks an answer's status and, for a refusal, its JSON error message.
func status(t *testing.T, what string, w *httptest.ResponseRecorder, code int, why string) {
	t.Helper()
	if w.Code != code {
		t.Errorf("%s: status %d, want %d (%s)", what, w.Code, code, w.Body)
	}
	if why != "" {
		var e struct{ Error string }
		if json.Unmarshal(w.Body.Bytes(), &e); e.Error != why {
			t.Errorf("%s: error %q, want %q", what, e.Error, why)
		}
	}
}

// expectCall is the server's next message to wai, a request for method; it returns the id to
// answer and the params.
func (h *host) expectCall(method string) (any, map[string]any) {
	h.t.Helper()
	m := h.next()
	if m["method"] != method || m["id"] == nil {
		h.t.Fatalf("want a %s request, got %v", method, m)
	}
	p, _ := m["params"].(map[string]any)
	return m["id"], p
}

func (h *host) reply(id any, result string) {
	h.t.Helper()
	b, _ := json.Marshal(id)
	h.send(`{"jsonrpc":"2.0","id":` + string(b) + `,"result":` + result + `}`)
}

func (h *host) replyError(id any, code int, msg string) {
	h.t.Helper()
	b, _ := json.Marshal(id)
	h.send(`{"jsonrpc":"2.0","id":` + string(b) + `,"error":{"code":` + strconv.Itoa(code) + `,"message":` + quote(msg) + `}}`)
}

// quiet checks the server sent wai nothing since the last message read, and that it handled
// everything sent to it before: it answers a marker request after them.
func (h *host) quiet(when string) {
	h.t.Helper()
	h.send(`{"jsonrpc":"2.0","id":"marker","method":"marker"}`)
	if m := h.next(); m["id"] != "marker" {
		h.t.Errorf("%s: want no message to wai, got %v", when, m)
	}
}

func (h *host) sessionState(agent, session, state string) {
	h.send(`{"jsonrpc":"2.0","method":"event/session_state","params":{"agent":` + quote(agent) + `,"session":` + quote(session) + `,"state":` + quote(state) + `}}`)
}

func jsonEqual(t *testing.T, what, got, want string) {
	t.Helper()
	var g, w any
	if err := json.Unmarshal([]byte(got), &g); err != nil {
		t.Errorf("%s: not JSON: %q", what, got)
		return
	}
	json.Unmarshal([]byte(want), &w)
	if !reflect.DeepEqual(g, w) {
		t.Errorf("%s: got %s, want %s", what, got, want)
	}
}

func params(t *testing.T, what string, got map[string]any, want string) {
	t.Helper()
	b, _ := json.Marshal(got)
	jsonEqual(t, what+" params", string(b), want)
}

// REMO-01: GET /v1/sessions answers wai's sessions/list as it came.
func TestSessionsListForwardsHost(t *testing.T) {
	a := startAPI(t)
	list := `[{"agent":"claude","session":"s1","title":"Fix the build","state":"idle"},{"agent":"codex","session":"s2","title":null,"state":"needs_you"}]`
	ch := a.do(a.req("GET", "/v1/sessions", ""))
	id, p := a.h.expectCall("sessions/list")
	if len(p) != 0 {
		t.Errorf("sessions/list params %v", p)
	}
	a.h.reply(id, list)
	w := a.wait(ch)
	status(t, "list", w, http.StatusOK, "")
	jsonEqual(t, "list", w.Body.String(), list)
	if ct := w.Header().Get("Content-Type"); ct != "application/json" {
		t.Errorf("Content-Type %q", ct)
	}

	// No session open: an empty list.
	ch = a.do(a.req("GET", "/v1/sessions", ""))
	id, _ = a.h.expectCall("sessions/list")
	a.h.reply(id, `[]`)
	w = a.wait(ch)
	status(t, "empty list", w, http.StatusOK, "")
	jsonEqual(t, "empty list", w.Body.String(), `[]`)

	// Without a token nothing reaches wai.
	r := a.req("GET", "/v1/sessions", "")
	r.Header.Del("Authorization")
	status(t, "no token", a.wait(a.do(r)), http.StatusUnauthorized, "Missing or unknown token")
	a.h.quiet("a request without a token")
}

// HOST-01 through wai-remoto: GET /v1/sessions/{agent}/{session} answers wai's sessions/read.
func TestReadForwardsHost(t *testing.T) {
	a := startAPI(t)
	page := `{"items":[{"kind":"user","text":"hi"},{"kind":"tool","title":"Run cargo test","tool_kind":"execute","status":"completed"}],"total":12}`
	ch := a.do(a.req("GET", "/v1/sessions/claude/s1?from=10&limit=2", ""))
	id, p := a.h.expectCall("sessions/read")
	params(t, "with from and limit", p, `{"agent":"claude","session":"s1","from":10,"limit":2}`)
	a.h.reply(id, page)
	w := a.wait(ch)
	status(t, "read", w, http.StatusOK, "")
	jsonEqual(t, "read", w.Body.String(), page)

	// Without from and limit wai picks the last page.
	ch = a.do(a.req("GET", "/v1/sessions/claude/s1", ""))
	id, p = a.h.expectCall("sessions/read")
	params(t, "without from and limit", p, `{"agent":"claude","session":"s1"}`)
	a.h.reply(id, page)
	status(t, "read the last page", a.wait(ch), http.StatusOK, "")

	// A session that is not open is 404, with wai's message.
	ch = a.do(a.req("GET", "/v1/sessions/claude/s9", ""))
	id, _ = a.h.expectCall("sessions/read")
	a.h.replyError(id, -32000, "No open session claude s9")
	status(t, "closed session", a.wait(ch), http.StatusNotFound, "No open session claude s9")

	for _, q := range []string{"from=x", "limit=-1", "from=1.5"} {
		status(t, q, a.wait(a.do(a.req("GET", "/v1/sessions/claude/s1?"+q, ""))), http.StatusBadRequest, "")
	}
	a.h.quiet("bad from or limit")
}

// REMO-08: a prompt to an idle session is forwarded to wai as sessions/prompt.
func TestPromptForwardsToIdleSession(t *testing.T) {
	a := startAPI(t)
	a.h.sessionState("claude", "s1", "idle")
	a.h.quiet("a session_state event")
	ch := a.do(a.req("POST", "/v1/sessions/claude/s1/prompt", `{"text":"Run the tests"}`))
	id, p := a.h.expectCall("sessions/prompt")
	params(t, "prompt", p, `{"agent":"claude","session":"s1","text":"Run the tests"}`)
	a.h.reply(id, `null`)
	w := a.wait(ch)
	status(t, "prompt", w, http.StatusNoContent, "")

	// A session wai-remoto has no state for yet: it asks wai for the list first.
	ch = a.do(a.req("POST", "/v1/sessions/codex/s2/prompt", `{"text":"Go on"}`))
	id, _ = a.h.expectCall("sessions/list")
	a.h.reply(id, `[{"agent":"codex","session":"s2","title":null,"state":"idle"}]`)
	id, p = a.h.expectCall("sessions/prompt")
	params(t, "prompt after the list", p, `{"agent":"codex","session":"s2","text":"Go on"}`)
	a.h.reply(id, `null`)
	status(t, "prompt after the list", a.wait(ch), http.StatusNoContent, "")

	// An empty prompt or a body that is not JSON never reaches wai.
	status(t, "empty prompt", a.wait(a.do(a.req("POST", "/v1/sessions/claude/s1/prompt", `{"text":"  "}`))), http.StatusBadRequest, "The prompt is empty")
	status(t, "not JSON", a.wait(a.do(a.req("POST", "/v1/sessions/claude/s1/prompt", `text`))), http.StatusBadRequest, "")
	a.h.quiet("bad prompts")
}

// EDGE-08: a prompt to a session that is not idle answers 409 and never reaches wai.
func TestPromptToBusySessionIs409(t *testing.T) {
	a := startAPI(t)
	for _, state := range []string{"working", "needs_you", "error"} {
		a.h.sessionState("claude", "s1", state)
		a.h.quiet("a session_state event")
		w := a.wait(a.do(a.req("POST", "/v1/sessions/claude/s1/prompt", `{"text":"Again"}`)))
		status(t, state, w, http.StatusConflict, "The session is busy")
		a.h.quiet("a prompt to a " + state + " session")
	}

	// The list's states count too.
	ch := a.do(a.req("GET", "/v1/sessions", ""))
	id, _ := a.h.expectCall("sessions/list")
	a.h.reply(id, `[{"agent":"codex","session":"s2","title":null,"state":"working"}]`)
	a.wait(ch)
	status(t, "working in the list", a.wait(a.do(a.req("POST", "/v1/sessions/codex/s2/prompt", `{"text":"x"}`))), http.StatusConflict, "The session is busy")
	a.h.quiet("a prompt to a session the list shows working")

	// A session wai does not have open is 404.
	ch = a.do(a.req("POST", "/v1/sessions/claude/gone/prompt", `{"text":"x"}`))
	id, _ = a.h.expectCall("sessions/list")
	a.h.reply(id, `[]`)
	status(t, "closed session", a.wait(ch), http.StatusNotFound, "No open session claude gone")
}

// REMO-14: a device's 31st prompt within a minute answers 429; other devices are not held.
func TestPromptRateLimit429(t *testing.T) {
	a := startAPI(t)
	a.h.sessionState("claude", "s1", "idle")
	a.h.quiet("a session_state event")
	send := func(token string) *httptest.ResponseRecorder {
		r := a.req("POST", "/v1/sessions/claude/s1/prompt", `{"text":"x"}`)
		r.Header.Set("Authorization", "Bearer "+token)
		return a.wait(a.do(r))
	}
	sent := func(what, token string) {
		t.Helper()
		ch := make(chan *httptest.ResponseRecorder, 1)
		go func() { ch <- send(token) }()
		id, _ := a.h.expectCall("sessions/prompt")
		a.h.reply(id, `null`)
		status(t, what, a.wait(ch), http.StatusNoContent, "")
	}
	for i := range 30 {
		sent("prompt "+strconv.Itoa(i+1), a.token)
		a.h.advance(time.Second)
	}
	status(t, "the 31st prompt", send(a.token), http.StatusTooManyRequests, "Too many prompts, wait a minute")
	status(t, "the 32nd prompt", send(a.token), http.StatusTooManyRequests, "Too many prompts, wait a minute")
	a.h.quiet("prompts over the limit")

	other, _, _ := a.h.s.store.add("iPhone")
	sent("another device's prompt", other)

	// A minute after the first prompt, one more is allowed.
	a.h.advance(30 * time.Second)
	sent("a minute after the first", a.token)
	status(t, "the next one", send(a.token), http.StatusTooManyRequests, "Too many prompts, wait a minute")
}

// REMO-10: cancel is forwarded to wai as sessions/cancel.
func TestCancelForwards(t *testing.T) {
	a := startAPI(t)
	ch := a.do(a.req("POST", "/v1/sessions/claude/s1/cancel", ""))
	id, p := a.h.expectCall("sessions/cancel")
	params(t, "cancel", p, `{"agent":"claude","session":"s1"}`)
	a.h.reply(id, `null`)
	status(t, "cancel", a.wait(ch), http.StatusNoContent, "")

	ch = a.do(a.req("POST", "/v1/sessions/claude/s9/cancel", ""))
	id, _ = a.h.expectCall("sessions/cancel")
	a.h.replyError(id, -32000, "No open session claude s9")
	status(t, "closed session", a.wait(ch), http.StatusNotFound, "No open session claude s9")
}

// PAIR-09, PAIR-10: POST /v1/pair answers a token for the live code; an unknown, expired or used
// code answers 401 with no token and counts as a failed sign-in.
func TestPairRoute(t *testing.T) {
	a := startAPI(t)
	pair := func(body string) *httptest.ResponseRecorder {
		r := a.req("POST", "/v1/pair", body)
		r.Header.Del("Authorization")
		return a.wait(a.do(r))
	}
	refused := func(what, code string) {
		t.Helper()
		w := pair(`{"code":` + quote(code) + `,"device":"Thief"}`)
		status(t, what, w, http.StatusUnauthorized, "Unknown, expired or used pairing code")
		if strings.Contains(w.Body.String(), "token") {
			t.Errorf("%s: a token in %s", what, w.Body)
		}
	}

	code, _ := a.h.s.pairing.issue()
	status(t, "no device name", pair(`{"code":`+quote(code)+`,"device":" "}`), http.StatusBadRequest, "The device name is empty")
	w := pair(`{"code":` + quote(code) + `,"device":"Pixel 9"}`)
	status(t, "a live code", w, http.StatusOK, "")
	var got struct{ Token string }
	json.Unmarshal(w.Body.Bytes(), &got)
	if got.Token == "" {
		t.Fatalf("no token: %s", w.Body)
	}
	if d, ok := a.h.s.store.lookup(got.Token); !ok || d.Name != "Pixel 9" {
		t.Errorf("the token's device: %+v %v", d, ok)
	}

	refused("a used", code)
	refused("an unknown", "AAAAAAAAAAAAAAAAAAAAAA")
	expired, _ := a.h.s.pairing.issue()
	a.h.advance(5 * time.Minute)
	refused("an expired", expired)
	if got := names(a.h.s.store.devices()); got != "Pixel,Pixel 9" {
		t.Errorf("devices %q", got)
	}

	// Those were 3 failed sign-ins: 16 more stay quiet, the 20th notifies.
	for range 16 {
		refused("another unknown", "nope")
	}
	a.h.quiet("19 failed sign-ins")
	refused("the 20th", "nope")
	m := a.h.next()
	if p, _ := m["params"].(map[string]any); m["method"] != "notify" || p["title"] != "wai-remoto: 20 failed sign-ins in the last hour" {
		t.Errorf("want the failed sign-ins notify, got %v", m)
	}
}

// DEV-06: POST /v1/unpair revokes the calling device's token, and only that one.
func TestUnpairRevokesOwnToken(t *testing.T) {
	a := startAPI(t)
	other, _, _ := a.h.s.store.add("iPhone")
	status(t, "unpair", a.wait(a.do(a.req("POST", "/v1/unpair", ""))), http.StatusNoContent, "")
	if got := names(a.h.s.store.devices()); got != "iPhone" {
		t.Errorf("devices after unpair: %q", got)
	}
	status(t, "the unpaired token", a.wait(a.do(a.req("POST", "/v1/unpair", ""))), http.StatusUnauthorized, "Missing or unknown token")
	status(t, "the unpaired token reading", a.wait(a.do(a.req("GET", "/v1/sessions", ""))), http.StatusUnauthorized, "Missing or unknown token")
	a.h.quiet("requests with the unpaired token")

	r := a.req("GET", "/v1/sessions", "")
	r.Header.Set("Authorization", "Bearer "+other)
	ch := a.do(r)
	id, _ := a.h.expectCall("sessions/list")
	a.h.reply(id, `[]`)
	status(t, "the other device", a.wait(ch), http.StatusOK, "")
}
