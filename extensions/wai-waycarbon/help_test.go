package main

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"wai-waycarbon/helpapi"
)

const token = "secret-token-123"

// fakeRelay speaks the relay's JSON: the device flow, the SSE stream and the request actions.
type fakeRelay struct {
	srv      *httptest.Server
	mu       sync.Mutex
	approved bool
	status   int // overrides GET /events when set
	syncs    int
	hits     int
	posts    []post
	streams  []chan []byte
	initial  []helpapi.Request
	answer   func(path string) (int, any) // overrides the answer to a POST /requests…
}

type post struct {
	Path string
	Auth string
	Body map[string]any
}

func newRelay(t *testing.T) *fakeRelay {
	t.Helper()
	f := &fakeRelay{}
	mux := http.NewServeMux()
	mux.HandleFunc("POST /auth/device", func(w http.ResponseWriter, r *http.Request) {
		json.NewEncoder(w).Encode(deviceStart{DeviceCode: "dc", UserCode: "ABCD-1234", VerificationURI: "https://github.com/login/device"})
	})
	mux.HandleFunc("POST /auth/device/poll", func(w http.ResponseWriter, r *http.Request) {
		f.mu.Lock()
		ok := f.approved
		f.mu.Unlock()
		if !ok {
			w.WriteHeader(http.StatusPreconditionRequired)
			io.WriteString(w, `{"error":"pending"}`)
			return
		}
		json.NewEncoder(w).Encode(map[string]string{"token": token, "login": "me"})
	})
	mux.HandleFunc("POST /auth/signout", func(w http.ResponseWriter, r *http.Request) {
		f.record(r)
		w.WriteHeader(http.StatusNoContent)
	})
	mux.HandleFunc("GET /events", func(w http.ResponseWriter, r *http.Request) {
		f.mu.Lock()
		f.hits++
		status := f.status
		f.mu.Unlock()
		if status != 0 {
			w.WriteHeader(status)
			io.WriteString(w, `{"error":"Only members of WayCarbon can use help requests"}`)
			return
		}
		if r.Header.Get("Authorization") != "Bearer "+token {
			w.WriteHeader(http.StatusUnauthorized)
			return
		}
		ch := make(chan []byte, 16)
		f.mu.Lock()
		f.syncs++
		f.streams = append(f.streams, ch)
		initial := f.initial
		f.mu.Unlock()
		w.Header().Set("Content-Type", "text/event-stream")
		w.WriteHeader(http.StatusOK)
		io.WriteString(w, ": hello\n\n")
		if initial == nil {
			initial = []helpapi.Request{}
		}
		w.Write(sse(helpapi.EvSync, initial))
		w.(http.Flusher).Flush()
		for {
			select {
			case <-r.Context().Done():
				return
			case b, ok := <-ch:
				if !ok {
					return
				}
				w.Write(b)
				w.(http.Flusher).Flush()
			}
		}
	})
	mux.HandleFunc("POST /requests", func(w http.ResponseWriter, r *http.Request) { f.act(w, r, "/requests") })
	mux.HandleFunc("POST /requests/{id}/{action}", func(w http.ResponseWriter, r *http.Request) { f.act(w, r, r.URL.Path) })
	f.srv = httptest.NewServer(mux)
	t.Cleanup(f.srv.Close)
	t.Setenv("WAI_WAYCARBON_RELAY", f.srv.URL)
	return f
}

func sse(event string, v any) []byte {
	b, _ := json.Marshal(v)
	return []byte("event: " + event + "\ndata: " + string(b) + "\n\n")
}

func (f *fakeRelay) record(r *http.Request) post {
	var body map[string]any
	json.NewDecoder(r.Body).Decode(&body)
	p := post{r.URL.Path, r.Header.Get("Authorization"), body}
	f.mu.Lock()
	f.posts = append(f.posts, p)
	f.mu.Unlock()
	return p
}

func (f *fakeRelay) act(w http.ResponseWriter, r *http.Request, path string) {
	p := f.record(r)
	f.mu.Lock()
	answer := f.answer
	f.mu.Unlock()
	code, v := http.StatusOK, any(nil)
	if answer != nil {
		code, v = answer(path)
	}
	if v == nil && path == "/requests" {
		v = helpapi.Request{ID: "r1", From: "me", To: p.Body["to"].(string), State: helpapi.Waiting, Text: p.Body["text"].(string), Created: time.Now()}
	} else if v == nil {
		state := map[string]helpapi.State{"accept": helpapi.Accepted, "decline": helpapi.Declined, "reply": helpapi.Replied, "cancel": helpapi.Cancelled}
		v = helpapi.Request{ID: r.PathValue("id"), State: state[r.PathValue("action")]}
	}
	w.WriteHeader(code)
	json.NewEncoder(w).Encode(v)
}

func (f *fakeRelay) push(event string, v any) {
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, ch := range f.streams {
		select {
		case ch <- sse(event, v):
		default:
		}
	}
}

func (f *fakeRelay) closeStreams() {
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, ch := range f.streams {
		close(ch)
	}
	f.streams = nil
}

// waitStream waits until a client holds a stream, which has then been sent its sync.
func (f *fakeRelay) waitStream(t *testing.T) {
	t.Helper()
	for i := 0; i < 200; i++ {
		f.mu.Lock()
		n := len(f.streams)
		f.mu.Unlock()
		if n > 0 {
			return
		}
		time.Sleep(25 * time.Millisecond)
	}
	t.Fatal("the client never opened the stream")
}

func (f *fakeRelay) count() (syncs, hits int) {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.syncs, f.hits
}

func (f *fakeRelay) lastPost(t *testing.T, path string) post {
	t.Helper()
	for i := 0; i < 200; i++ {
		f.mu.Lock()
		for j := len(f.posts) - 1; j >= 0; j-- {
			if f.posts[j].Path == path {
				p := f.posts[j]
				f.mu.Unlock()
				return p
			}
		}
		f.mu.Unlock()
		time.Sleep(25 * time.Millisecond)
	}
	t.Fatalf("no POST %s", path)
	return post{}
}

func fakeGit(t *testing.T) {
	t.Helper()
	fakeCLI(t, "git", `case "$*" in
  "-C /proj remote get-url origin") echo "https://user:ghp_x@github.com/waycarbon/inventories.git" ;;
  "-C /proj branch --show-current") echo "feat/x" ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
`)
}

// helpRPC starts the extension signed in (auth.json is there) with a project and the given sessions.
func helpRPC(t *testing.T, signedIn bool, sessions string) (*rpc, string) {
	t.Helper()
	fakeTools(t)
	fakeGit(t)
	minPoll = 10 * time.Millisecond
	dir := t.TempDir()
	if signedIn {
		if err := saveAuth(dir, auth{token, "me"}); err != nil {
			t.Fatal(err)
		}
	}
	r := start(t)
	r.sessions = sessions
	r.send(`{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":1,"ext_dir":"/x","data_dir":"` + dir + `","project":"/proj"}}`)
	r.reply(t, 0)
	return r, dir
}

func helpTab(t *testing.T, m map[string]any) map[string]any {
	t.Helper()
	tabs := find(render(t, m), "tabs")["tabs"].([]any)
	return map[string]any{"type": "column", "children": tabs[3].(map[string]any)["children"]}
}

// helpUntil waits for a render whose Help tab satisfies ok, and returns that tab.
func (r *rpc) helpUntil(t *testing.T, ok func(tab map[string]any) bool) map[string]any {
	t.Helper()
	var tab map[string]any
	r.until(t, func(m map[string]any) bool {
		if m["method"] != "panel/render" {
			return false
		}
		tab = helpTab(t, m)
		return ok(tab)
	})
	return tab
}

func hasText(s string) func(map[string]any) bool {
	return func(tab map[string]any) bool { return strings.Contains(textOf(tab), s) }
}

func byText(n map[string]any, typ, text string) map[string]any {
	if n["type"] == typ && n["text"] == text {
		return n
	}
	kids, _ := n["children"].([]any)
	for _, k := range kids {
		if f := byText(k.(map[string]any), typ, text); f != nil {
			return f
		}
	}
	return nil
}

func (r *rpc) click(id int, action string, values string) {
	r.send(`{"jsonrpc":"2.0","id":` + string(rune('0'+id)) + `,"method":"panel/action","params":{"panel":"waycarbon","action_id":"` + action + `","values":` + values + `}}`)
}

func waitFile(t *testing.T, path string) []byte {
	t.Helper()
	for i := 0; i < 200; i++ {
		if b, err := os.ReadFile(path); err == nil {
			return b
		}
		time.Sleep(25 * time.Millisecond)
	}
	t.Fatalf("no file %s", path)
	return nil
}

func incoming(id, text string) helpapi.Request {
	return helpapi.Request{ID: id, From: "ana", To: "me", State: helpapi.Waiting, Text: text, Created: time.Now(),
		Project: helpapi.Project{Repo: "github.com/waycarbon/inventories", Branch: "main"}}
}

func TestNoRelayConfigured(t *testing.T) {
	t.Setenv("WAI_WAYCARBON_RELAY", "")
	r, dir := helpRPC(t, false, "")
	r.helpUntil(t, hasText("Help requests need the WayCarbon relay, which is not set up yet."))
	if _, err := os.Stat(filepath.Join(dir, "help.json")); err == nil {
		t.Error("nothing may be written without a relay")
	}
}

func TestSignInSavesToken0600(t *testing.T) {
	f := newRelay(t)
	stderr, err := os.CreateTemp(t.TempDir(), "stderr")
	if err != nil {
		t.Fatal(err)
	}
	old := os.Stderr
	os.Stderr = stderr
	t.Cleanup(func() { os.Stderr = old })

	r, dir := helpRPC(t, false, "")
	r.helpUntil(t, func(tab map[string]any) bool { return findID(tab, "help:signin") != nil })
	r.click(1, "help:signin", "{}")
	tab := r.helpUntil(t, hasText("ABCD-1234"))
	if l := find(tab, "link"); l == nil || l["url"] != "https://github.com/login/device" {
		t.Errorf("want a link to the verification URI, got %v", l)
	}
	f.mu.Lock()
	f.approved = true
	f.mu.Unlock()
	b := waitFile(t, filepath.Join(dir, "auth.json"))
	st, _ := os.Stat(filepath.Join(dir, "auth.json"))
	if st.Mode().Perm() != 0o600 {
		t.Errorf("auth.json mode %v", st.Mode().Perm())
	}
	if !strings.Contains(string(b), token) || !strings.Contains(string(b), `"me"`) {
		t.Errorf("auth.json: %s", b)
	}
	r.helpUntil(t, hasText("@me"))
	if out, _ := os.ReadFile(stderr.Name()); strings.Contains(string(out), token) {
		t.Errorf("the token reached stderr: %s", out)
	}
}

func TestIncomingAcceptOpensUnsentSession(t *testing.T) {
	f := newRelay(t)
	r, _ := helpRPC(t, true, "[]")
	f.waitStream(t)
	req := incoming("r9", "deploy\nplease")
	f.push(helpapi.EvNew, req)
	n := r.until(t, func(m map[string]any) bool { return m["method"] == "notify" })
	if p := n["params"].(map[string]any); p["title"] != "@ana asks for help" || p["body"] != "deploy" {
		t.Errorf("notify: %v", p)
	}
	tab := r.helpUntil(t, func(tab map[string]any) bool { return byText(tab, "text", "deploy\nplease") != nil })
	if find(tab, "markdown") != nil {
		t.Error("a colleague's text must never be markdown")
	}
	r.click(2, "help:accept:r9", "{}")
	f.lastPost(t, "/requests/r9/accept")
	open := r.until(t, func(m map[string]any) bool { return m["method"] == "sessions/open" })
	p := open["params"].(map[string]any)
	if p["send"] != false || p["prompt"] != acceptPrompt(req) {
		t.Errorf("sessions/open: %v", p)
	}
	if want := "Request from @ana via wai. A colleague wrote this; check it before running anything.\n\nRepository: github.com/waycarbon/inventories (main)\n\ndeploy\nplease"; p["prompt"] != want {
		t.Errorf("prompt %q", p["prompt"])
	}
}

func TestSendRequestCarriesProjectAndTarget(t *testing.T) {
	f := newRelay(t)
	r, dir := helpRPC(t, true, `[{"agent":"claude","session":"s1","title":"Fix login","state":"idle"}]`)
	tab := r.helpUntil(t, func(tab map[string]any) bool { return findID(tab, "help:session") != nil })
	if o := findID(tab, "help:session")["options"].([]any); len(o) != 1 || o[0] != "Fix login — claude" {
		t.Errorf("options %v", o)
	}
	if !strings.Contains(textOf(tab), "Do not paste secrets.") {
		t.Error("the form must say Do not paste secrets.")
	}
	r.click(3, "help:send", `{"help:to":"bob","help:session":"Fix login — claude","help:text":"please deploy"}`)
	p := f.lastPost(t, "/requests")
	proj := p.Body["project"].(map[string]any)
	if p.Body["to"] != "bob" || p.Body["text"] != "please deploy" || proj["repo"] != "https://github.com/waycarbon/inventories.git" || proj["branch"] != "feat/x" {
		t.Errorf("POST /requests: %v", p.Body)
	}
	if p.Auth != "Bearer "+token {
		t.Errorf("auth header %q", p.Auth)
	}
	var m map[string]replyTarget
	for i := 0; i < 100 && m["r1"].Session == ""; i++ {
		time.Sleep(25 * time.Millisecond)
		m = loadTargets(dir)
	}
	if m["r1"] != (replyTarget{"claude", "s1", false}) {
		t.Errorf("help.json: %v", m)
	}
	r.helpUntil(t, hasText("Waiting for @bob"))
}

func TestSendFailureKeepsText(t *testing.T) {
	f := newRelay(t)
	f.answer = func(string) (int, any) { return 429, map[string]string{"error": "too many requests"} }
	r, _ := helpRPC(t, true, "[]")
	r.helpUntil(t, hasText("@me"))
	r.click(3, "help:send", `{"help:to":"bob","help:text":"please deploy"}`)
	r.helpUntil(t, func(tab map[string]any) bool { return find(tab, "spinner") != nil })
	tab := r.helpUntil(t, hasText("too many requests"))
	if in := findID(tab, "help:text"); in == nil || in["value"] != "please deploy" {
		t.Errorf("the typed text must come back: %v", in)
	}
	if in := findID(tab, "help:to"); in == nil || in["value"] != "bob" {
		t.Errorf("the recipient must come back: %v", in)
	}
}

func TestPersonFillsTo(t *testing.T) {
	f := newRelay(t)
	r, _ := helpRPC(t, true, "[]")
	f.waitStream(t)
	f.push(helpapi.EvPeople, []helpapi.Person{{Login: "bob", Online: true}})
	r.helpUntil(t, func(tab map[string]any) bool { return findID(tab, "help:person:bob") != nil })
	r.click(4, "help:person:bob", `{"help:to":"typed"}`)
	r.helpUntil(t, func(tab map[string]any) bool { in := findID(tab, "help:to"); return in != nil && in["value"] == "bob" })
}

func TestReplyGoesToIdleSessionOnly(t *testing.T) {
	f := newRelay(t)
	sent := helpapi.Request{ID: "r1", From: "me", To: "bob", State: helpapi.Accepted, Text: "deploy", Created: time.Now()}
	f.initial = []helpapi.Request{sent}
	fakeTools(t)
	dir := t.TempDir()
	saveAuth(dir, auth{token, "me"})
	saveTargets(dir, map[string]replyTarget{"r1": {Agent: "claude", Session: "s1"}})
	r2, _ := restartIn(t, dir, `[{"agent":"claude","session":"s1","title":null,"state":"working"}]`)
	r2.helpUntil(t, hasText("Accepted by @bob"))
	f.push(helpapi.EvUpdated, helpapi.Update{ID: "r1", State: helpapi.Replied, Reply: "done"})
	n := r2.until(t, func(m map[string]any) bool { return m["method"] == "notify" })
	if p := n["params"].(map[string]any); p["title"] != "@bob replied" {
		t.Errorf("notify: %v", p)
	}
	bar := r2.until(t, func(m map[string]any) bool {
		return m["method"] == "item/update" && m["params"].(map[string]any)["text"] == "1"
	})
	if bar == nil {
		t.Fatal("the bar counts the unsent reply")
	}
	tab := r2.helpUntil(t, func(tab map[string]any) bool {
		b := findID(tab, "help:tosession:r1")
		return b != nil && b["disabled"] == true
	})
	if !strings.Contains(textOf(tab), "done") {
		t.Error("the reply shows")
	}
	r2.send(`{"jsonrpc":"2.0","method":"event/session_state","params":{"agent":"claude","session":"s1","state":"idle"}}`)
	r2.helpUntil(t, func(tab map[string]any) bool {
		b := findID(tab, "help:tosession:r1")
		return b != nil && b["disabled"] == false
	})
	r2.sessions = `[{"agent":"claude","session":"s1","title":null,"state":"idle"}]`
	r2.click(5, "help:tosession:r1", "{}")
	pr := r2.until(t, func(m map[string]any) bool { return m["method"] == "sessions/prompt" })
	p := pr["params"].(map[string]any)
	if p["text"] != "@bob replied to your request: done" || p["agent"] != "claude" || p["session"] != "s1" {
		t.Errorf("sessions/prompt: %v", p)
	}
	r2.until(t, func(m map[string]any) bool {
		return m["method"] == "item/update" && m["params"].(map[string]any)["text"] == nil
	})
	var got map[string]replyTarget
	for i := 0; i < 100 && !got["r1"].Sent; i++ {
		time.Sleep(25 * time.Millisecond)
		got = loadTargets(dir)
	}
	if !got["r1"].Sent {
		t.Errorf("help.json: %v", got)
	}
}

// restartIn starts the extension on an existing data dir.
func restartIn(t *testing.T, dir, sessions string) (*rpc, string) {
	t.Helper()
	r := start(t)
	r.sessions = sessions
	r.send(`{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":1,"ext_dir":"/x","data_dir":"` + dir + `","project":"/proj"}}`)
	r.reply(t, 0)
	return r, dir
}

func TestGoneSessionOffersNewSession(t *testing.T) {
	f := newRelay(t)
	f.initial = []helpapi.Request{{ID: "r1", From: "me", To: "bob", State: helpapi.Replied, Text: "deploy", Reply: "done", Created: time.Now()}}
	fakeTools(t)
	dir := t.TempDir()
	saveAuth(dir, auth{token, "me"})
	saveTargets(dir, map[string]replyTarget{"r1": {Agent: "claude", Session: "closed"}})
	r, _ := restartIn(t, dir, "[]")
	r.helpUntil(t, func(tab map[string]any) bool { return findID(tab, "help:newsession:r1") != nil })
	r.click(6, "help:newsession:r1", "{}")
	p := r.until(t, func(m map[string]any) bool { return m["method"] == "sessions/open" })["params"].(map[string]any)
	if p["send"] != false || p["prompt"] != "@bob replied to your request: done" {
		t.Errorf("sessions/open: %v", p)
	}
}

func TestStreamReconnects(t *testing.T) {
	f := newRelay(t)
	r, dir := helpRPC(t, true, "[]")
	f.waitStream(t)
	f.closeStreams()
	for i := 0; i < 200; i++ {
		if s, _ := f.count(); s >= 2 {
			break
		}
		time.Sleep(50 * time.Millisecond)
	}
	if s, _ := f.count(); s < 2 {
		t.Fatalf("the client did not reconnect: %d syncs", s)
	}
	f.waitStream(t)
	f.mu.Lock()
	f.status = 401
	f.mu.Unlock()
	f.closeStreams()
	r.helpUntil(t, func(tab map[string]any) bool { return findID(tab, "help:signin") != nil })
	if _, err := os.Stat(filepath.Join(dir, "auth.json")); err == nil {
		t.Error("a 401 must delete auth.json")
	}
}

func TestForbiddenStopsTheStream(t *testing.T) {
	f := newRelay(t)
	f.status = 403
	r, dir := helpRPC(t, true, "[]")
	r.helpUntil(t, hasText("Only members of WayCarbon can use help requests"))
	time.Sleep(1500 * time.Millisecond)
	if _, hits := f.count(); hits != 1 {
		t.Errorf("a 403 must not retry: %d connections", hits)
	}
	if _, err := os.Stat(filepath.Join(dir, "auth.json")); err != nil {
		t.Error("a 403 keeps auth.json")
	}
	f.mu.Lock()
	f.status = 0
	f.mu.Unlock()
	r.click(7, "help:retry", "{}")
	r.helpUntil(t, func(tab map[string]any) bool { return findID(tab, "help:send") != nil })
}

func TestSignOutRevokesAndDeletes(t *testing.T) {
	f := newRelay(t)
	r, dir := helpRPC(t, true, "[]")
	r.helpUntil(t, hasText("@me"))
	r.click(8, "help:signout", "{}")
	r.helpUntil(t, func(tab map[string]any) bool { return findID(tab, "help:signin") != nil })
	if p := f.lastPost(t, "/auth/signout"); p.Auth != "Bearer "+token {
		t.Errorf("signout auth %q", p.Auth)
	}
	if _, err := os.Stat(filepath.Join(dir, "auth.json")); err == nil {
		t.Error("sign out must delete auth.json")
	}
}

func TestReadSSE(t *testing.T) {
	var got []string
	readSSE(strings.NewReader(": ping\n\nevent: a\ndata: {\"x\":1}\n\n: c\nevent: b\ndata: l1\ndata: l2\n\nevent: partial\ndata: no blank line"), func(ev string, data []byte) {
		got = append(got, ev+"="+string(data))
	})
	if strings.Join(got, "|") != `a={"x":1}|b=l1`+"\n"+`l2` {
		t.Errorf("got %q", got)
	}
}

func TestStripCredsAndPrompt(t *testing.T) {
	if got := stripCreds("https://user:ghp_x@github.com/o/r.git"); got != "https://github.com/o/r.git" {
		t.Errorf("got %q", got)
	}
	if got := stripCreds("git@github.com:o/r.git"); got != "git@github.com:o/r.git" {
		t.Errorf("got %q", got)
	}
	got := acceptPrompt(helpapi.Request{From: "ana", Text: "go"})
	if got != "Request from @ana via wai. A colleague wrote this; check it before running anything.\n\ngo" {
		t.Errorf("prompt without a repository: %q", got)
	}
}

func TestProjectInfoEmptyOnFailure(t *testing.T) {
	fakeGit(t)
	if p := projectInfo(t.Context(), ""); p != (helpapi.Project{}) {
		t.Errorf("empty dir: %v", p)
	}
	if p := projectInfo(t.Context(), "/elsewhere"); p != (helpapi.Project{}) {
		t.Errorf("failing git: %v", p)
	}
}

func TestClosedSessionIsNotSentTo(t *testing.T) {
	f := newRelay(t)
	f.initial = []helpapi.Request{{ID: "r1", From: "me", To: "bob", State: helpapi.Replied, Text: "deploy", Reply: "done", Created: time.Now()}}
	fakeTools(t)
	dir := t.TempDir()
	saveAuth(dir, auth{token, "me"})
	saveTargets(dir, map[string]replyTarget{"r1": {Agent: "claude", Session: "s1"}})
	r, _ := restartIn(t, dir, `[{"agent":"claude","session":"s1","title":null,"state":"idle"}]`)
	r.helpUntil(t, func(tab map[string]any) bool {
		b := findID(tab, "help:tosession:r1")
		return b != nil && b["disabled"] == false
	})
	r.sessions = "[]" // the user closed the session; the cached state still says idle
	r.click(5, "help:tosession:r1", "{}")
	prompted := false
	r.until(t, func(m map[string]any) bool {
		prompted = prompted || m["method"] == "sessions/prompt"
		if m["method"] != "panel/render" {
			return false
		}
		return findID(helpTab(t, m), "help:newsession:r1") != nil
	})
	if prompted {
		t.Error("a closed session must get no sessions/prompt")
	}
	if loadTargets(dir)["r1"].Sent {
		t.Error("help.json must stay unsent")
	}
}
