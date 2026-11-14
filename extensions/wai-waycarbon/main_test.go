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

// fakeJira puts a `jira` script first on PATH that only answers the exact argv the extension must use.
func fakeJira(t *testing.T, script string) {
	t.Helper()
	fakeCLI(t, "jira", script)
}

// fakeTools puts a harmless `claude` (plugin installed) first on PATH: initialize starts the plugin
// check and the daily update, and no test may reach the real CLI.
func fakeTools(t *testing.T) {
	t.Helper()
	fakeClaude(t)
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

// rpc runs the server on in-memory pipes; a goroutine drains its output so no write blocks.
type rpc struct {
	send func(string)
	msgs chan map[string]any
	done chan struct{}
	in   *io.PipeWriter
}

func start(t *testing.T) *rpc {
	t.Helper()
	inR, inW := io.Pipe()
	outR, outW := io.Pipe()
	r := &rpc{done: make(chan struct{}), in: inW, msgs: make(chan map[string]any, 1000)}
	go func() {
		run(inR, outW)
		outW.Close()
		close(r.done)
	}()
	go func() {
		br := bufio.NewReader(outR)
		for {
			line, err := br.ReadBytes('\n')
			if err != nil {
				return
			}
			var m map[string]any
			if json.Unmarshal(line, &m) == nil {
				r.msgs <- m
			}
		}
	}()
	r.send = func(s string) { io.WriteString(inW, s+"\n") }
	t.Cleanup(func() { inW.Close(); <-r.done })
	return r
}

// until discards messages up to the first one that satisfies ok, and returns it.
func (r *rpc) until(t *testing.T, ok func(map[string]any) bool) map[string]any {
	t.Helper()
	timeout := time.After(10 * time.Second)
	for {
		select {
		case m := <-r.msgs:
			if ok(m) {
				return m
			}
		case <-timeout:
			t.Fatal("timed out waiting for a message")
		}
	}
}

func (r *rpc) reply(t *testing.T, id float64) map[string]any {
	t.Helper()
	return r.until(t, func(m map[string]any) bool { return m["id"] == id && m["method"] == nil })
}

func (r *rpc) initialize(t *testing.T) {
	t.Helper()
	fakeTools(t)
	r.send(`{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":1,"ext_dir":"/x","data_dir":"` + t.TempDir() + `","project":""}}`)
	if m := r.reply(t, 0); m["result"] == nil {
		t.Fatalf("initialize reply: %v", m)
	}
}

// render is a panel/render of the panel; it returns its tree.
func render(t *testing.T, m map[string]any) map[string]any {
	t.Helper()
	p := m["params"].(map[string]any)
	if p["panel"] != "waycarbon" {
		t.Fatalf("panel: %v", p["panel"])
	}
	return p["tree"].(map[string]any)
}

// work returns the Work tab's children as a column, once the cards are no longer loading.
func (r *rpc) cards(t *testing.T) map[string]any {
	t.Helper()
	m := r.until(t, func(m map[string]any) bool {
		if m["method"] != "panel/render" {
			return false
		}
		return !strings.Contains(textOf(render(t, m)), "Loading…")
	})
	return workTab(t, render(t, m))
}

func workTab(t *testing.T, root map[string]any) map[string]any {
	t.Helper()
	tabs := find(root, "tabs")["tabs"].([]any)
	return map[string]any{"type": "column", "children": tabs[0].(map[string]any)["children"]}
}

// find returns the first node of the type, depth first.
func find(n map[string]any, typ string) map[string]any {
	if n["type"] == typ {
		return n
	}
	for _, key := range []string{"children", "tabs"} {
		kids, _ := n[key].([]any)
		for _, k := range kids {
			if f := find(k.(map[string]any), typ); f != nil {
				return f
			}
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

	r.send(`{"jsonrpc":"2.0","id":7,"method":"panel/action","params":{"panel":"waycarbon","action_id":"EC-2","values":{}}}`)
	ack := r.reply(t, 7)
	open := r.until(t, func(m map[string]any) bool { return m["method"] == "sessions/open" })
	if ack["error"] != nil {
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
	r.send(`{"jsonrpc":"2.0","id":3,"method":"panel/action","params":{"panel":"waycarbon","action_id":"refresh","values":{}}}`)
	r.reply(t, 3)
	if find(r.cards(t), "list") == nil {
		t.Error("refresh must render the cards")
	}
	// A request gets -32601; a notification stays silent (the next answer is for id 5).
	r.send(`{"jsonrpc":"2.0","method":"event/file_saved","params":{}}`)
	r.send(`{"jsonrpc":"2.0","id":4,"method":"nope/nothing","params":{}}`)
	m := r.reply(t, 4)
	e, _ := m["error"].(map[string]any)
	if m["id"] != float64(4) || e == nil || e["code"] != float64(-32601) {
		t.Errorf("want Method not found for id 4, got %v", m)
	}
}

// A slow jira must not block the loop: Loading arrives at once and calls are answered meanwhile.
func TestSlowJiraDoesNotBlock(t *testing.T) {
	release := filepath.Join(t.TempDir(), "release")
	t.Cleanup(func() { os.WriteFile(release, nil, 0o644) })
	fakeJira(t, "while [ ! -f '"+release+"' ]; do sleep 0.05; done\n"+happyJiraBody)
	r := start(t)
	r.initialize(t)
	loading := func(m map[string]any) bool {
		return m["method"] == "panel/render" && strings.Contains(textOf(render(t, m)), "Loading…")
	}
	r.until(t, loading)
	r.send(`{"jsonrpc":"2.0","id":8,"method":"panel/action","params":{"panel":"waycarbon","action_id":"EC-1","values":{}}}`)
	r.reply(t, 8) // answered while jira runs
	r.send(`{"jsonrpc":"2.0","id":9,"method":"panel/action","params":{"panel":"waycarbon","action_id":"refresh","values":{}}}`)
	r.reply(t, 9)
	r.until(t, loading)                // the newer refresh
	time.Sleep(500 * time.Millisecond) // the plugin check repaints meanwhile; let it settle
	for len(r.msgs) > 0 {
		<-r.msgs
	}
	os.WriteFile(release, nil, 0o644)
	hasList := func(m map[string]any) bool {
		return m["method"] == "panel/render" && find(render(t, m), "list") != nil
	}
	r.until(t, hasList)
	// The first refresh's result is stale: no second render with the cards follows.
	time.Sleep(300 * time.Millisecond)
	for {
		select {
		case m := <-r.msgs:
			if hasList(m) {
				t.Fatalf("a stale result was painted: %v", m)
			}
		default:
			return
		}
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
	for _, key := range []string{"children", "tabs"} {
		kids, _ := n[key].([]any)
		for _, k := range kids {
			s += " " + textOf(k.(map[string]any))
		}
	}
	return s
}

func TestInitializeRendersTabs(t *testing.T) {
	fakeJira(t, happyJira)
	r := start(t)
	r.initialize(t)
	m := r.until(t, func(m map[string]any) bool { return m["method"] == "panel/render" })
	root := render(t, m)
	kids := root["children"].([]any)
	tabs := kids[0].(map[string]any)
	if root["type"] != "column" || tabs["type"] != "tabs" {
		t.Fatalf("want column > tabs, got %v", root)
	}
	var ids []string
	for _, tb := range tabs["tabs"].([]any) {
		ids = append(ids, tb.(map[string]any)["id"].(string))
	}
	if strings.Join(ids, ",") != "work,help" {
		t.Errorf("tab ids: %v", ids)
	}
}

func TestBarItemOpensPanel(t *testing.T) {
	fakeJira(t, happyJira)
	r := start(t)
	r.initialize(t)
	r.send(`{"jsonrpc":"2.0","id":5,"method":"item/action","params":{"item":"attention"}}`)
	m := r.until(t, func(m map[string]any) bool { return m["method"] == "panel/open" })
	if p := m["params"].(map[string]any); p["panel"] != "waycarbon" || len(p) != 1 {
		t.Errorf("params: %v", p)
	}
}

func TestPluginMissingOffersInstall(t *testing.T) {
	fakeJira(t, happyJira)
	r := start(t)
	fakeCLI(t, "claude", `echo '[]'`)
	r.send(`{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"data_dir":"` + t.TempDir() + `"}}`)
	m := r.until(t, func(m map[string]any) bool {
		return m["method"] == "panel/render" && strings.Contains(textOf(render(t, m)), "The WayCarbon Claude plugin is not installed, so the /waycarbon commands will not work.")
	})
	found := false
	var walk func(n map[string]any)
	walk = func(n map[string]any) {
		if n["type"] == "button" && n["id"] == "plugin:install" {
			found = true
		}
		kids, _ := n["children"].([]any)
		for _, k := range kids {
			walk(k.(map[string]any))
		}
		tabs, _ := n["tabs"].([]any)
		for _, tb := range tabs {
			walk(tb.(map[string]any))
		}
	}
	walk(render(t, m))
	if !found {
		t.Error("want an Install plugin button")
	}
}
