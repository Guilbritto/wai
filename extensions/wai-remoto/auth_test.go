package main

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

const tunnelHost = "my-mac.tail0123ab.ts.net"

// guarded is a guard over a store with one paired device, and a route that answers 200 with the
// device's name and the body's length.
type guarded struct {
	g      *guard
	token  string
	public http.Handler // wrapped without a token, as the pair route
	route  http.Handler
	calls  int
}

func newGuarded(t *testing.T, now func() time.Time, notify func(string)) *guarded {
	t.Helper()
	s, err := openStore(t.TempDir(), now)
	if err != nil {
		t.Fatal(err)
	}
	s.st.TunnelURL = "https://" + tunnelHost
	token, _, err := s.add("Pixel")
	if err != nil {
		t.Fatal(err)
	}
	gd := &guarded{g: &guard{store: s, notify: notify}, token: token}
	next := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gd.calls++
		b, _ := io.ReadAll(r.Body)
		writeJSON(w, http.StatusOK, node{"device": deviceOf(r).Name, "len": len(b)})
	})
	gd.route = gd.g.wrap(next, true)
	gd.public = gd.g.wrap(next, false)
	return gd
}

// request is a good request (loopback host, the device's token, no Origin) changed by edit.
func (gd *guarded) request(body string, edit func(r *http.Request)) *http.Request {
	r := httptest.NewRequest(http.MethodPost, "/v1/sessions", strings.NewReader(body))
	r.Host = "127.0.0.1:7717"
	r.Header.Set("Authorization", "Bearer "+gd.token)
	if edit != nil {
		edit(r)
	}
	return r
}

func serveTo(h http.Handler, r *http.Request) *httptest.ResponseRecorder {
	w := httptest.NewRecorder()
	h.ServeHTTP(w, r)
	return w
}

// expect checks the status, and that a refusal never reached the route and carries a JSON error.
func (gd *guarded) expect(t *testing.T, what string, h http.Handler, r *http.Request, status int) *httptest.ResponseRecorder {
	t.Helper()
	before := gd.calls
	w := serveTo(h, r)
	if w.Code != status {
		t.Errorf("%s: status %d, want %d (%s)", what, w.Code, status, w.Body)
	}
	if status != http.StatusOK {
		var e struct{ Error string }
		if gd.calls != before || json.Unmarshal(w.Body.Bytes(), &e) != nil || e.Error == "" {
			t.Errorf("%s: reached the route (%v) or no JSON error: %s", what, gd.calls != before, w.Body)
		}
	}
	return w
}

func fixedNow() time.Time { return t0 }

// EDGE-01: a request with an Origin header answers 403, whatever its value.
func TestOriginHeaderIs403(t *testing.T) {
	gd := newGuarded(t, fixedNow, func(string) {})
	gd.expect(t, "no Origin", gd.route, gd.request("", nil), http.StatusOK)
	for _, origin := range []string{"https://evil.example", "https://" + tunnelHost, "null", ""} {
		set := func(r *http.Request) { r.Header["Origin"] = []string{origin} }
		gd.expect(t, "Origin "+origin, gd.route, gd.request("", set), http.StatusForbidden)
		gd.expect(t, "Origin "+origin+" on the pair route", gd.public, gd.request("", set), http.StatusForbidden)
	}
}

// EDGE-02: a Host that is neither the tunnel URL's host nor 127.0.0.1:<port> answers 421.
func TestUnknownHostIs421(t *testing.T) {
	gd := newGuarded(t, fixedNow, func(string) {})
	host := func(h string) func(*http.Request) { return func(r *http.Request) { r.Host = h } }
	for _, ok := range []string{"127.0.0.1:7717", tunnelHost, "MY-MAC.tail0123ab.ts.net"} {
		gd.expect(t, "Host "+ok, gd.route, gd.request("", host(ok)), http.StatusOK)
	}
	for _, bad := range []string{"localhost:7717", "127.0.0.1:7718", "127.0.0.1", "[::1]:7717", "evil.example", tunnelHost + ".evil.example", tunnelHost + ":7717", ""} {
		gd.expect(t, "Host "+bad, gd.route, gd.request("", host(bad)), http.StatusMisdirectedRequest)
		gd.expect(t, "Host "+bad+" on the pair route", gd.public, gd.request("", host(bad)), http.StatusMisdirectedRequest)
	}
	// The port is the configured one.
	gd.g.store.st.Port = 8000
	gd.expect(t, "Host 127.0.0.1:8000", gd.route, gd.request("", host("127.0.0.1:8000")), http.StatusOK)
	gd.expect(t, "Host 127.0.0.1:7717 after the port changed", gd.route, gd.request("", host("127.0.0.1:7717")), http.StatusMisdirectedRequest)
}

// EDGE-03: a missing or unknown token answers 401; a revoked or expired one is unknown.
func TestMissingOrUnknownTokenIs401(t *testing.T) {
	now := t0
	gd := newGuarded(t, func() time.Time { return now }, func(string) {})
	auth := func(v string) func(*http.Request) { return func(r *http.Request) { r.Header.Set("Authorization", v) } }

	w := gd.expect(t, "the device's token", gd.route, gd.request("", nil), http.StatusOK)
	if !strings.Contains(w.Body.String(), `"device":"Pixel"`) {
		t.Errorf("the route does not see the device: %s", w.Body)
	}
	gd.expect(t, "no Authorization", gd.route, gd.request("", func(r *http.Request) { r.Header.Del("Authorization") }), http.StatusUnauthorized)
	for _, v := range []string{"Bearer", "Bearer ", "Bearer nope", "Basic " + gd.token, gd.token, "Bearer " + hashToken(gd.token)} {
		gd.expect(t, "Authorization "+v, gd.route, gd.request("", auth(v)), http.StatusUnauthorized)
	}
	// The pair route takes no token.
	gd.expect(t, "the pair route without a token", gd.public, gd.request("", func(r *http.Request) { r.Header.Del("Authorization") }), http.StatusOK)

	other, d, _ := gd.g.store.add("Old")
	gd.g.store.revoke(d.ID)
	gd.expect(t, "a revoked token", gd.route, gd.request("", auth("Bearer "+other)), http.StatusUnauthorized)
	now = now.Add(30 * 24 * time.Hour)
	gd.expect(t, "a token idle for 30 days", gd.route, gd.request("", nil), http.StatusUnauthorized)
}

// EDGE-04: a body over 64 KB answers 413, with or without a Content-Length.
func TestBodyOver64KBIs413(t *testing.T) {
	gd := newGuarded(t, fixedNow, func(string) {})
	limit := strings.Repeat("x", 64*1024)
	w := gd.expect(t, "a 64 KB body", gd.route, gd.request(limit, nil), http.StatusOK)
	if !strings.Contains(w.Body.String(), `"len":65536`) {
		t.Errorf("the route got a cut body: %s", w.Body)
	}
	gd.expect(t, "64 KB and a byte", gd.route, gd.request(limit+"x", nil), http.StatusRequestEntityTooLarge)
	chunked := func(r *http.Request) { r.ContentLength = -1 }
	gd.expect(t, "64 KB and a byte, chunked", gd.route, gd.request(limit+"x", chunked), http.StatusRequestEntityTooLarge)
	gd.expect(t, "64 KB and a byte on the pair route", gd.public, gd.request(limit+"x", chunked), http.StatusRequestEntityTooLarge)
}

// EDGE-06: 20 failed sign-ins (tokens or pairing codes) within an hour send one notify to wai,
// "wai-remoto: 20 failed sign-ins in the last hour"; more in that hour send none.
func TestTwentyFailuresNotifyOnce(t *testing.T) {
	h := startHost(t)
	h.initialize(t.TempDir())
	h.next()
	h.next()
	gd := newGuarded(t, h.clock, h.s.notify)
	bad := func(n int) {
		for range n {
			gd.expect(t, "a bad token", gd.route, gd.request("", func(r *http.Request) { r.Header.Set("Authorization", "Bearer nope") }), http.StatusUnauthorized)
		}
	}
	// nothing: the server sent wai nothing since; a request it answers marks the place.
	nothing := func(when string) {
		t.Helper()
		h.send(`{"jsonrpc":"2.0","id":99,"method":"marker"}`)
		if m := h.next(); m["id"] != float64(99) {
			t.Errorf("%s: want no message, got %v", when, m)
		}
	}
	notified := func(when string) {
		t.Helper()
		m := h.next()
		p, _ := m["params"].(map[string]any)
		if m["method"] != "notify" || m["id"] != nil || p["title"] != "wai-remoto: 20 failed sign-ins in the last hour" {
			t.Errorf("%s: want the notify, got %v", when, m)
		}
	}

	bad(19)
	nothing("19 failures")
	gd.g.fail() // a refused pairing code, as the pair route counts it
	notified("the 20th failure")
	h.advance(30 * time.Minute)
	bad(30)
	nothing("30 more within the hour")

	// An hour and a minute after the notify, the 30 from 31 minutes ago and one more are 20 or
	// more in the last hour: a new hour's notify.
	h.advance(31 * time.Minute)
	bad(1)
	notified("the next hour")

	// A slow trickle never reaches 20 in an hour.
	h.advance(2 * time.Hour)
	for range 25 {
		bad(1)
		h.advance(4 * time.Minute)
	}
	nothing("25 failures, one every 4 minutes")
}

// EDGE-09: every response, a refusal or the route's, carries Cache-Control: no-store.
func TestResponsesAreNoStore(t *testing.T) {
	gd := newGuarded(t, fixedNow, func(string) {})
	cases := []struct {
		what string
		h    http.Handler
		edit func(*http.Request)
	}{
		{"200", gd.route, nil},
		{"200 on the pair route", gd.public, func(r *http.Request) { r.Header.Del("Authorization") }},
		{"401", gd.route, func(r *http.Request) { r.Header.Del("Authorization") }},
		{"403", gd.route, func(r *http.Request) { r.Header.Set("Origin", "https://evil.example") }},
		{"421", gd.route, func(r *http.Request) { r.Host = "evil.example" }},
		{"413", gd.route, func(r *http.Request) { r.Body = io.NopCloser(strings.NewReader(strings.Repeat("x", 64*1024+1))) }},
	}
	for _, c := range cases {
		w := serveTo(c.h, gd.request("", c.edit))
		if got := w.Header().Values("Cache-Control"); len(got) != 1 || got[0] != "no-store" {
			t.Errorf("%s (status %d): Cache-Control %q", c.what, w.Code, got)
		}
	}
}
