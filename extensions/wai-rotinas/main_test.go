package main

import (
	"bufio"
	"context"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// script writes an executable sh script dir/name.
func script(t *testing.T, dir, name, body string) string {
	t.Helper()
	p := filepath.Join(dir, name)
	if err := os.WriteFile(p, []byte("#!/bin/sh\n"+body), 0o755); err != nil {
		t.Fatal(err)
	}
	chmod(t, p, 0o755)
	return p
}

func chmod(t *testing.T, p string, mode os.FileMode) {
	t.Helper()
	if err := os.Chmod(p, mode); err != nil {
		t.Fatal(err)
	}
}

func config(t *testing.T, dir, text string) {
	t.Helper()
	p := filepath.Join(dir, configFile)
	if err := os.WriteFile(p, []byte(text), 0o600); err != nil {
		t.Fatal(err)
	}
	chmod(t, p, 0o600)
}

func resolved(t *testing.T, p string) string {
	t.Helper()
	r, err := filepath.EvalSymlinks(p)
	if err != nil {
		t.Fatal(err)
	}
	return r
}

// rotinas_refuses_unsafe_scripts (Review Focus 3): a relative path, a file or folder group or
// others can write, and a symlink to such are refused with a message naming the culprit.
func TestRotinasRefusesUnsafeScripts(t *testing.T) {
	dir := t.TempDir()
	safe := script(t, dir, "safe.sh", "echo ok\n")
	if got, err := trusted(safe); err != nil || got != resolved(t, safe) {
		t.Fatalf("a safe script: got %q, %v", got, err)
	}
	link := filepath.Join(dir, "link.sh")
	if err := os.Symlink(safe, link); err != nil {
		t.Fatal(err)
	}
	if got, err := trusted(link); err != nil || got != resolved(t, safe) {
		t.Fatalf("a symlink to a safe script runs its target: got %q, %v", got, err)
	}

	world := script(t, dir, "world.sh", "echo w\n")
	chmod(t, world, 0o757)
	group := script(t, dir, "group.sh", "echo g\n")
	chmod(t, group, 0o775)
	toWorld := filepath.Join(dir, "to-world.sh")
	if err := os.Symlink(world, toWorld); err != nil {
		t.Fatal(err)
	}
	open := filepath.Join(dir, "open")
	if err := os.Mkdir(open, 0o700); err != nil {
		t.Fatal(err)
	}
	inOpen := script(t, open, "x.sh", "echo x\n")
	chmod(t, open, 0o777)
	sticky := filepath.Join(dir, "sticky")
	if err := os.Mkdir(sticky, 0o700); err != nil {
		t.Fatal(err)
	}
	inSticky := script(t, sticky, "x.sh", "echo x\n")
	chmod(t, sticky, 0o777|os.ModeSticky)
	if _, err := trusted(inSticky); err != nil {
		t.Errorf("a sticky folder (like /tmp) is fine: %v", err)
	}

	cases := []struct{ path, want string }{
		{"bin/safe.sh", "must be absolute"},
		{"./safe.sh", "must be absolute"},
		{world, resolved(t, world) + " is writable by group or others"},
		{group, resolved(t, group) + " is writable by group or others"},
		{toWorld, resolved(t, world) + " is writable by group or others"},
		{inOpen, "its folder " + resolved(t, open) + " is writable by group or others"},
		{filepath.Join(dir, "missing.sh"), "does not exist"},
		{dir, "is not a regular file"},
	}
	for _, c := range cases {
		got, err := trusted(c.path)
		if err == nil || !strings.Contains(err.Error(), c.want) {
			t.Errorf("%s: want an error with %q, got %q, %v", c.path, c.want, got, err)
		}
	}

	// The allow-list itself must be safe too.
	config(t, dir, "")
	chmod(t, filepath.Join(dir, configFile), 0o666)
	if _, err := readConfig(filepath.Join(dir, configFile)); err == nil || !strings.Contains(err.Error(), "writable by group or others") {
		t.Errorf("a world-writable rotinas.toml: %v", err)
	}

	// Run now on an unsafe entry runs nothing and says why.
	marker := filepath.Join(dir, "ran")
	bad := script(t, dir, "bad.sh", "touch '"+marker+"'\n")
	chmod(t, bad, 0o777)
	config(t, dir, "[[routine]]\nname = \"bad\"\nscript = \""+bad+"\"\nevery = \"1h\"\n")
	r := start(t, dir)
	r.initialize(t)
	_, tree := r.runNow(t, "bad")
	if badge(tree, "bad") != "Refused" || !strings.Contains(textOf(tree), "refusing script "+bad) {
		t.Errorf("want Refused with the reason, got %v", tree)
	}
	if _, err := os.Stat(marker); err == nil {
		t.Error("an unsafe script ran")
	}
}

// schedules_next_run: `every` counts from the last run, `at` is the next such local time; a run
// missed while wai was closed happens at once.
func TestSchedulesNextRun(t *testing.T) {
	at := func(d, h, m int) time.Time { return time.Date(2026, 11, d, h, m, 0, 0, time.Local) }
	hourly := Routine{Name: "h", Every: time.Hour}
	daily := Routine{Name: "d", At: 9*60 + 30}
	cases := []struct {
		r           Routine
		after, want time.Time
	}{
		{hourly, at(9, 10, 0), at(9, 11, 0)},
		{daily, at(8, 9, 30), at(9, 9, 30)},
		{daily, at(9, 9, 30), at(10, 9, 30)},
		{daily, at(9, 8, 0), at(9, 9, 30)},
		{daily, at(9, 10, 0), at(10, 9, 30)},
		{daily, at(30, 23, 59), time.Date(2026, 12, 1, 9, 30, 0, 0, time.Local)},
	}
	for _, c := range cases {
		if got := c.r.next(c.after); !got.Equal(c.want) {
			t.Errorf("%s after %v: got %v, want %v", c.r.Name, c.after, got, c.want)
		}
	}

	rs, err := parseConfig(`# my routines
[[routine]]
name = "Inbox"   # a comment
script = "/usr/local/bin/inbox.sh"
every = "15m"

[[routine]]
name = 'Daily'
script = 'C:\scripts\daily.cmd'
at = "07:05"
`)
	want := []Routine{
		{Name: "Inbox", Script: "/usr/local/bin/inbox.sh", Every: 15 * time.Minute},
		{Name: "Daily", Script: `C:\scripts\daily.cmd`, At: 7*60 + 5},
	}
	if err != nil || len(rs) != 2 || rs[0] != want[0] || rs[1] != want[1] {
		t.Fatalf("got %+v, %v", rs, err)
	}
	const head = "[[routine]]\nname = \"a\"\nscript = \"/a\"\n"
	bad := map[string]string{
		head + "every = \"1h\"\nat = \"09:00\"\n": "exactly one of every or at",
		head:                        "exactly one of every or at",
		head + "every = \"30s\"\n":  "at least 1m",
		head + "at = \"24:00\"\n":   "HH:MM",
		head + "every = 5\n":        "line 4",
		head + "colour = \"red\"\n": "unknown key",
		"[routines]\n":              "line 1",
		"name = \"a\"\n":            "outside a [[routine]]",
		head + "every = \"1h\"\n" + head + "at = \"09:00\"\n": "used twice",
		"[[routine]]\nscript = \"/a\"\nevery = \"1h\"\n":      "needs a name",
	}
	for text, msg := range bad {
		if _, err := parseConfig(text); err == nil || !strings.Contains(err.Error(), msg) {
			t.Errorf("%q: want %q, got %v", text, msg, err)
		}
	}

	// The scheduler: a routine missed while wai was closed runs at start, without Run now; one not
	// due yet does not.
	dir := t.TempDir()
	due := filepath.Join(dir, "due-ran")
	later := filepath.Join(dir, "later-ran")
	config(t, dir, "[[routine]]\nname = \"due\"\nscript = \""+script(t, dir, "due.sh", "touch '"+due+"'\n")+
		"\"\nevery = \"1h\"\n[[routine]]\nname = \"later\"\nscript = \""+
		script(t, dir, "later.sh", "touch '"+later+"'\n")+"\"\nevery = \"1h\"\n")
	last, _ := json.Marshal(map[string]any{"due": map[string]any{"last_run": time.Now().Add(-2 * time.Hour)}})
	if err := os.WriteFile(filepath.Join(dir, stateFile), last, 0o600); err != nil {
		t.Fatal(err)
	}
	r := start(t, dir)
	r.initialize(t)
	for badge(r.render(t), "due") != "OK" {
	}
	if _, err := os.Stat(due); err != nil {
		t.Error("the missed routine did not run")
	}
	if _, err := os.Stat(later); err == nil {
		t.Error("a routine that is not due ran")
	}
}

// output_cap_and_timeout: the output is capped at 1 MB and a run is stopped after 60 min (here
// a shorter, injected timeout), even when a grandchild holds the output pipe.
func TestOutputCapAndTimeout(t *testing.T) {
	if runTimeout != 60*time.Minute || maxOutput != 1<<20 {
		t.Fatalf("limits: %v, %d", runTimeout, maxOutput)
	}
	dir := t.TempDir()
	if s := newServer(io.Discard, dir); s.timeout != runTimeout {
		t.Errorf("the server's timeout: %v", s.timeout)
	}
	ctx := context.Background()
	big := script(t, dir, "big.sh", "yes 0123456789 | head -c 3000000\necho done >&2\n")
	o := runScript(ctx, big, time.Minute)
	if o.status != "OK" || len(o.output) != maxOutput || !o.truncated {
		t.Errorf("big: status %q, %d bytes, truncated %v", o.status, len(o.output), o.truncated)
	}

	// `sleep` outlives the killed sh and holds the pipe: the run still ends about 2 s later.
	// (1 s: macOS checks a new executable on its first run, which can take a while.)
	slow := script(t, dir, "slow.sh", "echo started\nsleep 10\n")
	begin := time.Now()
	o = runScript(ctx, slow, time.Second)
	if o.status != "Timed out" || string(o.output) != "started\n" {
		t.Errorf("slow: status %q, output %q", o.status, o.output)
	}
	if took := time.Since(begin); took > 6*time.Second {
		t.Errorf("the timeout took %v", took)
	}

	fail := script(t, dir, "fail.sh", "echo no >&2\nexit 3\n")
	if o = runScript(ctx, fail, time.Minute); o.status != "Failed: exit status 3" || string(o.output) != "no\n" {
		t.Errorf("fail: status %q, output %q", o.status, o.output)
	}
}

// notifies_only_on_change: the first run notifies, the same output again does not, a new one
// does; what the last run printed survives a restart.
func TestNotifiesOnlyOnChange(t *testing.T) {
	dir := t.TempDir()
	news := filepath.Join(dir, "news.txt")
	write := func(s string) {
		if err := os.WriteFile(news, []byte(s), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	write("first\n")
	config(t, dir, "[[routine]]\nname = \"news\"\nscript = \""+script(t, dir, "news.sh", "cat '"+news+"'\n")+"\"\nat = \"03:00\"\n")
	r := start(t, dir)
	r.initialize(t)
	check := func(r *rpc, want ...string) {
		t.Helper()
		got, tree := r.runNow(t, "news")
		if badge(tree, "news") != "OK" {
			t.Errorf("status: %v", tree)
		}
		var bodies []string
		for _, n := range got {
			if n["title"] != "news" {
				t.Errorf("title: %v", n)
			}
			bodies = append(bodies, n["body"].(string))
		}
		if strings.Join(bodies, "|") != strings.Join(want, "|") {
			t.Errorf("notified %q, want %q", bodies, want)
		}
	}
	check(r, "first")
	check(r)
	write("second\n")
	check(r, "second")
	check(r)

	r2 := start(t, dir)
	r2.initialize(t)
	check(r2)
}

// At most one run at a time: Run now on a second routine waits for the first to finish.
func TestOneRunAtATime(t *testing.T) {
	dir := t.TempDir()
	release, marker := filepath.Join(dir, "release"), filepath.Join(dir, "b-ran")
	t.Cleanup(func() { os.WriteFile(release, nil, 0o600) })
	config(t, dir, "[[routine]]\nname = \"a\"\nscript = \""+
		script(t, dir, "a.sh", "while [ ! -f '"+release+"' ]; do sleep 0.05; done\n")+"\"\nevery = \"1h\"\n"+
		"[[routine]]\nname = \"b\"\nscript = \""+script(t, dir, "b.sh", "touch '"+marker+"'\n")+"\"\nevery = \"1h\"\n")
	r := start(t, dir)
	r.initialize(t)
	r.action(t, "run:a")
	for badge(r.render(t), "a") != "Running" {
	}
	r.action(t, "run:b")
	tree := r.render(t)
	if badge(tree, "a") != "Running" || badge(tree, "b") != "Queued" {
		t.Fatalf("want a Running and b Queued, got %v", tree)
	}
	time.Sleep(200 * time.Millisecond)
	if _, err := os.Stat(marker); err == nil {
		t.Fatal("b ran while a was running")
	}
	os.WriteFile(release, nil, 0o600)
	for badge(r.render(t), "b") != "OK" {
	}
	if _, err := os.Stat(marker); err != nil {
		t.Error("b never ran")
	}
}

// A broken rotinas.toml is shown with its line; Reload reads the fixed one.
func TestConfigProblemAndReload(t *testing.T) {
	dir := t.TempDir()
	config(t, dir, "[[routine]]\nname = \"a\"\nscript = /a\n")
	r := start(t, dir)
	r.initialize(t)
	if txt := textOf(r.last); !strings.Contains(txt, "line 3") {
		t.Fatalf("want the line of the problem, got %q", txt)
	}
	config(t, dir, "[[routine]]\nname = \"a\"\nscript = \"/a\"\nevery = \"1h\"\n")
	r.action(t, "reload")
	if tree := r.render(t); badge(tree, "a") != "Never run" {
		t.Errorf("after reload: %v", tree)
	}
	os.Remove(filepath.Join(dir, configFile))
	r.action(t, "reload")
	if txt := textOf(r.render(t)); !strings.Contains(txt, "No routines") {
		t.Errorf("without rotinas.toml: %q", txt)
	}
}

// rpc runs the server on in-memory pipes. Every notify it reads is kept in `notified`.
type rpc struct {
	send     func(string)
	recv     func() map[string]any
	last     map[string]any // the last tree
	notified []map[string]any
}

func start(t *testing.T, dir string) *rpc {
	t.Helper()
	inR, inW := io.Pipe()
	outR, outW := io.Pipe()
	done := make(chan struct{})
	go func() {
		serve(newServer(outW, dir), inR)
		outW.Close()
		close(done)
	}()
	br := bufio.NewReader(outR)
	r := &rpc{}
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
	// Drained, so a write after the test's last read never blocks the server's exit.
	t.Cleanup(func() {
		inW.Close()
		go io.Copy(io.Discard, outR)
		<-done
	})
	return r
}

func (r *rpc) initialize(t *testing.T) {
	t.Helper()
	r.send(`{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":1,"ext_dir":"/x"}}`)
	if m := r.recv(); m["id"] != float64(0) || m["result"] == nil {
		t.Fatalf("initialize reply: %v", m)
	}
	r.render(t)
}

// render reads up to the next panel/render and returns its tree.
func (r *rpc) render(t *testing.T) map[string]any {
	t.Helper()
	for {
		m := r.recv()
		switch m["method"] {
		case "notify":
			r.notified = append(r.notified, m["params"].(map[string]any))
		case "panel/render":
			p := m["params"].(map[string]any)
			if p["panel"] != panelID {
				t.Fatalf("panel: %v", p["panel"])
			}
			r.last = p["tree"].(map[string]any)
			return r.last
		default:
			t.Fatalf("unexpected %v", m)
		}
	}
}

// action clicks a button and reads its answer.
func (r *rpc) action(t *testing.T, id string) {
	t.Helper()
	r.send(`{"jsonrpc":"2.0","id":7,"method":"panel/action","params":{"panel":"rotinas","action_id":"` + id + `","values":{}}}`)
	if m := r.recv(); m["id"] != float64(7) {
		t.Fatalf("panel/action not answered: %v", m)
	}
}

// runNow clicks Run now and waits for the run to end: the notifications sent meanwhile, and the
// tree after it.
func (r *rpc) runNow(t *testing.T, name string) ([]map[string]any, map[string]any) {
	t.Helper()
	r.notified = nil
	r.action(t, "run:"+name)
	for {
		tree := r.render(t)
		if b := badge(tree, name); b != "Queued" && b != "Running" {
			return r.notified, tree
		}
	}
}

// badge is the status of routine `name`: the badge in the row that starts with its name.
func badge(n map[string]any, name string) string {
	kids, _ := n["children"].([]any)
	if n["type"] == "row" && len(kids) > 1 {
		first, second := kids[0].(map[string]any), kids[1].(map[string]any)
		if first["text"] == name && second["type"] == "badge" {
			return second["text"].(string)
		}
	}
	for _, k := range kids {
		if b := badge(k.(map[string]any), name); b != "" {
			return b
		}
	}
	return ""
}

func textOf(n map[string]any) string {
	s, _ := n["text"].(string)
	kids, _ := n["children"].([]any)
	for _, k := range kids {
		s += " " + textOf(k.(map[string]any))
	}
	return s
}
