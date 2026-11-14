package main

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"wai-waycarbon/helpapi"
)

type clock struct{ t time.Time }

func (c *clock) now() time.Time      { return c.t }
func (c *clock) add(d time.Duration) { c.t = c.t.Add(d) }
func newStore(t *testing.T) (*Store, *clock, string) {
	t.Helper()
	c := &clock{time.Date(2026, 11, 14, 12, 0, 0, 0, time.UTC)}
	path := filepath.Join(t.TempDir(), "relay.json")
	s, err := OpenStore(path, c.now)
	if err != nil {
		t.Fatal(err)
	}
	return s, c, path
}

var (
	alice = User{ID: 1, Login: "alice"}
	bob   = User{ID: 2, Login: "bob"}
	carol = User{ID: 3, Login: "carol"}
)

func TestRequestLifecycle(t *testing.T) {
	s, c, _ := newStore(t)
	r, err := s.Create(alice, bob, helpapi.Project{Repo: "r", Branch: "b"}, "run it")
	if err != nil || r.State != helpapi.Waiting || r.From != "alice" || r.ToID != 2 {
		t.Fatalf("create: %+v %v", r, err)
	}
	c.add(time.Minute)
	a, err := s.Act(2, r.ID, "accept", "")
	if err != nil || a.State != helpapi.Accepted || !a.Updated.After(r.Updated) {
		t.Fatalf("accept: %+v %v", a, err)
	}
	c.add(time.Minute)
	p, err := s.Act(2, r.ID, "reply", "done")
	if err != nil || p.State != helpapi.Replied || p.Reply != "done" || !p.Updated.After(a.Updated) {
		t.Fatalf("reply: %+v %v", p, err)
	}
	d, _ := s.Create(alice, bob, helpapi.Project{}, "x")
	if d, err = s.Act(2, d.ID, "decline", "no"); err != nil || d.State != helpapi.Declined || d.Reason != "no" {
		t.Fatalf("decline: %+v %v", d, err)
	}
	k, _ := s.Create(alice, bob, helpapi.Project{}, "y")
	if k, err = s.Act(1, k.ID, "cancel", ""); err != nil || k.State != helpapi.Cancelled {
		t.Fatalf("cancel: %+v %v", k, err)
	}
}

func TestOnlyTheRightSideActs(t *testing.T) {
	s, _, _ := newStore(t)
	r, _ := s.Create(alice, bob, helpapi.Project{}, "x")
	check := func(uid int64, action string, want error) {
		t.Helper()
		if _, err := s.Act(uid, r.ID, action, "t"); !errors.Is(err, want) {
			t.Fatalf("user %d %s: got %v want %v", uid, action, err, want)
		}
	}
	check(2, "cancel", ErrForbidden)
	check(1, "accept", ErrForbidden)
	check(1, "decline", ErrForbidden)
	check(1, "reply", ErrForbidden)
	check(3, "accept", ErrNotFound)
	check(3, "cancel", ErrNotFound)
	check(2, "reply", ErrBadState)
	if _, err := s.Act(2, "nope", "accept", ""); !errors.Is(err, ErrNotFound) {
		t.Fatalf("unknown id: %v", err)
	}
}

func TestLimits(t *testing.T) {
	s, c, _ := newStore(t)
	if _, err := s.Create(alice, bob, helpapi.Project{}, strings.Repeat("a", 8001)); !errors.Is(err, ErrTooLong) {
		t.Fatalf("too long: %v", err)
	}
	if _, err := s.Create(alice, bob, helpapi.Project{}, strings.Repeat("a", 8000)); err != nil {
		t.Fatalf("8000 chars: %v", err)
	}
	if _, err := s.Create(alice, alice, helpapi.Project{}, "x"); !errors.Is(err, ErrSelf) {
		t.Fatalf("self: %v", err)
	}
	r, _ := s.Create(alice, bob, helpapi.Project{}, "x")
	if _, err := s.Act(2, r.ID, "decline", ""); err != nil {
		t.Fatal(err)
	}
	// alice has made 2 so far; 18 more reach 20, spread over recipients to avoid ErrRecipientFull
	for i := 0; i < 18; i++ {
		to := User{ID: int64(100 + i), Login: "u"}
		if _, err := s.Create(alice, to, helpapi.Project{}, "x"); err != nil {
			t.Fatalf("create %d: %v", i, err)
		}
	}
	if _, err := s.Create(alice, carol, helpapi.Project{}, "x"); !errors.Is(err, ErrRateLimited) {
		t.Fatalf("21st: %v", err)
	}
	c.add(61 * time.Minute)
	if _, err := s.Create(alice, carol, helpapi.Project{}, "x"); err != nil {
		t.Fatalf("after an hour: %v", err)
	}

	s2, _, _ := newStore(t)
	for i := 0; i < 10; i++ {
		from := User{ID: int64(200 + i), Login: "f"}
		if _, err := s2.Create(from, bob, helpapi.Project{}, "x"); err != nil {
			t.Fatalf("waiting %d: %v", i, err)
		}
	}
	if _, err := s2.Create(carol, bob, helpapi.Project{}, "x"); !errors.Is(err, ErrRecipientFull) {
		t.Fatalf("11th waiting: %v", err)
	}
}

func TestExpiryAndRetention(t *testing.T) {
	s, c, _ := newStore(t)
	r, _ := s.Create(alice, bob, helpapi.Project{}, "x")
	c.add(helpapi.AcceptWithin - time.Minute)
	if got := s.Sweep(); len(got) != 0 {
		t.Fatalf("early sweep: %v", got)
	}
	c.add(2 * time.Minute)
	if _, err := s.Act(2, r.ID, "accept", ""); !errors.Is(err, ErrBadState) {
		t.Fatalf("accept after 24h: %v", err)
	}
	got := s.Sweep()
	if len(got) != 1 || got[0].ID != r.ID || got[0].State != helpapi.Expired {
		t.Fatalf("sweep: %+v", got)
	}
	if len(s.Open(1)) != 1 {
		t.Fatal("expired request should stay until retention")
	}
	c.add(helpapi.Retention)
	s.Sweep()
	if len(s.Open(1)) != 0 || len(s.Open(2)) != 0 {
		t.Fatal("request should be gone after retention")
	}
}

func TestOpenOrderAndScope(t *testing.T) {
	s, c, _ := newStore(t)
	a, _ := s.Create(alice, bob, helpapi.Project{}, "1")
	c.add(time.Minute)
	b, _ := s.Create(bob, alice, helpapi.Project{}, "2")
	s.Create(bob, carol, helpapi.Project{}, "3")
	got := s.Open(1)
	if len(got) != 2 || got[0].ID != a.ID || got[1].ID != b.ID {
		t.Fatalf("open: %+v", got)
	}
}

func TestTokens(t *testing.T) {
	s, c, _ := newStore(t)
	if err := s.PutUser(1, "Alice"); err != nil {
		t.Fatal(err)
	}
	h := [32]byte{1}
	s.AddToken(h, 1)
	if u, ok := s.TokenUser(h); !ok || u.Login != "Alice" {
		t.Fatalf("token: %+v %v", u, ok)
	}
	if u, ok := s.UserByLogin("aLiCe"); !ok || u.ID != 1 {
		t.Fatal("case-insensitive login")
	}
	c.add(helpapi.TokenLife + time.Second)
	if _, ok := s.TokenUser(h); ok {
		t.Fatal("expired token works")
	}
	h2 := [32]byte{2}
	s.AddToken(h2, 1)
	s.DropToken(h2)
	if _, ok := s.TokenUser(h2); ok {
		t.Fatal("dropped token works")
	}
}

func TestReloadKeepsWaitingRequests(t *testing.T) {
	s, _, path := newStore(t)
	if err := s.PutUser(1, "alice"); err != nil {
		t.Fatal(err)
	}
	s.AddToken([32]byte{9}, 1)
	r1, _ := s.Create(alice, bob, helpapi.Project{}, "one")
	r2, _ := s.Create(alice, carol, helpapi.Project{}, "two")
	s.Act(3, r2.ID, "accept", "")
	c := &clock{time.Date(2026, 11, 14, 12, 0, 0, 0, time.UTC)}
	s2, err := OpenStore(path, c.now)
	if err != nil {
		t.Fatal(err)
	}
	got := s2.Open(1)
	if len(got) != 2 || got[0].ID != r1.ID || got[0].State != helpapi.Waiting || got[1].State != helpapi.Accepted {
		t.Fatalf("reload: %+v", got)
	}
	if _, ok := s2.TokenUser([32]byte{9}); !ok {
		t.Fatal("token lost on reload")
	}
	fi, err := os.Stat(path)
	if err != nil || fi.Mode().Perm() != 0o600 {
		t.Fatalf("mode: %v %v", fi, err)
	}
}

func TestSweepDropsExpiredTokens(t *testing.T) {
	s, c, path := newStore(t)
	if err := s.PutUser(1, "alice"); err != nil {
		t.Fatal(err)
	}
	old, fresh := [32]byte{1}, [32]byte{2}
	s.AddToken(old, 1)
	c.add(helpapi.TokenLife - time.Hour)
	s.AddToken(fresh, 1)
	c.add(2 * time.Hour)
	s.Sweep()
	if len(s.d.Tokens) != 1 {
		t.Fatalf("tokens after sweep: %d", len(s.d.Tokens))
	}
	s2, _ := OpenStore(path, c.now)
	if _, ok := s2.TokenUser(fresh); !ok || len(s2.d.Tokens) != 1 {
		t.Fatal("sweep not persisted or fresh token lost")
	}
}

func TestCancelAccepted(t *testing.T) {
	s, _, _ := newStore(t)
	r, _ := s.Create(alice, bob, helpapi.Project{}, "x")
	s.Act(2, r.ID, "accept", "")
	got, err := s.Act(1, r.ID, "cancel", "")
	if err != nil || got.State != helpapi.Cancelled {
		t.Fatalf("cancel accepted: %+v %v", got, err)
	}
	if _, err := s.Act(1, r.ID, "cancel", ""); !errors.Is(err, ErrBadState) {
		t.Fatalf("cancel twice: %v", err)
	}
}

func TestDropUserTokens(t *testing.T) {
	s, _, _ := newStore(t)
	s.PutUser(1, "alice")
	s.PutUser(2, "bob")
	s.AddToken([32]byte{1}, 1)
	s.AddToken([32]byte{2}, 1)
	s.AddToken([32]byte{3}, 2)
	if err := s.DropUserTokens(1); err != nil {
		t.Fatal(err)
	}
	if _, ok := s.TokenUser([32]byte{1}); ok {
		t.Fatal("alice token survives")
	}
	if _, ok := s.TokenUser([32]byte{2}); ok {
		t.Fatal("alice token survives")
	}
	if _, ok := s.TokenUser([32]byte{3}); !ok {
		t.Fatal("bob token dropped")
	}
}
