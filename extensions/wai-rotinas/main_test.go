package main

import (
	"bufio"
	"context"
	"encoding/json"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"testing"
	"time"
	"unicode/utf8"
)

// TestMain lets a test run this binary as the extension itself (TestStdinEOFKillsRunningScript).
func TestMain(m *testing.M) {
	if os.Getenv("WAI_ROTINAS_RUN_MAIN") == "1" {
		main()
		return
	}
	os.Exit(m.Run())
}

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

func mkdir(t *testing.T, p string) string {
	t.Helper()
	if err := os.Mkdir(p, 0o700); err != nil {
		t.Fatal(err)
	}
	return p
}

func config(t *testing.T, dir, text string) {
	t.Helper()
	p := filepath.Join(dir, configFile)
	if err := os.WriteFile(p, []byte(text), 0o600); err != nil {
		t.Fatal(err)
	}
	chmod(t, p, 0o600)
}

// routine is one [[routine]] table.
func routine(name, script, schedule string) string {
	return "[[routine]]\nname = \"" + name + "\"\nscript = \"" + script + "\"\n" + schedule + "\n"
}

// seed records a run of each named routine a moment ago, so none of them is due.
func seed(t *testing.T, dir string, names ...string) {
	t.Helper()
	state := map[string]any{}
	for _, n := range names {
		state[n] = map[string]any{"last_run": time.Now()}
	}
	b, _ := json.Marshal(state)
	if err := os.WriteFile(filepath.Join(dir, stateFile), b, 0o600); err != nil {
		t.Fatal(err)
	}
}

func resolved(t *testing.T, p string) string {
	t.Helper()
	r, err := filepath.EvalSymlinks(p)
	if err != nil {
		t.Fatal(err)
	}
	return r
}

// rotinas_refuses_unsafe_scripts (Review Focus 3): a relative path, a file or any folder above it
// that group or others can write, and a symlink to such or through such are refused with a
// message naming the culprit.
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
	open := mkdir(t, filepath.Join(dir, "open"))
	inOpen := script(t, open, "x.sh", "echo x\n")
	// A symlink to a safe script, but in a folder anyone can write: it can be swapped.
	linkInOpen := filepath.Join(open, "link.sh")
	if err := os.Symlink(safe, linkInOpen); err != nil {
		t.Fatal(err)
	}
	chmod(t, open, 0o777)
	// The writable folder is two levels up.
	above := mkdir(t, filepath.Join(dir, "above"))
	deep := script(t, mkdir(t, filepath.Join(above, "sub")), "x.sh", "echo x\n")
	chmod(t, above, 0o777)
	sticky := mkdir(t, filepath.Join(dir, "sticky"))
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
		{linkInOpen, "its folder " + open + " is writable by group or others"},
		{deep, "its folder " + resolved(t, above) + " is writable by group or others"},
		{filepath.Join(dir, "missing.sh"), "does not exist"},
		{dir, "is not a regular file"},
	}
	for _, c := range cases {
		got, err := trusted(c.path)
		if err == nil || !strings.Contains(err.Error(), c.want) {
			t.Errorf("%s: want an error with %q, got %q, %v", c.path, c.want, got, err)
		}
	}

	// The allow-list itself must be safe too, and small.
	config(t, dir, "")
	chmod(t, filepath.Join(dir, configFile), 0o666)
	if _, err := readConfig(filepath.Join(dir, configFile)); err == nil || !strings.Contains(err.Error(), "writable by group or others") {
		t.Errorf("a world-writable rotinas.toml: %v", err)
	}
	config(t, dir, "# "+strings.Repeat("x", 1<<20)+"\n")
	if _, err := readConfig(filepath.Join(dir, configFile)); err == nil || !strings.Contains(err.Error(), "larger than 1 MB") {
		t.Errorf("a 1 MB rotinas.toml: %v", err)
	}

	// An unsafe entry runs nothing and says why, as soon as it is loaded.
	marker := filepath.Join(dir, "ran")
	bad := script(t, dir, "bad.sh", "touch '"+marker+"'\n")
	chmod(t, bad, 0o777)
	config(t, dir, routine("bad", bad, `every = "1h"`))
	r := start(t, dir)
	r.initialize(t)
	tree := r.until(t, "bad", "Refused")
	if !strings.Contains(textOf(tree), "refusing script "+bad) {
		t.Errorf("want the reason, got %v", tree)
	}
	if _, err := os.Stat(marker); err == nil {
		t.Error("an unsafe script ran")
	}
}

// schedules_next_run: `every` counts from the last run, `at` is the next such local time; a run
// missed while wai was closed, or never run yet, happens at once.
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

	// Daylight saving time: a time that does not exist runs once that day; one that happens
	// twice runs once.
	ny, err := time.LoadLocation("America/New_York")
	if err != nil {
		t.Fatal(err)
	}
	local := time.Local
	time.Local = ny
	t.Cleanup(func() { time.Local = local })
	spring := Routine{Name: "s", At: 2*60 + 30}
	first := spring.next(time.Date(2026, 3, 7, 12, 0, 0, 0, ny))
	if y, m, d := first.Date(); y != 2026 || m != 3 || d != 8 {
		t.Errorf("spring forward: got %v", first)
	}
	if again := spring.next(first); !again.Equal(time.Date(2026, 3, 9, 2, 30, 0, 0, ny)) {
		t.Errorf("after the spring-forward run: got %v", again)
	}
	fall := Routine{Name: "f", At: 1*60 + 30}
	first = fall.next(time.Date(2026, 10, 31, 12, 0, 0, 0, ny))
	if y, m, d := first.Date(); y != 2026 || m != 11 || d != 1 || first.Hour() != 1 || first.Minute() != 30 {
		t.Errorf("fall back: got %v", first)
	}
	if again := fall.next(first); !again.Equal(time.Date(2026, 11, 2, 1, 30, 0, 0, ny)) {
		t.Errorf("01:30 happens twice on Nov 1 but runs once: got %v", again)
	}
	time.Local = local

	// R66: due times are wall-clock (a monotonic clock stops while the computer sleeps), and the
	// scheduler never sleeps more than a minute before looking at the clock again.
	s := newServer(io.Discard, t.TempDir())
	s.routines = []Routine{{Name: "daily", Every: 24 * time.Hour}}
	s.state["daily"] = record{LastRun: time.Now()}
	if due := s.due(s.routines[0]); strings.Contains(due.String(), "m=") {
		t.Errorf("a due time with a monotonic reading: %v", due)
	}
	if r, wait := s.pick(time.Now()); r != nil || wait <= 0 || wait > time.Minute {
		t.Errorf("want a wait of at most a minute, got %v, %v", r, wait)
	}

	rs, err := parseConfig("\uFEFF" + `# my routines
[[routine]]   # the first
name = "Inbox"   # a comment
script = "/usr/local/bin/inbox.sh"
every = "15m"

[[ routine ]]
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
		"[[routine]] x\n":           "line 1",
		"name = \"a\"\n":            "outside a [[routine]]",
		head + "every = \"1h\"\n" + head + "at = \"09:00\"\n": "used twice",
		"[[routine]]\nscript = \"/a\"\nevery = \"1h\"\n":      "needs a name",
	}
	for text, msg := range bad {
		if _, err := parseConfig(text); err == nil || !strings.Contains(err.Error(), msg) {
			t.Errorf("%q: want %q, got %v", text, msg, err)
		}
	}

	// The scheduler: a routine missed while wai was closed runs at start without Run now, and so
	// does one never run (even every 24 h, after a restart); one that ran a moment ago does not.
	dir := t.TempDir()
	marker := func(name string) string { return filepath.Join(dir, name+"-ran") }
	sh := func(name string) string { return script(t, dir, name+".sh", "touch '"+marker(name)+"'\n") }
	config(t, dir, routine("due", sh("due"), `every = "1h"`)+routine("fresh", sh("fresh"), `every = "24h"`)+
		routine("later", sh("later"), `every = "1h"`))
	state, _ := json.Marshal(map[string]any{
		"due":   map[string]any{"last_run": time.Now().Add(-2 * time.Hour)},
		"later": map[string]any{"last_run": time.Now()},
	})
	if err := os.WriteFile(filepath.Join(dir, stateFile), state, 0o600); err != nil {
		t.Fatal(err)
	}
	r := start(t, dir)
	r.initialize(t)
	for tree := r.render(t); badge(tree, "due") != "OK" || badge(tree, "fresh") != "OK"; tree = r.render(t) {
	}
	for _, name := range []string{"due", "fresh"} {
		if _, err := os.Stat(marker(name)); err != nil {
			t.Errorf("%s did not run", name)
		}
	}
	if _, err := os.Stat(marker("later")); err == nil {
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
	if s := newServer(io.Discard, dir); s.timeout != runTimeout || s.notifyGap != 2500*time.Millisecond {
		t.Errorf("the server's timeout %v and notify gap %v", s.timeout, s.notifyGap)
	}
	ctx := context.Background()
	big := script(t, dir, "big.sh", "yes 0123456789 | head -c 3000000\necho done >&2\n")
	o := runScript(ctx, big, time.Minute)
	if o.status != "OK" || len(o.output) != maxOutput || !o.truncated {
		t.Errorf("big: status %q, %d bytes, truncated %v", o.status, len(o.output), o.truncated)
	}
	// One 1 MB line: the notification stays short and says the output was cut.
	line := script(t, dir, "line.sh", "head -c 3000000 /dev/zero | tr '\\0' x\n")
	o = runScript(ctx, line, time.Minute)
	// At most 300 characters, like wai shows, so the note is never cut off.
	if s := summary(o); utf8.RuneCountInString(s) > 300 || !strings.HasSuffix(s, "(output cut at 1 MB)") {
		t.Errorf("summary of %d runes: %.80q…", utf8.RuneCountInString(s), s)
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

// notifies_only_on_change: the first run notifies, the same result again does not, a new output
// or a new outcome does; the last result survives a restart.
func TestNotifiesOnlyOnChange(t *testing.T) {
	dir := t.TempDir()
	news, fail := filepath.Join(dir, "news.txt"), filepath.Join(dir, "fail")
	write := func(p, s string) {
		if err := os.WriteFile(p, []byte(s), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	write(news, "first\n")
	sh := script(t, dir, "news.sh", "cat '"+news+"'\n[ -f '"+fail+"' ] && exit 1\nexit 0\n")
	config(t, dir, routine("news", sh, `at = "03:00"`))
	r := start(t, dir)
	r.initialize(t)
	// The panel shows the result first: a notification may wait for wai's gap. One that was not
	// wanted would come before the next run starts, which runNow fails on.
	expect := func(r *rpc, tree map[string]any, status, want string) {
		t.Helper()
		if badge(tree, "news") != status || len(r.notified) > 0 {
			t.Errorf("want %s shown before any notification, got %v and %v", status, tree, r.notified)
		}
		if want == "" {
			return
		}
		if n := r.notification(t); n["title"] != "news" || n["body"] != want {
			t.Errorf("notified %v, want %q", n, want)
		}
	}
	// Never run: it runs once it is loaded.
	expect(r, r.until(t, "news", "OK"), "OK", "first")
	expect(r, r.runNow(t, "news"), "OK", "")
	write(news, "second\n")
	expect(r, r.runNow(t, "news"), "OK", "second")
	expect(r, r.runNow(t, "news"), "OK", "")
	write(fail, "")
	expect(r, r.runNow(t, "news"), "Failed", "Failed: exit status 1")
	expect(r, r.runNow(t, "news"), "Failed", "")

	r2 := start(t, dir)
	r2.initialize(t)
	expect(r2, r2.runNow(t, "news"), "Failed", "")
	r2.runNow(t, "news") // fails on a notification the last run should not have sent
}

// wai drops a notification within 2 s of the last: runs that end close together space theirs.
func TestNotificationsAreSpaced(t *testing.T) {
	dir := t.TempDir()
	config(t, dir, routine("a", script(t, dir, "a.sh", "echo a\n"), `every = "1h"`)+
		routine("b", script(t, dir, "b.sh", "echo b\n"), `every = "1h"`))
	const gap = 400 * time.Millisecond
	r := start(t, dir, func(s *server) { s.notifyGap = gap })
	r.initialize(t)
	var when []time.Time
	for len(when) < 2 {
		r.notification(t)
		when = append(when, time.Now())
	}
	if d := when[1].Sub(when[0]); d < gap-50*time.Millisecond {
		t.Errorf("two notifications %v apart, want at least %v", d, gap)
	}
}

// Closing stdin (wai stopping the extension) kills a running script before the process exits.
func TestStdinEOFKillsRunningScript(t *testing.T) {
	dir := t.TempDir()
	pidFile := filepath.Join(dir, "pid")
	config(t, dir, routine("long", script(t, dir, "long.sh", "echo $$ > '"+pidFile+"'\nexec sleep 30\n"), `every = "1h"`))
	cmd := exec.Command(os.Args[0], "-test.run=^$")
	cmd.Env = append(os.Environ(), "WAI_ROTINAS_RUN_MAIN=1")
	cmd.Dir = dir
	stdin, err := cmd.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	io.WriteString(stdin, `{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}`+"\n")
	var pid int
	for deadline := time.Now().Add(10 * time.Second); pid == 0; time.Sleep(20 * time.Millisecond) {
		if time.Now().After(deadline) {
			t.Fatal("the routine never started")
		}
		b, _ := os.ReadFile(pidFile)
		pid, _ = strconv.Atoi(strings.TrimSpace(string(b)))
	}
	proc, _ := os.FindProcess(pid)
	t.Cleanup(func() { proc.Kill() })
	stdin.Close()
	if err := cmd.Wait(); err != nil {
		t.Fatalf("the extension: %v", err)
	}
	if err := proc.Signal(syscall.Signal(0)); err == nil {
		t.Error("the script outlived the extension")
	}
}

// At most one run at a time: Run now on a second routine waits for the first to finish.
func TestOneRunAtATime(t *testing.T) {
	dir := t.TempDir()
	release, marker := filepath.Join(dir, "release"), filepath.Join(dir, "b-ran")
	t.Cleanup(func() { os.WriteFile(release, nil, 0o600) })
	config(t, dir, routine("a", script(t, dir, "a.sh", "while [ ! -f '"+release+"' ]; do sleep 0.05; done\n"), `every = "1h"`)+
		routine("b", script(t, dir, "b.sh", "touch '"+marker+"'\n"), `every = "1h"`))
	seed(t, dir, "a", "b")
	r := start(t, dir)
	r.initialize(t)
	r.action(t, "run:a")
	r.until(t, "a", "Running")
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
	r.until(t, "b", "OK")
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
	missing := filepath.Join(dir, "missing.sh")
	config(t, dir, routine("a", missing, `every = "1h"`))
	r.action(t, "reload")
	if txt := textOf(r.until(t, "a", "Refused")); !strings.Contains(txt, "does not exist") {
		t.Errorf("after reload: %q", txt)
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

// start runs a server on `dir`; notifications are not spaced unless an option says so.
func start(t *testing.T, dir string, opts ...func(*server)) *rpc {
	t.Helper()
	inR, inW := io.Pipe()
	outR, outW := io.Pipe()
	s := newServer(outW, dir)
	s.notifyGap = 0
	for _, o := range opts {
		o(s)
	}
	done := make(chan struct{})
	go func() {
		serve(s, inR)
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
	r.notified = nil
	r.render(t)
}

// read takes the next message: a render becomes r.last (true), a notification joins r.notified.
func (r *rpc) read(t *testing.T) (render bool) {
	t.Helper()
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
		return true
	default:
		t.Fatalf("unexpected %v", m)
	}
	return false
}

// render reads up to the next panel/render and returns its tree.
func (r *rpc) render(t *testing.T) map[string]any {
	t.Helper()
	for !r.read(t) {
	}
	return r.last
}

// notification reads up to the next notification and takes it.
func (r *rpc) notification(t *testing.T) map[string]any {
	t.Helper()
	for len(r.notified) == 0 {
		r.read(t)
	}
	n := r.notified[0]
	r.notified = r.notified[1:]
	return n
}

// until reads renders until routine `name` shows `status`.
func (r *rpc) until(t *testing.T, name, status string) map[string]any {
	t.Helper()
	for {
		if tree := r.render(t); badge(tree, name) == status {
			return tree
		}
	}
}

// action clicks a button and reads its answer; a notification of an earlier run may come first.
func (r *rpc) action(t *testing.T, id string) {
	t.Helper()
	r.send(`{"jsonrpc":"2.0","id":7,"method":"panel/action","params":{"panel":"rotinas","action_id":"` + id + `","values":{}}}`)
	for {
		m := r.recv()
		if m["id"] == float64(7) && m["method"] == nil {
			return
		}
		if m["method"] != "notify" {
			t.Fatalf("panel/action not answered: %v", m)
		}
		r.notified = append(r.notified, m["params"].(map[string]any))
	}
}

// runNow clicks Run now and returns the tree once the run ended. A notification that arrives
// before the run starts belongs to the run before, which wanted none: it fails the test.
func (r *rpc) runNow(t *testing.T, name string) map[string]any {
	t.Helper()
	r.notified = nil
	r.action(t, "run:"+name)
	r.until(t, name, "Running")
	if len(r.notified) > 0 {
		t.Errorf("an unexpected notification: %v", r.notified)
		r.notified = nil
	}
	for {
		tree := r.render(t)
		if b := badge(tree, name); b != "Queued" && b != "Running" {
			return tree
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
