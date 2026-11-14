package main

import (
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"net/http"
	"strings"
	"time"
)

type Server struct {
	store *Store
	gh    *GitHub
	cfg   Config
	now   func() time.Time
}

func writeJSON(w http.ResponseWriter, code int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	json.NewEncoder(w).Encode(v)
}

func writeErr(w http.ResponseWriter, code int, msg string) {
	writeJSON(w, code, map[string]string{"error": msg})
}

func (s *Server) Routes() *http.ServeMux {
	mux := http.NewServeMux()
	mux.HandleFunc("POST /auth/device", s.deviceStart)
	mux.HandleFunc("POST /auth/device/poll", s.devicePoll)
	mux.HandleFunc("POST /auth/signout", s.signout)
	return mux
}

func bearer(r *http.Request) (string, bool) {
	t, ok := strings.CutPrefix(r.Header.Get("Authorization"), "Bearer ")
	return t, ok && t != ""
}

func (s *Server) authed(r *http.Request) (User, bool) {
	t, ok := bearer(r)
	if !ok {
		return User{}, false
	}
	return s.store.TokenUser(sha256.Sum256([]byte(t)))
}

func (s *Server) deviceStart(w http.ResponseWriter, r *http.Request) {
	ds, err := s.gh.DeviceCode(r.Context())
	if err != nil {
		log.Printf("auth: device code: %v", err)
		writeErr(w, http.StatusBadGateway, "GitHub sign-in is unavailable")
		return
	}
	writeJSON(w, http.StatusOK, ds)
}

func (s *Server) devicePoll(w http.ResponseWriter, r *http.Request) {
	var body struct {
		DeviceCode string `json:"device_code"`
	}
	if json.NewDecoder(http.MaxBytesReader(w, r.Body, 4096)).Decode(&body) != nil || body.DeviceCode == "" {
		writeErr(w, http.StatusBadRequest, "device_code is required")
		return
	}
	ghTok, err := s.gh.PollToken(r.Context(), body.DeviceCode)
	if errors.Is(err, ErrPending) {
		writeErr(w, http.StatusPreconditionRequired, "pending")
		return
	}
	if err != nil {
		log.Printf("auth: poll: %v", err)
		writeErr(w, http.StatusBadRequest, "sign-in failed or expired")
		return
	}
	id, login, err := s.gh.User(r.Context(), ghTok)
	if err != nil {
		log.Printf("auth: user: %v", err)
		writeErr(w, http.StatusBadGateway, "GitHub sign-in is unavailable")
		return
	}
	member, err := s.gh.IsMember(r.Context(), login)
	if err != nil {
		log.Printf("auth: membership of %s: %v", login, err)
		writeErr(w, http.StatusBadGateway, "GitHub sign-in is unavailable")
		return
	}
	if !member {
		log.Printf("auth: refused %s (%d): not in %s", login, id, s.cfg.Org)
		writeErr(w, http.StatusForbidden, fmt.Sprintf("Only members of %s can use help requests", s.cfg.Org))
		return
	}
	raw := make([]byte, 32)
	if _, err := rand.Read(raw); err != nil {
		writeErr(w, http.StatusInternalServerError, "internal error")
		return
	}
	tok := base64.RawURLEncoding.EncodeToString(raw)
	s.store.PutUser(id, login)
	if err := s.store.AddToken(sha256.Sum256([]byte(tok)), id); err != nil {
		log.Printf("auth: store token for %s: %v", login, err)
		writeErr(w, http.StatusInternalServerError, "internal error")
		return
	}
	log.Printf("auth: signed in %s (%d)", login, id)
	writeJSON(w, http.StatusOK, map[string]string{"token": tok, "login": login})
}

func (s *Server) signout(w http.ResponseWriter, r *http.Request) {
	u, ok := s.authed(r)
	if !ok {
		writeErr(w, http.StatusUnauthorized, "sign in first")
		return
	}
	t, _ := bearer(r)
	if err := s.store.DropToken(sha256.Sum256([]byte(t))); err != nil {
		log.Printf("auth: drop token for %s: %v", u.Login, err)
		writeErr(w, http.StatusInternalServerError, "internal error")
		return
	}
	w.WriteHeader(http.StatusNoContent)
}
