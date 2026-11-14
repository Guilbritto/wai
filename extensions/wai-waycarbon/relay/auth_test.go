package main

import (
	"bytes"
	"context"
	"encoding/json"
	"log"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func call(h http.Handler, method, path, bearer, body string) *httptest.ResponseRecorder {
	r := httptest.NewRequest(method, path, strings.NewReader(body))
	if bearer != "" {
		r.Header.Set("Authorization", "Bearer "+bearer)
	}
	w := httptest.NewRecorder()
	h.ServeHTTP(w, r)
	return w
}

func routesWithWhoami(s *Server) http.Handler {
	mux := s.Routes()
	mux.HandleFunc("GET /whoami", func(w http.ResponseWriter, r *http.Request) {
		u, ok := s.authed(r)
		if !ok {
			w.WriteHeader(http.StatusUnauthorized)
			return
		}
		w.Write([]byte(u.Login))
	})
	return mux
}

func TestDeviceSignIn(t *testing.T) {
	s := newTestServer(t, newFakeGitHub(t))
	h := routesWithWhoami(s)

	w := call(h, "POST", "/auth/device", "", "")
	var ds DeviceStart
	if w.Code != 200 || json.Unmarshal(w.Body.Bytes(), &ds) != nil || ds.DeviceCode != fakeDevice || ds.UserCode != "ABCD-1234" || ds.Interval != 5 {
		t.Fatalf("device: %d %s", w.Code, w.Body)
	}
	body := `{"device_code":"` + ds.DeviceCode + `"}`
	if w = call(h, "POST", "/auth/device/poll", "", body); w.Code != 428 || strings.TrimSpace(w.Body.String()) != `{"error":"pending"}` {
		t.Fatalf("first poll: %d %s", w.Code, w.Body)
	}
	w = call(h, "POST", "/auth/device/poll", "", body)
	var res struct{ Token, Login string }
	if w.Code != 200 || json.Unmarshal(w.Body.Bytes(), &res) != nil || res.Login != "ana" || len(res.Token) != 43 {
		t.Fatalf("second poll: %d %s", w.Code, w.Body)
	}
	if w = call(h, "GET", "/whoami", res.Token, ""); w.Code != 200 || w.Body.String() != "ana" {
		t.Fatalf("whoami: %d %s", w.Code, w.Body)
	}
	if w = call(h, "POST", "/auth/signout", res.Token, ""); w.Code != 204 {
		t.Fatalf("signout: %d", w.Code)
	}
	if w = call(h, "GET", "/whoami", res.Token, ""); w.Code != 401 {
		t.Fatalf("after signout: %d", w.Code)
	}
	if w = call(h, "POST", "/auth/signout", "", ""); w.Code != 401 {
		t.Fatalf("signout without token: %d", w.Code)
	}
}

func TestNonMemberIsRefused(t *testing.T) {
	f := newFakeGitHub(t)
	f.members = map[string]bool{}
	s := newTestServer(t, f)
	h := s.Routes()
	body := `{"device_code":"` + fakeDevice + `"}`
	call(h, "POST", "/auth/device/poll", "", body)
	w := call(h, "POST", "/auth/device/poll", "", body)
	want := `{"error":"Only members of waycarbon can use help requests"}`
	if w.Code != 403 || strings.TrimSpace(w.Body.String()) != want {
		t.Fatalf("got %d %s", w.Code, w.Body)
	}
	if strings.Contains(w.Body.String(), "token") && !strings.Contains(w.Body.String(), "error") {
		t.Fatal("token issued")
	}
	if len(s.store.Users()) != 0 {
		t.Fatal("user stored")
	}
}

func TestInstallationTokenIsCached(t *testing.T) {
	f := newFakeGitHub(t)
	s := newTestServer(t, f)
	for i := 0; i < 2; i++ {
		ok, err := s.gh.IsMember(context.Background(), "ana")
		if err != nil || !ok {
			t.Fatalf("ana: %v %v", ok, err)
		}
	}
	if ok, err := s.gh.IsMember(context.Background(), "zed"); err != nil || ok {
		t.Fatalf("zed: %v %v", ok, err)
	}
	if f.exchanges != 1 {
		t.Fatalf("exchanges = %d", f.exchanges)
	}
	if f.jwtErr != "" {
		t.Fatal(f.jwtErr)
	}
}

func TestTokenNotLogged(t *testing.T) {
	var buf bytes.Buffer
	old := log.Writer()
	log.SetOutput(&buf)
	defer log.SetOutput(old)
	s := newTestServer(t, newFakeGitHub(t))
	h := s.Routes()
	body := `{"device_code":"` + fakeDevice + `"}`
	call(h, "POST", "/auth/device", "", "")
	call(h, "POST", "/auth/device/poll", "", body)
	w := call(h, "POST", "/auth/device/poll", "", body)
	var res struct{ Token string }
	json.Unmarshal(w.Body.Bytes(), &res)
	if res.Token == "" {
		t.Fatalf("no token: %s", w.Body)
	}
	for _, secret := range []string{res.Token, fakeGHToken, fakeDevice, "ghs_installation"} {
		if strings.Contains(buf.String(), secret) {
			t.Fatalf("log leaks %q: %s", secret, buf.String())
		}
	}
}

func TestUserSaveFailureIsA500WithoutToken(t *testing.T) {
	s := newTestServer(t, newFakeGitHub(t))
	st, err := OpenStore(filepath.Join(t.TempDir(), "missing", "relay.json"), time.Now)
	if err != nil {
		t.Fatal(err)
	}
	s.store = st
	h := s.Routes()
	body := `{"device_code":"` + fakeDevice + `"}`
	call(h, "POST", "/auth/device/poll", "", body)
	w := call(h, "POST", "/auth/device/poll", "", body)
	if w.Code != 500 || strings.TrimSpace(w.Body.String()) != `{"error":"internal error"}` {
		t.Fatalf("got %d %s", w.Code, w.Body)
	}
	if len(st.Users()) != 0 {
		t.Fatal("user kept after failed save")
	}
}
