package main

import (
	"bufio"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

// sse is an open GET /v1/events read line by line.
type sse struct {
	t     *testing.T
	resp  *http.Response
	lines chan string // closed at the stream's end
}

// listen serves the API on a real loopback server, which streams as a phone sees it.
func (a *api) listen() *httptest.Server {
	srv := httptest.NewServer(a.http)
	a.t.Cleanup(srv.Close)
	return srv
}

// openSSE sends GET /v1/events with token and returns the response, read in the background
// when it is 200.
func openSSE(t *testing.T, srv *httptest.Server, token string) *sse {
	t.Helper()
	return openSSEAt(t, srv.URL, "127.0.0.1:7717", token)
}

// openSSEAt is openSSE against base, with Host host.
func openSSEAt(t *testing.T, base, host, token string) *sse {
	t.Helper()
	r, _ := http.NewRequest("GET", base+"/v1/events", nil)
	r.Host = host
	r.Header.Set("Authorization", "Bearer "+token)
	resp, err := http.DefaultClient.Do(r)
	if err != nil {
		t.Fatal(err)
	}
	s := &sse{t: t, resp: resp, lines: make(chan string, 100)}
	t.Cleanup(func() { resp.Body.Close() })
	if resp.StatusCode != http.StatusOK {
		close(s.lines)
		return s
	}
	go func() {
		defer close(s.lines)
		sc := bufio.NewScanner(resp.Body)
		for sc.Scan() {
			s.lines <- sc.Text()
		}
	}()
	return s
}

func decodeBody(resp *http.Response, v any) {
	json.NewDecoder(resp.Body).Decode(v)
}

func (s *sse) line() string {
	s.t.Helper()
	select {
	case l, ok := <-s.lines:
		if !ok {
			s.t.Fatal("the stream ended")
		}
		return l
	case <-time.After(5 * time.Second):
		s.t.Fatal("no line on the stream")
	}
	return ""
}

// event reads one event: its name and its data.
func (s *sse) event() (string, string) {
	s.t.Helper()
	name, data, blank := s.line(), s.line(), s.line()
	if !strings.HasPrefix(name, "event: ") || !strings.HasPrefix(data, "data: ") || blank != "" {
		s.t.Fatalf("not one event: %q %q %q", name, data, blank)
	}
	return strings.TrimPrefix(name, "event: "), strings.TrimPrefix(data, "data: ")
}

// ends checks the stream ends within d, after what it already sent.
func (s *sse) ends(what string, d time.Duration) {
	s.t.Helper()
	deadline := time.After(d)
	for {
		select {
		case _, ok := <-s.lines:
			if !ok {
				return
			}
		case <-deadline:
			s.t.Errorf("%s: the stream is still open after %v", what, d)
			return
		}
	}
}

// REMO-02, REMO-04: every host session event goes to each open stream as one SSE event, and a
// ": ping" comment comes every ping interval (15 s).
func TestEventsForwardHostEvents(t *testing.T) {
	a := startAPI(t)
	if a.h.s.hub.ping != 15*time.Second {
		t.Errorf("ping every %v, want 15s", a.h.s.hub.ping)
	}
	srv := a.listen()
	one, two := openSSE(t, srv, a.token), openSSE(t, srv, a.token)
	for _, st := range []*sse{one, two} {
		if st.resp.StatusCode != http.StatusOK || st.resp.Header.Get("Content-Type") != "text/event-stream" || st.resp.Header.Get("Cache-Control") != "no-store" {
			t.Fatalf("stream: %d %v", st.resp.StatusCode, st.resp.Header)
		}
	}

	events := []struct{ method, params string }{
		{"event/session_state", `{"agent":"claude","session":"s1","state":"working"}`},
		{"event/session_update", `{"agent":"claude","session":"s1","total":2,"items":[{"index":1,"item":{"kind":"agent","text":"line one\nline two <b>"}}]}`},
		{"event/permission_requested", `{"agent":"claude","session":"s1","request":7,"title":"Run cargo test","options":[{"id":"allow","name":"Allow","kind":"allow_once"}]}`},
		{"event/permission_resolved", `{"agent":"claude","session":"s1","request":7,"option":"allow"}`},
		{"event/question_requested", `{"agent":"claude","session":"s1","request":3,"message":"Which branch?","fields":[{"key":"b","title":"Branch","description":null,"required":true,"kind":"single","choices":[{"value":"main","label":"main","description":null}]}]}`},
		{"event/question_resolved", `{"agent":"claude","session":"s1","request":3,"answer":"accept"}`},
		{"event/turn_ended", `{"agent":"claude","session":"s1","reason":"end_turn"}`},
	}
	for _, e := range events {
		// Events wai-remoto does not stream are skipped.
		a.h.send(`{"jsonrpc":"2.0","method":"event/file_saved","params":{"path":"/x"}}`)
		a.h.send(`{"jsonrpc":"2.0","method":"` + e.method + `","params":` + e.params + `}`)
		for _, st := range []*sse{one, two} {
			name, data := st.event()
			if name != strings.TrimPrefix(e.method, "event/") {
				t.Errorf("event name %q for %s", name, e.method)
			}
			jsonEqual(t, e.method, data, e.params)
		}
	}

	// The ping, on a stream opened with a short interval.
	a.h.s.hub.mu.Lock()
	a.h.s.hub.ping = 50 * time.Millisecond
	a.h.s.hub.mu.Unlock()
	quick := openSSE(t, srv, a.token)
	for range 2 {
		if l, blank := quick.line(), quick.line(); l != ": ping" || blank != "" {
			t.Errorf("want a ping comment, got %q %q", l, blank)
		}
	}
}

// EDGE-05: a device's fifth open stream answers 429; other devices, and the device once one of
// its streams closes, can open more.
func TestFifthStreamIs429(t *testing.T) {
	a := startAPI(t)
	srv := a.listen()
	var open []*sse
	for range 4 {
		st := openSSE(t, srv, a.token)
		if st.resp.StatusCode != http.StatusOK {
			t.Fatalf("stream %d: %d", len(open)+1, st.resp.StatusCode)
		}
		open = append(open, st)
	}
	fifth := openSSE(t, srv, a.token)
	if fifth.resp.StatusCode != http.StatusTooManyRequests {
		t.Errorf("the fifth stream: %d, want 429", fifth.resp.StatusCode)
	}
	var e struct{ Error string }
	if decodeBody(fifth.resp, &e); e.Error != "At most 4 event streams per device" {
		t.Errorf("the fifth stream's error %q", e.Error)
	}

	other, _, _ := a.h.s.store.add("iPhone")
	if st := openSSE(t, srv, other); st.resp.StatusCode != http.StatusOK {
		t.Errorf("another device's stream: %d", st.resp.StatusCode)
	}

	open[0].resp.Body.Close()
	deadline := time.Now().Add(2 * time.Second)
	for {
		st := openSSE(t, srv, a.token)
		if st.resp.StatusCode == http.StatusOK {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("after closing one of four streams, a new one is still %d", st.resp.StatusCode)
		}
		time.Sleep(20 * time.Millisecond)
	}
	if st := openSSE(t, srv, a.token); st.resp.StatusCode != http.StatusTooManyRequests {
		t.Errorf("a fifth stream again: %d", st.resp.StatusCode)
	}
}

// DEV-02, DEV-03, DEV-06: revoking a device, unpairing and signing out all end the streams of the
// devices concerned within 1 second, and their tokens no longer open one.
func TestRevokeClosesStreams(t *testing.T) {
	a := startAPI(t)
	srv := a.listen()
	other, _, _ := a.h.s.store.add("iPhone")
	third, _, _ := a.h.s.store.add("iPad")
	mine1, mine2 := openSSE(t, srv, a.token), openSSE(t, srv, a.token)
	theirs := openSSE(t, srv, other)
	ipad := openSSE(t, srv, third)

	a.h.s.revoke(a.device.ID)
	mine1.ends("the revoked device's first stream", time.Second)
	mine2.ends("the revoked device's second stream", time.Second)
	if st := openSSE(t, srv, a.token); st.resp.StatusCode != http.StatusUnauthorized {
		t.Errorf("a revoked token opens a stream: %d", st.resp.StatusCode)
	}
	// The other devices' streams stay open and get the next event.
	a.h.sessionState("claude", "s1", "idle")
	for _, st := range []*sse{theirs, ipad} {
		if name, _ := st.event(); name != "session_state" {
			t.Errorf("after the revoke another device got %q", name)
		}
	}

	// Unpairing from the phone ends that phone's streams.
	r, _ := http.NewRequest("POST", srv.URL+"/v1/unpair", nil)
	r.Host = "127.0.0.1:7717"
	r.Header.Set("Authorization", "Bearer "+third)
	if resp, err := srv.Client().Do(r); err != nil || resp.StatusCode != http.StatusNoContent {
		t.Fatalf("unpair: %v %v", resp, err)
	}
	ipad.ends("the unpaired device's stream", time.Second)

	a.h.s.signOutAll()
	theirs.ends("a stream after sign out all", time.Second)
	if st := openSSE(t, srv, other); st.resp.StatusCode != http.StatusUnauthorized {
		t.Errorf("a token opens a stream after sign out all: %d", st.resp.StatusCode)
	}
	if a.h.s.hub.connected() != 0 {
		t.Errorf("connected devices after sign out all: %d", a.h.s.hub.connected())
	}
}
