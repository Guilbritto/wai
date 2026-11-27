package main

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/tls"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/json"
	"encoding/pem"
	"io"
	"log"
	"math/big"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/coder/websocket"

	"wai-escritorio/office"
)

type logBuf struct {
	mu sync.Mutex
	b  strings.Builder
}

func (l *logBuf) Write(p []byte) (int, error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.b.Write(p)
}

func (l *logBuf) lines() []string {
	l.mu.Lock()
	defer l.mu.Unlock()
	s := strings.TrimSpace(l.b.String())
	if s == "" {
		return nil
	}
	return strings.Split(s, "\n")
}

// waitLine returns the first log line containing sub, failing after 2 s.
func (l *logBuf) waitLine(t *testing.T, sub string) string {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		for _, ln := range l.lines() {
			if strings.Contains(ln, sub) {
				return ln
			}
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatalf("no log line with %q in %q", sub, l.lines())
	return ""
}

func (l *logBuf) count(prefix string) int {
	n := 0
	for _, ln := range l.lines() {
		if strings.HasPrefix(ln, prefix) {
			n++
		}
	}
	return n
}

// captureLog sends the standard logger (stderr in production) to a buffer
// and drops the timestamp so each line starts with its verb.
func captureLog(t *testing.T) *logBuf {
	t.Helper()
	l := &logBuf{}
	log_flags := logFlags()
	setLog(l, 0)
	t.Cleanup(func() { setLog(os.Stderr, log_flags) })
	return l
}

func startServer(t *testing.T) string {
	t.Helper()
	m, err := office.ParseMap(roomMap)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	srv := newServer(ctx, testToken, m)
	ts := httptest.NewServer(srv.handler())
	t.Cleanup(func() { cancel(); ts.Close(); srv.h.stop() })
	return ts.URL
}

func dial(t *testing.T, url string) *websocket.Conn {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	c, _, err := websocket.Dial(ctx, url, nil)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { c.CloseNow() })
	return c
}

func send(t *testing.T, c *websocket.Conn, v any) {
	t.Helper()
	data, err := json.Marshal(v)
	if err != nil {
		t.Fatal(err)
	}
	sendRaw(t, c, data)
}

func sendRaw(t *testing.T, c *websocket.Conn, data []byte) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	if err := c.Write(ctx, websocket.MessageText, data); err != nil {
		t.Fatal(err)
	}
}

// next reads one frame: its "t" and raw JSON. A closed connection is an error.
func next(c *websocket.Conn, wait time.Duration) (string, []byte, error) {
	ctx, cancel := context.WithTimeout(context.Background(), wait)
	defer cancel()
	_, data, err := c.Read(ctx)
	if err != nil {
		return "", nil, err
	}
	var h struct{ T string }
	json.Unmarshal(data, &h)
	return h.T, data, nil
}

// expect reads frames until one of type typ arrives (2 s), and returns it.
func expect(t *testing.T, c *websocket.Conn, typ string) []byte {
	t.Helper()
	for {
		got, data, err := next(c, 2*time.Second)
		if err != nil {
			t.Fatalf("waiting for %q: %v", typ, err)
		}
		if got == typ {
			return data
		}
	}
}

func connect(t *testing.T, url, name string) (*websocket.Conn, office.Welcome) {
	t.Helper()
	c := dial(t, url)
	send(t, c, hello(name))
	var w office.Welcome
	if err := json.Unmarshal(expect(t, c, office.TWelcome), &w); err != nil {
		t.Fatal(err)
	}
	return c, w
}

func TestDefaultTimingsMeetSpec(t *testing.T) {
	if pingEvery != 3*time.Second || pongWithin != 7*time.Second {
		t.Errorf("ping %v, deadline %v, want 3s and 7s", pingEvery, pongWithin)
	}
	if pingEvery+pongWithin > 10*time.Second {
		t.Errorf("a dead peer would linger %v, spec says 10s", pingEvery+pongWithin)
	}
	if readLimit != 16*1024 || rateLimit != 30 || maxPeople != 50 {
		t.Errorf("limits %d bytes, %d msg/s, %d people; want 16384, 30, 50", readLimit, rateLimit, maxPeople)
	}
}

func TestServerAcceptsOnConfiguredAddress(t *testing.T) {
	logs := captureLog(t)
	probe, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	addr := probe.Addr().String()
	probe.Close()

	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() { done <- run(ctx, Config{Listen: addr, Token: testToken}, office.DefaultMap()) }()
	if ln := logs.waitLine(t, "listening on"); !strings.Contains(ln, addr) {
		t.Fatalf("log line %q does not name %s", ln, addr)
	}
	c, w := connect(t, "ws://"+addr, "Ana")
	if len(w.People) != 1 || w.You == "" {
		t.Fatalf("welcome = %+v", w)
	}
	c.CloseNow()
	cancel()
	select {
	case err := <-done:
		if err != nil {
			t.Fatalf("run: %v", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("run did not return after cancel")
	}
}

func TestListensOnPortZeroAndPrintsAddress(t *testing.T) {
	logs := captureLog(t)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go run(ctx, Config{Listen: "127.0.0.1:0", Token: testToken}, office.DefaultMap())
	line := logs.waitLine(t, "listening on")
	m := regexp.MustCompile(`listening on (127\.0\.0\.1:\d+)$`).FindStringSubmatch(line)
	if m == nil || strings.HasSuffix(m[1], ":0") {
		t.Fatalf("line %q has no bound address", line)
	}
	if _, w := connect(t, "ws://"+m[1], "Ana"); len(w.People) != 1 {
		t.Fatalf("welcome = %+v", w)
	}
	if n := logs.count("listening on"); n != 1 {
		t.Fatalf("%d listening lines, want 1", n)
	}
}

func TestServesTLSWhenConfigured(t *testing.T) {
	logs := captureLog(t)
	dir := t.TempDir()
	cert, key := writeSelfSigned(t, dir)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go run(ctx, Config{Listen: "127.0.0.1:0", Token: testToken, TLSCert: cert, TLSKey: key}, office.DefaultMap())
	line := logs.waitLine(t, "listening on")
	addr := line[strings.LastIndex(line, " ")+1:]

	dctx, dcancel := context.WithTimeout(ctx, 2*time.Second)
	defer dcancel()
	hc := &http.Client{Transport: &http.Transport{TLSClientConfig: &tls.Config{InsecureSkipVerify: true}}}
	c, _, err := websocket.Dial(dctx, "wss://"+addr, &websocket.DialOptions{HTTPClient: hc})
	if err != nil {
		t.Fatal(err)
	}
	defer c.CloseNow()
	send(t, c, hello("Ana"))
	expect(t, c, office.TWelcome)
	if _, _, err := websocket.Dial(dctx, "ws://"+addr, nil); err == nil {
		t.Fatal("plain ws accepted on a TLS server")
	}
}

func writeSelfSigned(t *testing.T, dir string) (certFile, keyFile string) {
	t.Helper()
	k, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	tpl := &x509.Certificate{
		SerialNumber: big.NewInt(1), Subject: pkix.Name{CommonName: "localhost"},
		NotBefore: time.Now().Add(-time.Hour), NotAfter: time.Now().Add(time.Hour),
		IPAddresses: []net.IP{net.ParseIP("127.0.0.1")},
	}
	der, err := x509.CreateCertificate(rand.Reader, tpl, tpl, &k.PublicKey, k)
	if err != nil {
		t.Fatal(err)
	}
	kb, _ := x509.MarshalECPrivateKey(k)
	certFile, keyFile = filepath.Join(dir, "c.pem"), filepath.Join(dir, "k.pem")
	os.WriteFile(certFile, pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: der}), 0o600)
	os.WriteFile(keyFile, pem.EncodeToMemory(&pem.Block{Type: "EC PRIVATE KEY", Bytes: kb}), 0o600)
	return
}

func TestWrongTokenRefusedAndLoggedWithoutToken(t *testing.T) {
	logs := captureLog(t)
	url := startServer(t)
	wrong := "wrong-token-wrong-token-wrong-tok"
	c := dial(t, url)
	send(t, c, office.Hello{T: office.THello, Token: wrong, Name: "Mallory"})
	var r office.Refused
	if err := json.Unmarshal(expect(t, c, office.TRefused), &r); err != nil || r.Reason != office.ReasonToken {
		t.Fatalf("refusal = %+v, %v", r, err)
	}
	if _, _, err := next(c, 2*time.Second); err == nil {
		t.Fatal("connection stayed open after the refusal")
	}
	line := logs.waitLine(t, "refused")
	if !strings.Contains(line, "127.0.0.1:") || !strings.Contains(line, "reason=token") {
		t.Errorf("line %q lacks the client address or reason", line)
	}
	for _, ln := range logs.lines() {
		if strings.Contains(ln, wrong) || strings.Contains(ln, testToken) {
			t.Errorf("token in log line %q", ln)
		}
	}
}

func TestRateLimitDropsPast30(t *testing.T) {
	url := startServer(t)
	a, _ := connect(t, url, "Ana")
	b, _ := connect(t, url, "Bob")
	expect(t, a, office.TJoined)
	for i := 0; i < 40; i++ {
		x := 2 - i%2 // 2,1,2,1... each an adjacent free tile from spawn 1,1
		send(t, a, office.Move{T: office.TMove, X: x, Y: 1})
	}
	moved := 0
	for {
		typ, _, err := next(b, 400*time.Millisecond)
		if err != nil {
			break
		}
		if typ == office.TMoved {
			moved++
		}
	}
	if moved != 30 {
		t.Fatalf("server acted on %d of 40 messages, want 30", moved)
	}
}

func TestOversizeMessageCloses(t *testing.T) {
	url := startServer(t)
	a, _ := connect(t, url, "Ana")
	b, wb := connect(t, url, "Bob")
	expect(t, a, office.TJoined)
	var anaID string
	for _, p := range wb.People {
		if p.Name == "Ana" {
			anaID = p.ID
		}
	}

	pad := func(total int) []byte {
		const frame = `{"t":"status","status":""}`
		return []byte(`{"t":"status","status":"` + strings.Repeat("x", total-len(frame)) + `"}`)
	}
	sendRaw(t, a, pad(16*1024)) // exactly 16 KiB is fine
	send(t, a, office.Move{T: office.TMove, X: 2, Y: 1})
	expect(t, b, office.TMoved)

	sendRaw(t, a, pad(16*1024+1))
	var closeErr error
	for {
		if _, _, closeErr = next(a, 2*time.Second); closeErr != nil {
			break
		}
	}
	if websocket.CloseStatus(closeErr) != websocket.StatusMessageTooBig {
		t.Fatalf("close error = %v, want message too big", closeErr)
	}
	var l office.Left
	json.Unmarshal(expect(t, b, office.TLeft), &l)
	if l.ID != anaID {
		t.Fatalf("others saw %+v leave, want %s", l, anaID)
	}
}

func TestSilentPeerLeaves(t *testing.T) {
	oldPing, oldPong := pingEvery, pongWithin
	pingEvery, pongWithin = 50*time.Millisecond, 150*time.Millisecond
	t.Cleanup(func() { pingEvery, pongWithin = oldPing, oldPong })
	url := startServer(t)

	silent, ws := connect(t, url, "Ghost") // never reads again, so never answers a ping
	_ = silent
	live, _ := connect(t, url, "Bob")
	start := time.Now()
	var l office.Left
	json.Unmarshal(expect(t, live, office.TLeft), &l)
	if l.ID != ws.You {
		t.Fatalf("saw %s leave, want the silent peer %s", l.ID, ws.You)
	}
	if took := time.Since(start); took > time.Second {
		t.Fatalf("silent peer left after %v", took)
	}

	// a peer that keeps answering stays, long after the deadline
	go func() {
		for {
			if _, _, err := next(live, 5*time.Second); err != nil {
				return
			}
		}
	}()
	time.Sleep(600 * time.Millisecond)
	_, wc := connect(t, url, "Cy")
	names := ""
	for _, p := range wc.People {
		names += p.Name + " "
	}
	if !strings.Contains(names, "Bob") || strings.Contains(names, "Ghost") {
		t.Fatalf("people = %q, want Bob and Cy only", names)
	}
}

func TestLogsConnectDisconnectRefusal(t *testing.T) {
	logs := captureLog(t)
	url := startServer(t)

	a, _ := connect(t, url, "Ana")
	logs.waitLine(t, "connect ")
	a.Close(websocket.StatusNormalClosure, "")
	logs.waitLine(t, "disconnect ")

	m := dial(t, url)
	send(t, m, office.Hello{T: office.THello, Token: "nope", Name: "Mallory"})
	logs.waitLine(t, "refused ")

	time.Sleep(100 * time.Millisecond) // let a stray extra line show up
	for _, verb := range []string{"connect ", "disconnect ", "refused "} {
		if n := logs.count(verb); n != 1 {
			t.Errorf("%d %q lines, want 1: %q", n, verb, logs.lines())
		}
	}
	for _, ln := range logs.lines() {
		if strings.Contains(ln, testToken) || strings.Contains(ln, "nope") {
			t.Errorf("token in line %q", ln)
		}
	}
	if ln := logs.waitLine(t, "connect "); !strings.Contains(ln, `name="Ana"`) || !strings.Contains(ln, "127.0.0.1:") {
		t.Errorf("connect line %q lacks name or address", ln)
	}
}

func TestMoveFanOutWithin200ms(t *testing.T) {
	url := startServer(t)
	a, _ := connect(t, url, "Ana")
	b, _ := connect(t, url, "Bob")
	expect(t, a, office.TJoined)
	for i := 0; i < 10; i++ {
		x := 2 - i%2
		start := time.Now()
		send(t, a, office.Move{T: office.TMove, X: x, Y: 1})
		var mv office.Moved
		json.Unmarshal(expect(t, b, office.TMoved), &mv)
		if took := time.Since(start); took > 200*time.Millisecond || mv.X != x {
			t.Fatalf("move %d reached the other client in %v as %+v", i, took, mv)
		}
	}
}

func logFlags() int { return log.Flags() }

func setLog(w io.Writer, flags int) { log.SetOutput(w); log.SetFlags(flags) }
