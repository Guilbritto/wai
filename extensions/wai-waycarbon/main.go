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
	"slices"
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
	Result json.RawMessage `json:"result,omitempty"`
	Error  *struct {
		Message string `json:"message"`
	} `json:"error,omitempty"`
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
	again      bool      // a refresh was asked mid-refresh: run once more
	signCmd    *exec.Cmd // the running gh login
	signKilled bool

	ctx context.Context // cancelled at EOF; plugin work stops with it
	wg  sync.WaitGroup  // plugin goroutines, drained before run returns

	dataDir, project string
	pluginMissing    bool
	installing       bool
	pluginErr        string
	counts           counts
	reviews          map[string]time.Time // newest review per PR of mine; nil until GitHub first loads
	lastBar          string

	sel         map[string]string // tab → selected item id
	scope       string            // the Jira board's cards: "" the current sprint, "mine" all assigned to me
	sprint      []Card
	prSearch    *PRSearch
	prDetails   map[string]*detail[PRDetail] // by PR URL; a refresh clears them
	cardDetails map[string]*detail[cardData] // by card key
	moveKey     string                       // the card whose last move failed (moveErr says why), shown in its detail
	moveErr     string                       // a refresh that starts after the failure clears it
}

func newServer(ctx context.Context, out io.Writer) *server {
	return &server{ctx: ctx, out: out, lastBar: jsonString(node{"item": "attention"}), sel: map[string]string{},
		prDetails: map[string]*detail[PRDetail]{}, cardDetails: map[string]*detail[cardData]{}}
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
// while one runs makes it run once more when it ends.
func (s *server) refreshWork() {
	s.smu.Lock()
	if s.refreshing {
		s.again = true
		s.smu.Unlock()
		return
	}
	s.refreshing = true
	seen := s.moveErr
	s.paintAll()
	s.smu.Unlock()
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		var snap Snapshot
		var cards, sprint []Card
		var ghErr, jiraErr, sprintErr error
		var all sync.WaitGroup
		all.Add(3)
		go func() { defer all.Done(); snap, ghErr = fetchGitHub(s.ctx, org(), time.Now().AddDate(0, 0, -30)) }()
		go func() { defer all.Done(); cards, jiraErr = myCards(s.ctx) }()
		go func() { defer all.Done(); sprint, sprintErr = sprintCards(s.ctx) }()
		all.Wait()
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
		} else if sprintErr != nil {
			st.SprintErr = sprintErr.Error()
		}
		s.smu.Lock()
		st.SignIn, st.Search = s.wst.SignIn, s.wst.Search
		s.work, s.wst, s.sprint, s.refreshing = buildWork(snap, cards), st, sprint, false
		clear(s.prDetails)
		clear(s.cardDetails)
		if s.moveErr == seen {
			s.moveErr = ""
		}
		s.counts.NeedsYou, s.counts.Reviews, s.counts.Cards = len(s.work.NeedsYou), s.work.requested(), s.work.merged()
		if ghErr == nil {
			// ponytail: the first load only records, so starting wai does not replay old reviews.
			first := s.reviews == nil
			if first {
				s.reviews = map[string]time.Time{}
			}
			if prs := newReviews(s.reviews, snap.Mine); !first && len(prs) > 0 {
				s.notify(reviewNote(prs))
			}
		}
		s.paintAll()
		again := s.again && s.ctx.Err() == nil
		s.again = false
		s.smu.Unlock()
		if again {
			s.refreshWork()
		}
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

// move moves a card to a status of the board, then refreshes; a failure shows in the card's detail.
func (s *server) move(key, status string) {
	if !keyRe.MatchString(key) || !slices.Contains(boardOrder, status) {
		return
	}
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		err := moveTo(s.ctx, key, status)
		s.smu.Lock()
		s.moveKey, s.moveErr = key, ""
		if err != nil {
			s.moveErr = "Could not move " + key + " to " + status + ": " + err.Error()
		}
		s.paintAll()
		s.smu.Unlock()
		if err == nil {
			s.refreshWork()
		}
	}()
}

// findPRs runs a PR search off the caller, like search does for cards.
func (s *server) findPRs(q string) {
	q = strings.TrimSpace(q)
	s.smu.Lock()
	defer s.smu.Unlock()
	if q == "" {
		s.prSearch = nil
		s.paintAll()
		return
	}
	ps := &PRSearch{Query: q, Busy: true}
	s.prSearch = ps
	s.paintAll()
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		prs, err := searchPRs(s.ctx, org(), q)
		s.smu.Lock()
		defer s.smu.Unlock()
		if s.prSearch != ps {
			return
		}
		s.prSearch = &PRSearch{Query: q, PRs: prs}
		if err != nil {
			s.prSearch.Err = err.Error()
		}
		s.paintAll()
	}()
}

// solvePrompt is Solve card for a listed card: solve:KEY.
func (s *server) solvePrompt(id string) (string, bool) {
	key, ok := strings.CutPrefix(id, "solve:")
	if _, listed := s.findCard(key); !ok || !listed {
		return "", false
	}
	return promptFor("solve", key), true
}

// search runs a card search off the caller; an empty query clears it. A newer search or Clear
// drops an older one's results.
func (s *server) search(q string) {
	q = strings.TrimSpace(q)
	s.smu.Lock()
	defer s.smu.Unlock()
	if q == "" {
		s.wst.Search = nil
		s.paintAll()
		return
	}
	cs := &CardSearch{Query: q, Busy: true}
	s.wst.Search = cs
	s.paintAll()
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		cards, err := searchCards(s.ctx, q)
		s.smu.Lock()
		defer s.smu.Unlock()
		if s.wst.Search != cs {
			return
		}
		s.wst.Search = &CardSearch{Query: q, Cards: cards}
		if err != nil {
			s.wst.Search.Err = err.Error()
		}
		s.paintAll()
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

func (s *server) notify(title, body string) {
	s.call("notify", node{"title": title, "body": body})
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
			Action string         `json:"action_id"`
			Values map[string]any `json:"values"`
		}
		json.Unmarshal(m.Params, &p)
		s.reply(m.ID, nil)
		values := map[string]string{}
		for k, v := range p.Values {
			if str, ok := v.(string); ok {
				values[k] = str
			}
		}
		switch p.Action {
		case "work:refresh", "jira:refresh", "prs:refresh":
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
		case "jira:search":
			s.search(values["jira:q"])
			return
		case "jira:clear":
			s.search("")
			return
		case "jira:scope:sprint", "jira:scope:mine":
			s.smu.Lock()
			s.scope = strings.TrimPrefix(p.Action, "jira:scope:")
			s.paintAll()
			s.smu.Unlock()
			return
		case "prs:search":
			s.findPRs(values["prs:q"])
			return
		case "prs:clear":
			s.findPRs("")
			return
		}
		kind, rest, _ := strings.Cut(p.Action, ":")
		switch kind {
		case "sel":
			tab, item, _ := strings.Cut(rest, ":")
			if tab != "work" && tab != "jira" && tab != "prs" {
				return
			}
			s.smu.Lock()
			s.sel[tab] = item
			s.paintAll()
			s.smu.Unlock()
		case "move":
			key, status, _ := strings.Cut(rest, ":")
			s.move(key, status)
		case "act":
			_, inner, _ := strings.Cut(rest, ":")
			if key, ok := strings.CutPrefix(inner, "done:"); ok {
				s.moveDone(key)
				return
			}
			s.smu.Lock()
			text, ok := s.work.clickPrompt(inner)
			if !ok {
				text, ok = s.prSearch.clickPrompt(inner)
			}
			if !ok {
				text, ok = s.solvePrompt(inner)
			}
			s.smu.Unlock()
			if ok {
				s.call("sessions/open", node{"prompt": text, "send": false})
			}
		}
	default:
		if m.ID != nil {
			s.write(node{"jsonrpc": "2.0", "id": m.ID, "error": node{"code": -32601, "message": "Method not found"}})
		}
	}
}

// run reads until EOF. Answers to our own requests are ignored.
func run(in io.Reader, out io.Writer) {
	ctx, cancel := context.WithCancel(context.Background())
	s := newServer(ctx, out)
	r := bufio.NewReaderSize(in, 1<<20)
	for {
		line, err := r.ReadBytes('\n')
		var m message
		if len(line) > 0 && json.Unmarshal(line, &m) == nil {
			if m.Method != "" {
				s.handle(m)
			}
		}
		if err != nil {
			cancel()
			s.wg.Wait()
			return
		}
	}
}

func main() { run(os.Stdin, os.Stdout) }
