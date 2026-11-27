package main

import (
	"bytes"
	"encoding/json"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/coder/websocket"

	"wai-escritorio/office"
)

// outBuf collects what the extension writes to wai, line by line.
type outBuf struct {
	mu    sync.Mutex
	buf   []byte
	lines []map[string]json.RawMessage
	trees map[string]string // panel to the last tree sent
}

func (o *outBuf) Write(p []byte) (int, error) {
	o.mu.Lock()
	defer o.mu.Unlock()
	o.buf = append(o.buf, p...)
	for {
		i := bytes.IndexByte(o.buf, '\n')
		if i < 0 {
			return len(p), nil
		}
		var m map[string]json.RawMessage
		if json.Unmarshal(o.buf[:i], &m) == nil {
			o.lines = append(o.lines, m)
			if string(m["method"]) == `"panel/render"` {
				var p struct {
					Panel string
					Tree  json.RawMessage
				}
				json.Unmarshal(m["params"], &p)
				if o.trees == nil {
					o.trees = map[string]string{}
				}
				o.trees[p.Panel] = string(p.Tree)
			}
		}
		o.buf = o.buf[i+1:]
	}
}

type harness struct {
	t      *testing.T
	s      *ext
	in     *io.PipeWriter
	out    *outBuf
	clock  atomic.Int64 // unix nanos
	dir    string
	nextID int
}

func newHarness(t *testing.T, dir string) *harness {
	h := &harness{t: t, out: &outBuf{}, dir: dir}
	h.clock.Store(time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC).UnixNano())
	h.s = newExt(h.out, func() time.Time { return time.Unix(0, h.clock.Load()) })
	pr, pw := io.Pipe()
	h.in = pw
	done := make(chan struct{})
	go func() { defer close(done); serve(h.s, pr) }()
	t.Cleanup(func() { pw.Close(); <-done })
	return h
}

func (h *harness) advance(d time.Duration) { h.clock.Add(int64(d)) }

func (h *harness) send(method string, params any) {
	h.t.Helper()
	b, _ := json.Marshal(map[string]any{"jsonrpc": "2.0", "method": method, "params": params})
	h.in.Write(append(b, '\n'))
}

func (h *harness) initialize() {
	h.t.Helper()
	b, _ := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": map[string]any{"data_dir": h.dir}})
	h.in.Write(append(b, '\n'))
	h.barrier()
}

func (h *harness) action(id string, values map[string]any) {
	h.t.Helper()
	h.send("panel/action", map[string]any{"panel": "presence", "action_id": id, "values": values})
}

func (h *harness) key(k string) {
	h.t.Helper()
	h.send("panel/key", map[string]any{"panel": "office", "scene": "office", "key": k, "repeat": false})
}

// barrier returns once the extension has handled everything sent before it.
func (h *harness) barrier() {
	h.t.Helper()
	h.nextID++
	id := 1000 + h.nextID
	b, _ := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": id, "method": "panel/action", "params": map[string]any{"panel": "presence", "action_id": "noop", "values": map[string]any{}}})
	h.in.Write(append(b, '\n'))
	h.until(func() bool {
		h.out.mu.Lock()
		defer h.out.mu.Unlock()
		for _, l := range h.out.lines {
			if string(l["id"]) == strconv.Itoa(id) {
				return true
			}
		}
		return false
	})
}

func (h *harness) until(f func() bool) {
	h.t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for !f() {
		if time.Now().After(deadline) {
			h.t.Fatal("timed out waiting")
		}
		time.Sleep(5 * time.Millisecond)
	}
}

func (h *harness) tree(panel string) string {
	h.out.mu.Lock()
	defer h.out.mu.Unlock()
	return h.out.trees[panel]
}

func (h *harness) waitTree(panel, sub string) {
	h.t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for !strings.Contains(h.tree(panel), sub) {
		if time.Now().After(deadline) {
			h.t.Fatalf("panel %s never showed %q; last tree: %s", panel, sub, h.tree(panel))
		}
		time.Sleep(5 * time.Millisecond)
	}
}

func (h *harness) methods(name string) []map[string]json.RawMessage {
	h.out.mu.Lock()
	defer h.out.mu.Unlock()
	var out []map[string]json.RawMessage
	for _, l := range h.out.lines {
		if string(l["method"]) == strconv.Quote(name) {
			out = append(out, l)
		}
	}
	return out
}

// fakeOffice is a scripted office server: it welcomes one client at a chosen tile and records
// every frame the client sends.
type fakeOffice struct {
	srv    *httptest.Server
	rows   []string
	start  office.Point
	refuse string // when set, answer hello with this refusal
	hellos chan office.Hello
	frames chan string
	closed chan struct{}
	mu     sync.Mutex
	conn   *websocket.Conn
	conns  atomic.Int32
}

func newFakeOffice(t *testing.T, start office.Point) *fakeOffice {
	f := &fakeOffice{rows: office.DefaultMap().Rows, start: start, hellos: make(chan office.Hello, 8), frames: make(chan string, 64), closed: make(chan struct{}, 8)}
	f.srv = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		c, err := websocket.Accept(w, r, nil)
		if err != nil {
			return
		}
		f.conns.Add(1)
		defer func() { f.closed <- struct{}{} }()
		_, b, err := c.Read(r.Context())
		if err != nil {
			return
		}
		var h office.Hello
		json.Unmarshal(b, &h)
		f.hellos <- h
		if f.refuse != "" {
			out, _ := json.Marshal(office.Refused{T: office.TRefused, Reason: f.refuse})
			c.Write(r.Context(), websocket.MessageText, out)
			c.Close(websocket.StatusNormalClosure, "")
			return
		}
		out, _ := json.Marshal(office.Welcome{T: office.TWelcome, You: "me", Map: f.rows, People: []office.Person{{ID: "me", Name: h.Name, Avatar: h.Avatar, X: f.start.X, Y: f.start.Y}}})
		c.Write(r.Context(), websocket.MessageText, out)
		f.mu.Lock()
		f.conn = c
		f.mu.Unlock()
		for {
			_, b, err := c.Read(r.Context())
			if err != nil {
				return
			}
			f.frames <- string(b)
		}
	}))
	t.Cleanup(f.srv.Close)
	return f
}

func (f *fakeOffice) url() string { return "ws" + strings.TrimPrefix(f.srv.URL, "http") }

func (f *fakeOffice) frame(t *testing.T) string {
	t.Helper()
	select {
	case s := <-f.frames:
		return s
	case <-time.After(3 * time.Second):
		t.Fatal("no frame from the extension")
	}
	return ""
}

func (f *fakeOffice) noFrame(t *testing.T) {
	t.Helper()
	select {
	case s := <-f.frames:
		t.Fatalf("unexpected frame %s", s)
	case <-time.After(300 * time.Millisecond):
	}
}

// connectedHarness has a saved setup pointing at f and waits for the welcome to show.
func connectedHarness(t *testing.T, f *fakeOffice) *harness {
	t.Helper()
	dir := t.TempDir()
	if err := saveSetup(dir, Setup{Name: "Ana", Server: f.url(), Token: testToken, Avatar: 2}); err != nil {
		t.Fatal(err)
	}
	h := newHarness(t, dir)
	h.initialize()
	h.waitTree("office", `"scene"`)
	return h
}

func (h *harness) myTile() (int, int) {
	var tree struct {
		Children []struct {
			Sprites []struct {
				ID   string
				X, Y int
			}
		}
	}
	json.Unmarshal([]byte(h.tree("office")), &tree)
	for _, s := range tree.Children[0].Sprites {
		if s.ID == "me" {
			return s.X, s.Y
		}
	}
	h.t.Fatal("no own sprite")
	return 0, 0
}

func TestNoSetupNoDial(t *testing.T) {
	f := newFakeOffice(t, office.Point{X: 10, Y: 16})
	dir := t.TempDir()
	h := newHarness(t, dir)
	h.initialize()
	for _, p := range []string{"presence", "office"} {
		tree := h.tree(p)
		for _, id := range []string{`"id":"name"`, `"id":"server"`, `"id":"token"`, `"id":"avatar"`} {
			if !strings.Contains(tree, id) {
				t.Errorf("%s has no %s: %s", p, id, tree)
			}
		}
	}
	time.Sleep(300 * time.Millisecond)
	if n := f.conns.Load(); n != 0 {
		t.Fatalf("dialled %d times before the form was submitted", n)
	}
	h.action("save", map[string]any{"name": "Ana", "server": f.url(), "token": testToken, "avatar": "Green"})
	select {
	case hello := <-f.hellos:
		if hello.Name != "Ana" || hello.Token != testToken || hello.Avatar != 2 {
			t.Errorf("hello %+v", hello)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("no hello after submit")
	}
	if st, _ := loadSetup(dir); st == nil || st.Name != "Ana" {
		t.Errorf("setup not saved: %+v", st)
	}
}

func TestSubmitKeepsFormOnBadName(t *testing.T) {
	f := newFakeOffice(t, office.Point{})
	h := newHarness(t, t.TempDir())
	h.initialize()
	h.action("save", map[string]any{"name": "", "server": f.url(), "token": testToken, "avatar": "Red"})
	h.barrier()
	h.waitTree("presence", "Name must be 1 to 32 characters")
	if f.conns.Load() != 0 {
		t.Error("dialled with a bad name")
	}
}

func TestTokenRefusedShowsForm(t *testing.T) {
	f := newFakeOffice(t, office.Point{})
	f.refuse = office.ReasonToken
	dir := t.TempDir()
	saveSetup(dir, Setup{Name: "Ana", Server: f.url(), Token: testToken})
	h := newHarness(t, dir)
	h.initialize()
	for _, p := range []string{"presence", "office"} {
		h.waitTree(p, "The server refused the team token")
		if !strings.Contains(h.tree(p), `"id":"token"`) {
			t.Errorf("%s: form not kept", p)
		}
	}
}

func TestKeyMovesOneTile(t *testing.T) {
	f := newFakeOffice(t, office.Point{X: 10, Y: 16})
	h := connectedHarness(t, f)
	steps := []struct {
		key  string
		x, y int
	}{
		{"right", 11, 16}, {"d", 12, 16}, {"down", 12, 17}, {"s", 12, 18},
		{"left", 11, 18}, {"a", 10, 18}, {"up", 10, 17}, {"w", 10, 16},
	}
	for _, st := range steps {
		h.advance(200 * time.Millisecond)
		h.key(st.key)
		h.barrier()
		want, _ := json.Marshal(office.Move{T: office.TMove, X: st.x, Y: st.y})
		if got := f.frame(t); got != string(want) {
			t.Fatalf("key %s: frame %s, want %s", st.key, got, want)
		}
		if x, y := h.myTile(); x != st.x || y != st.y {
			t.Errorf("key %s: sprite at %d,%d, want %d,%d", st.key, x, y, st.x, st.y)
		}
	}
	h.key("space")
	h.barrier()
	f.noFrame(t)
}

func TestBlockedTileStays(t *testing.T) {
	// (2,2) has a wall to the north? No: row 1 is floor. Use (1,1): walls at left and above, a desk
	// two tiles right at (3,2) from (2,2).
	f := newFakeOffice(t, office.Point{X: 1, Y: 1})
	h := connectedHarness(t, f)
	for _, k := range []string{"left", "a", "up", "w"} {
		h.advance(200 * time.Millisecond)
		h.key(k)
		h.barrier()
		f.noFrame(t)
		if x, y := h.myTile(); x != 1 || y != 1 {
			t.Errorf("key %s moved to %d,%d into a wall", k, x, y)
		}
	}
	// Furniture: from (2,2) the desk at (3,2) blocks.
	h.advance(200 * time.Millisecond)
	h.key("down")
	h.barrier()
	h.advance(200 * time.Millisecond)
	h.key("right")
	h.barrier()
	f.frame(t) // down to (1,2)
	f.frame(t) // right to (2,2)
	h.advance(200 * time.Millisecond)
	h.key("right")
	h.barrier()
	f.noFrame(t)
	if x, y := h.myTile(); x != 2 || y != 2 {
		t.Errorf("walked into a desk: %d,%d", x, y)
	}
}

func TestBlockedByMapEdge(t *testing.T) {
	f := newFakeOffice(t, office.Point{X: 0, Y: 0})
	f.rows = []string{"S.", ".."}
	h := connectedHarness(t, f)
	for _, k := range []string{"left", "up"} {
		h.advance(200 * time.Millisecond)
		h.key(k)
		h.barrier()
		f.noFrame(t)
	}
	if x, y := h.myTile(); x != 0 || y != 0 {
		t.Errorf("left the map: %d,%d", x, y)
	}
}

func TestHeldKeyPace(t *testing.T) {
	if moveEvery != 120*time.Millisecond {
		t.Fatalf("pace %v, spec says 120 ms", moveEvery)
	}
	f := newFakeOffice(t, office.Point{X: 10, Y: 16})
	h := connectedHarness(t, f)
	h.advance(time.Second)
	for i := 0; i < 5; i++ {
		h.key("right")
	}
	h.barrier()
	f.frame(t)
	f.noFrame(t)
	h.advance(119 * time.Millisecond)
	h.key("right")
	h.barrier()
	f.noFrame(t)
	h.advance(time.Millisecond)
	h.key("right")
	h.barrier()
	f.frame(t)
	f.noFrame(t)
	if x, _ := h.myTile(); x != 12 {
		t.Errorf("x = %d, want 12 after two paced moves", x)
	}
}

func TestChangeSetupDisconnectsAndPrefills(t *testing.T) {
	f := newFakeOffice(t, office.Point{X: 10, Y: 16})
	h := connectedHarness(t, f)
	<-f.hellos
	h.action("change", nil)
	h.barrier()
	select {
	case <-f.closed:
	case <-time.After(3 * time.Second):
		t.Fatal("still connected after Change setup")
	}
	for _, p := range []string{"presence", "office"} {
		tree := h.tree(p)
		for _, want := range []string{`"value":"Ana"`, `"value":"` + f.url() + `"`, `"value":"Green"`} {
			if !strings.Contains(tree, want) {
				t.Errorf("%s: form lacks %s: %s", p, want, tree)
			}
		}
		if strings.Contains(tree, testToken) {
			t.Errorf("%s draws the token", p)
		}
		var tok struct{ Value *string }
		for _, n := range ofType(decode(t, tree), "input") {
			if n["id"] == "token" {
				if v, has := n["value"]; has && v != "" {
					t.Errorf("token field is filled: %v", v)
				}
			}
		}
		_ = tok
	}
	// An empty token keeps the saved one.
	h.action("save", map[string]any{"name": "Ana", "server": f.url(), "token": "", "avatar": "Green"})
	select {
	case hello := <-f.hellos:
		if hello.Token != testToken {
			t.Errorf("reconnected with token %q, want the saved one", hello.Token)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("no reconnect after saving")
	}
}

func decode(t *testing.T, s string) node {
	t.Helper()
	var n node
	if err := json.Unmarshal([]byte(s), &n); err != nil {
		t.Fatal(err)
	}
	return n
}

func TestEnterOfficeOpensPane(t *testing.T) {
	f := newFakeOffice(t, office.Point{X: 10, Y: 16})
	h := connectedHarness(t, f)
	h.action("enter", nil)
	h.barrier()
	opens := h.methods("panel/open")
	if len(opens) != 1 {
		t.Fatalf("panel/open sent %d times", len(opens))
	}
	if got := string(opens[0]["params"]); got != `{"panel":"office"}` {
		t.Errorf("params %s", got)
	}
}

func TestStatusCarriesNoSessionData(t *testing.T) {
	f := newFakeOffice(t, office.Point{X: 10, Y: 16})
	h := connectedHarness(t, f)
	state := func(session, st string) {
		h.send("event/session_state", map[string]any{"agent": "claude-secret-agent", "session": session, "state": st})
		h.barrier()
	}
	state("sess-secret-1", "working")
	if got := f.frame(t); got != `{"t":"status","status":"focused"}` {
		t.Fatalf("frame %s", got)
	}
	state("sess-secret-1", "working") // no change, nothing sent
	f.noFrame(t)
	state("sess-secret-2", "needs_you")
	if got := f.frame(t); got != `{"t":"status","status":"needs_you"}` {
		t.Fatalf("frame %s", got)
	}
	state("sess-secret-2", "idle")
	if got := f.frame(t); got != `{"t":"status","status":"focused"}` {
		t.Fatalf("frame %s", got)
	}
	state("sess-secret-1", "idle")
	got := f.frame(t)
	if got != `{"t":"status","status":""}` {
		t.Fatalf("frame %s", got)
	}
	for _, s := range []string{got} {
		if strings.Contains(s, "secret") {
			t.Error("session data leaked")
		}
	}
}

func TestStatusFromSessionsListAtStart(t *testing.T) {
	f := newFakeOffice(t, office.Point{X: 10, Y: 16})
	h := connectedHarness(t, f)
	// The answer to our sessions/list (id 1) says a session is working.
	b, _ := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": 1, "result": []map[string]any{{"agent": "a", "session": "s", "title": "private title", "state": "working"}}})
	h.in.Write(append(b, '\n'))
	h.barrier()
	if got := f.frame(t); got != `{"t":"status","status":"focused"}` {
		t.Fatalf("frame %s", got)
	}
}

func TestUnreachableShowsAddress(t *testing.T) {
	ln, _ := net.Listen("tcp", "127.0.0.1:0")
	addr := "ws://" + ln.Addr().String()
	ln.Close()
	dir := t.TempDir()
	saveSetup(dir, Setup{Name: "Ana", Server: addr, Token: testToken})
	h := newHarness(t, dir)
	h.initialize()
	for _, p := range []string{"presence", "office"} {
		h.waitTree(p, "Cannot reach "+addr)
	}
}

func TestOfflineReasonsThroughLoop(t *testing.T) {
	for _, c := range []struct{ refuse, want string }{{office.ReasonFull, "The office is full"}, {office.ReasonReplaced, "Signed in elsewhere"}} {
		f := newFakeOffice(t, office.Point{})
		f.refuse = c.refuse
		dir := t.TempDir()
		saveSetup(dir, Setup{Name: "Ana", Server: f.url(), Token: testToken})
		h := newHarness(t, dir)
		h.initialize()
		for _, p := range []string{"presence", "office"} {
			h.waitTree(p, c.want)
			h.waitTree(p, `"Offline"`)
		}
	}
	// A refused token goes back to the form (OFC-03).
	f := newFakeOffice(t, office.Point{})
	f.refuse = office.ReasonToken
	dir := t.TempDir()
	saveSetup(dir, Setup{Name: "Ana", Server: f.url(), Token: testToken})
	h := newHarness(t, dir)
	h.initialize()
	h.waitTree("presence", reasonToken)
}

func TestSavedSetupRevalidatedBeforeDial(t *testing.T) {
	f := newFakeOffice(t, office.Point{})
	dir := t.TempDir()
	saveSetup(dir, Setup{Name: "Ana", Server: "ws://office.example.com:7420", Token: testToken})
	h := newHarness(t, dir)
	h.initialize()
	h.waitTree("presence", "Use wss:// for a server that is not on this computer")
	h.waitTree("presence", `"id":"token"`)
	time.Sleep(300 * time.Millisecond)
	if f.conns.Load() != 0 {
		t.Error("dialled with an invalid saved setup")
	}
}
