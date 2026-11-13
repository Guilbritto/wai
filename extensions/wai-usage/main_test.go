package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"log"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

const secret = "sekrit"

const okBody = `{"five_hour":{"utilization":16,"resets_at":"2026-11-14T03:20:00Z"},"seven_day":{"utilization":59,"resets_at":"2026-11-20T03:20:00Z"}}`

func fakeToken(context.Context) (string, error) { return secret, nil }

// sink is the server's stdout: every line is kept and handed to the test in order.
type sink struct {
	ch  chan string
	mu  sync.Mutex
	all bytes.Buffer
}

func newSink() *sink { return &sink{ch: make(chan string, 64)} }

func (s *sink) Write(p []byte) (int, error) {
	s.mu.Lock()
	s.all.Write(p)
	s.mu.Unlock()
	s.ch <- strings.TrimSpace(string(p))
	return len(p), nil
}

func (s *sink) text() string {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.all.String()
}

func (s *sink) next(t *testing.T) node {
	t.Helper()
	select {
	case l := <-s.ch:
		var m node
		if err := json.Unmarshal([]byte(l), &m); err != nil {
			t.Fatalf("not JSON: %q", l)
		}
		return m
	case <-time.After(5 * time.Second):
		t.Fatal("timed out waiting for output")
		return nil
	}
}

// nextUpdate skips responses and returns the next item/update's params.
func (s *sink) nextUpdate(t *testing.T) node {
	t.Helper()
	for {
		m := s.next(t)
		if m["method"] == "item/update" {
			return m["params"].(node)
		}
	}
}

type logBuf struct {
	mu sync.Mutex
	b  bytes.Buffer
}

func (l *logBuf) Write(p []byte) (int, error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.b.Write(p)
}

func (l *logBuf) String() string {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.b.String()
}

func captureLog(t *testing.T) *logBuf {
	t.Helper()
	l := &logBuf{}
	log.SetOutput(l)
	t.Cleanup(func() { log.SetOutput(os.Stderr) })
	return l
}

type session struct {
	out  *sink
	in   *io.PipeWriter
	done chan struct{}
}

func start(t *testing.T, url string, tok tokenSource, c *http.Client) *session {
	t.Helper()
	s := newServer(newSink(), url, c, tok)
	out := s.out.(*sink)
	pr, pw := io.Pipe()
	done := make(chan struct{})
	go func() { serve(s, pr); close(done) }()
	t.Cleanup(func() { pw.Close() })
	return &session{out, pw, done}
}

func (s *session) send(line string) { io.WriteString(s.in, line+"\n") }

func (s *session) stop(t *testing.T) {
	t.Helper()
	s.in.Close()
	select {
	case <-s.done:
	case <-time.After(5 * time.Second):
		t.Fatal("serve did not return")
	}
}

func (s *session) init(t *testing.T) {
	t.Helper()
	s.send(`{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}`)
}

func TestInitializeThenUpdate(t *testing.T) {
	captureLog(t)
	var got atomic.Value
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		got.Store(r.Header.Clone())
		io.WriteString(w, okBody)
	}))
	defer ts.Close()
	s := start(t, ts.URL+usagePath, fakeToken, ts.Client())
	s.init(t)
	resp := s.out.next(t)
	if resp["id"] != float64(0) || resp["result"] == nil {
		t.Fatalf("first line is not the initialize response: %v", resp)
	}
	if loading := s.out.nextUpdate(t); loading["text"] != "Usage …" {
		t.Fatalf("loading state first, got %v", loading["text"])
	}
	u := s.out.nextUpdate(t)
	if u["item"] != "usage" || u["text"] != "16% · 59%" {
		t.Fatalf("update = %v", u)
	}
	s.stop(t)
	h := got.Load().(http.Header)
	if h.Get("Authorization") != "Bearer "+secret || h.Get("anthropic-beta") != "oauth-2025-04-20" {
		t.Fatalf("headers = %v", h)
	}
}

func TestClickRefreshes(t *testing.T) {
	captureLog(t)
	var n atomic.Int32
	hit := make(chan int32, 8)
	release := make(chan struct{})
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		i := n.Add(1)
		hit <- i
		if i == 2 {
			<-release
		}
		io.WriteString(w, okBody)
	}))
	defer ts.Close()
	s := start(t, ts.URL+usagePath, fakeToken, ts.Client())
	s.init(t)
	s.out.nextUpdate(t) // loading
	s.out.nextUpdate(t) // first result
	click := func(id int) {
		s.send(`{"jsonrpc":"2.0","id":` + string(rune('0'+id)) + `,"method":"item/action","params":{"item":"usage"}}`)
	}
	if <-hit != 1 { // the initial fetch
		t.Fatal("unexpected")
	}
	click(1)
	if <-hit != 2 {
		t.Fatal("click did not fetch")
	}
	click(2) // in flight: nothing
	for {
		m := s.out.next(t)
		if m["id"] == float64(2) {
			if v, ok := m["result"]; !ok || v != nil {
				t.Fatalf("item/action must answer null, got %v", m)
			}
			break
		}
	}
	close(release)
	s.out.nextUpdate(t)
	s.stop(t)
	if n.Load() != 2 {
		t.Fatalf("requests = %d", n.Load())
	}
}

func TestExpiredNeverRefreshes(t *testing.T) {
	captureLog(t)
	dir := t.TempDir()
	t.Setenv("CLAUDE_CONFIG_DIR", dir)
	creds := []byte(`{"claudeAiOauth": {"accessToken": "` + secret + `", "refreshToken": "r"}}`)
	path := filepath.Join(dir, ".credentials.json")
	if err := os.WriteFile(path, creds, 0o600); err != nil {
		t.Fatal(err)
	}
	old := keychain
	keychain = func(context.Context) (string, error) { return "", errNoLogin }
	defer func() { keychain = old }()

	var mu sync.Mutex
	var paths []string
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		paths = append(paths, r.URL.Path)
		mu.Unlock()
		w.WriteHeader(http.StatusUnauthorized)
	}))
	defer ts.Close()
	s := start(t, ts.URL+usagePath, claudeToken, ts.Client())
	s.init(t)
	s.out.nextUpdate(t)
	u := s.out.nextUpdate(t)
	s.stop(t)
	if u["text"] != "Usage" || u["tone"] != "warning" {
		t.Fatalf("update = %v", u)
	}
	after, _ := os.ReadFile(path)
	if !bytes.Equal(after, creds) {
		t.Fatal("credentials file changed")
	}
	if len(paths) != 1 || paths[0] != "/api/oauth/usage" {
		t.Fatalf("requests = %v", paths)
	}
}

func get(t *testing.T, h http.HandlerFunc) ([]window, error) {
	t.Helper()
	ts := httptest.NewServer(h)
	defer ts.Close()
	return fetch(context.Background(), ts.Client(), ts.URL+usagePath, secret)
}

func TestRateLimited(t *testing.T) {
	_, err := get(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Retry-After", "120")
		w.WriteHeader(429)
	})
	var rl rateLimited
	if !errors.As(err, &rl) || rl.retry != 120*time.Second {
		t.Fatalf("err = %v", err)
	}
	_, err = get(t, func(w http.ResponseWriter, r *http.Request) { w.WriteHeader(429) })
	if !errors.As(err, &rl) || rl.retry != 15*time.Minute {
		t.Fatalf("err = %v", err)
	}
	_, err = get(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Retry-After", "soon")
		w.WriteHeader(429)
	})
	if !errors.As(err, &rl) || rl.retry != 15*time.Minute {
		t.Fatalf("bad Retry-After: err = %v", err)
	}
	v := view{updated: at(14, 0), failed: now, err: rl}
	if got := next(v, 120*time.Second, now); !got.Equal(now.Add(120 * time.Second)) {
		t.Fatalf("next = %v", got)
	}
}

func TestFetchErrors(t *testing.T) {
	if _, err := get(t, func(w http.ResponseWriter, r *http.Request) { w.WriteHeader(401) }); !errors.Is(err, errExpired) {
		t.Fatalf("401: %v", err)
	}
	if _, err := get(t, func(w http.ResponseWriter, r *http.Request) { w.WriteHeader(503) }); err == nil || !strings.Contains(err.Error(), "503") {
		t.Fatalf("503: %v", err)
	}
	if _, err := get(t, func(w http.ResponseWriter, r *http.Request) { io.WriteString(w, "<html>") }); !errors.Is(err, errUnreadable) {
		t.Fatalf("garbage: %v", err)
	}
	ws, err := get(t, func(w http.ResponseWriter, r *http.Request) { io.WriteString(w, okBody) })
	if err != nil || len(ws) != 2 {
		t.Fatalf("ok: %v %v", ws, err)
	}
}

func TestNext(t *testing.T) {
	ws := func(resets ...time.Time) []window {
		var out []window
		for _, r := range resets {
			out = append(out, window{Label: labelSession, Resets: r})
		}
		return out
	}
	tests := []struct {
		name string
		v    view
		now  time.Time
		want time.Time
	}{
		{"five minutes after the update", view{updated: at(14, 0)}, now, at(14, 5)},
		{"a reset soon after wins", view{updated: at(14, 0), windows: ws(at(14, 2))}, now, at(14, 2).Add(5 * time.Second)},
		{"the earliest reset", view{updated: at(14, 0), windows: ws(at(14, 3), at(14, 2))}, now, at(14, 2).Add(5 * time.Second)},
		{"a reset before the update is ignored", view{updated: at(14, 0), windows: ws(at(13, 0))}, now, at(14, 5)},
		{"a reset at the update is ignored", view{updated: at(14, 0), windows: ws(at(14, 0))}, now, at(14, 5)},
		{"a reset far off is not sooner", view{updated: at(14, 0), windows: ws(at(20, 0))}, now, at(14, 5)},
		{"a reset missed since the update is due at once", view{updated: at(14, 0), windows: ws(at(14, 1))}, now.Add(3 * time.Hour), at(14, 1).Add(5 * time.Second)},
		{"after a sleep it is due at once", view{updated: at(14, 0)}, now.Add(3 * time.Hour), at(14, 5)},
		{"a failure counts as an attempt", view{updated: at(14, 0), failed: at(14, 10), err: errors.New("x")}, now, at(14, 15)},
		{"never fetched is due at once", view{}, now, time.Time{}.Add(5 * time.Minute)},
	}
	for _, tc := range tests {
		got := next(tc.v, 0, tc.now)
		if !got.Equal(tc.want) {
			t.Errorf("%s: got %v want %v", tc.name, got, tc.want)
		}
	}
	if d := next(view{updated: at(14, 0)}, 0, now.Add(3*time.Hour)); d.After(now.Add(3 * time.Hour)) {
		t.Error("not due after a sleep")
	}
}

func TestTokenNeverLogged(t *testing.T) {
	logs := captureLog(t)
	leaky := func(code int, body string) http.HandlerFunc {
		return func(w http.ResponseWriter, r *http.Request) {
			// the body and a header echo the token: neither may reach the card or the log
			w.Header().Set("X-Echo", r.Header.Get("Authorization"))
			w.WriteHeader(code)
			io.WriteString(w, body+r.Header.Get("Authorization"))
		}
	}
	cases := map[string]http.HandlerFunc{
		"ok":      func(w http.ResponseWriter, r *http.Request) { io.WriteString(w, okBody) },
		"401":     leaky(401, ""),
		"429":     leaky(429, ""),
		"500":     leaky(500, "boom "),
		"garbage": leaky(200, "not json "),
	}
	var all strings.Builder
	for name, h := range cases {
		ts := httptest.NewServer(h)
		s := start(t, ts.URL+usagePath, fakeToken, ts.Client())
		s.init(t)
		s.out.nextUpdate(t)
		s.out.nextUpdate(t)
		s.stop(t)
		all.WriteString(s.out.text())
		ts.Close()
		if strings.Contains(s.out.text(), secret) {
			t.Errorf("%s: token in output", name)
		}
	}
	// a dead server: the transport error
	ts := httptest.NewServer(http.NotFoundHandler())
	url, c := ts.URL+usagePath, ts.Client()
	ts.Close()
	s := start(t, url, fakeToken, c)
	s.init(t)
	s.out.nextUpdate(t)
	u := s.out.nextUpdate(t)
	s.stop(t)
	if !strings.Contains(u["text"].(string), "Usage") {
		t.Fatalf("update = %v", u)
	}
	all.WriteString(s.out.text())
	// a token source that fails
	s = start(t, url, func(context.Context) (string, error) { return "", errors.New("keychain: " + "denied") }, c)
	s.init(t)
	s.out.nextUpdate(t)
	s.out.nextUpdate(t)
	s.stop(t)
	all.WriteString(s.out.text())

	if strings.Contains(all.String(), secret) || strings.Contains(logs.String(), secret) {
		t.Fatalf("token leaked.\nlog: %s", logs.String())
	}
	if logs.String() == "" {
		t.Fatal("failed fetches should be logged")
	}
}

func TestKeychainTimeout(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("CLAUDE_CONFIG_DIR", dir)
	called := false
	old := keychain
	keychain = func(context.Context) (string, error) { called = true; return secret, nil }
	defer func() { keychain = old }()

	ctx, cancel := context.WithDeadline(context.Background(), time.Now().Add(-time.Second))
	defer cancel()
	if _, err := claudeToken(ctx); !errors.Is(err, errNoLogin) {
		t.Fatalf("no file: err = %v", err)
	}
	if called {
		t.Fatal("an expired context must not start the keychain read")
	}
	os.WriteFile(filepath.Join(dir, ".credentials.json"), []byte(`{"claudeAiOauth":{"accessToken":" from-file\n"}}`), 0o600)
	if tok, err := claudeToken(ctx); err != nil || tok != "from-file" {
		t.Fatalf("file fallback: %q %v", tok, err)
	}
}

func TestClaudeTokenKeychain(t *testing.T) {
	if runtime.GOOS != "darwin" {
		t.Skip("the keychain is read on macOS only")
	}
	t.Setenv("CLAUDE_CONFIG_DIR", t.TempDir())
	old := keychain
	defer func() { keychain = old }()
	keychain = func(context.Context) (string, error) {
		return `{"claudeAiOauth":{"accessToken":"from-keychain"}}` + "\n", nil
	}
	if tok, err := claudeToken(context.Background()); err != nil || tok != "from-keychain" {
		t.Fatalf("%q %v", tok, err)
	}
	keychain = func(context.Context) (string, error) { return "garbage", nil }
	if _, err := claudeToken(context.Background()); !errors.Is(err, errNoLogin) {
		t.Fatalf("garbage secret: %v", err)
	}
}
