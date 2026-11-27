// wai-escritorio: a wai extension (line-delimited JSON-RPC 2.0 on stdio) that shows a shared office
// map where teammates walk as avatars. It talks to wai on stdio and to the office server over a
// WebSocket.
package main

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"strconv"
	"strings"
	"time"

	"wai-escritorio/office"
)

const (
	presencePanel = "presence"
	officePanel   = "office"
	// moveEvery is the walking pace: one tile per 120 ms, however fast the key repeats.
	moveEvery = 120 * time.Millisecond
)

type message struct {
	ID     json.RawMessage `json:"id,omitempty"`
	Method string          `json:"method,omitempty"`
	Params json.RawMessage `json:"params,omitempty"`
	Result json.RawMessage `json:"result,omitempty"`
}

type ext struct {
	out io.Writer
	now func() time.Time
	dir string // data_dir; setup.json lives here

	ready bool
	v     view
	gmap  office.Map // the server's map, for collision
	// session id to wai state, only to derive the status; never sent.
	states     map[string]string
	sentStatus office.Status
	lastMove   time.Time

	listID, reqID int
	sent          map[string]string // the last tree sent per panel

	fromSrv chan serverMsg
	toSrv   chan any
	cancel  context.CancelFunc
	done    chan struct{}
}

func newExt(out io.Writer, now func() time.Time) *ext {
	return &ext{
		out: out, now: now,
		states: map[string]string{}, sent: map[string]string{},
		fromSrv: make(chan serverMsg), toSrv: make(chan any, 64),
	}
}

func (s *ext) write(v any) {
	b, _ := json.Marshal(v)
	fmt.Fprintf(s.out, "%s\n", b)
}

func (s *ext) request(method string, params any) {
	s.reqID++
	s.write(node{"jsonrpc": "2.0", "id": "r" + strconv.Itoa(s.reqID), "method": method, "params": params})
}

func key(agent, session *string) string {
	if agent == nil || session == nil {
		return ""
	}
	return *agent + "\x00" + *session
}

// connect dials the saved setup, dropping any earlier connection first.
func (s *ext) connect() {
	s.stop()
	ctx, cancel := context.WithCancel(context.Background())
	s.cancel, s.done = cancel, make(chan struct{})
	done, setup := s.done, *s.v.Setup
	go func() { defer close(done); run(ctx, setup, s.fromSrv, s.toSrv) }()
}

func (s *ext) stop() {
	if s.cancel != nil {
		s.cancel()
		<-s.done
		s.cancel = nil
	}
	s.v.Online, s.v.People = false, nil
}

func (s *ext) trySend(msg any) {
	select {
	case s.toSrv <- msg:
	default:
	}
}

// prefill is the form for the saved setup: name, server and avatar, never the token.
func prefill(st *Setup) Setup {
	if st == nil {
		return Setup{}
	}
	return Setup{Name: st.Name, Server: st.Server, Avatar: st.Avatar}
}

func (s *ext) render() {
	if !s.ready {
		return
	}
	for panel, tree := range map[string]node{presencePanel: presenceTree(s.v), officePanel: officeTree(s.v)} {
		b, _ := json.Marshal(tree)
		if s.sent[panel] == string(b) {
			continue
		}
		s.sent[panel] = string(b)
		s.write(node{"jsonrpc": "2.0", "method": "panel/render", "params": node{"panel": panel, "tree": tree}})
	}
}

// syncStatus tells the server the derived status when it changed, and nothing else (OFC-53).
func (s *ext) syncStatus() {
	if !s.v.Online {
		return
	}
	st := office.Derive(s.states)
	if st == s.sentStatus {
		return
	}
	s.sentStatus = st
	s.trySend(office.SetStatus{T: office.TStatus, Status: st})
	if p, ok := s.v.People[s.v.You]; ok {
		p.Status = st
		s.v.People[s.v.You] = p
	}
}

func (s *ext) onServer(m serverMsg) {
	v := &s.v
	switch m.T {
	case office.TWelcome:
		v.Online, v.Reason, v.FormErr = true, "", ""
		v.Rows, v.You = m.Map, m.You
		v.People = map[string]office.Person{}
		for _, p := range m.People {
			v.People[p.ID] = p
		}
		s.gmap, _ = office.ParseMap(strings.Join(m.Map, "\n"))
		s.sentStatus = office.StatusNone
		s.syncStatus()
	case office.TJoined:
		if v.People != nil {
			v.People[m.Person.ID] = m.Person
		}
	case office.TMoved:
		if p, ok := v.People[m.ID]; ok {
			p.X, p.Y = m.X, m.Y
			v.People[m.ID] = p
		}
	case office.TLeft:
		delete(v.People, m.ID)
	case office.TStatus:
		if p, ok := v.People[m.ID]; ok {
			p.Status = m.Status
			v.People[m.ID] = p
		}
	case office.TPosition:
		if p, ok := v.People[v.You]; ok {
			p.X, p.Y = m.X, m.Y
			v.People[v.You] = p
		}
	case office.TRefused:
		v.Online, v.People = false, nil
		switch m.Reason {
		case office.ReasonToken:
			v.Editing, v.Form, v.FormErr = true, prefill(v.Setup), reasonToken
		case office.ReasonReplaced:
			v.Reason = reasonElsewhere
		case office.ReasonFull:
			v.Reason = reasonFull
		}
	case tUnreachable:
		v.Online, v.People = false, nil
		if v.Setup != nil {
			v.Reason = reasonUnreachable(v.Setup.Server)
		}
	}
	s.render()
}

func (s *ext) onKey(k string) {
	me, ok := s.v.People[s.v.You]
	if !s.v.Online || !ok {
		return
	}
	to, ok := office.Step(office.Point{X: me.X, Y: me.Y}, k)
	if !ok || s.gmap.Blocked(to) {
		return
	}
	if now := s.now(); now.Sub(s.lastMove) < moveEvery {
		return
	} else {
		s.lastMove = now
	}
	me.X, me.Y = to.X, to.Y
	s.v.People[s.v.You] = me
	s.trySend(office.Move{T: office.TMove, X: to.X, Y: to.Y})
	s.render()
}

func str(v any) string { s, _ := v.(string); return s }

func (s *ext) onAction(id string, values map[string]any) {
	switch id {
	case "enter":
		s.request("panel/open", node{"panel": officePanel})
	case "change":
		s.stop()
		s.v.Editing, s.v.FormErr, s.v.Reason = true, "", ""
		s.v.Form = prefill(s.v.Setup)
	case "save":
		form := Setup{Name: strings.TrimSpace(str(values["name"])), Server: strings.TrimSpace(str(values["server"])), Token: strings.TrimSpace(str(values["token"]))}
		for i, n := range avatarNames {
			if n == str(values["avatar"]) {
				form.Avatar = i
			}
		}
		if form.Token == "" && s.v.Setup != nil {
			form.Token = s.v.Setup.Token
		}
		if msg := validate(form); msg != "" {
			s.v.Editing, s.v.Form, s.v.FormErr = true, prefill(&form), msg
			return
		}
		if err := saveSetup(s.dir, form); err != nil {
			fmt.Fprintln(os.Stderr, "wai-escritorio: cannot save the setup:", err)
			s.v.Editing, s.v.Form, s.v.FormErr = true, prefill(&form), "Could not save the setup"
			return
		}
		s.v.Setup, s.v.Editing, s.v.FormErr, s.v.Reason = &form, false, "", ""
		s.v.Form = prefill(&form)
		s.connect()
	}
}

func (s *ext) handle(m message) {
	switch m.Method {
	case "initialize":
		var p struct {
			DataDir string `json:"data_dir"`
		}
		json.Unmarshal(m.Params, &p)
		s.write(node{"jsonrpc": "2.0", "id": m.ID, "result": node{}})
		s.dir = p.DataDir
		if s.dir == "" {
			s.dir = os.Getenv("WAI_EXT_DATA")
		}
		if s.dir == "" {
			s.dir, _ = os.Getwd()
		}
		st, err := loadSetup(s.dir)
		if err != nil {
			fmt.Fprintln(os.Stderr, "wai-escritorio: cannot read the setup:", err)
		}
		s.v.Setup, s.v.Form = st, prefill(st)
		s.ready = true
		s.listID++
		s.write(node{"jsonrpc": "2.0", "id": s.listID, "method": "sessions/list"})
		if st != nil {
			s.connect()
		}
		s.render()
	case "panel/action":
		var p struct {
			Action string         `json:"action_id"`
			Values map[string]any `json:"values"`
		}
		json.Unmarshal(m.Params, &p)
		if m.ID != nil {
			s.write(node{"jsonrpc": "2.0", "id": m.ID, "result": nil})
		}
		s.onAction(p.Action, p.Values)
		s.render()
	case "panel/key":
		var p struct{ Panel, Scene, Key string }
		json.Unmarshal(m.Params, &p)
		if p.Panel == officePanel {
			s.onKey(p.Key)
		}
	case "event/session_state":
		var p struct{ Agent, Session, State *string }
		json.Unmarshal(m.Params, &p)
		if k := key(p.Agent, p.Session); k != "" && p.State != nil {
			s.states[k] = *p.State
			s.syncStatus()
			s.render()
		}
	case "":
		// The answer to our sessions/list: the open sessions' states, closed ones dropped.
		if m.ID == nil || string(m.ID) != strconv.Itoa(s.listID) || m.Result == nil {
			return
		}
		var list []struct{ Agent, Session, State *string }
		if json.Unmarshal(m.Result, &list) == nil {
			s.states = map[string]string{}
			for _, l := range list {
				if k := key(l.Agent, l.Session); k != "" && l.State != nil {
					s.states[k] = *l.State
				}
			}
			s.syncStatus()
			s.render()
		}
	default:
		if m.ID != nil {
			s.write(node{"jsonrpc": "2.0", "id": m.ID, "error": node{"code": -32601, "message": "Method not found"}})
		}
	}
}

// serve handles wai's messages and the server's on one goroutine, until stdin closes.
func serve(s *ext, in io.Reader) {
	msgs := make(chan message)
	go func() {
		defer close(msgs)
		r := bufio.NewReaderSize(in, 1<<20)
		for {
			line, err := r.ReadBytes('\n')
			var m message
			if len(line) > 0 && json.Unmarshal(line, &m) == nil {
				msgs <- m
			}
			if err != nil {
				return
			}
		}
	}()
	defer s.stop()
	for {
		select {
		case m, ok := <-msgs:
			if !ok {
				return
			}
			s.handle(m)
		case m := <-s.fromSrv:
			s.onServer(m)
		}
	}
}

func main() {
	serve(newExt(os.Stdout, time.Now), os.Stdin)
}
