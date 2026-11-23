package main

import (
	"encoding/json"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"testing"
	"time"
)

const tunnelURL = "https://my-mac.tail0123ab.ts.net"

// freePort is a loopback port nothing listens on right now.
func freePort(t *testing.T) int {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer ln.Close()
	return ln.Addr().(*net.TCPAddr).Port
}

// refused checks nothing accepts a connection at addr.
func refused(t *testing.T, what, addr string) {
	t.Helper()
	c, err := net.DialTimeout("tcp", addr, 500*time.Millisecond)
	if err == nil {
		c.Close()
		t.Errorf("%s: %s accepts connections", what, addr)
	}
}

// otherAddress is one of this computer's IPv4 addresses that is not loopback, or "" when it has
// none.
func otherAddress() string {
	addrs, _ := net.InterfaceAddrs()
	for _, a := range addrs {
		if ip, ok := a.(*net.IPNet); ok && !ip.IP.IsLoopback() && ip.IP.To4() != nil && !ip.IP.IsLinkLocalUnicast() {
			return ip.IP.String()
		}
	}
	return ""
}

func (a *api) stopAtEnd() { a.t.Cleanup(a.h.s.stop) }

func notListening(t *testing.T, a *api, what string) {
	t.Helper()
	if st, on := a.h.s.status(); on || st != "Not listening" {
		t.Errorf("%s: status %q, %v", what, st, on)
	}
}

// PAIR-03: Start refuses a port outside 1024-65535, before it binds or saves anything.
func TestStartRejectsPortOutOfRange(t *testing.T) {
	a := startAPI(t)
	a.stopAtEnd()
	for _, port := range []int{0, 80, 1023, 65536, -1} {
		err := a.h.s.start(port, tunnelURL)
		if err == nil || err.Error() != "Port must be between 1024 and 65535" {
			t.Errorf("port %d: %v", port, err)
		}
		notListening(t, a, "port "+strconv.Itoa(port))
	}
	if port, tunnel := a.h.s.store.settings(); port != 7717 || tunnel != "" {
		t.Errorf("refused settings were saved: %d %q", port, tunnel)
	}
	for _, port := range []int{1024, 65535} {
		if err := checkSettings(port, tunnelURL); err != nil {
			t.Errorf("port %d: %v", port, err)
		}
	}
}

// PAIR-04: Start refuses a tunnel URL that does not begin with https://.
func TestStartRejectsNonHTTPSTunnel(t *testing.T) {
	a := startAPI(t)
	a.stopAtEnd()
	port := freePort(t)
	for _, tunnel := range []string{"", "http://my-mac.tail0123ab.ts.net", "my-mac.tail0123ab.ts.net", " https://my-mac.ts.net", "wss://my-mac.ts.net"} {
		err := a.h.s.start(port, tunnel)
		if err == nil || err.Error() != "The tunnel URL must start with https://" {
			t.Errorf("tunnel %q: %v", tunnel, err)
		}
		notListening(t, a, "tunnel "+tunnel)
		refused(t, "tunnel "+tunnel, "127.0.0.1:"+strconv.Itoa(port))
	}
	if _, tunnel := a.h.s.store.settings(); tunnel != "" {
		t.Errorf("a refused tunnel URL was saved: %q", tunnel)
	}
}

// PAIR-05: Start listens on 127.0.0.1:<port> only, saves the settings, and the API answers there.
func TestStartListensOnLoopbackOnly(t *testing.T) {
	a := startAPI(t)
	a.stopAtEnd()
	port := freePort(t)
	addr := "127.0.0.1:" + strconv.Itoa(port)
	if err := a.h.s.start(port, tunnelURL); err != nil {
		t.Fatal(err)
	}
	if st, on := a.h.s.status(); !on || st != "Listening on "+addr {
		t.Errorf("status %q, %v", st, on)
	}

	// The API answers on the port, through the tunnel's Host as through the loopback one.
	for _, host := range []string{addr, "my-mac.tail0123ab.ts.net"} {
		r, _ := http.NewRequest("GET", "http://"+addr+"/v1/sessions", nil)
		r.Host = host
		r.Header.Set("Authorization", "Bearer "+a.token)
		done := make(chan *http.Response, 1)
		go func() {
			resp, err := http.DefaultClient.Do(r)
			if err != nil {
				t.Error(err)
			}
			done <- resp
		}()
		id, _ := a.h.expectCall("sessions/list")
		a.h.reply(id, `[]`)
		if resp := <-done; resp == nil || resp.StatusCode != http.StatusOK {
			t.Errorf("Host %s: %v", host, resp)
		} else {
			resp.Body.Close()
		}
	}

	refused(t, "IPv6 loopback", "[::1]:"+strconv.Itoa(port))
	if other := otherAddress(); other != "" {
		refused(t, "a network address", net.JoinHostPort(other, strconv.Itoa(port)))
	}

	// The settings are saved in state.json.
	b, _ := os.ReadFile(filepath.Join(a.h.s.dir, "state.json"))
	var st State
	if json.Unmarshal(b, &st); st.Port != port || st.TunnelURL != tunnelURL {
		t.Errorf("state.json: %s", b)
	}
}

// PAIR-06: a port in use is reported as "Port <port> is in use", and nothing listens.
func TestStartReportsPortInUse(t *testing.T) {
	a := startAPI(t)
	a.stopAtEnd()
	taken, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer taken.Close()
	port := taken.Addr().(*net.TCPAddr).Port
	err = a.h.s.start(port, tunnelURL)
	if want := "Port " + strconv.Itoa(port) + " is in use"; err == nil || err.Error() != want {
		t.Errorf("start on a taken port: %v, want %q", err, want)
	}
	notListening(t, a, "a taken port")

	// Once the port is free, Start works.
	taken.Close()
	if err := a.h.s.start(port, tunnelURL); err != nil {
		t.Errorf("start after the port was freed: %v", err)
	}
}

// PAIR-07: before Start nothing listens, not even on the saved port.
func TestNothingListensBeforeStart(t *testing.T) {
	dir := dataDir(t)
	port := freePort(t)
	state := `{"port":` + strconv.Itoa(port) + `,"tunnel_url":"` + tunnelURL + `","devices":[]}`
	if err := os.WriteFile(filepath.Join(dir, "state.json"), []byte(state), 0o600); err != nil {
		t.Fatal(err)
	}
	h := startHost(t)
	h.initialize(dir)
	h.next()
	h.next()
	if p, _ := h.s.store.settings(); p != port {
		t.Fatalf("saved port %d, want %d", p, port)
	}
	if st, on := h.s.status(); on || st != "Not listening" {
		t.Errorf("status %q, %v", st, on)
	}
	refused(t, "before Start", "127.0.0.1:"+strconv.Itoa(port))
}

// PAIR-12: Stop closes the listener and every connection, streams included, within 1 second;
// Start works again afterwards.
func TestStopClosesListenerAndStreams(t *testing.T) {
	a := startAPI(t)
	a.stopAtEnd()
	port := freePort(t)
	addr := "127.0.0.1:" + strconv.Itoa(port)
	if err := a.h.s.start(port, tunnelURL); err != nil {
		t.Fatal(err)
	}
	other, _, _ := a.h.s.store.add("iPhone")
	base := "http://" + addr
	streams := []*sse{openSSEAt(t, base, addr, a.token), openSSEAt(t, base, addr, a.token), openSSEAt(t, base, addr, other)}
	for _, st := range streams {
		if st.resp.StatusCode != http.StatusOK {
			t.Fatalf("stream: %d", st.resp.StatusCode)
		}
	}
	idle, err := net.Dial("tcp", addr) // a connection that has sent nothing yet
	if err != nil {
		t.Fatal(err)
	}
	defer idle.Close()

	began := time.Now()
	a.h.s.stop()
	for _, st := range streams {
		st.ends("a stream after Stop", time.Second)
	}
	idle.SetReadDeadline(time.Now().Add(time.Second))
	if _, err := idle.Read(make([]byte, 1)); err == nil || isTimeout(err) {
		t.Errorf("an idle connection is still open after Stop: %v", err)
	}
	if took := time.Since(began); took > time.Second {
		t.Errorf("Stop took %v to close everything", took)
	}
	refused(t, "after Stop", addr)
	notListening(t, a, "after Stop")
	if n := a.h.s.hub.connected(); n != 0 {
		t.Errorf("%d devices connected after Stop", n)
	}

	if err := a.h.s.start(port, tunnelURL); err != nil {
		t.Errorf("start again: %v", err)
	}
}

func isTimeout(err error) bool {
	ne, ok := err.(net.Error)
	return ok && ne.Timeout()
}
