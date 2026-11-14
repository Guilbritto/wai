package main

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"log"
	"os"
	"sort"
	"strings"
	"sync"
	"time"

	"wai-waycarbon/helpapi"
)

var (
	ErrTooLong       = errors.New("text too long")
	ErrRateLimited   = errors.New("too many requests in the last hour")
	ErrRecipientFull = errors.New("recipient has too many waiting requests")
	ErrSelf          = errors.New("cannot send a request to yourself")
	ErrNotFound      = errors.New("request not found")
	ErrForbidden     = errors.New("not allowed for this side of the request")
	ErrBadState      = errors.New("request is not in a state that allows this")
)

type User struct {
	ID       int64
	Login    string
	LastSeen time.Time
}

type token struct {
	UserID  int64     `json:"user_id"`
	Expires time.Time `json:"expires"`
}

type data struct {
	Users    map[int64]User    `json:"users"`
	Tokens   map[string]token  `json:"tokens"`
	Requests []helpapi.Request `json:"requests"`
}

type Store struct {
	mu   sync.Mutex
	path string
	now  func() time.Time
	d    data
}

func OpenStore(path string, now func() time.Time) (*Store, error) {
	s := &Store{path: path, now: now, d: data{Users: map[int64]User{}, Tokens: map[string]token{}}}
	b, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return s, nil
	}
	if err != nil {
		return nil, err
	}
	if err := json.Unmarshal(b, &s.d); err != nil {
		return nil, err
	}
	if s.d.Users == nil {
		s.d.Users = map[int64]User{}
	}
	if s.d.Tokens == nil {
		s.d.Tokens = map[string]token{}
	}
	return s, nil
}

// save must run under s.mu.
// ponytail: one JSON file rewritten per change; past a few thousand open requests, move to SQLite.
func (s *Store) save() error {
	b, err := json.Marshal(s.d)
	if err != nil {
		return err
	}
	tmp := s.path + ".tmp"
	if err := os.WriteFile(tmp, b, 0o600); err != nil {
		return err
	}
	if err := os.Chmod(tmp, 0o600); err != nil {
		return err
	}
	return os.Rename(tmp, s.path)
}

func (s *Store) saveLogged() {
	if err := s.save(); err != nil {
		log.Printf("store: save: %v", err)
	}
}

func (s *Store) PutUser(id int64, login string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.d.Users[id] = User{ID: id, Login: login, LastSeen: s.now()}
	s.saveLogged()
}

func (s *Store) UserByLogin(login string) (User, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, u := range s.d.Users {
		if strings.EqualFold(u.Login, login) {
			return u, true
		}
	}
	return User{}, false
}

func (s *Store) Users() []User {
	s.mu.Lock()
	defer s.mu.Unlock()
	out := make([]User, 0, len(s.d.Users))
	for _, u := range s.d.Users {
		out = append(out, u)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].ID < out[j].ID })
	return out
}

func (s *Store) AddToken(hash [32]byte, userID int64) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.d.Tokens[hex.EncodeToString(hash[:])] = token{userID, s.now().Add(helpapi.TokenLife)}
	s.saveLogged()
}

func (s *Store) TokenUser(hash [32]byte) (User, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	t, ok := s.d.Tokens[hex.EncodeToString(hash[:])]
	if !ok || !s.now().Before(t.Expires) {
		return User{}, false
	}
	u, ok := s.d.Users[t.UserID]
	return u, ok
}

func (s *Store) DropToken(hash [32]byte) {
	s.mu.Lock()
	defer s.mu.Unlock()
	delete(s.d.Tokens, hex.EncodeToString(hash[:]))
	s.saveLogged()
}

func (s *Store) Create(from, to User, p helpapi.Project, text string) (helpapi.Request, error) {
	if len(text) > helpapi.MaxText {
		return helpapi.Request{}, ErrTooLong
	}
	if from.ID == to.ID {
		return helpapi.Request{}, ErrSelf
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	now := s.now()
	sent, waiting := 0, 0
	for _, r := range s.d.Requests {
		if r.FromID == from.ID && now.Sub(r.Created) < time.Hour {
			sent++
		}
		if r.ToID == to.ID && r.State == helpapi.Waiting {
			waiting++
		}
	}
	if sent >= helpapi.PerHour {
		return helpapi.Request{}, ErrRateLimited
	}
	if waiting >= helpapi.WaitingPerRecipient {
		return helpapi.Request{}, ErrRecipientFull
	}
	var id [16]byte
	if _, err := rand.Read(id[:]); err != nil {
		return helpapi.Request{}, err
	}
	r := helpapi.Request{
		ID: hex.EncodeToString(id[:]), From: from.Login, To: to.Login, FromID: from.ID, ToID: to.ID,
		Project: p, Text: text, State: helpapi.Waiting, Created: now, Updated: now,
	}
	s.d.Requests = append(s.d.Requests, r)
	if err := s.save(); err != nil {
		s.d.Requests = s.d.Requests[:len(s.d.Requests)-1]
		return helpapi.Request{}, err
	}
	return r, nil
}

// Act on a waiting request past AcceptWithin fails with ErrBadState; Sweep marks it expired.
func (s *Store) Act(userID int64, id, action, text string) (helpapi.Request, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	var r *helpapi.Request
	for i := range s.d.Requests {
		if s.d.Requests[i].ID == id && (s.d.Requests[i].FromID == userID || s.d.Requests[i].ToID == userID) {
			r = &s.d.Requests[i]
			break
		}
	}
	if r == nil {
		return helpapi.Request{}, ErrNotFound
	}
	sender := r.FromID == userID
	var next helpapi.State
	switch action {
	case "cancel":
		if !sender {
			return helpapi.Request{}, ErrForbidden
		}
		next = helpapi.Cancelled
	case "accept", "decline", "reply":
		if sender {
			return helpapi.Request{}, ErrForbidden
		}
		next = map[string]helpapi.State{"accept": helpapi.Accepted, "decline": helpapi.Declined, "reply": helpapi.Replied}[action]
		if len(text) > helpapi.MaxText {
			return helpapi.Request{}, ErrTooLong
		}
	default:
		return helpapi.Request{}, ErrBadState
	}
	now := s.now()
	ok := r.State == helpapi.Waiting && now.Sub(r.Created) <= helpapi.AcceptWithin
	switch action {
	case "cancel":
		ok = ok || r.State == helpapi.Accepted
	case "reply":
		ok = r.State == helpapi.Accepted
	}
	if !ok {
		return helpapi.Request{}, ErrBadState
	}
	old := *r
	r.State, r.Updated = next, now
	switch action {
	case "reply":
		r.Reply = text
	case "decline":
		r.Reason = text
	}
	if err := s.save(); err != nil {
		*r = old
		return helpapi.Request{}, err
	}
	return *r, nil
}

func (s *Store) Open(userID int64) []helpapi.Request {
	s.mu.Lock()
	defer s.mu.Unlock()
	now := s.now()
	var out []helpapi.Request
	for _, r := range s.d.Requests {
		if (r.FromID == userID || r.ToID == userID) && !pastRetention(r, now) {
			out = append(out, r)
		}
	}
	sort.SliceStable(out, func(i, j int) bool { return out[i].Created.Before(out[j].Created) })
	return out
}

func ended(st helpapi.State) bool {
	return st != helpapi.Waiting && st != helpapi.Accepted
}

func pastRetention(r helpapi.Request, now time.Time) bool {
	return ended(r.State) && now.Sub(r.Updated) >= helpapi.Retention
}

func (s *Store) Sweep() (expired []helpapi.Request) {
	s.mu.Lock()
	defer s.mu.Unlock()
	now := s.now()
	kept := s.d.Requests[:0]
	for _, r := range s.d.Requests {
		if r.State == helpapi.Waiting && now.Sub(r.Created) > helpapi.AcceptWithin {
			r.State, r.Updated = helpapi.Expired, now
			expired = append(expired, r)
		}
		if !pastRetention(r, now) {
			kept = append(kept, r)
		}
	}
	changed := len(expired) > 0 || len(kept) != len(s.d.Requests)
	s.d.Requests = kept
	if changed {
		s.saveLogged()
	}
	return expired
}
