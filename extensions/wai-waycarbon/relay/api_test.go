package main

import (
	"bufio"
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"wai-waycarbon/helpapi"
)

type env struct {
	t   *testing.T
	s   *Server
	ts  *httptest.Server
	f   *fakeGitHub
	tok map[string]string
}

func newEnv(t *testing.T) *env {
	t.Helper()
	f := newFakeGitHub(t)
	f.members = map[string]bool{"ana": true, "bob": true, "eve": true}
	s := newTestServer(t, f)
	ts := httptest.NewServer(s.Routes())
	t.Cleanup(ts.Close)
	e := &env{t: t, s: s, ts: ts, f: f, tok: map[string]string{}}
	for id, login := range map[int64]string{7: "ana", 8: "bob", 9: "eve"} {
		tok := "tok-" + login
		e.tok[login] = tok
		if err := s.store.PutUser(id, login); err != nil {
			t.Fatal(err)
		}
		if err := s.store.AddToken(sha256.Sum256([]byte(tok)), id); err != nil {
			t.Fatal(err)
		}
	}
	return e
}

func (e *env) do(method, login, path string, body any) *http.Response {
	e.t.Helper()
	var rd *bytes.Reader
	if raw, ok := body.([]byte); ok {
		rd = bytes.NewReader(raw)
	} else {
		b, _ := json.Marshal(body)
		rd = bytes.NewReader(b)
	}
	req, _ := http.NewRequest(method, e.ts.URL+path, rd)
	req.Header.Set("Authorization", "Bearer "+e.tok[login])
	resp, err := e.ts.Client().Do(req)
	if err != nil {
		e.t.Fatal(err)
	}
	e.t.Cleanup(func() { resp.Body.Close() })
	return resp
}

func (e *env) post(login, path string, body any) *http.Response {
	e.t.Helper()
	return e.do("POST", login, path, body)
}

func decode[T any](t *testing.T, resp *http.Response) T {
	t.Helper()
	var v T
	if err := json.NewDecoder(resp.Body).Decode(&v); err != nil {
		t.Fatal(err)
	}
	return v
}

func (e *env) send(from, to, text string) helpapi.Request {
	e.t.Helper()
	resp := e.post(from, "/requests", map[string]any{"to": to, "text": text})
	if resp.StatusCode != 201 {
		e.t.Fatalf("send: %d", resp.StatusCode)
	}
	return decode[helpapi.Request](e.t, resp)
}

type sseFrame struct{ event, data string }

type stream struct {
	t    *testing.T
	resp *http.Response
	ch   chan sseFrame
}

func (e *env) stream(login string) *stream {
	e.t.Helper()
	resp := e.do("GET", login, "/events", nil)
	if resp.StatusCode != 200 || resp.Header.Get("Content-Type") != "text/event-stream" {
		e.t.Fatalf("events: %d %s", resp.StatusCode, resp.Header.Get("Content-Type"))
	}
	st := &stream{t: e.t, resp: resp, ch: make(chan sseFrame, 64)}
	go func() {
		defer close(st.ch)
		sc := bufio.NewScanner(resp.Body)
		var f sseFrame
		for sc.Scan() {
			line := sc.Text()
			switch {
			case strings.HasPrefix(line, "event: "):
				f.event = line[7:]
			case strings.HasPrefix(line, "data: "):
				f.data = line[6:]
			case line == "" && f.event != "":
				st.ch <- f
				f = sseFrame{}
			}
		}
	}()
	return st
}

func (st *stream) next(want string) string {
	st.t.Helper()
	select {
	case f, ok := <-st.ch:
		if !ok {
			st.t.Fatalf("stream closed, wanted %s", want)
		}
		if f.event != want {
			st.t.Fatalf("got %s %s, wanted %s", f.event, f.data, want)
		}
		return f.data
	case <-time.After(3 * time.Second):
		st.t.Fatalf("timeout waiting for %s", want)
	}
	return ""
}

func (st *stream) closed() bool {
	select {
	case _, ok := <-st.ch:
		return !ok
	case <-time.After(3 * time.Second):
		return false
	}
}

func person(ps []helpapi.Person, login string) (helpapi.Person, bool) {
	for _, p := range ps {
		if p.Login == login {
			return p, true
		}
	}
	return helpapi.Person{}, false
}

func TestRequestFlowOverStreams(t *testing.T) {
	e := newEnv(t)
	bob := e.stream("bob")
	var sync []helpapi.Request
	if err := json.Unmarshal([]byte(bob.next(helpapi.EvSync)), &sync); err != nil || len(sync) != 0 {
		t.Fatalf("sync: %v %v", sync, err)
	}
	var people []helpapi.Person
	json.Unmarshal([]byte(bob.next(helpapi.EvPeople)), &people)
	if p, ok := person(people, "ana"); !ok || p.Online {
		t.Fatalf("people: %+v", people)
	}
	if _, ok := person(people, "bob"); ok {
		t.Fatal("self listed")
	}
	e.stream("ana")
	json.Unmarshal([]byte(bob.next(helpapi.EvPeople)), &people)
	if p, _ := person(people, "ana"); !p.Online {
		t.Fatalf("ana not online: %+v", people)
	}

	req := e.send("ana", "bob", "please run the migration")
	var got helpapi.Request
	json.Unmarshal([]byte(bob.next(helpapi.EvNew)), &got)
	if got.ID != req.ID || got.Text != "please run the migration" || got.From != "ana" {
		t.Fatalf("new: %+v", got)
	}

	anaS := e.stream("ana")
	if d := anaS.next(helpapi.EvSync); !strings.Contains(d, req.ID) {
		t.Fatalf("sync lacks the request: %s", d)
	}
	anaS.next(helpapi.EvPeople)
	if r := e.post("bob", "/requests/"+req.ID+"/accept", nil); r.StatusCode != 200 {
		t.Fatalf("accept: %d", r.StatusCode)
	}
	var up helpapi.Update
	json.Unmarshal([]byte(anaS.next(helpapi.EvUpdated)), &up)
	if up.ID != req.ID || up.State != helpapi.Accepted {
		t.Fatalf("accepted: %+v", up)
	}
	r := e.post("bob", "/requests/"+req.ID+"/reply", map[string]string{"text": "done"})
	if r.StatusCode != 200 {
		t.Fatalf("reply: %d", r.StatusCode)
	}
	json.Unmarshal([]byte(anaS.next(helpapi.EvUpdated)), &up)
	if up.State != helpapi.Replied || up.Reply != "done" {
		t.Fatalf("replied: %+v", up)
	}
}

func TestStrangersAndLeaversAreRefused(t *testing.T) {
	e := newEnv(t)
	req := e.send("ana", "bob", "secret ask")
	if r := e.post("eve", "/requests/"+req.ID+"/accept", nil); r.StatusCode != 404 {
		t.Fatalf("eve accept: %d", r.StatusCode)
	}
	e.f.mu.Lock()
	e.f.members["bob"] = false
	e.f.mu.Unlock()
	if r := e.do("GET", "bob", "/events", nil); r.StatusCode != 403 {
		t.Fatalf("leaver events: %d", r.StatusCode)
	}
	r := e.post("bob", "/requests/"+req.ID+"/accept", nil)
	if r.StatusCode != 403 {
		t.Fatalf("leaver accept: %d", r.StatusCode)
	}
	if m := decode[map[string]string](t, r); m["error"] == "" {
		t.Fatal("no error body")
	}
	if open := e.s.store.Open(8); len(open) != 1 || open[0].State != helpapi.Waiting {
		t.Fatalf("store changed: %+v", open)
	}
}

func TestOfflineRecipientGetsSyncOnConnect(t *testing.T) {
	e := newEnv(t)
	req := e.send("ana", "bob", "while you were out")
	bob := e.stream("bob")
	var sync []helpapi.Request
	json.Unmarshal([]byte(bob.next(helpapi.EvSync)), &sync)
	if len(sync) != 1 || sync[0].ID != req.ID || sync[0].Text != "while you were out" {
		t.Fatalf("sync: %+v", sync)
	}
}

func TestLimitsOverHTTP(t *testing.T) {
	e := newEnv(t)
	big := strings.Repeat("x", helpapi.MaxBody+10)
	if r := e.post("ana", "/requests", map[string]string{"to": "bob", "text": big}); r.StatusCode != 413 {
		t.Fatalf("big body: %d", r.StatusCode)
	}
	if r := e.post("ana", "/requests", map[string]string{"to": "bob", "text": strings.Repeat("x", helpapi.MaxText+1)}); r.StatusCode != 400 {
		t.Fatalf("long text: %d", r.StatusCode)
	}
	if r := e.post("ana", "/requests", map[string]string{"to": "bob", "text": ""}); r.StatusCode != 400 {
		t.Fatalf("empty text: %d", r.StatusCode)
	}
	r := e.post("ana", "/requests", map[string]string{"to": "zed", "text": "hi"})
	if m := decode[map[string]string](t, r); r.StatusCode != 404 || m["error"] != "@zed has not signed in to help requests" {
		t.Fatalf("unknown: %d %v", r.StatusCode, m)
	}
	for i := 0; i < helpapi.StreamsPerUser; i++ {
		e.stream("ana")
	}
	if r := e.do("GET", "ana", "/events", nil); r.StatusCode != 429 {
		t.Fatalf("5th stream: %d", r.StatusCode)
	}
	if r := e.do("GET", "nobody", "/events", nil); r.StatusCode != 401 {
		t.Fatalf("no token: %d", r.StatusCode)
	}
}

func TestSweepAndMemberChecks(t *testing.T) {
	e := newEnv(t)
	bob := e.stream("bob")
	bob.next(helpapi.EvSync)
	bob.next(helpapi.EvPeople)
	req := e.send("ana", "bob", "slow")
	bob.next(helpapi.EvNew)
	e.s.store.now = func() time.Time { return time.Now().Add(helpapi.AcceptWithin + time.Hour) }
	e.s.sweepOnce()
	var up helpapi.Update
	json.Unmarshal([]byte(bob.next(helpapi.EvUpdated)), &up)
	if up.ID != req.ID || up.State != helpapi.Expired {
		t.Fatalf("expired: %+v", up)
	}
	e.s.store.now = time.Now

	e.f.mu.Lock()
	e.f.members["bob"] = false
	e.f.mu.Unlock()
	e.s.checkMembersOnce(context.Background())
	if !bob.closed() {
		t.Fatal("leaver stream still open")
	}
	if r := e.post("bob", "/requests/"+req.ID+"/cancel", nil); r.StatusCode != 401 {
		t.Fatalf("leaver token: %d", r.StatusCode)
	}
}

func TestLastStreamLeavingBroadcastsOffline(t *testing.T) {
	e := newEnv(t)
	ana := e.stream("ana")
	ana.next(helpapi.EvSync)
	ana.next(helpapi.EvPeople)
	bob1 := e.stream("bob")
	var people []helpapi.Person
	json.Unmarshal([]byte(ana.next(helpapi.EvPeople)), &people)
	if p, _ := person(people, "bob"); !p.Online {
		t.Fatalf("bob not online: %+v", people)
	}
	bob2 := e.stream("bob")
	bob1.resp.Body.Close()
	bob2.resp.Body.Close()
	// If closing bob's first stream had broadcast, ana's next people event would still say online.
	json.Unmarshal([]byte(ana.next(helpapi.EvPeople)), &people)
	if p, ok := person(people, "bob"); !ok || p.Online {
		t.Fatalf("bob should be offline: %+v", people)
	}
}
