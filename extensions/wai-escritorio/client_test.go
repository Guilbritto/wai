package main

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"testing"
	"time"

	"wai-escritorio/office"
)

const testToken = "0123456789abcdef0123456789abcdef"

var (
	srvOnce sync.Once
	srvBin  string
	srvErr  error
)

// serverBinary builds ./server once per test run.
func serverBinary(t testing.TB) string {
	srvOnce.Do(func() {
		dir, err := os.MkdirTemp("", "escr-srv")
		if err != nil {
			srvErr = err
			return
		}
		srvBin = filepath.Join(dir, "srv")
		if out, err := exec.Command("go", "build", "-o", srvBin, "./server").CombinedOutput(); err != nil {
			srvErr = fmt.Errorf("%v: %s", err, out)
		}
	})
	if srvErr != nil {
		t.Fatal(srvErr)
	}
	return srvBin
}

func TestMain(m *testing.M) {
	code := m.Run()
	if srvBin != "" {
		os.RemoveAll(filepath.Dir(srvBin))
	}
	os.Exit(code)
}

// realServer is the office server binary running as a subprocess.
type realServer struct {
	cmd  *exec.Cmd
	addr string // host:port
	mu   sync.Mutex
	logs []string
}

func (r *realServer) URL() string { return "ws://" + r.addr }

func (r *realServer) log() []string {
	r.mu.Lock()
	defer r.mu.Unlock()
	return append([]string(nil), r.logs...)
}

func (r *realServer) count(sub string) int {
	n := 0
	for _, l := range r.log() {
		if strings.Contains(l, sub) {
			n++
		}
	}
	return n
}

func (r *realServer) stop() {
	r.cmd.Process.Kill()
	r.cmd.Wait()
}

var listenRe = regexp.MustCompile(`listening on (\S+)$`)

// startServer runs the server on listen ("127.0.0.1:0" for a free port) and waits for its address.
func startServer(t testing.TB, listen string) *realServer {
	t.Helper()
	cfg := filepath.Join(t.TempDir(), "office.json")
	b, _ := json.Marshal(map[string]string{"listen": listen, "token": testToken, "tls_cert": "", "tls_key": "", "map": ""})
	os.WriteFile(cfg, b, 0o600)
	cmd := exec.Command(serverBinary(t), "-config", cfg)
	pipe, err := cmd.StderrPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	r := &realServer{cmd: cmd}
	ready := make(chan string, 1)
	go func() {
		sc := bufio.NewScanner(pipe)
		for sc.Scan() {
			line := sc.Text()
			r.mu.Lock()
			r.logs = append(r.logs, line)
			r.mu.Unlock()
			if m := listenRe.FindStringSubmatch(line); m != nil {
				select {
				case ready <- m[1]:
				default:
				}
			}
		}
	}()
	select {
	case r.addr = <-ready:
	case <-time.After(10 * time.Second):
		cmd.Process.Kill()
		t.Fatal("server did not print its address")
	}
	t.Cleanup(r.stop)
	return r
}

// clientRun starts run in a goroutine; done closes when it returns.
type clientRun struct {
	to     chan serverMsg
	from   chan any
	cancel context.CancelFunc
	done   chan struct{}
}

func startClient(t testing.TB, s Setup) *clientRun {
	ctx, cancel := context.WithCancel(context.Background())
	c := &clientRun{to: make(chan serverMsg), from: make(chan any, 16), cancel: cancel, done: make(chan struct{})}
	go func() { defer close(c.done); run(ctx, s, c.to, c.from) }()
	t.Cleanup(func() { cancel(); <-c.done })
	return c
}

// expect waits for the next message and requires its type.
func (c *clientRun) expect(t testing.TB, typ string, within time.Duration) serverMsg {
	t.Helper()
	select {
	case m := <-c.to:
		if m.T != typ {
			t.Fatalf("got %q (%+v), want %q", m.T, m, typ)
		}
		return m
	case <-time.After(within):
		t.Fatalf("no %q within %v", typ, within)
	}
	return serverMsg{}
}

func setupFor(url, name string) Setup {
	return Setup{Name: name, Server: url, Token: testToken, Avatar: 1}
}

func TestTokenRefusedStops(t *testing.T) {
	srv := startServer(t, "127.0.0.1:0")
	s := setupFor(srv.URL(), "Ana")
	s.Token = strings.Repeat("x", 32)
	c := startClient(t, s)
	m := c.expect(t, office.TRefused, 5*time.Second)
	if m.Reason != office.ReasonToken {
		t.Fatalf("reason %q", m.Reason)
	}
	select {
	case <-c.done:
	case <-time.After(2 * time.Second):
		t.Fatal("run did not stop after a token refusal")
	}
	time.Sleep(retryEvery + 500*time.Millisecond)
	if n := srv.count("refused"); n != 1 {
		t.Errorf("server saw %d refusals, want 1 (no redial)", n)
	}
}

func TestUnreachableRetries(t *testing.T) {
	if retryEvery != 5*time.Second {
		t.Fatalf("retry interval %v, spec says 5 s", retryEvery)
	}
	ln, _ := net.Listen("tcp", "127.0.0.1:0")
	addr := ln.Addr().String()
	ln.Close()
	c := startClient(t, setupFor("ws://"+addr, "Ana"))
	c.expect(t, tUnreachable, 3*time.Second)
	start := time.Now()
	c.expect(t, tUnreachable, 7*time.Second)
	if d := time.Since(start); d < 4500*time.Millisecond || d > 5800*time.Millisecond {
		t.Errorf("second attempt after %v, want about 5 s", d)
	}
}

func TestReplacedStopsRetrying(t *testing.T) {
	srv := startServer(t, "127.0.0.1:0")
	first := startClient(t, setupFor(srv.URL(), "Ana"))
	first.expect(t, office.TWelcome, 5*time.Second)
	second := startClient(t, setupFor(srv.URL(), "Ana"))
	second.expect(t, office.TWelcome, 5*time.Second)
	m := first.expect(t, office.TRefused, 5*time.Second)
	if m.Reason != office.ReasonReplaced {
		t.Fatalf("reason %q", m.Reason)
	}
	select {
	case <-first.done:
	case <-time.After(2 * time.Second):
		t.Fatal("run did not stop after being replaced")
	}
	time.Sleep(retryEvery + 500*time.Millisecond)
	if n := srv.count(" connect addr="); n != 2 {
		t.Errorf("server saw %d connects, want 2 (the older never redials)", n)
	}
}

func TestReconnectAfterServerRestart(t *testing.T) {
	srv := startServer(t, "127.0.0.1:0")
	addr := srv.addr
	c := startClient(t, setupFor(srv.URL(), "Ana"))
	w := c.expect(t, office.TWelcome, 5*time.Second)
	// Walk away from the spawn point so a restart visibly resets it.
	c.from <- office.Move{T: office.TMove, X: w.People[0].X, Y: w.People[0].Y + 1}
	time.Sleep(200 * time.Millisecond)
	srv.stop()
	c.expect(t, tUnreachable, 5*time.Second)
	// The next dial is 5 s after this failure; the server is back well before it.
	time.Sleep(500 * time.Millisecond)
	startServer(t, addr)
	up := time.Now()
	w2 := c.expect(t, office.TWelcome, 6*time.Second)
	if d := time.Since(up); d > 5*time.Second {
		t.Errorf("reconnected %v after the server was up, want within 5 s", d)
	}
	spawn := office.DefaultMap().Spawn
	if got := w2.People[0]; got.X != spawn.X || got.Y != spawn.Y {
		t.Errorf("after restart at %d,%d, want spawn %v", got.X, got.Y, spawn)
	}
}
