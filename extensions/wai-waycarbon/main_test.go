package main

import (
	"bufio"
	"context"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"strconv"
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

// mineArgs and sprintArgs are the jira argv of my open cards and of those in the current sprint.
const (
	mineArgs   = `"issue list -a dev@example.com -s~Done --plain --no-headers --columns key,summary,status"`
	sprintArgs = `"issue list -a dev@example.com -s~Done -q sprint in openSprints() --plain --no-headers --columns key,summary,status"`
)

const happyJiraBody = `
case "$*" in
  "me") echo "dev@example.com" ;;
  ` + mineArgs + `|` + sprintArgs + `)
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

// cards returns the panel tree, all tabs, once the cards are no longer loading.
func (r *rpc) cards(t *testing.T) map[string]any {
	t.Helper()
	m := r.until(t, func(m map[string]any) bool {
		if m["method"] != "panel/render" {
			return false
		}
		return !strings.Contains(textOf(render(t, m)), "Loading…")
	})
	return render(t, m)
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
	first := findID(root, "sel:jira:card:EC-1")
	if first == nil || first["title"] != "Fix the login timeout" || first["subtitle"] != "EC-1" || first["badge"] != nil {
		t.Fatalf("first card: %v", first)
	}
	// To Do comes first on the board, so EC-2 is selected; In Progress is not a board status: it goes last.
	if c := findID(root, "sel:jira:card:EC-2"); c == nil || c["selected"] != true {
		t.Errorf("want EC-2 selected: %v", c)
	}
	text := textOf(findID(root, "jira"))
	if i, j := strings.Index(text, "To Do · 1"), strings.Index(text, "In Progress · 1"); i < 0 || j < i {
		t.Errorf("board columns: %q", text)
	}
}

func TestCardClickOpensSession(t *testing.T) {
	fakeJira(t, happyJira)
	r := start(t)
	r.initialize(t)
	r.cards(t)

	// Selecting a card shows its detail; only the detail's Solve card opens a session.
	r.action(t, 6, "sel:jira:card:EC-1")
	m := r.until(t, func(m map[string]any) bool {
		if m["method"] == "sessions/open" {
			t.Fatal("a selection must not open a session")
		}
		return m["method"] == "panel/render" && findID(render(t, m), "act:jira:solve:EC-1") != nil
	})
	if c := findID(render(t, m), "sel:jira:card:EC-1"); c["selected"] != true {
		t.Errorf("want EC-1 selected: %v", c)
	}
	if c := findID(render(t, m), "sel:jira:card:EC-2"); c["selected"] != nil {
		t.Errorf("EC-2 is no longer selected: %v", c)
	}
	r.send(`{"jsonrpc":"2.0","id":7,"method":"panel/action","params":{"panel":"waycarbon","action_id":"act:jira:solve:EC-1","values":{}}}`)
	ack := r.reply(t, 7)
	open := r.until(t, func(m map[string]any) bool { return m["method"] == "sessions/open" })
	if ack["error"] != nil {
		t.Errorf("panel/action not answered: %v", ack)
	}
	p := open["params"].(map[string]any)
	if p["prompt"] != "/waycarbon:solve-card EC-1" || p["send"] != false {
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
	if findID(r.cards(t), "sel:jira:card:EC-1") == nil {
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
	r.send(`{"jsonrpc":"2.0","id":8,"method":"panel/action","params":{"panel":"waycarbon","action_id":"sel:jira:card:EC-1","values":{}}}`)
	r.reply(t, 8) // answered while jira runs
	r.send(`{"jsonrpc":"2.0","id":9,"method":"panel/action","params":{"panel":"waycarbon","action_id":"work:refresh","values":{}}}`)
	r.reply(t, 9)
	time.Sleep(300 * time.Millisecond)
	os.WriteFile(release, nil, 0o644)
	r.cards(t)
	time.Sleep(300 * time.Millisecond)
	// Each refresh runs jira me twice: my cards and the sprint's.
	waitFor(t, "the queued refresh", func() bool { return strings.Count(string(mustRead(runs)), "me\n") == 4 })
	time.Sleep(300 * time.Millisecond)
	if n := strings.Count(string(mustRead(runs)), "me\n"); n != 4 {
		t.Errorf("jira me ran %d times, want 4: the refresh asked mid-refresh reruns once", n)
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
	if strings.Join(ids, ",") != "work,jira,prs" {
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
	s := newServer(context.Background(), &out)
	s.smu.Lock()
	s.counts = counts{NeedsYou: 2, Cards: 1}
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
	if len(items) != 2 || items[0].(map[string]any)["key"] != "Needs you" || items[1].(map[string]any)["key"] != "Cards to update" {
		t.Errorf("card: %v", items)
	}
	out.Reset()
	s.smu.Lock()
	s.counts = counts{}
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
	installing := func(root map[string]any) bool {
		return strings.Contains(jsonString(root), "Installing the WayCarbon plugin")
	}
	m := r.until(t, func(m map[string]any) bool { return m["method"] == "panel/render" && installing(render(t, m)) })
	if findID(render(t, m), "plugin:install") != nil {
		t.Error("the install button must give way to the spinner")
	}
	r.send(strings.Replace(click, `"id":1`, `"id":2`, 1)) // lands while the first install runs
	r.reply(t, 2)
	os.WriteFile(release, nil, 0o644)
	m = r.until(t, has("marketplace unreachable"))
	root := render(t, m)
	if installing(root) {
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
	search, _ := filepath.Abs("testdata/search.json")
	detail, _ := filepath.Abs("testdata/prdetail.json")
	issue, _ := filepath.Abs("testdata/issue.json")
	// The refresh's call is the one with mine=; a search has q=; any other GraphQL call is a PR detail.
	fakeCLI(t, "gh", `echo "$*" >> '`+ghLog+`'
case "$*" in
  *" mine="*) cat '`+data+`' ;;
  *" q="*) cat '`+search+`' ;;
  "api graphql"*) cat '`+detail+`' ;;
esac
exit 0
`)
	fakeJira(t, `echo "$*" >> '`+jiraLog+`'
case "$*" in
  "me") echo "dev@example.com" ;;
  `+mineArgs+`|`+sprintArgs+`)
    printf 'EC-7\tShipped thing\tIn Review\nEC-8\tTwo parts\tIn Progress\nEC-5\tNothing yet\tTo Do\n' ;;
  "issue view "*" --raw") cat '`+issue+`' ;;
  "issue move EC-7 "*) ;;
  "issue move EC-8 "*) echo "transition not allowed" >&2; exit 1 ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
`)
	fakeClaude(t)
	return
}

func (r *rpc) action(t *testing.T, id int, action string) {
	t.Helper()
	r.actionWith(t, id, action, "{}")
}

// actionWith sends a panel action with the form values, a JSON object.
func (r *rpc) actionWith(t *testing.T, id int, action, values string) {
	t.Helper()
	r.send(`{"jsonrpc":"2.0","id":` + strconv.Itoa(id) + `,"method":"panel/action","params":{"panel":"waycarbon","action_id":"` + action + `","values":` + values + `}}`)
	r.reply(t, float64(id))
}

// tree waits for a render whose tree satisfies ok, and returns the tree.
func (r *rpc) tree(t *testing.T, ok func(map[string]any) bool) map[string]any {
	t.Helper()
	return render(t, r.until(t, func(m map[string]any) bool { return m["method"] == "panel/render" && ok(render(t, m)) }))
}

// refreshes counts the GitHub refreshes in a gh log: only the refresh's call has mine=.
func refreshes(log string) int { return strings.Count(string(mustRead(log)), " mine=") }

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
	for _, id := range []string{"sel:work:pr:" + prURL + "3", "sel:work:pr:" + prURL + "20", "sel:work:card:EC-7",
		"sel:prs:pr:" + prURL + "20", "sel:jira:card:EC-7", "sel:jira:card:EC-8", "sel:jira:card:EC-5"} {
		if findID(root, id) == nil {
			t.Errorf("no card %s", id)
		}
	}
	// The first item, PR 1, is selected: its detail offers what it needs and links to GitHub.
	work := findID(root, "work")
	if b := findID(work, "act:work:pr:"+prURL+"1"); b == nil || b["label"] != "Resolve conflict" {
		t.Errorf("PR 1 detail action: %v", b)
	}
	if !strings.Contains(jsonString(work), `"Open on GitHub","type":"link","url":"`+prURL+`1"`) {
		t.Error("the PR detail links to GitHub")
	}
	r.action(t, 6, "sel:work:pr:"+prURL+"3")
	root = r.tree(t, func(root map[string]any) bool { return findID(root, "act:work:pr:"+prURL+"3") != nil })
	if b := findID(root, "act:work:pr:"+prURL+"3"); b["label"] != "Fix review" {
		t.Errorf("PR 3 detail action: %v", b)
	}
	// The detail loads: size, failed checks first, reviews and open threads.
	root = r.tree(t, func(root map[string]any) bool { return findID(root, "work:pr-checks") != nil })
	if !strings.Contains(jsonString(findID(root, "work")), `"value":"+120 −8 · 5 files"`) || findID(root, "work:pr-threads") == nil {
		t.Errorf("PR detail: %v", textOf(findID(root, "work")))
	}
	r.action(t, 7, "act:work:pr:"+prURL+"3")
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
	// 4 needs you + 1 review request (the team queue does not count) + 1 card whose PR merged (EC-7).
	if bar["text"] != "6" {
		t.Errorf("bar: %v", bar)
	}
}

func TestMoveToDone(t *testing.T) {
	ghLog, jiraLog := workFakes(t)
	r := start(t)
	r.initializeRaw(t)
	r.cards(t)
	r.action(t, 6, "sel:work:card:EC-7")
	root := r.tree(t, func(root map[string]any) bool { return findID(root, "act:work:done:EC-7") != nil })
	if c, _ := findID(root, "act:work:done:EC-7")["confirm"].(map[string]any); c["title"] != "Move EC-7 to Done?" {
		t.Errorf("confirm: %v", c)
	}
	if b := findID(root, "act:work:card:EC-7"); b == nil || b["label"] != "Deploy" {
		t.Errorf("want Deploy: %v", b)
	}
	if !strings.Contains(jsonString(findID(root, "work")), "PR merged") {
		t.Error("the detail says the PR merged")
	}
	r.action(t, 7, "act:work:done:EC-7")
	waitFor(t, "jira issue move", func() bool { return strings.Contains(string(mustRead(jiraLog)), "issue move EC-7 Done\n") })
	waitFor(t, "a refresh after the move", func() bool { return refreshes(ghLog) >= 2 })
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
	if findID(root, "sel:jira:card:EC-1") == nil {
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
	waitFor(t, "the refresh after sign-in", func() bool { return refreshes(log) >= 2 })
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
case "$*" in
  *" mine="*)
    [ "$(grep -c ' mine=' '`+ghLog+`')" = 2 ] && while [ ! -f '`+gate+`' ]; do sleep 0.02; done
    cat '`+data+`' ;;
esac
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
	waitFor(t, "the slow refresh", func() bool { return refreshes(ghLog) == 2 })
	r.action(t, 8, "act:work:done:EC-7")
	waitFor(t, "jira move", func() bool { return fileExists(moved) })
	time.Sleep(200 * time.Millisecond)
	os.WriteFile(gate, nil, 0o644)
	waitFor(t, "the rerun", func() bool { return refreshes(ghLog) == 3 })
	r.tree(t, func(root map[string]any) bool {
		return findID(root, "sel:work:card:EC-7") == nil && findID(root, "sel:jira:card:EC-7") == nil &&
			!strings.Contains(textOf(root), "Loading…") && findID(root, "work:refresh") != nil
	})
	time.Sleep(300 * time.Millisecond)
	if n := refreshes(ghLog); n != 3 {
		t.Errorf("%d refreshes, want 3 (start, manual, one rerun)", n)
	}
}

func fileExists(p string) bool { _, err := os.Stat(p); return err == nil }

// The panel drops a button, section or tab whose id repeats anywhere in the tree, across tabs.
func TestPanelIdsAreUnique(t *testing.T) {
	workFakes(t)
	r := start(t)
	r.initializeRaw(t)
	seen := map[string]bool{}
	var walk func(n map[string]any)
	walk = func(n map[string]any) {
		switch n["type"] {
		case "button", "input", "select", "checkbox", "section", "tabs", nil:
			if id, _ := n["id"].(string); id != "" {
				if seen[id] {
					t.Errorf("repeated id %q", id)
				}
				seen[id] = true
			}
		}
		for _, key := range []string{"children", "tabs"} {
			kids, _ := n[key].([]any)
			for _, k := range kids {
				walk(k.(map[string]any))
			}
		}
	}
	r.cards(t)
	// Wait for both details, the PR's and the card's, so their sections are checked too.
	walk(r.tree(t, func(root map[string]any) bool {
		return findID(root, "work:pr-checks") != nil && findID(root, "jira:subtasks") != nil
	}))
	for _, id := range []string{"work", "jira", "prs", "work:refresh", "jira:refresh", "prs:refresh", "work:needs", "work:requested",
		"work:update", "work:waiting", "prs:mine", "prs:req", "prs:queue", "prs:q", "prs:search", "jira:q", "jira:search",
		"jira:scope:sprint", "jira:scope:mine", "act:work:pr:" + prURL + "1", "act:jira:solve:EC-5", "work:pr-checks", "jira:subtasks"} {
		if !seen[id] {
			t.Errorf("no %q in the tree", id)
		}
	}
}

// Search finds any card by text or key, a found card opens solve-card, and Clear brings the statuses back.
func TestJiraSearch(t *testing.T) {
	fakeJira(t, `
case "$*" in
  "me") echo "dev@example.com" ;;
  `+mineArgs+`|`+sprintArgs+`)
    printf 'EC-1\tFix the login timeout\tIn Progress\n' ;;
  "issue list --plain --no-headers --columns key,summary,status --paginate 50 -- -export button")
    printf 'EC-9\tAdd the export button\t\t\tDone\n' ;;
  "issue list --plain --no-headers --columns key,summary,status --paginate 50 -q key = EC-1")
    printf 'EC-1\tFix the login timeout\tIn Progress\n' ;;
  "issue list --plain --no-headers --columns key,summary,status --paginate 50 -- nothing")
    echo "No result found for given query in project EC" >&2; exit 1 ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
`)
	r := start(t)
	r.initialize(t)
	r.cards(t)
	// A search is done when the field holds it and nothing spins: the results and the first card's detail.
	search := func(id int, q string) map[string]any {
		t.Helper()
		r.actionWith(t, id, "jira:search", `{"jira:q":"`+q+`"}`)
		return r.tree(t, func(root map[string]any) bool {
			return findID(root, "jira:q")["value"] == strings.TrimSpace(q) && find(findID(root, "jira"), "spinner") == nil
		})
	}

	root := search(10, "  -export button ")
	if c := findID(root, "sel:jira:card:EC-9"); c == nil || c["badge"] != "Done" || c["selected"] != true {
		t.Fatalf("want EC-9 found by text: %v", c)
	}
	if strings.Contains(textOf(root), "In Progress · 1") {
		t.Error("results replace the board")
	}
	if findID(root, "act:jira:solve:EC-9") == nil {
		t.Error("the found card's detail offers Solve card")
	}
	r.action(t, 11, "act:jira:solve:EC-9")
	open := r.until(t, func(m map[string]any) bool { return m["method"] == "sessions/open" })
	if p := open["params"].(map[string]any); p["prompt"] != "/waycarbon:solve-card EC-9" {
		t.Errorf("found card click: %v", p)
	}

	if findID(search(12, "ec-1"), "sel:jira:card:EC-1") == nil {
		t.Error("want EC-1 found by key")
	}
	if !strings.Contains(textOf(search(13, "nothing")), "No card matches nothing.") {
		t.Error("want the no-match text")
	}

	r.action(t, 14, "jira:clear")
	root = r.tree(t, func(root map[string]any) bool { return findID(root, "jira:clear") == nil })
	if !strings.Contains(textOf(root), "In Progress · 1") || findID(root, "jira:q")["value"] != "" {
		t.Errorf("Clear brings back the board and empties the field: %v", findID(root, "jira:q"))
	}
}

// A review that lands between two refreshes sends one notification; the first load sends none.
func TestNewReviewNotifies(t *testing.T) {
	data := filepath.Join(t.TempDir(), "graphql.json")
	os.WriteFile(data, []byte(reviewedJSON("2026-11-16T14:00:00Z")), 0o644)
	fakeCLI(t, "gh", `[ "$1 $2" = "api graphql" ] && cat '`+data+`'
exit 0
`)
	fakeJira(t, happyJira)
	fakeClaude(t)
	r := start(t)
	r.initializeRaw(t)
	early := false
	m := r.until(t, func(m map[string]any) bool {
		early = early || m["method"] == "notify"
		return m["method"] == "panel/render" && !strings.Contains(textOf(render(t, m)), "Loading…")
	})
	if early {
		t.Error("the first load must not notify")
	}
	if c := findID(findID(render(t, m), "work:needs"), "sel:work:pr:https://github.com/waycarbon/front/pull/12"); c == nil || !strings.Contains(jsonString(c), "Approved") {
		t.Fatalf("an approved PR with CI running needs you: %v", c)
	}

	os.WriteFile(data, []byte(reviewedJSON("2026-11-16T16:00:00Z")), 0o644)
	r.action(t, 8, "work:refresh")
	m = r.until(t, func(m map[string]any) bool { return m["method"] == "notify" })
	if p := m["params"].(map[string]any); p["title"] != "@felipe approved #12" {
		t.Errorf("notify: %v", p)
	}
}

// Move to sends the status to jira and refreshes; a status off the board never reaches jira, and a
// failed move shows in the card's detail.
func TestMoveTo(t *testing.T) {
	ghLog, jiraLog := workFakes(t)
	r := start(t)
	r.initializeRaw(t)
	root := r.cards(t)
	menu := find(findID(root, "jira"), "menu")
	if menu == nil || menu["label"] != "Move to" {
		t.Fatalf("want a Move to menu: %v", menu)
	}
	// EC-5 is selected and To Do: the menu offers the other nine statuses, each confirmed.
	items := menu["items"].([]any)
	first := items[0].(map[string]any)
	if len(items) != 9 || first["id"] != "move:EC-5:Tech Refinement" || first["confirm"] == nil || strings.Contains(jsonString(items), "To Do") {
		t.Errorf("menu items: %v", items)
	}
	r.action(t, 6, "move:EC-7:Done")
	r.action(t, 7, "move:EC-7 --x:Test")
	r.action(t, 8, "move:EC-7:Code Review")
	waitFor(t, "jira issue move", func() bool { return strings.Contains(string(mustRead(jiraLog)), "issue move EC-7 Code Review\n") })
	waitFor(t, "a refresh after the move", func() bool { return refreshes(ghLog) >= 2 })
	if n := strings.Count(string(mustRead(jiraLog)), "issue move"); n != 1 {
		t.Errorf("%d moves, want 1: %s", n, mustRead(jiraLog))
	}

	r.action(t, 9, "sel:jira:card:EC-8")
	r.action(t, 10, "move:EC-8:Test")
	r.tree(t, func(root map[string]any) bool {
		return strings.Contains(jsonString(findID(root, "jira")), "Could not move EC-8 to Test: transition not allowed")
	})
}

// A PR search covers the org's PRs in any state; an open one offers a review, a merged one nothing.
func TestPRSearch(t *testing.T) {
	ghLog, _ := workFakes(t)
	r := start(t)
	r.initializeRaw(t)
	r.cards(t)
	r.actionWith(t, 6, "prs:search", `{"prs:q":" EC-1240 "}`)
	root := r.tree(t, func(root map[string]any) bool {
		return findID(root, "prs:clear") != nil && find(findID(root, "prs"), "spinner") == nil
	})
	if !strings.Contains(string(mustRead(ghLog)), "q=EC-1240 in:title is:pr org:waycarbon") {
		t.Errorf("search argv: %s", mustRead(ghLog))
	}
	prs := findID(root, "prs")
	if !strings.Contains(textOf(prs), "1 card, 3 PRs for EC-1240") || findID(prs, "prs:mine") != nil {
		t.Errorf("results replace the sections: %v", textOf(prs))
	}
	front, proto := "https://github.com/waycarbon/waycarbon-front/pull/92", "https://github.com/waycarbon/proto-repository/pull/93"
	if c := findID(prs, "sel:prs:pr:"+proto); c == nil || c["badge"] != "Merged" {
		t.Errorf("merged result: %v", c)
	}
	// The card's group comes first and is selected: its PRs together, with no Review all.
	if g := findID(prs, "sel:prs:group:search:EC-1240"); g == nil || g["selected"] != true || g["badge"] != "3 PRs" {
		t.Errorf("group: %v", g)
	}
	if strings.Contains(jsonString(prs), "act:prs:reviewall") {
		t.Error("a search group offers no Review all")
	}

	r.action(t, 7, "sel:prs:pr:"+proto)
	root = r.tree(t, func(root map[string]any) bool { return findID(root, "sel:prs:pr:"+proto)["selected"] == true })
	if strings.Contains(jsonString(findID(root, "prs")), `"act:prs:`) {
		t.Error("a merged PR has no action")
	}
	r.action(t, 8, "sel:prs:pr:"+front)
	r.tree(t, func(root map[string]any) bool { return findID(root, "act:prs:review:"+front) != nil })
	r.action(t, 9, "act:prs:review:"+front)
	open := r.until(t, func(m map[string]any) bool { return m["method"] == "sessions/open" })
	if p := open["params"].(map[string]any); p["prompt"] != "/waycarbon:code-review "+front {
		t.Errorf("review: %v", p)
	}

	r.action(t, 10, "prs:clear")
	root = r.tree(t, func(root map[string]any) bool { return findID(root, "prs:clear") == nil })
	if findID(root, "prs:mine") == nil || findID(root, "prs:q")["value"] != "" {
		t.Error("Clear brings back the sections and empties the field")
	}
}

// The Jira board shows the current sprint by default, all my cards on Todos os meus.
func TestJiraScope(t *testing.T) {
	fakeJira(t, `
case "$*" in
  "me") echo "dev@example.com" ;;
  `+mineArgs+`) printf 'EC-1\tFix the login timeout\tTo Do\nEC-3\tOld one\tTest\n' ;;
  `+sprintArgs+`) printf 'EC-1\tFix the login timeout\tTo Do\n' ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
`)
	r := start(t)
	r.initialize(t)
	root := r.cards(t)
	if findID(root, "sel:jira:card:EC-1") == nil || findID(root, "sel:jira:card:EC-3") != nil || findID(root, "jira:scope:sprint")["style"] != "primary" {
		t.Errorf("sprint scope: %v", textOf(findID(root, "jira")))
	}
	r.action(t, 6, "jira:scope:mine")
	root = r.tree(t, func(root map[string]any) bool { return findID(root, "sel:jira:card:EC-3") != nil })
	if findID(root, "jira:scope:mine")["style"] != "primary" || findID(root, "jira:scope:sprint")["style"] != nil {
		t.Error("Todos os meus is the active toggle")
	}
}
