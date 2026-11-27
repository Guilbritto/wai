package main

import (
	"net/http"
	"testing"
)

// REMO-12: a tapped option is forwarded to wai as permission/answer.
func TestPermissionAnswerForwards(t *testing.T) {
	a := startAPI(t)
	ch := a.do(a.req("POST", "/v1/sessions/claude/s1/permissions/7", `{"option":"allow"}`))
	id, p := a.h.expectCall("permission/answer")
	params(t, "permission/answer", p, `{"agent":"claude","session":"s1","request":7,"option":"allow"}`)
	a.h.reply(id, `null`)
	status(t, "answer", a.wait(ch), http.StatusNoContent, "")

	// An option the request does not offer: wai's message, 400.
	ch = a.do(a.req("POST", "/v1/sessions/claude/s1/permissions/7", `{"option":"maybe"}`))
	id, _ = a.h.expectCall("permission/answer")
	a.h.replyError(id, -32000, "No option maybe in request 7")
	status(t, "unknown option", a.wait(ch), http.StatusBadRequest, "No option maybe in request 7")

	// A session that is not open: 404.
	ch = a.do(a.req("POST", "/v1/sessions/claude/s9/permissions/7", `{"option":"allow"}`))
	id, _ = a.h.expectCall("permission/answer")
	a.h.replyError(id, -32000, "No open session claude s9")
	status(t, "closed session", a.wait(ch), http.StatusNotFound, "No open session claude s9")

	// A bad request id or no option never reaches wai.
	status(t, "request x", a.wait(a.do(a.req("POST", "/v1/sessions/claude/s1/permissions/x", `{"option":"allow"}`))), http.StatusBadRequest, "The request id must be a whole number")
	status(t, "no option", a.wait(a.do(a.req("POST", "/v1/sessions/claude/s1/permissions/7", `{}`))), http.StatusBadRequest, "Choose an option")
	a.h.quiet("bad permission answers")
}

// HOST-07 through wai-remoto: a permission already answered (in wai, say) answers 409.
func TestAnsweredPermissionIs409(t *testing.T) {
	a := startAPI(t)
	ch := a.do(a.req("POST", "/v1/sessions/claude/s1/permissions/7", `{"option":"allow"}`))
	id, _ := a.h.expectCall("permission/answer")
	a.h.replyError(id, -32000, "Request 7 is already answered or unknown")
	status(t, "answered", a.wait(ch), http.StatusConflict, "Request 7 is already answered or unknown")
}

// QST-02, QST-08: Submit, Decline and Cancel are forwarded to wai as question/answer, accept with
// the values.
func TestQuestionAnswerForwards(t *testing.T) {
	a := startAPI(t)
	ch := a.do(a.req("POST", "/v1/sessions/claude/s1/questions/3", `{"answer":"accept","values":{"branch":"main","tags":["a","b"],"force":true,"n":2,"why":"ok"}}`))
	id, p := a.h.expectCall("question/answer")
	params(t, "accept", p, `{"agent":"claude","session":"s1","request":3,"answer":"accept","values":{"branch":"main","tags":["a","b"],"force":true,"n":2,"why":"ok"}}`)
	a.h.reply(id, `null`)
	status(t, "accept", a.wait(ch), http.StatusNoContent, "")

	for _, answer := range []string{"decline", "cancel"} {
		ch = a.do(a.req("POST", "/v1/sessions/claude/s1/questions/4", `{"answer":"`+answer+`"}`))
		id, p = a.h.expectCall("question/answer")
		params(t, answer, p, `{"agent":"claude","session":"s1","request":4,"answer":"`+answer+`"}`)
		a.h.reply(id, `null`)
		status(t, answer, a.wait(ch), http.StatusNoContent, "")
	}

	// Answered already: 409.
	ch = a.do(a.req("POST", "/v1/sessions/claude/s1/questions/3", `{"answer":"cancel"}`))
	id, _ = a.h.expectCall("question/answer")
	a.h.replyError(id, -32000, "Request 3 is already answered or unknown")
	status(t, "answered", a.wait(ch), http.StatusConflict, "Request 3 is already answered or unknown")

	status(t, "request -1", a.wait(a.do(a.req("POST", "/v1/sessions/claude/s1/questions/-1", `{"answer":"cancel"}`))), http.StatusBadRequest, "The request id must be a whole number")
	a.h.quiet("a bad question id")
}

// QST-05 through wai-remoto: a value wai refuses answers 422 naming the field.
func TestInvalidQuestionAnswerIs422(t *testing.T) {
	a := startAPI(t)
	for _, c := range []struct{ field, msg string }{
		{"branch", "Field branch: a value is required"},
		{"n", "Field n: must be a number"},
		{"tags", "Field tags: dev is not a choice"},
	} {
		ch := a.do(a.req("POST", "/v1/sessions/claude/s1/questions/3", `{"answer":"accept","values":{}}`))
		id, _ := a.h.expectCall("question/answer")
		a.h.replyError(id, -32000, c.msg)
		w := a.wait(ch)
		status(t, c.msg, w, http.StatusUnprocessableEntity, c.msg)
		jsonEqual(t, c.msg, w.Body.String(), `{"error":`+quote(c.msg)+`,"field":`+quote(c.field)+`}`)
	}
}
