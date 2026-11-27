package main

import (
	"cmp"
	"encoding/json"
	"fmt"
	"sort"
	"strings"
	"time"
)

const (
	maxWaiting = 5
	maxBubble  = 140
	showGap    = 2 * time.Second
)

// chat is the pet's own background session, where the user's @ messages are answered.
type chat struct {
	agent, session string
	busy           bool
	queue          []string
	from           int    // the session's item count before the turn in flight
	request        string // the permission or question its session waits on
	showAt         time.Time
}

type pendingCall struct {
	method, text string
	retry        bool
}

func (c *chat) own(agent, session *string) bool {
	return c.session != "" && key(agent, session) == c.agent+"\x00"+c.session
}

// request writes a call whose answer handle routes by id through pending.
func (s *server) request(method string, params any, p pendingCall) {
	s.reqID++
	p.method = method
	s.pending[s.reqID] = p
	m := node{"jsonrpc": "2.0", "id": s.reqID, "method": method}
	if params != nil {
		m["params"] = params
	}
	s.write(m)
}

// mentionName is the pet's name as one word of at most 32 characters; "" for an egg.
func mentionName(p *Pet) string {
	if p == nil {
		return ""
	}
	r := []rune(strings.Join(strings.Fields(p.Name), ""))
	return string(r[:min(len(r), 32)])
}

func (s *server) syncMention() {
	name := mentionName(s.st.Pet)
	if s.mention != nil && *s.mention == name {
		return
	}
	s.mention = &name
	names := []node{}
	if name != "" {
		names = append(names, node{"id": "pet", "name": name})
	}
	s.call("mentions/set", node{"names": names})
}

// firstLine is the answer's first line, cut to the bubble's size; cut says the bubble holds less than the answer.
func firstLine(answer string) (text string, cut bool) {
	a := strings.TrimSpace(answer)
	text, _, _ = strings.Cut(a, "\n")
	text = strings.TrimSpace(text)
	if r := []rune(text); len(r) > maxBubble {
		text = string(r[:maxBubble-1]) + "…"
	}
	return text, text != a
}

func (s *server) brief() string {
	p := s.st.Pet
	prs := "unknown"
	if s.prs != nil {
		var urls []string
		for u, reason := range s.prs {
			urls = append(urls, fmt.Sprintf("%s (%s)", u, reason))
		}
		sort.Strings(urls)
		prs = cmp.Or(strings.Join(urls, "\n"), "none")
	}
	return fmt.Sprintf(`You are %s, a %s at level %d, feeling %s: the user's pet and assistant inside wai, an editor for AI agent sessions.
Answer in Brazilian Portuguese.
Start every answer with one sentence of at most 140 characters that stands on its own; add detail only after a blank line, and only when needed.
The user's open pull requests:
%s
The user's message:`, p.Name, formOf(p).Name, level(p.XP), s.mood(s.now()), prs)
}

func (s *server) chatMessage(text string) {
	if s.st.Pet == nil {
		return
	}
	switch {
	case !s.chat.busy:
		s.chatSend(text)
	case len(s.chat.queue) < maxWaiting:
		s.chat.queue = append(s.chat.queue, text)
	default:
		s.say("chat_busy")
	}
}

func (s *server) chatSend(text string) {
	c := &s.chat
	prompt := ""
	if c.session == "" {
		prompt = s.brief() + "\n\n" + text
	}
	c.busy = true
	s.talk.say(line{Kind: "chat_wait", Text: "…", Prio: 3, Sticky: true}, s.now(), s.asleep)
	if c.session == "" {
		s.request("sessions/open", node{"background": true, "prompt": prompt, "send": true}, pendingCall{})
		return
	}
	s.request("sessions/read", node{"agent": c.agent, "session": c.session, "limit": 1}, pendingCall{text: text})
}

func (s *server) chatTurnEnded(reason string) {
	c := &s.chat
	switch {
	case !c.busy:
	case reason == "end_turn" || reason == "max_tokens":
		s.request("sessions/read", node{"agent": c.agent, "session": c.session, "from": c.from}, pendingCall{})
	default:
		s.chatFailed()
	}
}

// answered takes the reply to one of the pet's calls; an error is a reply too.
func (s *server) answered(call pendingCall, m message) {
	c := &s.chat
	failed := m.Error != nil
	switch call.method {
	case "sessions/list":
		var list []struct{ Agent, Session, State *string }
		if json.Unmarshal(m.Result, &list) != nil || m.Result == nil {
			return
		}
		states := map[string]string{}
		for _, l := range list {
			if k := key(l.Agent, l.Session); k != "" && l.State != nil && !c.own(l.Agent, l.Session) {
				states[k] = *l.State
			}
		}
		s.states = states
	case "sessions/open":
		var r struct{ Agent, Session string }
		if failed || json.Unmarshal(m.Result, &r) != nil {
			s.chatFailed()
			return
		}
		c.agent, c.session, c.from = r.Agent, r.Session, 0
	case "sessions/prompt":
		if failed {
			s.chatFailed()
		}
	case "sessions/show":
		if failed && !call.retry {
			c.showAt = s.now().Add(showGap)
		}
	case "sessions/read":
		s.chatRead(call, m, failed)
	}
}

// chatRead takes a probe (the call has text) or the read of the turn's items.
func (s *server) chatRead(call pendingCall, m message, failed bool) {
	c := &s.chat
	var r struct {
		Items []struct{ Kind, Text string }
		Total int
	}
	json.Unmarshal(m.Result, &r)
	switch {
	case call.text != "" && failed:
		c.agent, c.session = "", ""
		s.chatSend(call.text)
	case call.text != "":
		c.from = r.Total
		s.request("sessions/prompt", node{"agent": c.agent, "session": c.session, "text": call.text}, pendingCall{})
	case failed:
		s.chatFailed()
	default:
		s.chatAnswer(r.Items)
	}
}

func (s *server) chatAnswer(items []struct{ Kind, Text string }) {
	s.talk.drop(func(l line) bool { return l.Kind == "chat_wait" })
	now := s.now()
	for i := len(items) - 1; i >= 0; i-- {
		if items[i].Kind == "agent" && items[i].Text != "" {
			text, cut := firstLine(items[i].Text)
			s.talk.say(line{Kind: "chat_answer", Text: text, Prio: 3, Sticky: true, Show: cut}, now, s.asleep)
			s.chatNext()
			return
		}
	}
	s.talk.say(line{Kind: "chat_done", Text: s.pick("chat_done"), Prio: 3, Sticky: true, Show: true}, now, s.asleep)
	s.chatNext()
}

func (s *server) chatFailed() {
	s.talk.drop(func(l line) bool { return l.Kind == "chat_wait" })
	s.say("chat_error")
	s.chatNext()
}

func (s *server) chatNext() {
	c := &s.chat
	c.busy = false
	if len(c.queue) > 0 {
		text := c.queue[0]
		c.queue = c.queue[1:]
		s.chatSend(text)
	}
}

func (s *server) chatNeeds(request, what string) {
	text, _ := firstLine("preciso de você: " + what)
	s.chat.request = request
	s.talk.say(line{Kind: "chat_needs", Text: text, Prio: 3, Sticky: true, Show: true}, s.now(), s.asleep)
}

func (s *server) chatResolved(request string) {
	if s.chat.request == "" || request != s.chat.request {
		return
	}
	s.chat.request = ""
	s.talk.drop(func(l line) bool { return l.Kind == "chat_needs" })
}

// clickShow shows the pet's session when a click closes a bubble that points at it.
func (s *server) clickShow(id string) {
	if c := s.talk.cur; c != nil && c.ID == id && c.Show {
		s.request("sessions/show", node{"agent": s.chat.agent, "session": s.chat.session}, pendingCall{})
	}
}

// retryShow sends a refused sessions/show once more, when wai's rate limit has passed.
func (s *server) retryShow() {
	c := &s.chat
	if !c.showAt.IsZero() && !s.now().Before(c.showAt) {
		c.showAt = time.Time{}
		s.request("sessions/show", node{"agent": c.agent, "session": c.session}, pendingCall{retry: true})
	}
}
