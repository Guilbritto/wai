package main

import (
	"bufio"
	"encoding/json"
	"io"
	"sync"
	"testing"
	"time"
)

// host is a fake wai: it talks to a server over pipes, as wai does over the process's stdio.
type host struct {
	t     *testing.T
	s     *server
	in    *io.PipeWriter
	lines chan map[string]any

	mu  sync.Mutex
	now time.Time
}

func startHost(t *testing.T) *host {
	t.Helper()
	inR, inW := io.Pipe()
	outR, outW := io.Pipe()
	h := &host{t: t, in: inW, lines: make(chan map[string]any, 100), now: time.Date(2026, 11, 21, 12, 0, 0, 0, time.UTC)}
	h.s = newServer(outW, h.clock)
	done := make(chan struct{})
	go func() {
		defer close(done)
		serve(h.s, inR)
		outW.Close()
	}()
	go func() {
		defer close(h.lines)
		sc := bufio.NewScanner(outR)
		sc.Buffer(nil, 4<<20)
		for sc.Scan() {
			var v map[string]any
			if err := json.Unmarshal(sc.Bytes(), &v); err != nil {
				t.Errorf("bad line %q: %v", sc.Text(), err)
				continue
			}
			h.lines <- v
		}
	}()
	t.Cleanup(func() {
		inW.Close()
		<-done
	})
	return h
}

func (h *host) clock() time.Time {
	h.mu.Lock()
	defer h.mu.Unlock()
	return h.now
}

func (h *host) advance(d time.Duration) {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.now = h.now.Add(d)
}

func (h *host) send(line string) {
	h.t.Helper()
	if _, err := io.WriteString(h.in, line+"\n"); err != nil {
		h.t.Fatal(err)
	}
}

// next is the server's next message to wai.
func (h *host) next() map[string]any {
	h.t.Helper()
	select {
	case m, ok := <-h.lines:
		if !ok {
			h.t.Fatal("the server closed its output")
		}
		return m
	case <-time.After(5 * time.Second):
		h.t.Fatal("no message from the server")
	}
	return nil
}

func quote(s string) string { b, _ := json.Marshal(s); return string(b) }

func (h *host) initialize(dir string) {
	h.t.Helper()
	h.send(`{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":1,"api":"1.10","data_dir":` + quote(dir) + `}}`)
}

// PAIR-01: wai starts the process and it answers initialize, then draws its sidebar panel.
func TestInitializeAndRender(t *testing.T) {
	dir := t.TempDir()
	h := startHost(t)
	h.initialize(dir)
	reply := h.next()
	if reply["id"] != float64(0) || reply["result"] == nil || reply["error"] != nil {
		t.Fatalf("initialize answered %v", reply)
	}
	render := h.next()
	p, _ := render["params"].(map[string]any)
	if render["method"] != "panel/render" || render["id"] != nil || p["panel"] != "remoto" {
		t.Fatalf("want a panel/render notification for remoto, got %v", render)
	}
	if tree, _ := p["tree"].(map[string]any); tree["type"] != "column" {
		t.Errorf("tree: %v", p["tree"])
	}
	if h.s.dir != dir {
		t.Errorf("data folder %q, want %q", h.s.dir, dir)
	}

	// An unknown request gets Method not found; a notification gets nothing.
	h.send(`{"jsonrpc":"2.0","method":"event/file_saved","params":{"path":"/x"}}`)
	h.send(`{"jsonrpc":"2.0","id":7,"method":"bogus"}`)
	if m := h.next(); m["id"] != float64(7) || m["error"].(map[string]any)["code"] != float64(-32601) {
		t.Errorf("unknown method: %v", m)
	}

	// Without data_dir, the folder comes from WAI_EXT_DATA.
	env := t.TempDir()
	t.Setenv("WAI_EXT_DATA", env)
	h2 := startHost(t)
	h2.send(`{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":1}}`)
	h2.next()
	h2.next()
	if h2.s.dir != env {
		t.Errorf("data folder %q, want WAI_EXT_DATA %q", h2.s.dir, env)
	}
}
