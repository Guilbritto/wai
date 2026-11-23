package main

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"sync"
	"time"
)

const (
	maxBody    = 64 << 10 // EDGE-04
	failLimit  = 20       // EDGE-06: this many failed sign-ins in failWindow send one notify
	failWindow = time.Hour
	failNotice = "wai-remoto: 20 failed sign-ins in the last hour"
)

// guard checks every request before a route sees it, in this order: Origin (403), Host (421),
// body size (413), then the device's Bearer token (401).
type guard struct {
	store  *store
	notify func(title string)

	mu       sync.Mutex
	fails    []time.Time // the last failLimit failures
	notified time.Time
}

type deviceKey struct{}

// deviceOf is the device whose token authenticated r.
func deviceOf(r *http.Request) Device {
	d, _ := r.Context().Value(deviceKey{}).(Device)
	return d
}

func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	json.NewEncoder(w).Encode(v)
}

func refuse(w http.ResponseWriter, status int, why string) {
	writeJSON(w, status, node{"error": why})
}

// hostOK: the tunnel URL's host, or 127.0.0.1:<port> (tailscale serve may pass either).
func (g *guard) hostOK(host string) bool {
	port, tunnel := g.store.settings()
	if strings.EqualFold(host, "127.0.0.1:"+strconv.Itoa(port)) {
		return true
	}
	u, err := url.Parse(tunnel)
	return err == nil && u.Host != "" && strings.EqualFold(dropDefaultPort(host, u.Scheme), dropDefaultPort(u.Host, u.Scheme))
}

// dropDefaultPort strips the scheme's default port: a client sends `Host: name` for
// `https://name:443`.
func dropDefaultPort(host, scheme string) string {
	def := map[string]string{"https": ":443", "http": ":80"}[strings.ToLower(scheme)]
	if def != "" && strings.HasSuffix(host, def) {
		return strings.TrimSuffix(host, def)
	}
	return host
}

// wrap guards next; with needToken false (the pair route) it skips the token only.
func (g *guard) wrap(next http.Handler, needToken bool) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store") // EDGE-09
		if _, ok := r.Header["Origin"]; ok {
			refuse(w, http.StatusForbidden, "Requests from a web page are refused")
			return
		}
		if !g.hostOK(r.Host) {
			refuse(w, http.StatusMisdirectedRequest, "Unknown host "+r.Host)
			return
		}
		if r.ContentLength > maxBody {
			refuse(w, http.StatusRequestEntityTooLarge, "The request body is over 64 KB")
			return
		}
		body, err := io.ReadAll(io.LimitReader(r.Body, maxBody+1))
		if err != nil {
			refuse(w, http.StatusBadRequest, "Cannot read the request body")
			return
		}
		if len(body) > maxBody {
			refuse(w, http.StatusRequestEntityTooLarge, "The request body is over 64 KB")
			return
		}
		r.Body = io.NopCloser(bytes.NewReader(body))
		if needToken {
			scheme, token, _ := strings.Cut(r.Header.Get("Authorization"), " ")
			if !strings.EqualFold(scheme, "Bearer") {
				token = ""
			}
			d, ok := g.store.lookup(token)
			if !ok {
				g.fail()
				refuse(w, http.StatusUnauthorized, "Missing or unknown token")
				return
			}
			r = r.WithContext(context.WithValue(r.Context(), deviceKey{}, d))
		}
		next.ServeHTTP(w, r)
	})
}

// fail counts a refused token or pairing code. The failLimit-th within failWindow notifies the
// user, at most once per failWindow.
func (g *guard) fail() {
	now := g.store.now()
	g.mu.Lock()
	kept := g.fails[:0]
	for _, f := range g.fails {
		if now.Sub(f) < failWindow {
			kept = append(kept, f)
		}
	}
	g.fails = append(kept, now)
	if len(g.fails) > failLimit {
		g.fails = g.fails[1:]
	}
	send := len(g.fails) >= failLimit && (g.notified.IsZero() || now.Sub(g.notified) >= failWindow)
	if send {
		g.notified = now
	}
	g.mu.Unlock()
	if send {
		g.notify(failNotice)
	}
}
