package main

import (
	"fmt"
	"strings"
	"testing"
	"time"

	"wai-escritorio/office"
)

const testToken = "0123456789abcdef0123456789abcdef"

// A 7x5 room: walls around, a desk at 3,2, spawn at 1,1.
const roomMap = "#######\n#S....#\n#..D..#\n#.....#\n#######\n"

func testHub(t *testing.T) *hub {
	t.Helper()
	m, err := office.ParseMap(roomMap)
	if err != nil {
		t.Fatal(err)
	}
	h := newHub(testToken, m)
	h.logf = func(string, ...any) {}
	t.Cleanup(h.stop)
	return h
}

func hello(name string) office.Hello {
	return office.Hello{T: office.THello, Token: testToken, Name: name}
}

// recv returns the next message to s, failing when none comes.
func recv(t *testing.T, s *session) any {
	t.Helper()
	select {
	case m, ok := <-s.out:
		if !ok {
			t.Fatal("session closed, wanted a message")
		}
		return m
	case <-time.After(time.Second):
		t.Fatal("no message")
	}
	return nil
}

func expectClosed(t *testing.T, s *session) {
	t.Helper()
	select {
	case m, ok := <-s.out:
		if ok {
			t.Fatalf("got %+v, wanted the session closed", m)
		}
	case <-time.After(time.Second):
		t.Fatal("session not closed")
	}
}

// expectQuiet fails if s has a message. Call it after the hub has handled
// everything sent before (see sync).
func expectQuiet(t *testing.T, s *session) {
	t.Helper()
	select {
	case m := <-s.out:
		t.Fatalf("unexpected message %+v", m)
	default:
	}
}

// sync returns once the hub has handled every event submitted before it.
func sync(h *hub) {
	done := make(chan struct{})
	h.submit(func() { close(done) })
	<-done
}

func joinAs(t *testing.T, h *hub, name string) (*session, office.Welcome) {
	t.Helper()
	s := newSession("10.0.0.1:1000")
	h.join(s, hello(name))
	w, ok := recv(t, s).(office.Welcome)
	if !ok {
		t.Fatalf("%s: first message is not a welcome", name)
	}
	return s, w
}

func TestAuthenticate(t *testing.T) {
	good := office.Hello{T: office.THello, Token: testToken, Name: "Ana", Avatar: 3}
	id, err := authenticate(good, testToken)
	if err != nil || id.Name != "Ana" || id.Avatar != 3 {
		t.Fatalf("good hello: %+v %v", id, err)
	}
	bad := map[string]office.Hello{
		"wrong token":  {T: office.THello, Token: "x" + testToken[1:], Name: "Ana"},
		"empty token":  {T: office.THello, Name: "Ana"},
		"token prefix": {T: office.THello, Token: testToken[:31], Name: "Ana"},
		"token suffix": {T: office.THello, Token: testToken + "x", Name: "Ana"},
		"empty name":   {T: office.THello, Token: testToken},
		"control name": {T: office.THello, Token: testToken, Name: "A\nna"},
		"avatar low":   {T: office.THello, Token: testToken, Name: "Ana", Avatar: -1},
		"avatar high":  {T: office.THello, Token: testToken, Name: "Ana", Avatar: 8},
		"not a hello":  {T: office.TMove, Token: testToken, Name: "Ana"},
	}
	for name, h := range bad {
		if _, err := authenticate(h, testToken); err == nil {
			t.Errorf("%s: accepted", name)
		}
	}
	if _, err := authenticate(good, "another token entirely, 32+ chars long"); err == nil {
		t.Error("hello accepted against another token")
	}

	// avatars are identified by a server-assigned id, never by the token
	h := testHub(t)
	_, wa := joinAs(t, h, "Ana")
	_, wb := joinAs(t, h, "Bob")
	if wa.You == "" || wa.You == wb.You || strings.Contains(wa.You, testToken) || wa.You == "Ana" {
		t.Fatalf("ids %q %q", wa.You, wb.You)
	}
}

func TestRejoinAtLastPosition(t *testing.T) {
	h := testHub(t)
	s, w := joinAs(t, h, "Ana")
	spawn := h.m.Spawn
	if me := w.People[0]; me.X != spawn.X || me.Y != spawn.Y {
		t.Fatalf("first join at %d,%d, want spawn %v", me.X, me.Y, spawn)
	}
	h.move(s, office.Move{T: office.TMove, X: 2, Y: 1})
	h.move(s, office.Move{T: office.TMove, X: 2, Y: 2})
	h.leave(s)
	sync(h)

	_, w2 := joinAs(t, h, "Ana")
	if me := w2.People[0]; me.X != 2 || me.Y != 2 {
		t.Fatalf("rejoin at %d,%d, want 2,2", me.X, me.Y)
	}
	if w2.You != w.You {
		t.Errorf("id changed on rejoin: %q then %q", w.You, w2.You)
	}
	_, w3 := joinAs(t, h, "Bob")
	for _, p := range w3.People {
		if p.Name == "Bob" && (p.X != spawn.X || p.Y != spawn.Y) {
			t.Fatalf("new person at %d,%d, want spawn", p.X, p.Y)
		}
	}
	if strings.Join(w3.Map, "\n")+"\n" != roomMap {
		t.Errorf("welcome map = %q", w3.Map)
	}
}

func TestIllegalMoveSnapsBack(t *testing.T) {
	h := testHub(t)
	a, _ := joinAs(t, h, "Ana")
	b, _ := joinAs(t, h, "Bob")
	recv(t, a) // joined Bob

	cases := map[string]office.Move{
		"wall":      {X: 1, Y: 0},
		"two tiles": {X: 3, Y: 1},
		"diagonal":  {X: 2, Y: 2},
		"same tile": {X: 1, Y: 1},
		"far":       {X: 5, Y: 3},
		"outside":   {X: -1, Y: 1},
	}
	for name, m := range cases {
		m.T = office.TMove
		h.move(a, m)
		got, ok := recv(t, a).(office.Position)
		if !ok || got.X != 1 || got.Y != 1 {
			t.Errorf("%s: got %+v, want position 1,1", name, got)
		}
	}
	// furniture: walk next to the desk at 3,2 and into it
	h.move(a, office.Move{T: office.TMove, X: 2, Y: 1})
	h.move(a, office.Move{T: office.TMove, X: 2, Y: 2})
	h.move(a, office.Move{T: office.TMove, X: 3, Y: 2})
	if got, ok := recv(t, a).(office.Position); !ok || got.X != 2 || got.Y != 2 {
		t.Errorf("desk: got %+v, want position 2,2", got)
	}
	// others see only the legal moves, never the refused ones
	for _, want := range []office.Point{{X: 2, Y: 1}, {X: 2, Y: 2}} {
		mv, ok := recv(t, b).(office.Moved)
		if !ok || mv.X != want.X || mv.Y != want.Y {
			t.Fatalf("other got %+v, want move to %v", mv, want)
		}
	}
	sync(h)
	expectQuiet(t, b)
	expectQuiet(t, a)
}

func TestSameNameReplacesOlder(t *testing.T) {
	h := testHub(t)
	old, wOld := joinAs(t, h, "Ana")
	obs, _ := joinAs(t, h, "Bob")
	recv(t, old) // joined Bob
	h.move(old, office.Move{T: office.TMove, X: 2, Y: 1})
	recv(t, obs) // moved

	fresh, w := joinAs(t, h, "Ana")
	if r, ok := recv(t, old).(office.Refused); !ok || r.Reason != office.ReasonReplaced {
		t.Fatalf("older got %+v, want refused replaced", r)
	}
	expectClosed(t, old)
	if len(w.People) != 2 {
		t.Fatalf("people = %+v, want Ana once and Bob", w.People)
	}
	for _, p := range w.People {
		if p.Name == "Ana" && (p.X != 2 || p.Y != 1) {
			t.Errorf("replacement at %d,%d, want the old position 2,1", p.X, p.Y)
		}
	}
	if l, ok := recv(t, obs).(office.Left); !ok || l.ID != wOld.You {
		t.Fatalf("observer got %+v, want left", l)
	}
	if j, ok := recv(t, obs).(office.Joined); !ok || j.Person.Name != "Ana" {
		t.Fatalf("observer got %+v, want joined Ana", j)
	}

	// the replaced session ending later must not remove the new one
	h.leave(old)
	sync(h)
	expectQuiet(t, obs)
	h.move(fresh, office.Move{T: office.TMove, X: 3, Y: 1})
	if mv, ok := recv(t, obs).(office.Moved); !ok || mv.X != 3 {
		t.Fatalf("new session cannot move: %+v", mv)
	}
}

func TestFiftyFirstRefused(t *testing.T) {
	h := testHub(t)
	var first *session
	for i := 0; i < 50; i++ {
		s, _ := joinAs(t, h, fmt.Sprintf("p%02d", i))
		if i == 0 {
			first = s
		}
	}
	// drain the joined notices of the first person
	for i := 0; i < 49; i++ {
		recv(t, first)
	}
	extra := newSession("10.0.0.9:9")
	h.join(extra, hello("late"))
	if r, ok := recv(t, extra).(office.Refused); !ok || r.Reason != office.ReasonFull {
		t.Fatalf("51st got %+v, want refused full", r)
	}
	expectClosed(t, extra)
	sync(h)
	expectQuiet(t, first)

	// a same-name login still replaces when the office is full
	again := newSession("10.0.0.9:10")
	h.join(again, hello("p07"))
	if _, ok := recv(t, again).(office.Welcome); !ok {
		t.Fatal("replacement refused in a full office")
	}
	// and a seat frees up when someone leaves
	h.leave(first)
	late := newSession("10.0.0.9:11")
	h.join(late, hello("late"))
	if _, ok := recv(t, late).(office.Welcome); !ok {
		t.Fatal("refused with a free seat")
	}
}

func TestStatusFansOut(t *testing.T) {
	h := testHub(t)
	a, wa := joinAs(t, h, "Ana")
	b, _ := joinAs(t, h, "Bob")
	recv(t, a)
	for _, st := range []office.Status{office.StatusFocused, office.StatusNeedsYou, office.StatusNone} {
		h.status(a, office.SetStatus{T: office.TStatus, Status: st})
		got, ok := recv(t, b).(office.StatusChanged)
		if !ok || got.ID != wa.You || got.Status != st {
			t.Fatalf("got %+v, want %q for %s", got, st, wa.You)
		}
	}
	h.status(a, office.SetStatus{T: office.TStatus, Status: "typing a secret"})
	sync(h)
	expectQuiet(t, b)

	// a later joiner sees the current status in the welcome
	h.status(a, office.SetStatus{T: office.TStatus, Status: office.StatusFocused})
	recv(t, b)
	_, wc := joinAs(t, h, "Cy")
	for _, p := range wc.People {
		if p.Name == "Ana" && p.Status != office.StatusFocused {
			t.Fatalf("welcome shows Ana as %q", p.Status)
		}
	}
}
