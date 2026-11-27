package main

import (
	"encoding/json"
	"strings"
	"testing"
	"time"
)

// e2eClient is one extension loop wired to a real server subprocess.
func e2eClient(t *testing.T, srv *realServer, name string) *harness {
	t.Helper()
	dir := t.TempDir()
	if err := saveSetup(dir, Setup{Name: name, Server: srv.URL(), Token: testToken, Avatar: 1}); err != nil {
		t.Fatal(err)
	}
	h := newHarness(t, dir)
	h.initialize()
	h.waitTree("office", `"scene"`)
	return h
}

// spriteOf finds the sprite labelled name in h's office tree.
func (h *harness) spriteOf(name string) (x, y, n int) {
	var tree struct {
		Children []struct {
			Sprites []struct {
				X, Y  int
				Label string
			}
		}
	}
	json.Unmarshal([]byte(h.tree("office")), &tree)
	if len(tree.Children) == 0 {
		return 0, 0, 0
	}
	for _, s := range tree.Children[0].Sprites {
		if s.Label == name {
			x, y = s.X, s.Y
			n++
		}
	}
	return
}

func (h *harness) waitSprite(t *testing.T, name string, within time.Duration, ok func(x, y int) bool) time.Duration {
	t.Helper()
	start := time.Now()
	for time.Since(start) < within {
		if x, y, n := h.spriteOf(name); n == 1 && ok(x, y) {
			return time.Since(start)
		}
		time.Sleep(2 * time.Millisecond)
	}
	t.Fatalf("%s never reached the expected tile within %v; tree: %s", name, within, h.tree("office"))
	return 0
}

func TestMoveReachesOtherWithin200ms(t *testing.T) {
	srv := startServer(t, "127.0.0.1:0")
	a := e2eClient(t, srv, "Ana")
	b := e2eClient(t, srv, "Bia")
	var ax, ay int
	a.waitSprite(t, "Ana", 3*time.Second, func(int, int) bool { return true })
	b.waitSprite(t, "Ana", 3*time.Second, func(int, int) bool { return true })
	ax, ay, _ = b.spriteOf("Ana")
	a.advance(time.Second)
	// Try each direction until one is free of walls and furniture.
	for _, k := range []string{"up", "down", "left", "right"} {
		a.key(k)
		a.barrier()
		start := time.Now()
		deadline := start.Add(200 * time.Millisecond)
		for time.Now().Before(deadline) {
			if x, y, _ := b.spriteOf("Ana"); x != ax || y != ay {
				return
			}
			time.Sleep(time.Millisecond)
		}
		a.advance(time.Second)
	}
	t.Fatal("no move from Ana reached Bia within 200 ms")
}

func TestStatusReachesOthersWithin1s(t *testing.T) {
	srv := startServer(t, "127.0.0.1:0")
	a := e2eClient(t, srv, "Ana")
	b := e2eClient(t, srv, "Bia")
	b.waitSprite(t, "Ana", 3*time.Second, func(int, int) bool { return true })
	a.send("event/session_state", map[string]any{"agent": "x", "session": "s1", "state": "needs_you"})
	start := time.Now()
	for !strings.Contains(b.tree("office"), "Needs you") {
		if time.Since(start) > time.Second {
			t.Fatalf("Bia never saw Needs you within 1 s: %s", b.tree("office"))
		}
		time.Sleep(5 * time.Millisecond)
	}
}

func TestSecondLoginReplacesFirst(t *testing.T) {
	srv := startServer(t, "127.0.0.1:0")
	first := e2eClient(t, srv, "Ana")
	watcher := e2eClient(t, srv, "Bia")
	watcher.waitSprite(t, "Ana", 3*time.Second, func(int, int) bool { return true })
	second := e2eClient(t, srv, "Ana")
	first.waitTree("presence", "Signed in elsewhere")
	// The replaced client stays out: no reconnect fight.
	time.Sleep(500 * time.Millisecond)
	if !strings.Contains(first.tree("presence"), "Signed in elsewhere") {
		t.Error("first client reconnected after being replaced")
	}
	second.waitSprite(t, "Ana", 3*time.Second, func(int, int) bool { return true })
	deadline := time.Now().Add(3 * time.Second)
	for {
		if _, _, n := watcher.spriteOf("Ana"); n == 1 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("watcher sees Ana %d times", func() int { _, _, n := watcher.spriteOf("Ana"); return n }())
		}
		time.Sleep(10 * time.Millisecond)
	}
}
