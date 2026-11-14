package main

import (
	"crypto"
	"crypto/rand"
	"crypto/rsa"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

const (
	fakeGHToken = "gho_fake_github_user_token"
	fakeDevice  = "dev-code-123"
)

var (
	testKeyOnce sync.Once
	testKey     *rsa.PrivateKey
)

func sharedKey(t *testing.T) *rsa.PrivateKey {
	t.Helper()
	testKeyOnce.Do(func() {
		k, err := rsa.GenerateKey(rand.Reader, 2048)
		if err != nil {
			panic(err)
		}
		testKey = k
	})
	return testKey
}

type fakeGitHub struct {
	srv       *httptest.Server
	mu        sync.Mutex
	members   map[string]bool
	polls     int
	exchanges int
	jwtErr    string
	expires   time.Time
}

func newFakeGitHub(t *testing.T) *fakeGitHub {
	t.Helper()
	key := sharedKey(t)
	f := &fakeGitHub{members: map[string]bool{"ana": true}, expires: time.Now().Add(time.Hour)}
	mux := http.NewServeMux()
	writeJSON := func(w http.ResponseWriter, v any) {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(v)
	}
	mux.HandleFunc("POST /login/device/code", func(w http.ResponseWriter, r *http.Request) {
		r.ParseForm()
		if r.Form.Get("client_id") != "cid" || r.Header.Get("Accept") != "application/json" {
			http.Error(w, "bad request", 400)
			return
		}
		writeJSON(w, map[string]any{"device_code": fakeDevice, "user_code": "ABCD-1234", "verification_uri": "https://github.com/login/device", "interval": 5})
	})
	mux.HandleFunc("POST /login/oauth/access_token", func(w http.ResponseWriter, r *http.Request) {
		r.ParseForm()
		if r.Form.Get("device_code") != fakeDevice || r.Form.Get("client_id") != "cid" ||
			r.Form.Get("grant_type") != "urn:ietf:params:oauth:grant-type:device_code" {
			writeJSON(w, map[string]any{"error": "bad_verification_code"})
			return
		}
		f.mu.Lock()
		f.polls++
		n := f.polls
		f.mu.Unlock()
		if n == 1 {
			writeJSON(w, map[string]any{"error": "authorization_pending"})
			return
		}
		writeJSON(w, map[string]any{"access_token": fakeGHToken, "token_type": "bearer"})
	})
	mux.HandleFunc("GET /user", func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer "+fakeGHToken {
			http.Error(w, "unauthorized", 401)
			return
		}
		writeJSON(w, map[string]any{"id": 7, "login": "ana"})
	})
	mux.HandleFunc("POST /app/installations/42/access_tokens", func(w http.ResponseWriter, r *http.Request) {
		f.mu.Lock()
		f.exchanges++
		f.mu.Unlock()
		if msg := verifyJWT(strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer "), &key.PublicKey, "99"); msg != "" {
			f.mu.Lock()
			f.jwtErr = msg
			f.mu.Unlock()
			http.Error(w, msg, 401)
			return
		}
		w.WriteHeader(201)
		writeJSON(w, map[string]any{"token": "ghs_installation", "expires_at": f.expires.UTC().Format(time.RFC3339)})
	})
	mux.HandleFunc("GET /orgs/waycarbon/members/{login}", func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer ghs_installation" {
			http.Error(w, "unauthorized", 401)
			return
		}
		f.mu.Lock()
		ok := f.members[r.PathValue("login")]
		f.mu.Unlock()
		if ok {
			w.WriteHeader(204)
		} else {
			w.WriteHeader(404)
		}
	})
	f.srv = httptest.NewServer(mux)
	t.Cleanup(f.srv.Close)
	return f
}

func verifyJWT(tok string, pub *rsa.PublicKey, iss string) string {
	parts := strings.Split(tok, ".")
	if len(parts) != 3 {
		return "not a jwt"
	}
	sig, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		return "bad signature encoding"
	}
	sum := sha256.Sum256([]byte(parts[0] + "." + parts[1]))
	if err := rsa.VerifyPKCS1v15(pub, crypto.SHA256, sum[:], sig); err != nil {
		return "bad signature"
	}
	hb, _ := base64.RawURLEncoding.DecodeString(parts[0])
	cb, _ := base64.RawURLEncoding.DecodeString(parts[1])
	var h struct{ Alg string }
	var c struct {
		Iss      string
		Iat, Exp int64
	}
	if json.Unmarshal(hb, &h) != nil || json.Unmarshal(cb, &c) != nil || h.Alg != "RS256" {
		return "bad header or claims"
	}
	now := time.Now().Unix()
	if c.Iss != iss || c.Iat > now || c.Exp <= now || c.Exp-c.Iat > 600 {
		return "bad claims"
	}
	return ""
}

func newTestServer(t *testing.T, f *fakeGitHub) *Server {
	t.Helper()
	cfg := Config{Org: "waycarbon", ClientID: "cid", AppID: "99", InstallationID: "42", AppKey: sharedKey(t), API: f.srv.URL, Web: f.srv.URL}
	st, err := OpenStore(filepath.Join(t.TempDir(), "relay.json"), time.Now)
	if err != nil {
		t.Fatal(err)
	}
	return &Server{store: st, gh: NewGitHub(cfg, f.srv.Client()), cfg: cfg, now: time.Now}
}
