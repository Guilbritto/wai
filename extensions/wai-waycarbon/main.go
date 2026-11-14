// wai-waycarbon: a wai extension (line-delimited JSON-RPC 2.0 on stdio) with one tabbed "WayCarbon" panel,
// a bar item and upkeep of the waycarbon Claude plugin.
package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

const panelID = "waycarbon"

type message struct {
	ID     json.RawMessage `json:"id,omitempty"`
	Method string          `json:"method,omitempty"`
	Params json.RawMessage `json:"params,omitempty"`
}

type node = map[string]any

type server struct {
	nextID atomic.Int64 // request ids; no lock needed
	wmu    sync.Mutex   // serialises writes
	out    io.Writer

	smu        sync.Mutex // guards the rest
	work       Work
	wst        WorkState
	refreshing bool
	signCmd    *exec.Cmd // the running gh login
	signKilled bool

	ctx context.Context // cancelled at EOF; plugin work stops with it
	wg  sync.WaitGroup  // plugin goroutines, drained before run returns

	dataDir, project string
	pluginMissing    bool
	installing       bool
	pluginErr        string
	counts           counts
	lastBar          string
}

func jsonString(v any) string {
	b, _ := json.Marshal(v)
	return string(b)
}

func (s *server) write(v any) {
	s.wmu.Lock()
	defer s.wmu.Unlock()
	b, _ := json.Marshal(v)
	fmt.Fprintf(s.out, "%s\n", b)
}

// call sends a request; safe with or without smu held.
func (s *server) call(method string, params any) {
	id := s.nextID.Add(1)
	s.write(node{"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

// refreshWork loads GitHub and Jira in parallel, off the caller. One refresh at a time: a request
// while one runs is dropped.
func (s *server) refreshWork() {
	s.smu.Lock()
	if s.refreshing {
		s.smu.Unlock()
		return
	}
	s.refreshing = true
	s.paintAll()
	s.smu.Unlock()
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		var snap Snapshot
		var cards []Card
		var ghErr, jiraErr error
		var both sync.WaitGroup
		both.Add(2)
		go func() { defer both.Done(); snap, ghErr = fetchGitHub(s.ctx, org(), time.Now().AddDate(0, 0, -30)) }()
		go func() { defer both.Done(); cards, jiraErr = myCards(s.ctx) }()
		both.Wait()
		st := WorkState{Login: snap.Login, Err: snap.Warn, Updated: time.Now()}
		var nf ErrNotFound
		switch {
		case ghErr == nil:
		case errors.As(ghErr, &nf):
			st.GhMissing = true
		case errors.Is(ghErr, errSignedOut):
			st.GhSignedOut = true
		default:
			st.Err = ghErr.Error()
		}
		if jiraErr != nil {
			if _, err := exec.LookPath("jira"); err != nil {
				st.JiraMissing = true
			} else {
				st.JiraProblem = jiraErr.Error()
			}
		}
		s.smu.Lock()
		defer s.smu.Unlock()
		st.SignIn = s.wst.SignIn
		s.work, s.wst, s.refreshing = buildWork(snap, cards), st, false
		s.counts.NeedsYou, s.counts.Reviews, s.counts.Cards = len(s.work.NeedsYou), len(s.work.Reviews), s.work.merged()
		s.paintAll()
	}()
}

// signIn runs gh's device login; its code and a Cancel show in the tab meanwhile.
func (s *server) signIn() {
	s.smu.Lock()
	if s.signCmd != nil {
		s.smu.Unlock()
		return
	}
	cmd, codeCh, exitCh := ghLogin(s.ctx)
	if cmd == nil {
		err := <-exitCh
		s.wst.SignIn = &GhSignIn{Err: err.Error()}
		s.paintAll()
		s.smu.Unlock()
		return
	}
	s.signCmd, s.signKilled = cmd, false
	s.wst.SignIn = &GhSignIn{}
	s.paintAll()
	s.smu.Unlock()
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		var err error
		for exitCh != nil {
			select {
			case code, ok := <-codeCh:
				if !ok {
					codeCh = nil
					continue
				}
				s.smu.Lock()
				s.wst.SignIn.Code = code
				s.paintAll()
				s.smu.Unlock()
			case err = <-exitCh:
				exitCh = nil
			}
		}
		s.smu.Lock()
		s.wst.SignIn = nil
		if err != nil && !s.signKilled {
			s.wst.SignIn = &GhSignIn{Err: err.Error()}
		}
		s.signCmd = nil
		s.paintAll()
		s.smu.Unlock()
		if err == nil {
			s.refreshWork()
		}
	}()
}

func (s *server) cancelSignIn() {
	s.smu.Lock()
	defer s.smu.Unlock()
	if s.signCmd != nil {
		s.signKilled = true
		s.signCmd.Process.Kill()
	}
}

// moveDone moves a card whose PR merged, then refreshes.
func (s *server) moveDone(key string) {
	s.smu.Lock()
	ok := false
	for _, c := range s.work.Cards {
		ok = ok || c.Key == key && c.MergedNoOpen
	}
	s.smu.Unlock()
	if !ok || !keyRe.MatchString(key) {
		return
	}
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		if err := moveCard(s.ctx, key); err != nil {
			s.smu.Lock()
			s.wst.Err = "Could not move " + key + ": " + err.Error()
			s.paintAll()
			s.smu.Unlock()
			return
		}
		s.refreshWork()
	}()
}

// checkPlugin looks for the plugin and repaints. A missing claude counts as a missing plugin.
func (s *server) checkPlugin() {
	_, ok, err := pluginInstalled(s.ctx)
	var nf ErrNotFound
	s.smu.Lock()
	defer s.smu.Unlock()
	if err == nil || errors.As(err, &nf) {
		s.pluginMissing = !ok
	}
	s.installing = false
	s.paintAll()
}

func (s *server) installPluginClick() {
	s.smu.Lock()
	if s.installing {
		s.smu.Unlock()
		return
	}
	s.installing, s.pluginErr = true, ""
	s.paintAll()
	s.smu.Unlock()
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		err := installPlugin(s.ctx)
		s.smu.Lock()
		if err != nil {
			s.pluginErr = err.Error()
		}
		s.smu.Unlock()
		s.checkPlugin()
	}()
}

func (s *server) reply(id json.RawMessage, result any) {
	s.write(node{"jsonrpc": "2.0", "id": id, "result": result})
}

func (s *server) handle(m message) {
	switch m.Method {
	case "initialize":
		var p struct {
			DataDir string `json:"data_dir"`
			Project string `json:"project"`
		}
		json.Unmarshal(m.Params, &p)
		s.smu.Lock()
		s.dataDir, s.project = p.DataDir, p.Project
		s.smu.Unlock()
		s.reply(m.ID, node{})
		s.refreshWork()
		s.wg.Add(3)
		go func() {
			defer s.wg.Done()
			t := time.NewTicker(10 * time.Minute)
			defer t.Stop()
			for {
				select {
				case <-s.ctx.Done():
					return
				case <-t.C:
					s.refreshWork()
				}
			}
		}()
		go func() { defer s.wg.Done(); s.checkPlugin() }()
		go func() {
			defer s.wg.Done()
			if p.DataDir == "" {
				return
			}
			if err := updatePluginDaily(s.ctx, p.DataDir, time.Now()); err != nil && s.ctx.Err() == nil {
				fmt.Fprintln(os.Stderr, "wai-waycarbon: plugin update:", err)
			}
		}()
	case "item/action":
		if m.ID != nil {
			s.reply(m.ID, nil)
		}
		s.call("panel/open", node{"panel": panelID})
	case "panel/action":
		var p struct {
			Action string `json:"action_id"`
		}
		json.Unmarshal(m.Params, &p)
		s.reply(m.ID, nil)
		switch p.Action {
		case "work:refresh":
			s.refreshWork()
			return
		case "gh:signin":
			s.signIn()
			return
		case "gh:cancel":
			s.cancelSignIn()
			return
		case "plugin:install":
			s.installPluginClick()
			return
		}
		if key, ok := strings.CutPrefix(p.Action, "done:"); ok {
			s.moveDone(key)
			return
		}
		s.smu.Lock()
		text, ok := s.work.clickPrompt(p.Action)
		s.smu.Unlock()
		if ok {
			s.call("sessions/open", node{"prompt": text, "send": false})
		}
	default:
		if m.ID != nil {
			s.write(node{"jsonrpc": "2.0", "id": m.ID, "error": node{"code": -32601, "message": "Method not found"}})
		}
	}
}

// run reads until EOF. Responses to our own calls (no method) are ignored.
func run(in io.Reader, out io.Writer) {
	ctx, cancel := context.WithCancel(context.Background())
	s := &server{ctx: ctx, out: out, lastBar: jsonString(node{"item": "attention"})}
	r := bufio.NewReaderSize(in, 1<<20)
	for {
		line, err := r.ReadBytes('\n')
		var m message
		if len(line) > 0 && json.Unmarshal(line, &m) == nil && m.Method != "" {
			s.handle(m)
		}
		if err != nil {
			cancel()
			s.wg.Wait()
			return
		}
	}
}

func main() { run(os.Stdin, os.Stdout) }
