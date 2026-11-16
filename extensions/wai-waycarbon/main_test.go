package main

import (
	"bufio"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"

	"wai-waycarbon/helpapi"
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
	fakeGh(t)
}

// fakeGh answers the GraphQL call with an empty viewer and `auth status` with success.
func fakeGh(t *testing.T) {
	t.Helper()
	fakeCLI(t, "gh", `[ "$1 $2" = "api graphql" ] && echo '{"data":{"viewer":{"login":"dev"}}}'
exit 0
`)
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
	out := "WARN: config is old\nEC-1\tFix the login timeout\tIn Progress\r\n\n  \nEC-2\tSummary with  spaces\tTo Do\nbroken line\nEC-3\tNo status\nEC-4\t\tTo Do\nec-5\tlower\tTo Do\nEC-\tx\ty\nEC-6\tPadded to align\t\t\t\tCode Review\n"
	got := parseCards(out)
	want := []Card{
		{"EC-1", "Fix the login timeout", "In Progress"},
		{"EC-2", "Summary with  spaces", "To Do"},
		{"EC-3", "No status", ""},
		{"EC-4", "", "To Do"},
		{"EC-6", "Padded to align", "Code Review"},
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

// rpc runs the server on in-memory pipes; a goroutine drains its output so no write blocks.
type rpc struct {
	send func(string)
	msgs chan map[string]any
	done chan struct{}
	in   *io.PipeWriter
	// sessions, when set, is the JSON answer to every sessions/list request seen by until.
	sessions string
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
			if m["method"] == "sessions/list" && r.sessions != "" {
				id, _ := json.Marshal(m["id"])
				r.send(`{"jsonrpc":"2.0","id":` + string(id) + `,"result":` + r.sessions + `}`)
			}
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
	r.initializeRaw(t)
}

// initializeRaw sends initialize without installing any fake: the test did it.
func (r *rpc) initializeRaw(t *testing.T) {
	t.Helper()
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
func findID(n map[string]any, id string) map[string]any {
	if n["id"] == id {
		return n
	}
	for _, key := range []string{"children", "tabs"} {
		kids, _ := n[key].([]any)
		for _, k := range kids {
			if f := findID(k.(map[string]any), id); f != nil {
				return f
			}
		}
	}
	return nil
}

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

	root := r.cards(t)
	first := findID(root, "card:EC-1")
	if first == nil || first["title"] != "Fix the login timeout" || first["subtitle"] != "EC-1 · In Progress" || first["badge"] != "In Progress" {
		t.Fatalf("first card: %v", first)
	}
	if findID(root, "card:EC-2") == nil {
		t.Error("want a card for EC-2")
	}
}

func TestCardClickOpensSession(t *testing.T) {
	fakeJira(t, happyJira)
	r := start(t)
	r.initialize(t)
	r.cards(t)

	r.send(`{"jsonrpc":"2.0","id":7,"method":"panel/action","params":{"panel":"waycarbon","action_id":"card:EC-2","values":{}}}`)
	ack := r.reply(t, 7)
	open := r.until(t, func(m map[string]any) bool { return m["method"] == "sessions/open" })
	if ack["error"] != nil {
		t.Errorf("panel/action not answered: %v", ack)
	}
	p := open["params"].(map[string]any)
	if p["prompt"] != "/waycarbon:solve-card EC-2" || p["send"] != false {
		t.Errorf("sessions/open params: %v", p)
	}
}

func TestRefreshAndUnknownMethod(t *testing.T) {
	fakeJira(t, happyJira)
	r := start(t)
	r.initialize(t)
	r.cards(t)
	r.send(`{"jsonrpc":"2.0","id":3,"method":"panel/action","params":{"panel":"waycarbon","action_id":"work:refresh","values":{}}}`)
	r.reply(t, 3)
	if findID(r.cards(t), "card:EC-1") == nil {
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

// A slow jira must not block the loop: Loading arrives at once, calls are answered meanwhile, and a
// refresh asked while one runs runs once more.
func TestSlowJiraDoesNotBlock(t *testing.T) {
	dir := t.TempDir()
	release, runs := filepath.Join(dir, "release"), filepath.Join(dir, "runs")
	t.Cleanup(func() { os.WriteFile(release, nil, 0o644) })
	fakeJira(t, "echo \"$*\" >> '"+runs+"'\nwhile [ ! -f '"+release+"' ]; do sleep 0.05; done\n"+happyJiraBody)
	r := start(t)
	fakeClaude(t)
	fakeGh(t)
	r.initializeRaw(t)
	loading := func(m map[string]any) bool {
		return m["method"] == "panel/render" && strings.Contains(textOf(render(t, m)), "Loading…")
	}
	r.until(t, loading)
	r.send(`{"jsonrpc":"2.0","id":8,"method":"panel/action","params":{"panel":"waycarbon","action_id":"card:EC-1","values":{}}}`)
	r.reply(t, 8) // answered while jira runs
	r.send(`{"jsonrpc":"2.0","id":9,"method":"panel/action","params":{"panel":"waycarbon","action_id":"work:refresh","values":{}}}`)
	r.reply(t, 9)
	time.Sleep(300 * time.Millisecond)
	os.WriteFile(release, nil, 0o644)
	r.cards(t)
	time.Sleep(300 * time.Millisecond)
	waitFor(t, "the queued refresh", func() bool { return strings.Count(string(mustRead(runs)), "me\n") == 2 })
	time.Sleep(300 * time.Millisecond)
	if n := strings.Count(string(mustRead(runs)), "me\n"); n != 2 {
		t.Errorf("jira me ran %d times, want 2: the refresh asked mid-refresh reruns once", n)
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
	fakeGh(t)
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

func TestBarItemFollowsCounts(t *testing.T) {
	fakeJira(t, happyJira)
	r := start(t)
	r.initialize(t)
	// The server is internal to run(); drive the real paint path through a second instance.
	var out strings.Builder
	s := &server{out: &out, lastBar: jsonString(node{"item": "attention"})}
	s.smu.Lock()
	s.counts = counts{NeedsYou: 2}
	s.help.auth = &auth{"t", "me"}
	s.help.reqs = map[string]helpapi.Request{"r": {ID: "r", From: "ana", To: "me", State: helpapi.Waiting}}
	s.paintAll() // must not deadlock on smu
	s.smu.Unlock()
	var bar map[string]any
	for _, l := range strings.Split(strings.TrimSpace(out.String()), "\n") {
		var m map[string]any
		json.Unmarshal([]byte(l), &m)
		if m["method"] == "item/update" {
			bar = m["params"].(map[string]any)
		}
	}
	if bar["item"] != "attention" || bar["text"] != "3" || bar["icon"] != "bell" || bar["tone"] != "warning" {
		t.Fatalf("bar: %v", bar)
	}
	items := bar["card"].(map[string]any)["items"].([]any)
	if len(items) != 2 || items[0].(map[string]any)["key"] != "Needs you" || items[1].(map[string]any)["key"] != "Help requests" {
		t.Errorf("card: %v", items)
	}
	out.Reset()
	s.smu.Lock()
	s.counts, s.help.reqs = counts{}, nil
	s.paintAll()
	s.smu.Unlock()
	for _, l := range strings.Split(strings.TrimSpace(out.String()), "\n") {
		var m map[string]any
		json.Unmarshal([]byte(l), &m)
		if m["method"] == "item/update" {
			if p := m["params"].(map[string]any); len(p) != 1 || p["item"] != "attention" {
				t.Errorf("hidden bar must be only the item: %v", p)
			}
			return
		}
	}
	t.Error("no item/update when the count returned to 0")
}

func TestPluginInstallClick(t *testing.T) {
	fakeJira(t, happyJira)
	dir := t.TempDir()
	log, release := filepath.Join(dir, "log"), filepath.Join(dir, "release")
	t.Cleanup(func() { os.WriteFile(release, nil, 0o644) })
	fakeCLI(t, "claude", `case "$1 $2" in
  "plugin list") echo '[]' ;;
  "plugin install")
    echo install >> '`+log+`'
    while [ ! -f '`+release+`' ]; do sleep 0.02; done
    echo "noise" >&2; echo "marketplace unreachable" >&2; exit 1 ;;
esac
exit 0
`)
	fakeGh(t)
	r := start(t)
	r.send(`{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"data_dir":"` + t.TempDir() + `"}}`)
	r.reply(t, 0)
	has := func(text string) func(map[string]any) bool {
		return func(m map[string]any) bool {
			return m["method"] == "panel/render" && strings.Contains(textOf(render(t, m)), text)
		}
	}
	r.until(t, has("is not installed"))
	click := `{"jsonrpc":"2.0","id":1,"method":"panel/action","params":{"panel":"waycarbon","action_id":"plugin:install","values":{}}}`
	r.send(click)
	m := r.until(t, func(m map[string]any) bool {
		return m["method"] == "panel/render" && find(render(t, m), "spinner") != nil
	})
	if find(render(t, m), "button") != nil && find(find(render(t, m), "button"), "button")["id"] == "plugin:install" {
		t.Error("the install button must give way to the spinner")
	}
	r.send(strings.Replace(click, `"id":1`, `"id":2`, 1)) // lands while the first install runs
	r.reply(t, 2)
	os.WriteFile(release, nil, 0o644)
	m = r.until(t, has("marketplace unreachable"))
	root := render(t, m)
	if find(root, "spinner") != nil {
		t.Error("spinner must be gone after the failure")
	}
	var btn bool
	var walk func(n map[string]any)
	walk = func(n map[string]any) {
		if n["id"] == "plugin:install" {
			btn = true
		}
		for _, key := range []string{"children", "tabs"} {
			kids, _ := n[key].([]any)
			for _, k := range kids {
				walk(k.(map[string]any))
			}
		}
	}
	walk(root)
	if !btn {
		t.Error("the Install plugin button must come back")
	}
	if got := strings.Count(string(mustRead(log)), "install"); got != 1 {
		t.Errorf("install ran %d times, want 1", got)
	}
}

func mustRead(p string) []byte { b, _ := os.ReadFile(p); return b }

// workFakes puts the fixture-backed gh and a jira with EC-7 (PR merged), EC-8 (open PR) and EC-5.
func workFakes(t *testing.T) (ghLog, jiraLog string) {
	t.Helper()
	dir := t.TempDir()
	ghLog, jiraLog = filepath.Join(dir, "gh.log"), filepath.Join(dir, "jira.log")
	data, _ := filepath.Abs("testdata/graphql.json")
	fakeCLI(t, "gh", `echo "$*" >> '`+ghLog+`'
case "$1 $2" in
  "api graphql") cat '`+data+`' ;;
esac
exit 0
`)
	fakeJira(t, `echo "$*" >> '`+jiraLog+`'
case "$*" in
  "me") echo "dev@example.com" ;;
  "issue list -a dev@example.com -s~Done --plain --no-headers --columns key,summary,status")
    printf 'EC-7\tShipped thing\tIn Review\nEC-8\tTwo parts\tIn Progress\nEC-5\tNothing yet\tTo Do\n' ;;
  "issue move EC-7 Done") ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
`)
	fakeClaude(t)
	return
}

func (r *rpc) action(t *testing.T, id int, action string) {
	t.Helper()
	r.send(`{"jsonrpc":"2.0","id":` + strconv.Itoa(id) + `,"method":"panel/action","params":{"panel":"waycarbon","action_id":"` + action + `","values":{}}}`)
	r.reply(t, float64(id))
}

func waitFor(t *testing.T, what string, ok func() bool) {
	t.Helper()
	for i := 0; i < 200; i++ {
		if ok() {
			return
		}
		time.Sleep(50 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for %s", what)
}

func TestPRClickOpensSession(t *testing.T) {
	workFakes(t)
	r := start(t)
	r.initializeRaw(t)
	root := r.cards(t)
	for _, id := range []string{"pr:" + prURL + "3", "review:" + prURL + "20", "card:EC-7", "card:EC-8", "card:EC-5"} {
		if findID(root, id) == nil {
			t.Errorf("no card %s", id)
		}
	}
	if find(findID(root, "pr:"+prURL+"3"), "link") == nil {
		t.Error("a PR card has an Open on GitHub link")
	}
	r.action(t, 7, "pr:"+prURL+"3")
	open := r.until(t, func(m map[string]any) bool { return m["method"] == "sessions/open" })
	p := open["params"].(map[string]any)
	if p["prompt"] != "/waycarbon:fix-review "+prURL+"3" || p["send"] != false {
		t.Errorf("sessions/open params: %v", p)
	}
}

func TestBarCountsFromWork(t *testing.T) {
	workFakes(t)
	r := start(t)
	r.initializeRaw(t)
	m := r.until(t, func(m map[string]any) bool { return m["method"] == "item/update" })
	bar := m["params"].(map[string]any)
	// 4 needs you + 2 reviews + 1 card whose PR merged (EC-7).
	if bar["text"] != "7" {
		t.Errorf("bar: %v", bar)
	}
}

func TestMoveToDone(t *testing.T) {
	ghLog, jiraLog := workFakes(t)
	r := start(t)
	r.initializeRaw(t)
	root := r.cards(t)
	btn := findID(root, "done:EC-7")
	if btn == nil || btn["type"] != "button" {
		t.Fatalf("no Move to Done button: %v", btn)
	}
	if c, _ := btn["confirm"].(map[string]any); c["title"] != "Move EC-7 to Done?" {
		t.Errorf("confirm: %v", btn["confirm"])
	}
	r.action(t, 7, "done:EC-7")
	waitFor(t, "jira issue move", func() bool { return strings.Contains(string(mustRead(jiraLog)), "issue move EC-7 Done\n") })
	waitFor(t, "a refresh after the move", func() bool { return strings.Count(string(mustRead(ghLog)), "api graphql") >= 2 })
}

func TestGhMissing(t *testing.T) {
	t.Setenv("PATH", t.TempDir())
	fakeJira(t, happyJira)
	fakeClaude(t)
	r := start(t)
	r.initializeRaw(t)
	root := r.cards(t)
	if !strings.Contains(textOf(root), "GitHub CLI not found") {
		t.Errorf("want the alert in %v", root)
	}
	if l := find(root, "log"); l == nil || l["text"] != "brew install gh" {
		t.Errorf("want a log with brew install gh, got %v", l)
	}
	if findID(root, "card:EC-1") == nil {
		t.Error("the jira cards must still render")
	}
}

func TestGhSignIn(t *testing.T) {
	dir := t.TempDir()
	log := filepath.Join(dir, "gh.log")
	fakeCLI(t, "gh", `echo "$*" >> '`+log+`'
case "$1 $2" in
  "auth login")
    echo '! First copy your one-time code: AB12-CD34' >&2
    sleep 1
    exit 0 ;;
esac
echo "gh: not logged in" >&2
exit 1
`)
	fakeJira(t, happyJira)
	fakeClaude(t)
	r := start(t)
	r.initializeRaw(t)
	root := r.cards(t)
	if findID(root, "gh:signin") == nil {
		t.Fatalf("want a Sign in button: %v", root)
	}
	r.action(t, 7, "gh:signin")
	m := r.until(t, func(m map[string]any) bool {
		return m["method"] == "panel/render" && strings.Contains(textOf(render(t, m)), "AB12-CD34")
	})
	tree := render(t, m)
	if l := find(tree, "link"); l == nil || l["url"] != "https://github.com/login/device" {
		t.Errorf("want the device link: %v", l)
	}
	if find(tree, "spinner") == nil || findID(tree, "gh:cancel") == nil {
		t.Error("want a spinner and Cancel")
	}
	r.action(t, 8, "gh:signin") // a second click while it runs does nothing
	waitFor(t, "the refresh after sign-in", func() bool { return strings.Count(string(mustRead(log)), "api graphql") >= 2 })
	if n := strings.Count(string(mustRead(log)), "auth login"); n != 1 {
		t.Errorf("auth login ran %d times, want 1", n)
	}
	if got := calls(log); !strings.Contains(strings.Join(got, "\n"), "auth login --web --hostname github.com --git-protocol https --skip-ssh-key") {
		t.Errorf("argv: %v", got)
	}
}

func TestGhSignInCancel(t *testing.T) {
	fakeCLI(t, "gh", `case "$1 $2" in
  "auth login")
    echo '! First copy your one-time code: AB12-CD34' >&2
    exec sleep 30 ;;
esac
echo "gh: not logged in" >&2
exit 1
`)
	fakeJira(t, happyJira)
	fakeClaude(t)
	r := start(t)
	r.initializeRaw(t)
	r.cards(t)
	r.action(t, 7, "gh:signin")
	r.until(t, func(m map[string]any) bool {
		return m["method"] == "panel/render" && strings.Contains(textOf(render(t, m)), "AB12-CD34")
	})
	r.action(t, 8, "gh:cancel")
	m := r.until(t, func(m map[string]any) bool {
		return m["method"] == "panel/render" && findID(render(t, m), "gh:signin") != nil && find(render(t, m), "spinner") == nil
	})
	if strings.Contains(textOf(render(t, m)), "signal") {
		t.Errorf("a cancel is not an error: %v", textOf(render(t, m)))
	}
}

// A Move to Done confirmed while a slow refresh runs leads to one more refresh, so the card goes away.
func TestMoveToDoneDuringRefresh(t *testing.T) {
	dir := t.TempDir()
	gate, moved, ghLog := filepath.Join(dir, "gate"), filepath.Join(dir, "moved"), filepath.Join(dir, "gh.log")
	t.Cleanup(func() { os.WriteFile(gate, nil, 0o644) })
	data, _ := filepath.Abs("testdata/graphql.json")
	fakeCLI(t, "gh", `echo "$*" >> '`+ghLog+`'
if [ "$1 $2" = "api graphql" ]; then
  [ "$(grep -c 'api graphql' '`+ghLog+`')" = 2 ] && while [ ! -f '`+gate+`' ]; do sleep 0.02; done
  cat '`+data+`'
fi
exit 0
`)
	fakeJira(t, `case "$*" in
  "me") echo "dev@example.com" ;;
  "issue list"*)
    [ -f '`+moved+`' ] || printf 'EC-7\tShipped thing\tIn Review\n' ;;
  "issue move EC-7 Done") touch '`+moved+`' ;;
esac
`)
	fakeClaude(t)
	r := start(t)
	r.initializeRaw(t)
	r.cards(t)                     // first refresh done: EC-7 shows Move to Done
	r.action(t, 7, "work:refresh") // second graphql call: blocks on the gate
	waitFor(t, "the slow refresh", func() bool { return strings.Count(string(mustRead(ghLog)), "api graphql") == 2 })
	r.action(t, 8, "done:EC-7")
	waitFor(t, "jira move", func() bool { return fileExists(moved) })
	time.Sleep(200 * time.Millisecond)
	os.WriteFile(gate, nil, 0o644)
	waitFor(t, "the rerun", func() bool { return strings.Count(string(mustRead(ghLog)), "api graphql") == 3 })
	r.until(t, func(m map[string]any) bool {
		return m["method"] == "panel/render" && findID(render(t, m), "card:EC-7") == nil && find(render(t, m), "tabs") != nil &&
			!strings.Contains(textOf(render(t, m)), "Loading…") && findID(render(t, m), "work:refresh") != nil
	})
	time.Sleep(300 * time.Millisecond)
	if n := strings.Count(string(mustRead(ghLog)), "api graphql"); n != 3 {
		t.Errorf("%d refreshes, want 3 (start, manual, one rerun)", n)
	}
}

func fileExists(p string) bool { _, err := os.Stat(p); return err == nil }
