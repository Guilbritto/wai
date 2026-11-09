// wai-rotinas: a wai extension (line-delimited JSON-RPC 2.0 on stdio) that runs the user's own
// scripts on a schedule, one at a time, from the allow-list <config>/rotinas.toml, and notifies
// when a run's result changed.
package main

import (
	"bufio"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"
)

// maxNap is R66: the scheduler looks at the wall clock at least once a minute. Go's timers pause
// while the computer sleeps, so a run due meanwhile starts at most a minute after it wakes.
const maxNap = time.Minute

const panelID = "rotinas"

type message struct {
	ID     json.RawMessage `json:"id,omitempty"`
	Method string          `json:"method,omitempty"`
	Params json.RawMessage `json:"params,omitempty"`
}

type node = map[string]any

type server struct {
	wmu sync.Mutex // serialises writes
	out io.Writer

	dir     string        // wai's config dir: rotinas.toml and the state file
	timeout time.Duration // per run
	// notifyGap: wai drops an extension's notification within 2 s (CALL_GAP) of its last.
	// ponytail: CALL_GAP plus 0.5 s of slack, since wai times each one when it handles it, after a
	// pipe, a reader thread and a frame.
	notifyGap time.Duration
	notified  time.Time // the last notification; the scheduler's own
	ctx       context.Context
	stop      context.CancelFunc // kills a running script
	wake      chan struct{}      // the scheduler looks again
	scheduler sync.WaitGroup     // serve waits for it, so a running script is killed and reaped

	mu       sync.Mutex // guards the rest
	routines []Routine
	problem  string // why rotinas.toml was not read
	state    map[string]record
	running  string
	queued   map[string]bool // Run now
}

func newServer(out io.Writer, dir string) *server {
	ctx, stop := context.WithCancel(context.Background())
	return &server{
		out: out, dir: dir, timeout: runTimeout, notifyGap: 2*time.Second + 500*time.Millisecond, ctx: ctx, stop: stop,
		wake:  make(chan struct{}, 1),
		state: loadState(filepath.Join(dir, stateFile)), queued: map[string]bool{},
	}
}

func (s *server) write(v any) {
	s.wmu.Lock()
	defer s.wmu.Unlock()
	b, _ := json.Marshal(v)
	fmt.Fprintf(s.out, "%s\n", b)
}

func (s *server) kick() {
	select {
	case s.wake <- struct{}{}:
	default:
	}
}

func (s *server) load() {
	routines, err := readConfig(filepath.Join(s.dir, configFile))
	s.mu.Lock()
	defer s.mu.Unlock()
	s.routines, s.problem, s.queued = routines, "", map[string]bool{}
	if err != nil {
		s.problem = err.Error()
	}
}

// render paints the panel; under s.mu, so paints go out in the order of the state they show.
func (s *server) render() {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.write(node{"jsonrpc": "2.0", "method": "panel/render", "params": node{"panel": panelID, "tree": s.tree()}})
}

func (s *server) tree() node {
	reload := node{"type": "button", "id": "reload", "label": "Reload"}
	text := func(t string, muted bool) node { return node{"type": "text", "text": t, "muted": muted} }
	switch {
	case s.problem != "":
		return node{"type": "column", "children": []node{text(s.problem, false), reload}}
	case len(s.routines) == 0:
		hint := "No routines. Add them to " + filepath.Join(s.dir, configFile) + " (see the wai-rotinas README), then Reload."
		return node{"type": "column", "children": []node{text(hint, true), reload}}
	}
	var kids []node
	for _, r := range s.routines {
		rec, ran := s.state[r.Name]
		status, why, _ := strings.Cut(rec.Status, ": ")
		var detail []string
		if ran {
			detail = append(detail, "Last run: "+rec.LastRun.Local().Format("Jan 2 15:04"))
		}
		switch {
		case s.running == r.Name:
			status = "Running"
		case s.queued[r.Name]:
			status = "Queued"
		default:
			if !ran {
				status = "Never run"
			}
			next := "now"
			if due := s.due(r); due.After(time.Now()) {
				next = due.Local().Format("Jan 2 15:04")
			}
			detail = append(detail, "Next: "+next)
		}
		kids = append(kids, node{"type": "row", "children": []node{
			{"type": "text", "text": r.Name},
			{"type": "badge", "text": status},
			{"type": "button", "id": "run:" + r.Name, "label": "Run now"}}})
		if len(detail) > 0 {
			kids = append(kids, text(strings.Join(detail, " · "), true))
		}
		if why != "" {
			kids = append(kids, text(why, false))
		}
	}
	return node{"type": "column", "children": append(kids, reload)}
}

// due is when r runs next, wall-clock; in the past when it was missed. A routine never run is due
// at once: it runs when added, which also shows at once whether its script is refused.
func (s *server) due(r Routine) time.Time {
	if rec, ok := s.state[r.Name]; ok {
		return r.next(rec.LastRun)
	}
	return time.Time{}
}

// pick takes the routine to run now (Run now first), or says how long to sleep, at most maxNap:
// < 0 is until woken (nothing to schedule).
func (s *server) pick(now time.Time) (*Routine, time.Duration) {
	now = now.Round(0)
	for i, r := range s.routines {
		if s.queued[r.Name] {
			return &s.routines[i], 0
		}
	}
	var first *Routine
	var at time.Time
	for i, r := range s.routines {
		if d := s.due(r); first == nil || d.Before(at) {
			first, at = &s.routines[i], d
		}
	}
	switch {
	case first == nil:
		return nil, -1
	case !at.After(now):
		return first, 0
	}
	return nil, min(at.Sub(now), maxNap)
}

// schedule runs one routine at a time. R59/R66: between runs it sleeps until the next one is due
// but at most maxNap, or until woken (Run now, Reload, any message from wai).
// ponytail: one wakeup a minute while routines exist; a host "system woke" event would avoid it.
func (s *server) schedule() {
	for s.ctx.Err() == nil {
		s.mu.Lock()
		r, wait := s.pick(time.Now().Round(0))
		var job Routine
		if r != nil {
			job = *r
			delete(s.queued, job.Name)
			s.running = job.Name
		}
		s.mu.Unlock()
		if r != nil {
			s.render()
			s.run(job)
			continue
		}
		var timer *time.Timer
		var tick <-chan time.Time // nil: nothing is scheduled, no timer
		if wait >= 0 {
			timer = time.NewTimer(wait)
			tick = timer.C
		}
		select {
		case <-s.wake:
		case <-tick:
		case <-s.ctx.Done():
		}
		if timer != nil {
			timer.Stop()
		}
	}
}

func (s *server) run(r Routine) {
	start := time.Now().Round(0)
	var o outcome
	if real, err := trusted(r.Script); err != nil {
		o.status = fmt.Sprintf("Refused: refusing script %s: %v", r.Script, err)
	} else {
		o = runScript(s.ctx, real, s.timeout)
	}
	if s.ctx.Err() != nil {
		return
	}
	sum := sha256.Sum256(append([]byte(o.status+"\n"), o.output...))
	hash := hex.EncodeToString(sum[:])
	s.mu.Lock()
	prev, seen := s.state[r.Name]
	s.state[r.Name] = record{LastRun: start, Status: o.status, Hash: hash}
	s.running = ""
	s.mu.Unlock()
	s.render()
	if !seen || prev.Hash != hash {
		// ponytail: waits on the scheduler, so a run due meanwhile starts up to notifyGap late.
		time.Sleep(time.Until(s.notified.Add(s.notifyGap)))
		s.notified = time.Now()
		s.write(node{"jsonrpc": "2.0", "method": "notify", "params": node{"title": r.Name, "body": summary(o)}})
	}
	// Saved after notifying: quitting in between notifies twice after a restart, never not at all.
	s.mu.Lock()
	defer s.mu.Unlock()
	if err := saveState(filepath.Join(s.dir, stateFile), s.state); err != nil {
		fmt.Fprintln(os.Stderr, "wai-rotinas: cannot save the last runs:", err)
	}
}

// summary is the notification's body: what went wrong, or the output's first line, in at most
// 300 characters, as wai shows them (it drops a message above 4 MB), the cut note included.
func summary(o outcome) string {
	const cut = " (output cut at 1 MB)"
	s := o.status
	if s == "OK" {
		s = "Finished with no output"
		for _, line := range strings.Split(string(o.output), "\n") {
			if line = strings.TrimSpace(line); line != "" {
				s = line
				break
			}
		}
	}
	room := 300
	if o.truncated {
		room -= len(cut)
	}
	if r := []rune(s); len(r) > room {
		s = string(r[:room-1]) + "…"
	}
	if o.truncated {
		s += cut
	}
	return s
}

func (s *server) handle(m message) {
	switch m.Method {
	case "initialize":
		s.write(node{"jsonrpc": "2.0", "id": m.ID, "result": node{"panels": []string{}}})
		s.load()
		s.render()
		s.scheduler.Add(1)
		go func() {
			defer s.scheduler.Done()
			s.schedule()
		}()
	case "panel/action":
		var p struct {
			Action string `json:"action_id"`
		}
		json.Unmarshal(m.Params, &p)
		s.write(node{"jsonrpc": "2.0", "id": m.ID, "result": nil})
		if p.Action == "reload" {
			s.load()
		} else if name, ok := strings.CutPrefix(p.Action, "run:"); ok {
			s.mu.Lock()
			for _, r := range s.routines {
				if r.Name == name && s.running != name {
					s.queued[name] = true
				}
			}
			s.mu.Unlock()
		}
		s.render()
		s.kick()
	default:
		if m.ID != nil {
			s.write(node{"jsonrpc": "2.0", "id": m.ID, "error": node{"code": -32601, "message": "Method not found"}})
		}
		s.kick()
	}
}

// serve reads until EOF, then kills a running script and waits for it, so main never exits
// before the kill. Responses to our own calls are ignored.
func serve(s *server, in io.Reader) {
	defer s.scheduler.Wait()
	defer s.stop()
	r := bufio.NewReaderSize(in, 1<<20)
	for {
		line, err := r.ReadBytes('\n')
		var m message
		if len(line) > 0 && json.Unmarshal(line, &m) == nil && m.Method != "" {
			s.handle(m)
		}
		if err != nil {
			return
		}
	}
}

func main() {
	// R18: wai starts its extensions from its config dir.
	dir, err := os.Getwd()
	if err != nil {
		fmt.Fprintln(os.Stderr, "wai-rotinas:", err)
		os.Exit(1)
	}
	serve(newServer(os.Stdout, dir), os.Stdin)
}
