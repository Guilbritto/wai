package main

import (
	"crypto/rand"
	"crypto/subtle"
	"encoding/hex"
	"errors"
	"log"
	"sort"

	"wai-escritorio/office"
)

const (
	maxPeople   = 50
	avatarCount = 8
	outBuffer   = 256
)

// Identity is who a connection is. It never holds the token.
type Identity struct {
	Name   string
	Avatar int
}

var (
	errToken = errors.New("wrong token")
	errHello = errors.New("bad hello")
)

// authenticate is the only place that decides who a connection is (OFC-27).
// Swapping the team token for another login means changing this function.
func authenticate(h office.Hello, token string) (Identity, error) {
	if h.T != office.THello {
		return Identity{}, errHello
	}
	if subtle.ConstantTimeCompare([]byte(h.Token), []byte(token)) != 1 {
		return Identity{}, errToken
	}
	if !office.ValidName(h.Name) || h.Avatar < 0 || h.Avatar >= avatarCount {
		return Identity{}, errHello
	}
	return Identity{Name: h.Name, Avatar: h.Avatar}, nil
}

// session is one connection as the hub sees it. Only the hub sends on out and
// closes it; the connection's writer goroutine ranges over it.
type session struct {
	addr   string
	out    chan any
	closed bool // owned by the hub goroutine
}

func newSession(addr string) *session {
	return &session{addr: addr, out: make(chan any, outBuffer)}
}

type person struct {
	office.Person
	s *session
}

// hub owns all state: one goroutine, no locks.
type hub struct {
	token  string
	m      office.Map
	logf   func(format string, args ...any)
	events chan func()
	quit   chan struct{}

	people map[string]*person      // by id
	last   map[string]office.Point // by name, for this server run
	ids    map[string]string       // by name, assigned by the server
}

func newHub(token string, m office.Map) *hub {
	h := &hub{
		token:  token,
		m:      m,
		logf:   log.Printf,
		events: make(chan func()),
		quit:   make(chan struct{}),
		people: map[string]*person{},
		last:   map[string]office.Point{},
		ids:    map[string]string{},
	}
	go h.run()
	return h
}

func (h *hub) run() {
	for {
		select {
		case f := <-h.events:
			f()
		case <-h.quit:
			return
		}
	}
}

func (h *hub) stop() { close(h.quit) }

func (h *hub) submit(f func()) {
	select {
	case h.events <- f:
	case <-h.quit:
	}
}

// Calls from connection goroutines.

func (h *hub) join(s *session, hello office.Hello) { h.submit(func() { h.doJoin(s, hello) }) }
func (h *hub) move(s *session, m office.Move)      { h.submit(func() { h.doMove(s, m) }) }
func (h *hub) status(s *session, st office.SetStatus) {
	h.submit(func() { h.doStatus(s, st) })
}
func (h *hub) leave(s *session) { h.submit(func() { h.doLeave(s) }) }

// Everything below runs on the hub goroutine.

func (h *hub) doJoin(s *session, hello office.Hello) {
	id, err := authenticate(hello, h.token)
	if err != nil {
		reason := "bad hello"
		if err == errToken {
			reason = office.ReasonToken
			h.send(s, office.Refused{T: office.TRefused, Reason: office.ReasonToken})
		}
		h.logf("refused addr=%s name=%q reason=%s", s.addr, hello.Name, reason)
		h.closeSession(s)
		return
	}
	for _, old := range h.people {
		if old.Name == id.Name {
			h.send(old.s, office.Refused{T: office.TRefused, Reason: office.ReasonReplaced})
			h.remove(old)
			h.logf("disconnect addr=%s name=%q reason=%s", old.s.addr, old.Name, office.ReasonReplaced)
			h.closeSession(old.s)
		}
	}
	if len(h.people) >= maxPeople {
		h.send(s, office.Refused{T: office.TRefused, Reason: office.ReasonFull})
		h.logf("refused addr=%s name=%q reason=%s", s.addr, id.Name, office.ReasonFull)
		h.closeSession(s)
		return
	}
	pos, ok := h.last[id.Name]
	if !ok || h.m.Blocked(pos) {
		pos = h.m.Spawn
	}
	pid, ok := h.ids[id.Name]
	if !ok {
		pid = newID()
		h.ids[id.Name] = pid
	}
	p := &person{Person: office.Person{ID: pid, Name: id.Name, Avatar: id.Avatar, X: pos.X, Y: pos.Y}, s: s}
	h.broadcast(p, office.Joined{T: office.TJoined, Person: p.Person})
	h.people[pid] = p
	h.last[id.Name] = pos
	h.send(s, office.Welcome{T: office.TWelcome, You: pid, Map: h.m.Rows, People: h.list()})
	h.logf("connect addr=%s name=%q", s.addr, id.Name)
}

func (h *hub) doMove(s *session, m office.Move) {
	p := h.of(s)
	if p == nil {
		return
	}
	to := office.Point{X: m.X, Y: m.Y}
	from := office.Point{X: p.X, Y: p.Y}
	if !office.Adjacent(from, to) || h.m.Blocked(to) {
		h.send(s, office.Position{T: office.TPosition, X: from.X, Y: from.Y})
		return
	}
	p.X, p.Y = to.X, to.Y
	h.last[p.Name] = to
	h.broadcast(p, office.Moved{T: office.TMoved, ID: p.ID, X: to.X, Y: to.Y})
}

func (h *hub) doStatus(s *session, st office.SetStatus) {
	p := h.of(s)
	if p == nil {
		return
	}
	switch st.Status {
	case office.StatusNone, office.StatusFocused, office.StatusNeedsYou:
	default:
		return
	}
	p.Status = st.Status
	h.broadcast(p, office.StatusChanged{T: office.TStatus, ID: p.ID, Status: st.Status})
}

func (h *hub) doLeave(s *session) {
	if p := h.of(s); p != nil {
		h.remove(p)
		h.logf("disconnect addr=%s name=%q", s.addr, p.Name)
	}
	h.closeSession(s)
}

// remove takes p out of the office and tells everyone else.
func (h *hub) remove(p *person) {
	delete(h.people, p.ID)
	h.last[p.Name] = office.Point{X: p.X, Y: p.Y}
	h.broadcast(p, office.Left{T: office.TLeft, ID: p.ID})
}

func (h *hub) of(s *session) *person {
	for _, p := range h.people {
		if p.s == s {
			return p
		}
	}
	return nil
}

// list is every person sorted by name, so welcomes are deterministic.
func (h *hub) list() []office.Person {
	out := make([]office.Person, 0, len(h.people))
	for _, p := range h.people {
		out = append(out, p.Person)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Name < out[j].Name })
	return out
}

// broadcast sends msg to everyone except from.
func (h *hub) broadcast(from *person, msg any) {
	for _, p := range h.people {
		if p != from {
			h.send(p.s, msg)
		}
	}
}

// send never blocks the hub: a client that cannot keep up is dropped.
func (h *hub) send(s *session, msg any) {
	if s.closed {
		return
	}
	select {
	case s.out <- msg:
	default:
		if p := h.of(s); p != nil {
			h.remove(p)
			h.logf("disconnect addr=%s name=%q reason=slow", s.addr, p.Name)
		}
		h.closeSession(s)
	}
}

func (h *hub) closeSession(s *session) {
	if !s.closed {
		s.closed = true
		close(s.out)
	}
}

func newID() string {
	b := make([]byte, 8)
	rand.Read(b)
	return hex.EncodeToString(b)
}
