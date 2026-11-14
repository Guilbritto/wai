package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"net/http"
	"sort"
	"time"

	"wai-waycarbon/helpapi"
)

const pingEvery = 30 * time.Second

func (s *Server) apiRoutes(mux *http.ServeMux) {
	mux.HandleFunc("GET /events", s.events)
	mux.HandleFunc("POST /requests", s.createRequest)
	mux.HandleFunc("POST /requests/{id}/{action}", s.actOnRequest)
}

// member authenticates the caller and confirms they are still in the org.
func (s *Server) member(w http.ResponseWriter, r *http.Request) (User, bool) {
	u, ok := s.authed(r)
	if !ok {
		writeErr(w, http.StatusUnauthorized, "sign in first")
		return User{}, false
	}
	in, err := s.gh.IsMember(r.Context(), u.Login)
	if err != nil {
		log.Printf("api: membership of %s: %v", u.Login, err)
		writeErr(w, http.StatusBadGateway, "GitHub is unavailable")
		return User{}, false
	}
	if !in {
		writeErr(w, http.StatusForbidden, fmt.Sprintf("Only members of %s can use help requests", s.cfg.Org))
		return User{}, false
	}
	return u, true
}

func (s *Server) people(self int64) []helpapi.Person {
	out := []helpapi.Person{}
	for _, u := range s.store.Users() {
		if u.ID != self {
			out = append(out, helpapi.Person{Login: u.Login, Online: s.hub.Online(u.ID)})
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Login < out[j].Login })
	return out
}

// broadcastPeople tells everyone else that changed went online or offline.
func (s *Server) broadcastPeople(changed int64) {
	for _, id := range s.hub.Connected() {
		if id != changed {
			s.hub.Send(id, helpapi.EvPeople, s.people(id))
		}
	}
}

func (s *Server) events(w http.ResponseWriter, r *http.Request) {
	u, ok := s.member(w, r)
	if !ok {
		return
	}
	fl, ok := w.(http.Flusher)
	if !ok {
		writeErr(w, http.StatusInternalServerError, "streaming unsupported")
		return
	}
	ch, leave, ok := s.hub.Join(u.ID)
	if !ok {
		writeErr(w, http.StatusTooManyRequests, "too many open streams")
		return
	}
	defer leave()
	h := w.Header()
	h.Set("Content-Type", "text/event-stream")
	h.Set("Cache-Control", "no-cache")
	w.WriteHeader(http.StatusOK)
	open := s.store.Open(u.ID)
	if open == nil {
		open = []helpapi.Request{}
	}
	for _, f := range [][]byte{frame(helpapi.EvSync, open), frame(helpapi.EvPeople, s.people(u.ID))} {
		w.Write(f)
	}
	fl.Flush()
	tick := time.NewTicker(pingEvery)
	defer tick.Stop()
	for {
		select {
		case <-r.Context().Done():
			return
		case msg, ok := <-ch:
			if !ok {
				return
			}
			w.Write(msg)
		case <-tick.C:
			io.WriteString(w, ": ping\n\n")
		}
		fl.Flush()
	}
}

func readBody(w http.ResponseWriter, r *http.Request, v any) bool {
	err := json.NewDecoder(http.MaxBytesReader(w, r.Body, helpapi.MaxBody)).Decode(v)
	var tooBig *http.MaxBytesError
	switch {
	case errors.As(err, &tooBig):
		writeErr(w, http.StatusRequestEntityTooLarge, "request body too large")
	case err != nil && !errors.Is(err, io.EOF):
		writeErr(w, http.StatusBadRequest, "invalid JSON body")
	default:
		return true
	}
	return false
}

func storeStatus(err error) int {
	switch {
	case errors.Is(err, ErrNotFound):
		return http.StatusNotFound
	case errors.Is(err, ErrForbidden):
		return http.StatusForbidden
	case errors.Is(err, ErrBadState):
		return http.StatusConflict
	case errors.Is(err, ErrRateLimited), errors.Is(err, ErrRecipientFull):
		return http.StatusTooManyRequests
	case errors.Is(err, ErrTooLong), errors.Is(err, ErrSelf):
		return http.StatusBadRequest
	}
	return http.StatusInternalServerError
}

func (s *Server) createRequest(w http.ResponseWriter, r *http.Request) {
	u, ok := s.member(w, r)
	if !ok {
		return
	}
	var body struct {
		To      string          `json:"to"`
		Project helpapi.Project `json:"project"`
		Text    string          `json:"text"`
	}
	if !readBody(w, r, &body) {
		return
	}
	if body.Text == "" {
		writeErr(w, http.StatusBadRequest, "text is required")
		return
	}
	to, ok := s.store.UserByLogin(body.To)
	if !ok {
		writeErr(w, http.StatusNotFound, fmt.Sprintf("@%s has not signed in to help requests", body.To))
		return
	}
	req, err := s.store.Create(u, to, body.Project, body.Text)
	if err != nil {
		code := storeStatus(err)
		msg := err.Error()
		if code == http.StatusInternalServerError {
			log.Printf("api: create: %v", err)
			msg = "internal error"
		}
		writeErr(w, code, msg)
		return
	}
	s.hub.Send(req.FromID, helpapi.EvNew, req)
	s.hub.Send(req.ToID, helpapi.EvNew, req)
	writeJSON(w, http.StatusCreated, req)
}

func (s *Server) actOnRequest(w http.ResponseWriter, r *http.Request) {
	u, ok := s.member(w, r)
	if !ok {
		return
	}
	action := r.PathValue("action")
	switch action {
	case "cancel", "accept", "decline", "reply":
	default:
		writeErr(w, http.StatusNotFound, "unknown action")
		return
	}
	var body struct {
		Text string `json:"text"`
	}
	if !readBody(w, r, &body) {
		return
	}
	req, err := s.store.Act(u.ID, r.PathValue("id"), action, body.Text)
	if err != nil {
		code := storeStatus(err)
		msg := err.Error()
		if code == http.StatusInternalServerError {
			log.Printf("api: %s: %v", action, err)
			msg = "internal error"
		}
		writeErr(w, code, msg)
		return
	}
	s.sendUpdate(req)
	writeJSON(w, http.StatusOK, req)
}

func (s *Server) sendUpdate(req helpapi.Request) {
	up := helpapi.Update{ID: req.ID, State: req.State, Reply: req.Reply, Reason: req.Reason}
	s.hub.Send(req.FromID, helpapi.EvUpdated, up)
	s.hub.Send(req.ToID, helpapi.EvUpdated, up)
}

func (s *Server) sweepOnce() {
	for _, req := range s.store.Sweep() {
		s.sendUpdate(req)
	}
}

// checkMembersOnce signs out connected users who left the org.
func (s *Server) checkMembersOnce(ctx context.Context) {
	logins := map[int64]string{}
	for _, u := range s.store.Users() {
		logins[u.ID] = u.Login
	}
	for _, id := range s.hub.Connected() {
		in, err := s.gh.IsMember(ctx, logins[id])
		if err != nil {
			log.Printf("api: membership check: %v", err)
			continue
		}
		if in {
			continue
		}
		if err := s.store.DropUserTokens(id); err != nil {
			log.Printf("api: drop tokens of %s: %v", logins[id], err)
		}
		s.hub.Kick(id)
	}
}

// Loop runs the minute sweep and the hourly membership check until ctx ends.
func (s *Server) Loop(ctx context.Context) {
	minute, hour := time.NewTicker(time.Minute), time.NewTicker(time.Hour)
	defer minute.Stop()
	defer hour.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-minute.C:
			s.sweepOnce()
		case <-hour.C:
			s.checkMembersOnce(ctx)
		}
	}
}
