package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"time"
	"unicode"
)

const (
	runTimeout = 60 * time.Minute
	maxOutput  = 1 << 20
	maxConfig  = 1 << 20
	minEvery   = time.Minute
	// Both live in wai's config dir, which wai starts extensions from (R18).
	configFile = "rotinas.toml"
	stateFile  = "rotinas-state.json"
)

// Routine is one [[routine]] of rotinas.toml: Every > 0, or At (minutes after local midnight).
type Routine struct {
	Name   string
	Script string
	Every  time.Duration
	At     int
}

// next is the first run after the last one. Wall-clock (R66): a monotonic reading stops while
// the computer sleeps.
func (r Routine) next(after time.Time) time.Time {
	after = after.Round(0)
	if r.Every > 0 {
		return after.Add(r.Every)
	}
	after = after.Local()
	y, m, d := after.Date()
	at := time.Date(y, m, d, r.At/60, r.At%60, 0, 0, time.Local)
	if !at.After(after) {
		at = time.Date(y, m, d+1, r.At/60, r.At%60, 0, 0, time.Local)
	}
	return at
}

// readConfig reads the allow-list. A missing file is no routines; an unsafe one is refused.
func readConfig(path string) ([]Routine, error) {
	if _, err := os.Lstat(path); errors.Is(err, fs.ErrNotExist) {
		return nil, nil
	}
	if _, err := trusted(path); err != nil {
		return nil, fmt.Errorf("refusing %s: %w", path, err)
	}
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	text, err := io.ReadAll(io.LimitReader(f, maxConfig+1))
	if err != nil {
		return nil, err
	}
	if len(text) > maxConfig {
		return nil, fmt.Errorf("refusing %s: it is larger than 1 MB", path)
	}
	routines, err := parseConfig(string(text))
	if err != nil {
		return nil, fmt.Errorf("%s: %w", path, err)
	}
	return routines, nil
}

// parseConfig reads the TOML subset rotinas.toml uses: comments, [[routine]] tables and
// `key = "string"` (or 'literal') lines.
// ponytail: no TOML library; switch to one if the schema grows arrays or nested tables.
func parseConfig(text string) ([]Routine, error) {
	type raw struct {
		line int
		keys map[string]string
	}
	var tables []raw
	for i, line := range strings.Split(strings.TrimPrefix(text, "\uFEFF"), "\n") {
		n := i + 1
		line = strings.TrimSpace(line)
		switch {
		case line == "" || strings.HasPrefix(line, "#"):
			continue
		case strings.HasPrefix(line, "["):
			h, _, _ := strings.Cut(line, "#")
			h = strings.TrimSpace(h)
			if !strings.HasPrefix(h, "[[") || !strings.HasSuffix(h, "]]") || strings.TrimSpace(h[2:len(h)-2]) != "routine" {
				return nil, fmt.Errorf("line %d: only [[routine]] tables are allowed", n)
			}
			tables = append(tables, raw{n, map[string]string{}})
			continue
		}
		key, value, ok := strings.Cut(line, "=")
		key = strings.TrimSpace(key)
		if !ok {
			return nil, fmt.Errorf("line %d: expected key = \"value\"", n)
		}
		if len(tables) == 0 {
			return nil, fmt.Errorf("line %d: %s is outside a [[routine]]", n, key)
		}
		switch key {
		case "name", "script", "every", "at":
		default:
			return nil, fmt.Errorf("line %d: unknown key %q (use name, script, every or at)", n, key)
		}
		t := tables[len(tables)-1]
		if _, dup := t.keys[key]; dup {
			return nil, fmt.Errorf("line %d: %s is set twice", n, key)
		}
		v, err := parseString(strings.TrimSpace(value))
		if err != nil {
			return nil, fmt.Errorf("line %d: %v", n, err)
		}
		t.keys[key] = v
	}

	routines := make([]Routine, 0, len(tables))
	names := map[string]bool{}
	for _, t := range tables {
		fail := func(format string, a ...any) error {
			return fmt.Errorf("routine at line %d: "+format, append([]any{t.line}, a...)...)
		}
		r := Routine{Name: t.keys["name"], Script: t.keys["script"]}
		if r.Name == "" || len(r.Name) > 100 || strings.IndexFunc(r.Name, unicode.IsControl) >= 0 {
			return nil, fail("needs a name of 1 to 100 bytes on one line")
		}
		if names[r.Name] {
			return nil, fail("the name %q is used twice", r.Name)
		}
		names[r.Name] = true
		if r.Script == "" {
			return nil, fail("needs a script")
		}
		every, hasEvery := t.keys["every"]
		at, hasAt := t.keys["at"]
		if hasEvery == hasAt {
			return nil, fail("needs exactly one of every or at")
		}
		if hasEvery {
			d, err := time.ParseDuration(every)
			if err != nil || d < minEvery {
				return nil, fail("every must be a duration of at least 1m, like \"30m\" or \"2h\"")
			}
			r.Every = d
		} else {
			hm, err := time.Parse("15:04", at)
			if err != nil {
				return nil, fail("at must be a time of day as HH:MM, like \"09:30\"")
			}
			r.At = hm.Hour()*60 + hm.Minute()
		}
		routines = append(routines, r)
	}
	return routines, nil
}

// parseString reads a "basic" (escapes as in Go) or 'literal' string, then an optional comment.
func parseString(s string) (string, error) {
	var v, rest string
	switch {
	case strings.HasPrefix(s, `"`):
		q, err := strconv.QuotedPrefix(s)
		if err != nil {
			return "", errors.New("unterminated or invalid string")
		}
		v, _ = strconv.Unquote(q)
		rest = s[len(q):]
	case strings.HasPrefix(s, "'"):
		end := strings.IndexByte(s[1:], '\'')
		if end < 0 {
			return "", errors.New("unterminated string")
		}
		v, rest = s[1:1+end], s[2+end:]
	default:
		return "", errors.New(`values are quoted strings, like "30m"`)
	}
	if rest = strings.TrimSpace(rest); rest != "" && !strings.HasPrefix(rest, "#") {
		return "", fmt.Errorf("unexpected %q after the value", rest)
	}
	return v, nil
}

// trusted is Review Focus 3: the file a path names, through any symlinks, when only the user (or
// root) can change it, like ssh's StrictModes: an absolute path to a regular file owned by them
// that neither group nor others can write, and every folder up to / above it and above the path
// as written (where a symlink could be swapped) owned by them and not writable by group or others
// either (unless sticky, like /tmp). A group-writable /Applications (root:admin 0775) is refused.
func trusted(path string) (string, error) {
	if !filepath.IsAbs(path) {
		return "", errors.New("the path must be absolute")
	}
	real, err := filepath.EvalSymlinks(path)
	if errors.Is(err, fs.ErrNotExist) {
		return "", errors.New("it does not exist")
	}
	if err != nil {
		return "", err
	}
	if err := safe(real, false); err != nil {
		return "", err
	}
	for _, p := range []string{real, path} {
		for d := filepath.Dir(p); ; d = filepath.Dir(d) {
			if err := safe(d, true); err != nil {
				return "", fmt.Errorf("its folder %w", err)
			}
			if d == filepath.Dir(d) {
				break
			}
		}
	}
	return real, nil
}

func safe(path string, dir bool) error {
	fi, err := os.Stat(path)
	if err != nil {
		return err
	}
	if !dir && !fi.Mode().IsRegular() {
		return fmt.Errorf("%s is not a regular file", path)
	}
	if err := owned(path, fi); err != nil {
		return err
	}
	if fi.Mode().Perm()&0o022 != 0 && !(dir && fi.Mode()&fs.ModeSticky != 0) {
		return fmt.Errorf("%s is writable by group or others; run chmod go-w %s", path, path)
	}
	return nil
}

// outcome is what one run left: its output (stdout and stderr, in order) and its status.
type outcome struct {
	output    []byte
	truncated bool
	status    string // "OK", "Failed: …", "Timed out", "Stopped" or "Refused: …"
}

// capped keeps the first maxOutput bytes and drains the rest, so the script never blocks on a
// full pipe.
type capped struct {
	buf       []byte
	truncated bool
}

func (c *capped) Write(p []byte) (int, error) {
	keep := min(len(p), maxOutput-len(c.buf))
	c.buf = append(c.buf, p[:keep]...)
	c.truncated = c.truncated || keep < len(p)
	return len(p), nil
}

// runScript runs path with no arguments and no shell, stdin empty, from wai's config dir. It
// stays in the extension's process group (Windows: job), so it dies with the extension.
// A timeout or stop kills the script and everything it started (killTree). The tree, not a
// group of its own: a group of its own would outlive wai's SIGKILL of the extension's group.
func runScript(ctx context.Context, path string, timeout time.Duration) outcome {
	run, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	out := &capped{}
	cmd := exec.CommandContext(run, path)
	cmd.Cancel = func() error { return killTree(cmd.Process.Pid) }
	cmd.Stdout, cmd.Stderr = out, out
	// A grandchild holding the pipes cannot keep the run going after the script ends.
	cmd.WaitDelay = 2 * time.Second
	err := cmd.Run()
	o := outcome{output: out.buf, truncated: out.truncated}
	switch {
	case errors.Is(run.Err(), context.DeadlineExceeded):
		o.status = "Timed out"
	case ctx.Err() != nil:
		o.status = "Stopped"
	case cmd.ProcessState != nil && cmd.ProcessState.Success():
		o.status = "OK"
	case cmd.ProcessState != nil:
		o.status = "Failed: " + cmd.ProcessState.String()
	default:
		o.status = "Failed: " + err.Error()
	}
	return o
}

// descendants lists every process below root in rows of {pid, ppid}, breadth first, never root.
func descendants(rows [][2]int, root int) []int {
	seen := map[int]bool{root: true}
	var out []int
	for queue := []int{root}; len(queue) > 0; queue = queue[1:] {
		for _, r := range rows {
			if r[1] == queue[0] && !seen[r[0]] {
				seen[r[0]] = true
				out = append(out, r[0])
				queue = append(queue, r[0])
			}
		}
	}
	return out
}

// record is what wai-rotinas remembers of a routine's last run, across restarts.
type record struct {
	LastRun time.Time `json:"last_run"`
	Status  string    `json:"status"`
	// Of the status and the output: a run notifies when it differs.
	Hash string `json:"hash"`
}

// loadState: a missing or broken file is no history.
func loadState(path string) map[string]record {
	state := map[string]record{}
	if b, err := os.ReadFile(path); err == nil {
		json.Unmarshal(b, &state)
	}
	return state
}

// saveState replaces the file whole: a crash leaves the old one or the new one.
func saveState(path string, state map[string]record) error {
	b, err := json.MarshalIndent(state, "", "  ")
	if err != nil {
		return err
	}
	tmp, err := os.CreateTemp(filepath.Dir(path), stateFile+".*")
	if err != nil {
		return err
	}
	defer os.Remove(tmp.Name())
	if _, err := tmp.Write(b); err != nil {
		tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	return os.Rename(tmp.Name(), path)
}
