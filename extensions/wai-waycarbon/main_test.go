package main

import (
	"bufio"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// fakeJira puts a `jira` script on PATH that only answers the exact argv the extension must use.
func fakeJira(t *testing.T, script string) {
	t.Helper()
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "jira"), []byte("#!/bin/sh\n"+script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", dir)
}

const happyJira = happyJiraBody

const happyJiraBody = `
case "$*" in
  "me") echo "dev@example.com" ;;
  "issue list -a dev@example.com -s~Done --plain --no-headers --columns key,summary,status")
    printf 'EC-1\tFix the login timeout\tIn Progress\n'
    printf '\n'
    printf 'EC-2\tAdd the export button\tTo Do\n' ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
`

func TestParsesJiraPlainOutput(t *testing.T) {
	out := "WARN: config is old\nEC-1\tFix the login timeout\tIn Progress\r\n\n  \nEC-2\tSummary with  spaces\tTo Do\nbroken line\nEC-3\tNo status\nEC-4\t\tTo Do\nec-5\tlower\tTo Do\nEC-\tx\ty\n"
	got := parseCards(out)
	want := []Card{
		{"EC-1", "Fix the login timeout", "In Progress"},
		{"EC-2", "Summary with  spaces", "To Do"},
		{"EC-3", "No status", ""},
		{"EC-4", "", "To Do"},
	}
	if len(got) != len(want) {
		t.Fatalf("got %v, want %v", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Errorf("card %d: got %v, want %v", i, got[i], want[i])
		}
	}
}

func TestPromptStripsControlCharacters(t *testing.T) {
	got := prompt(Card{Key: "EC-9", Summary: "Fix\x1b[31m this\nnow\t\x00ok"})
	if got != "Work on EC-9: Fix [31m this now ok" {
		t.Errorf("got %q", got)
	}
	if got := prompt(Card{Key: "EC-9"}); got != "Work on EC-9" {
		t.Errorf("got %q", got)
	}
}

// rpc runs the server on in-memory pipes; send writes a message, recv reads the next one.
type rpc struct {
	send func(string)
	recv func() map[string]any
	done chan struct{}
	in   *io.PipeWriter
}

func start(t *testing.T) *rpc {
	t.Helper()
	inR, inW := io.Pipe()
	outR, outW := io.Pipe()
	r := &rpc{done: make(chan struct{}), in: inW}
	go func() {
		run(inR, outW)
		outW.Close()
		close(r.done)
	}()
	br := bufio.NewReader(outR)
	r.send = func(s string) { io.WriteString(inW, s+"\n") }
	r.recv = func() map[string]any {
		line, err := br.ReadBytes('\n')
		if err != nil {
			t.Fatalf("read: %v", err)
		}
		var m map[string]any
		if err := json.Unmarshal(line, &m); err != nil {
			t.Fatalf("bad json %q: %v", line, err)
		}
		return m
	}
	t.Cleanup(func() { inW.Close(); <-r.done })
	return r
}

func (r *rpc) initialize(t *testing.T) {
	r.send(`{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":1,"ext_dir":"/x"}}`)
	if m := r.recv(); m["id"] != float64(0) || m["result"] == nil {
		t.Fatalf("initialize reply: %v", m)
	}
}

// cards reads the Loading tree, then the cards tree that follows it.
func (r *rpc) cards(t *testing.T) map[string]any {
	t.Helper()
	if txt := textOf(tree(t, r.recv())); txt != "Loading…" {
		t.Fatalf("want the Loading tree first, got %q", txt)
	}
	return tree(t, r.recv())
}

func tree(t *testing.T, m map[string]any) map[string]any {
	t.Helper()
	if m["method"] != "panel/render" {
		t.Fatalf("want panel/render, got %v", m)
	}
	p := m["params"].(map[string]any)
	if p["panel"] != "cards" {
		t.Fatalf("panel: %v", p["panel"])
	}
	return p["tree"].(map[string]any)
}

// find returns the first node of the type, depth first.
func find(n map[string]any, typ string) map[string]any {
	if n["type"] == typ {
		return n
	}
	kids, _ := n["children"].([]any)
	for _, k := range kids {
		if f := find(k.(map[string]any), typ); f != nil {
			return f
		}
	}
	return nil
}

func TestInitializeAndRender(t *testing.T) {
	fakeJira(t, happyJira)
	r := start(t)
	r.initialize(t)

	list := find(r.cards(t), "list")
	if list == nil || list["fill"] != true {
		t.Fatalf("want a filled list, got %v", list)
	}
	items := list["items"].([]any)
	if len(items) != 2 {
		t.Fatalf("items: %v", items)
	}
	first := items[0].(map[string]any)
	if first["id"] != "EC-1" || first["title"] != "Fix the login timeout" || first["subtitle"] != "In Progress" {
		t.Errorf("first item: %v", first)
	}
}

func TestCardClickOpensSession(t *testing.T) {
	fakeJira(t, happyJira)
	r := start(t)
	r.initialize(t)
	r.cards(t)

	r.send(`{"jsonrpc":"2.0","id":7,"method":"panel/action","params":{"panel":"cards","action_id":"EC-2","values":{}}}`)
	// The host's request gets its answer, and the session is opened pre-filled.
	var open, ack map[string]any
	for i := 0; i < 2; i++ {
		m := r.recv()
		if m["method"] == "sessions/open" {
			open = m
		} else {
			ack = m
		}
	}
	if ack["id"] != float64(7) {
		t.Errorf("panel/action not answered: %v", ack)
	}
	p := open["params"].(map[string]any)
	if p["prompt"] != "Work on EC-2: Add the export button" || p["send"] != false {
		t.Errorf("sessions/open params: %v", p)
	}
}

func TestRefreshAndUnknownMethod(t *testing.T) {
	fakeJira(t, happyJira)
	r := start(t)
	r.initialize(t)
	r.cards(t)
	r.send(`{"jsonrpc":"2.0","id":3,"method":"panel/action","params":{"panel":"cards","action_id":"refresh","values":{}}}`)
	if m := r.recv(); m["id"] != float64(3) {
		t.Errorf("refresh not answered first: %v", m)
	}
	if find(r.cards(t), "list") == nil {
		t.Error("refresh must render the cards")
	}
	// A request gets -32601; a notification stays silent (the next answer is for id 5).
	r.send(`{"jsonrpc":"2.0","method":"event/file_saved","params":{}}`)
	r.send(`{"jsonrpc":"2.0","id":4,"method":"nope/nothing","params":{}}`)
	m := r.recv()
	e, _ := m["error"].(map[string]any)
	if m["id"] != float64(4) || e == nil || e["code"] != float64(-32601) {
		t.Errorf("want Method not found for id 4, got %v", m)
	}
}

// A slow jira must not block the loop: Loading arrives at once and calls are answered meanwhile.
func TestSlowJiraDoesNotBlock(t *testing.T) {
	release := filepath.Join(t.TempDir(), "release")
	t.Cleanup(func() { os.WriteFile(release, nil, 0o644) })
	dir := t.TempDir()
	script := "#!/bin/sh\nwhile [ ! -f '" + release + "' ]; do sleep 0.05; done\n" + happyJiraBody
	if err := os.WriteFile(filepath.Join(dir, "jira"), []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", dir+":/bin:/usr/bin")
	r := start(t)
	r.initialize(t)
	if txt := textOf(tree(t, r.recv())); txt != "Loading…" {
		t.Fatalf("want Loading, got %q", txt)
	}
	r.send(`{"jsonrpc":"2.0","id":8,"method":"panel/action","params":{"panel":"cards","action_id":"EC-1","values":{}}}`)
	if m := r.recv(); m["id"] != float64(8) {
		t.Fatalf("action not answered while jira runs: %v", m)
	}
	r.send(`{"jsonrpc":"2.0","id":9,"method":"panel/action","params":{"panel":"cards","action_id":"refresh","values":{}}}`)
	r.recv() // ack 9
	r.recv() // Loading of the newer refresh
	os.WriteFile(release, nil, 0o644)
	if find(tree(t, r.recv()), "list") == nil {
		t.Error("want the cards once jira finishes")
	}
	// The first render's result is stale: nothing is painted before the next reply.
	time.Sleep(300 * time.Millisecond)
	r.send(`{"jsonrpc":"2.0","id":10,"method":"probe","params":{}}`)
	if m := r.recv(); m["id"] != float64(10) {
		t.Errorf("a stale result was painted: %v", m)
	}
}

func TestJiraProblemsAreShown(t *testing.T) {
	cases := map[string]struct{ script, text string }{
		"missing":       {"", "jira CLI not found"},
		"not logged in": {"exit 1\n", "jira is not logged in"},
		"me stderr":     {"echo 'token expired' >&2; exit 1\n", "token expired"},
		"list fails": {`[ "$1" = me ] && { echo me; exit 0; }
echo "boom" >&2; exit 1
`, "boom"},
	}
	for name, c := range cases {
		t.Run(name, func(t *testing.T) {
			if name == "missing" {
				t.Setenv("PATH", t.TempDir())
			} else {
				fakeJira(t, c.script)
			}
			r := start(t)
			r.initialize(t)
			root := r.cards(t)
			if !strings.Contains(textOf(root), c.text) {
				t.Errorf("want %q in %v", c.text, root)
			}
			if find(root, "button") == nil {
				t.Error("want a Refresh button")
			}
		})
	}
}

func textOf(n map[string]any) string {
	s, _ := n["text"].(string)
	kids, _ := n["children"].([]any)
	for _, k := range kids {
		s += " " + textOf(k.(map[string]any))
	}
	return s
}
