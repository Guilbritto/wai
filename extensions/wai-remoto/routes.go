package main

import (
	"encoding/json"
	"errors"
	"net/http"
	"strconv"
	"strings"
	"time"
)

const (
	// REMO-14: a device may send this many prompts in promptWindow.
	promptLimit  = 30
	promptWindow = time.Minute
)

// sessionKey names a session in wai: its agent and its id.
type sessionKey struct{ agent, session string }

// handler is the HTTP API. Every route goes through the guard; only the pair route takes no
// token, and an unknown route answers 404 only to a paired device.
func (s *server) handler() http.Handler {
	auth := func(h http.HandlerFunc) http.Handler { return s.guard.wrap(h, true) }
	mux := http.NewServeMux()
	mux.Handle("POST /v1/pair", s.guard.wrap(http.HandlerFunc(s.pair), false))
	mux.Handle("POST /v1/unpair", auth(s.unpair))
	mux.Handle("GET /v1/sessions", auth(s.listSessions))
	mux.Handle("GET /v1/sessions/{agent}/{session}", auth(s.readSession))
	mux.Handle("POST /v1/sessions/{agent}/{session}/prompt", auth(s.prompt))
	mux.Handle("POST /v1/sessions/{agent}/{session}/cancel", auth(s.cancel))
	mux.Handle("POST /v1/sessions/{agent}/{session}/permissions/{request}", auth(s.answerPermission))
	mux.Handle("POST /v1/sessions/{agent}/{session}/questions/{request}", auth(s.answerQuestion))
	mux.Handle("/", auth(func(w http.ResponseWriter, r *http.Request) {
		refuse(w, http.StatusNotFound, "No such route")
	}))
	return mux
}

// decode reads a JSON body into v, or answers 400.
func decode(w http.ResponseWriter, r *http.Request, v any) bool {
	if err := json.NewDecoder(r.Body).Decode(v); err != nil {
		refuse(w, http.StatusBadRequest, "The body must be a JSON object")
		return false
	}
	return true
}

// writeRaw answers 200 with wai's result as it came.
func writeRaw(w http.ResponseWriter, result json.RawMessage) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusOK)
	w.Write(result)
}

// hostRefusal answers a failed call to wai with the status its message means.
func hostRefusal(w http.ResponseWriter, err error) {
	var he *rpcError
	if !errors.As(err, &he) {
		refuse(w, http.StatusGatewayTimeout, err.Error())
		return
	}
	msg := he.Message
	switch {
	case strings.HasPrefix(msg, "No open session "):
		refuse(w, http.StatusNotFound, msg)
	case strings.HasPrefix(msg, "Request ") && strings.HasSuffix(msg, " is already answered or unknown"):
		refuse(w, http.StatusConflict, msg)
	case strings.HasPrefix(msg, "Field "):
		field, _, _ := strings.Cut(strings.TrimPrefix(msg, "Field "), ": ")
		writeJSON(w, http.StatusUnprocessableEntity, node{"error": msg, "field": field})
	case he.Code == -32000 || he.Code == -32602: // wai refused what the device asked
		refuse(w, http.StatusBadRequest, msg)
	default: // wai refused wai-remoto itself: a permission not granted, an older wai
		refuse(w, http.StatusBadGateway, msg)
	}
}

func (s *server) setState(k sessionKey, state string) {
	s.smu.Lock()
	defer s.smu.Unlock()
	s.states[k] = state
}

// sessions calls sessions/list; its answer refreshes the state of every session.
func (s *server) sessions() (json.RawMessage, error) {
	res, err := s.request("sessions/list", nil)
	if err != nil {
		return nil, err
	}
	var list []struct{ Agent, Session, State string }
	json.Unmarshal(res, &list)
	s.smu.Lock()
	clear(s.states)
	for _, x := range list {
		s.states[sessionKey{x.Agent, x.Session}] = x.State
	}
	s.smu.Unlock()
	if list == nil {
		return json.RawMessage("[]"), nil
	}
	return res, nil
}

// state is a session's last known state, asking wai when there is none; "" when it is not open.
func (s *server) state(k sessionKey) (string, error) {
	s.smu.Lock()
	st, ok := s.states[k]
	s.smu.Unlock()
	if ok {
		return st, nil
	}
	if _, err := s.sessions(); err != nil {
		return "", err
	}
	s.smu.Lock()
	defer s.smu.Unlock()
	return s.states[k], nil
}

// allowPrompt counts a prompt from device; false when it already sent promptLimit in
// promptWindow (REMO-14). A refused prompt is not counted.
func (s *server) allowPrompt(device string) bool {
	now := s.now()
	s.smu.Lock()
	defer s.smu.Unlock()
	kept := s.prompts[device][:0]
	for _, t := range s.prompts[device] {
		if now.Sub(t) < promptWindow {
			kept = append(kept, t)
		}
	}
	if len(kept) >= promptLimit {
		s.prompts[device] = kept
		return false
	}
	s.prompts[device] = append(kept, now)
	return true
}

func pathSession(r *http.Request) sessionKey {
	return sessionKey{r.PathValue("agent"), r.PathValue("session")}
}

// POST /v1/pair {code, device}: a new device token, or 401 (PAIR-09, PAIR-10).
func (s *server) pair(w http.ResponseWriter, r *http.Request) {
	var body struct{ Code, Device string }
	if !decode(w, r, &body) {
		return
	}
	if strings.TrimSpace(body.Device) == "" {
		refuse(w, http.StatusBadRequest, "The device name is empty")
		return
	}
	token, err := s.pairing.redeem(body.Code, strings.TrimSpace(body.Device))
	if errors.Is(err, errBadCode) {
		s.guard.fail()
		refuse(w, http.StatusUnauthorized, err.Error())
		return
	}
	if err != nil {
		refuse(w, http.StatusInternalServerError, "Cannot save the device: "+err.Error())
		return
	}
	writeJSON(w, http.StatusOK, node{"token": token})
}

// POST /v1/unpair: the calling device's token stops working (DEV-06).
func (s *server) unpair(w http.ResponseWriter, r *http.Request) {
	s.store.revoke(deviceOf(r).ID)
	w.WriteHeader(http.StatusNoContent)
}

// GET /v1/sessions: wai's open sessions (REMO-01).
func (s *server) listSessions(w http.ResponseWriter, r *http.Request) {
	res, err := s.sessions()
	if err != nil {
		hostRefusal(w, err)
		return
	}
	writeRaw(w, res)
}

// GET /v1/sessions/{agent}/{session}?from=&limit=: a page of the transcript.
func (s *server) readSession(w http.ResponseWriter, r *http.Request) {
	k := pathSession(r)
	params := node{"agent": k.agent, "session": k.session}
	for _, name := range []string{"from", "limit"} {
		v := r.URL.Query().Get(name)
		if v == "" {
			continue
		}
		n, err := strconv.Atoi(v)
		if err != nil || n < 0 {
			refuse(w, http.StatusBadRequest, name+" must be a whole number")
			return
		}
		params[name] = n
	}
	res, err := s.request("sessions/read", params)
	if err != nil {
		hostRefusal(w, err)
		return
	}
	writeRaw(w, res)
}

// POST /v1/sessions/{agent}/{session}/prompt {text}: 204, 409 unless the session is idle
// (EDGE-08), 429 past the device's limit (REMO-14).
func (s *server) prompt(w http.ResponseWriter, r *http.Request) {
	var body struct{ Text string }
	if !decode(w, r, &body) {
		return
	}
	if strings.TrimSpace(body.Text) == "" {
		refuse(w, http.StatusBadRequest, "The prompt is empty")
		return
	}
	if !s.allowPrompt(deviceOf(r).ID) {
		refuse(w, http.StatusTooManyRequests, "Too many prompts, wait a minute")
		return
	}
	k := pathSession(r)
	state, err := s.state(k)
	if err != nil {
		hostRefusal(w, err)
		return
	}
	switch state {
	case "":
		refuse(w, http.StatusNotFound, "No open session "+k.agent+" "+k.session)
		return
	case "idle":
	default:
		refuse(w, http.StatusConflict, "The session is busy")
		return
	}
	if _, err := s.request("sessions/prompt", node{"agent": k.agent, "session": k.session, "text": body.Text}); err != nil {
		hostRefusal(w, err)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// POST /v1/sessions/{agent}/{session}/cancel: stops the running turn (REMO-10).
func (s *server) cancel(w http.ResponseWriter, r *http.Request) {
	k := pathSession(r)
	if _, err := s.request("sessions/cancel", node{"agent": k.agent, "session": k.session}); err != nil {
		hostRefusal(w, err)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}
