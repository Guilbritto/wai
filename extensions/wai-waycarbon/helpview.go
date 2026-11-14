package main

import (
	"sort"
	"strconv"
	"time"

	"wai-waycarbon/helpapi"
)

// HelpState is what the Help tab draws.
type HelpState struct {
	Configured bool
	Login      string // empty when signed out
	SignIn     *deviceStart
	Err        string
	NotMember  bool
	Requests   []helpapi.Request
	People     []helpapi.Person
	Sessions   []string // option labels
	Sending    bool
	To, Text   string // the form's initial values
	Session    string
	Replies    map[string]replyMode // by request id, for replied requests I sent
	Now        time.Time
}

type replyMode int

const (
	replyBusy replyMode = iota // the session is not idle
	replyIdle
	replyNew  // the session is gone or unknown
	replySent // already sent
)

const draftPrompt = "Write a request for a colleague who has the rights I lack: what to run, where (repo, branch, environment), why, and what to send back. Plain text."

func muted(s string) node { return node{"type": "text", "text": s, "muted": true} }

func button(id, label string, extra ...string) node {
	b := node{"type": "button", "id": id, "label": label}
	if len(extra) > 0 {
		b["style"] = extra[0]
	}
	return b
}

func shortTime(t, now time.Time) string {
	t = t.Local()
	if y, m, d := now.Local().Date(); t.Year() == y && t.Month() == m && t.Day() == d {
		return t.Format("15:04")
	}
	return t.Format("Jan 2")
}

func (st HelpState) where(r helpapi.Request) string {
	out := shortTime(r.Created, st.Now)
	if r.Project.Repo != "" {
		out += " · " + oneLine(r.Project.Repo)
		if r.Project.Branch != "" {
			out += " (" + oneLine(r.Project.Branch) + ")"
		}
	}
	return out
}

// Colleagues' text goes only in text nodes: plain text, never markdown.
func plain(s string) node { return node{"type": "text", "text": s} }

func (st HelpState) incoming(r helpapi.Request) node {
	kids := []node{plain(r.Text)}
	switch r.State {
	case helpapi.Waiting:
		kids = append(kids,
			node{"type": "input", "id": "help:reason:" + r.ID, "placeholder": "Reason for declining (optional)"},
			node{"type": "row", "children": []node{button("help:accept:"+r.ID, "Accept", "primary"), button("help:decline:"+r.ID, "Decline")}})
	case helpapi.Accepted:
		kids = append(kids,
			muted("Accepted. Paste the outcome when it is done."),
			node{"type": "input", "id": "help:replytext:" + r.ID, "placeholder": "Reply", "multiline": true},
			button("help:reply:"+r.ID, "Send reply", "primary"))
	}
	return node{"type": "card", "title": "@" + r.From, "subtitle": st.where(r), "children": kids}
}

func (st HelpState) sent(r helpapi.Request) node {
	var kids []node
	switch r.State {
	case helpapi.Waiting:
		kids = []node{muted("Waiting for @" + r.To), plain(r.Text), button("help:cancel:"+r.ID, "Cancel")}
	case helpapi.Accepted:
		kids = []node{muted("Accepted by @" + r.To), plain(r.Text), button("help:cancel:"+r.ID, "Cancel")}
	case helpapi.Replied:
		kids = []node{muted("@" + r.To + " replied:"), plain(r.Reply)}
		switch st.Replies[r.ID] {
		case replyBusy, replyIdle:
			b := button("help:tosession:"+r.ID, "Send to session", "primary")
			b["disabled"] = st.Replies[r.ID] == replyBusy
			kids = append(kids, b)
		case replyNew:
			kids = append(kids, button("help:newsession:"+r.ID, "Open in a new session", "primary"))
		case replySent:
			kids = append(kids, muted("Sent to the session."))
		}
	case helpapi.Declined:
		kids = []node{muted("@" + r.To + " declined"), plain(r.Reason)}
	}
	return node{"type": "card", "title": "To @" + r.To, "subtitle": st.where(r), "children": kids}
}

func (st HelpState) form() node {
	kids := []node{}
	if st.Sending {
		kids = append(kids, node{"type": "spinner", "label": "Sending…"})
		return node{"type": "section", "id": "help:new", "title": "New request", "open": len(st.Requests) == 0, "children": kids}
	}
	to := node{"type": "input", "id": "help:to", "placeholder": "GitHub login"}
	if st.To != "" {
		to["value"] = st.To
	}
	kids = append(kids, to)
	for _, p := range st.People {
		tone := "neutral"
		if p.Online {
			tone = "success"
		}
		state := "offline"
		if p.Online {
			state = "online"
		}
		kids = append(kids, node{"type": "row", "children": []node{button("help:person:"+p.Login, "@"+p.Login), node{"type": "badge", "text": state, "tone": tone}}})
	}
	if len(st.Sessions) > 0 {
		sel := node{"type": "select", "id": "help:session", "options": st.Sessions}
		if st.Session != "" {
			sel["value"] = st.Session
		}
		kids = append(kids, muted("Reply goes to"), sel)
	}
	text := node{"type": "input", "id": "help:text", "placeholder": "Request", "multiline": true, "rows": 8}
	if st.Text != "" {
		text["value"] = st.Text
	}
	kids = append(kids, muted("Suggested prompt for your agent: "+draftPrompt), text, muted("Do not paste secrets."),
		muted("Up to "+strconv.Itoa(helpapi.MaxText)+" characters."), button("help:send", "Send", "primary"))
	return node{"type": "section", "id": "help:new", "title": "New request", "open": len(st.Requests) == 0, "children": kids}
}

func helpTree(st HelpState) node {
	if !st.Configured {
		return muted("Help requests need the WayCarbon relay, which is not set up yet.")
	}
	var kids []node
	if st.Err != "" {
		kids = append(kids, node{"type": "alert", "tone": "danger", "text": st.Err})
	}
	if st.Login == "" {
		if st.SignIn == nil {
			kids = append(kids, button("help:signin", "Sign in with GitHub", "primary"))
		} else if st.SignIn.UserCode == "" {
			kids = append(kids, node{"type": "spinner", "label": "Starting sign-in…"})
		} else {
			kids = append(kids,
				node{"type": "text", "text": "Enter this code on GitHub:"},
				node{"type": "text", "text": st.SignIn.UserCode, "mono": true},
				node{"type": "link", "text": "Open " + st.SignIn.VerificationURI, "url": st.SignIn.VerificationURI},
				node{"type": "spinner", "label": "Waiting for you to approve…"})
		}
		return node{"type": "column", "children": kids}
	}
	kids = append(kids, node{"type": "row", "children": []node{
		node{"type": "text", "text": "@" + st.Login}, node{"type": "spacer"}, button("help:signout", "Sign out")}})
	if st.NotMember {
		kids = append(kids, button("help:retry", "Try again"))
		return node{"type": "column", "children": kids}
	}
	reqs := append([]helpapi.Request{}, st.Requests...)
	sort.SliceStable(reqs, func(i, j int) bool { return reqs[i].Created.After(reqs[j].Created) })
	var in, out []node
	for _, r := range reqs {
		switch {
		case r.To == st.Login && (r.State == helpapi.Waiting || r.State == helpapi.Accepted):
			in = append(in, st.incoming(r))
		case r.From == st.Login && st.Replies[r.ID] != replySent && (r.State == helpapi.Waiting || r.State == helpapi.Accepted ||
			r.State == helpapi.Replied || r.State == helpapi.Declined):
			out = append(out, st.sent(r))
		}
	}
	if len(in) > 0 {
		kids = append(kids, node{"type": "section", "id": "help:incoming", "title": "Incoming", "badge": strconv.Itoa(len(in)), "children": in})
	}
	if len(out) > 0 {
		kids = append(kids, node{"type": "section", "id": "help:sent", "title": "Sent", "badge": strconv.Itoa(len(out)), "children": out})
	}
	kids = append(kids, st.form())
	return node{"type": "column", "children": kids}
}

// helpBody builds the Help tab from the server's state. The caller holds smu.
func (s *server) helpBody() node {
	h := &s.help
	st := HelpState{Configured: relayBase() != "", SignIn: h.signing, Err: h.err, NotMember: h.notMember, People: h.people,
		Sending: h.sending, To: h.to, Text: h.text, Session: h.session, Login: s.me(), Replies: map[string]replyMode{}, Now: time.Now()}
	for _, r := range h.reqs {
		st.Requests = append(st.Requests, r)
		if r.State != helpapi.Replied {
			continue
		}
		t := h.targets[r.ID]
		switch {
		case t.Sent:
			st.Replies[r.ID] = replySent
		case s.sessionOf(t) == "idle":
			st.Replies[r.ID] = replyIdle
		case s.sessionOf(t) == "gone":
			st.Replies[r.ID] = replyNew
		}
	}
	for _, si := range h.sess {
		if len(st.Sessions) < 100 {
			st.Sessions = append(st.Sessions, si.label())
		}
	}
	return helpTree(st)
}
