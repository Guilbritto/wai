package main

import (
	"encoding/json"
	"net/http"
	"strconv"
)

// pathRequest is the {request} of an answer route, a permission or question id; false (after a
// 400) when it is not a whole number.
func pathRequest(w http.ResponseWriter, r *http.Request) (int64, bool) {
	id, err := strconv.ParseInt(r.PathValue("request"), 10, 64)
	if err != nil || id < 0 {
		refuse(w, http.StatusBadRequest, "The request id must be a whole number")
		return 0, false
	}
	return id, true
}

// POST /v1/sessions/{agent}/{session}/permissions/{request} {option}: 204, or 409 when the request
// is already answered (REMO-12).
func (s *server) answerPermission(w http.ResponseWriter, r *http.Request) {
	id, ok := pathRequest(w, r)
	if !ok {
		return
	}
	var body struct{ Option string }
	if !decode(w, r, &body) {
		return
	}
	if body.Option == "" {
		refuse(w, http.StatusBadRequest, "Choose an option")
		return
	}
	k := pathSession(r)
	if _, err := s.request("permission/answer", node{"agent": k.agent, "session": k.session, "request": id, "option": body.Option}); err != nil {
		hostRefusal(w, err)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// POST /v1/sessions/{agent}/{session}/questions/{request} {answer, values?}: 204, 409 when the
// question is already answered, 422 {field} when a value does not fit (QST-02, QST-05).
func (s *server) answerQuestion(w http.ResponseWriter, r *http.Request) {
	id, ok := pathRequest(w, r)
	if !ok {
		return
	}
	var body struct {
		Answer string
		Values json.RawMessage
	}
	if !decode(w, r, &body) {
		return
	}
	k := pathSession(r)
	params := node{"agent": k.agent, "session": k.session, "request": id, "answer": body.Answer}
	if len(body.Values) > 0 && string(body.Values) != "null" {
		params["values"] = body.Values
	}
	if _, err := s.request("question/answer", params); err != nil {
		hostRefusal(w, err)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}
