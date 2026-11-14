package main

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"

	"wai-waycarbon/helpapi"
)

// relayURL is empty until WayCarbon hosts the relay; WAI_WAYCARBON_RELAY overrides it.
const relayURL = "" // ponytail: set when WayCarbon hosts the relay

const notMemberMsg = "Only members of WayCarbon can use help requests"

func relayBase() string {
	if v := os.Getenv("WAI_WAYCARBON_RELAY"); v != "" {
		return strings.TrimRight(v, "/")
	}
	return relayURL
}

// auth is the relay token and the login it belongs to; it lives in <data>/auth.json, mode 0600.
type auth struct{ Token, Login string }

// replyTarget is where a request's reply goes, and whether it was already sent there.
type replyTarget struct {
	Agent, Session string
	Sent           bool
}

func writePrivate(path string, v any) error {
	b, err := json.Marshal(v)
	if err != nil {
		return err
	}
	tmp, err := os.CreateTemp(filepath.Dir(path), ".tmp-")
	if err != nil {
		return err
	}
	defer os.Remove(tmp.Name())
	if err := tmp.Chmod(0o600); err != nil {
		tmp.Close()
		return err
	}
	if _, err := tmp.Write(b); err != nil {
		tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	return os.Rename(tmp.Name(), path)
}

func loadAuth(dir string) (auth, bool) {
	var a auth
	b, err := os.ReadFile(filepath.Join(dir, "auth.json"))
	if err != nil || json.Unmarshal(b, &a) != nil || a.Token == "" {
		return auth{}, false
	}
	return a, true
}

func saveAuth(dir string, a auth) error { return writePrivate(filepath.Join(dir, "auth.json"), a) }

func dropAuth(dir string) { os.Remove(filepath.Join(dir, "auth.json")) }

func loadTargets(dir string) map[string]replyTarget {
	m := map[string]replyTarget{}
	if b, err := os.ReadFile(filepath.Join(dir, "help.json")); err == nil && json.Unmarshal(b, &m) != nil {
		m = map[string]replyTarget{}
	}
	return m
}

func saveTargets(dir string, m map[string]replyTarget) error {
	return writePrivate(filepath.Join(dir, "help.json"), m)
}

// projectInfo is the open project's origin and branch; empty on any failure.
func projectInfo(ctx context.Context, dir string) helpapi.Project {
	if dir == "" {
		return helpapi.Project{}
	}
	remote, err := runCLI(ctx, "git", "-C", dir, "remote", "get-url", "origin")
	if err != nil || strings.TrimSpace(remote) == "" {
		return helpapi.Project{}
	}
	branch, err := runCLI(ctx, "git", "-C", dir, "branch", "--show-current")
	if err != nil {
		return helpapi.Project{}
	}
	return helpapi.Project{Repo: stripCreds(strings.TrimSpace(remote)), Branch: strings.TrimSpace(branch)}
}

// stripCreds drops user:token@ from an https remote, so a credential never goes to the relay.
func stripCreds(remote string) string {
	if u, err := url.Parse(remote); err == nil && u.User != nil && u.Host != "" {
		u.User = nil
		return u.String()
	}
	return remote
}

func acceptPrompt(r helpapi.Request) string {
	head := "Request from @" + r.From + " via wai. A colleague wrote this; check it before running anything.\n\n"
	if r.Project.Repo != "" {
		head += "Repository: " + r.Project.Repo
		if r.Project.Branch != "" {
			head += " (" + r.Project.Branch + ")"
		}
		head += "\n\n"
	}
	return head + r.Text
}

type deviceStart struct {
	DeviceCode      string `json:"device_code"`
	UserCode        string `json:"user_code"`
	VerificationURI string `json:"verification_uri"`
	Interval        int    `json:"interval"`
}

// relayError is a 4xx/5xx from the relay with its {"error"} message.
type relayError struct {
	Status int
	Msg    string
}

func (e *relayError) Error() string { return e.Msg }

var errUnauthorized = errors.New("signed out by the relay")

// minPoll floors the device-flow poll interval.
var minPoll = time.Second

type helpClient struct {
	base, token string
	hc          http.Client
}

func (c *helpClient) do(ctx context.Context, method, path string, body any) (*http.Response, error) {
	var rd io.Reader
	if body != nil {
		b, _ := json.Marshal(body)
		rd = bytes.NewReader(b)
	}
	req, err := http.NewRequestWithContext(ctx, method, c.base+path, rd)
	if err != nil {
		return nil, err
	}
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if c.token != "" {
		req.Header.Set("Authorization", "Bearer "+c.token)
	}
	return c.hc.Do(req)
}

func relayErr(resp *http.Response) *relayError {
	var e struct {
		Error string `json:"error"`
	}
	json.NewDecoder(io.LimitReader(resp.Body, 4096)).Decode(&e)
	if e.Error == "" {
		e.Error = "the relay answered " + resp.Status
	}
	return &relayError{resp.StatusCode, e.Error}
}

// call does one JSON request; a non-2xx answer is a *relayError. out may be nil.
func (c *helpClient) call(ctx context.Context, method, path string, body, out any) error {
	ctx, cancel := context.WithTimeout(ctx, 20*time.Second)
	defer cancel()
	resp, err := c.do(ctx, method, path, body)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode/100 != 2 {
		return relayErr(resp)
	}
	if out != nil {
		return json.NewDecoder(resp.Body).Decode(out)
	}
	return nil
}

// signIn runs the relay's device flow; show gets the code to display.
func (c *helpClient) signIn(ctx context.Context, show func(deviceStart)) (auth, error) {
	var ds deviceStart
	if err := c.call(ctx, "POST", "/auth/device", nil, &ds); err != nil {
		return auth{}, err
	}
	show(ds)
	every := max(time.Duration(ds.Interval)*time.Second, minPoll)
	for {
		select {
		case <-ctx.Done():
			return auth{}, ctx.Err()
		case <-time.After(every):
		}
		var a auth
		var out struct{ Token, Login string }
		err := c.call(ctx, "POST", "/auth/device/poll", map[string]string{"device_code": ds.DeviceCode}, &out)
		var re *relayError
		if errors.As(err, &re) && re.Status == http.StatusPreconditionRequired {
			continue
		}
		if err != nil {
			return auth{}, err
		}
		a = auth{out.Token, out.Login}
		if a.Token == "" {
			return auth{}, errors.New("the relay sent no token")
		}
		return a, nil
	}
}

func (c *helpClient) create(ctx context.Context, to string, p helpapi.Project, text string) (helpapi.Request, error) {
	var r helpapi.Request
	err := c.call(ctx, "POST", "/requests", map[string]any{"to": to, "project": p, "text": text}, &r)
	return r, err
}

func (c *helpClient) act(ctx context.Context, id, action, text string) (helpapi.Request, error) {
	var r helpapi.Request
	err := c.call(ctx, "POST", "/requests/"+url.PathEscape(id)+"/"+action, map[string]string{"text": text}, &r)
	return r, err
}

func (c *helpClient) signOut(ctx context.Context) error {
	return c.call(ctx, "POST", "/auth/signout", nil, nil)
}

// stream reads GET /events and reconnects with a 1 s to 60 s backoff, reset after a sync. It returns
// nil when ctx ends, errUnauthorized on a 401 and the *relayError on a 403.
func (c *helpClient) stream(ctx context.Context, onEvent func(event string, data []byte)) error {
	backoff := time.Second
	for {
		synced := false
		resp, err := c.do(ctx, "GET", "/events", nil)
		if err == nil {
			switch resp.StatusCode {
			case http.StatusOK:
				readSSE(resp.Body, func(ev string, data []byte) {
					synced = synced || ev == helpapi.EvSync
					onEvent(ev, data)
				})
			case http.StatusUnauthorized:
				resp.Body.Close()
				return errUnauthorized
			case http.StatusForbidden:
				defer resp.Body.Close()
				return relayErr(resp)
			}
			resp.Body.Close()
		}
		if synced {
			backoff = time.Second
		}
		select {
		case <-ctx.Done():
			return nil
		case <-time.After(backoff):
		}
		backoff = min(backoff*2, time.Minute)
	}
}

// readSSE calls f per event: `event:` and `data:` lines, a blank line ends one, `:` lines are comments.
func readSSE(r io.Reader, f func(event string, data []byte)) {
	br := bufio.NewReader(r)
	var ev string
	var data []string
	for {
		line, err := br.ReadString('\n')
		line = strings.TrimRight(line, "\r\n")
		switch {
		case line == "" && err == nil:
			if ev != "" || len(data) > 0 {
				f(ev, []byte(strings.Join(data, "\n")))
			}
			ev, data = "", nil
		case strings.HasPrefix(line, ":"):
		case strings.HasPrefix(line, "event:"):
			ev = strings.TrimSpace(line[len("event:"):])
		case strings.HasPrefix(line, "data:"):
			data = append(data, strings.TrimPrefix(line[len("data:"):], " "))
		}
		if err != nil {
			return
		}
	}
}

// ---- server side ----

type sessInfo struct{ Agent, Session, Title, State string }

func (si sessInfo) label() string {
	t := si.Title
	if t == "" {
		t = si.Session
	}
	return oneLine(t) + " — " + si.Agent
}

func firstLine(s string) string {
	l, _, _ := strings.Cut(strings.TrimSpace(s), "\n")
	return l
}

// helpState is the server's help data; all of it is guarded by smu.
type helpData struct {
	auth       *auth
	signing    *deviceStart
	err        string
	notMember  bool
	reqs       map[string]helpapi.Request
	people     []helpapi.Person
	sess       []sessInfo
	sessLoaded bool
	targets    map[string]replyTarget
	sending    bool
	to, text   string // the form's initial values
	session    string
	cancel     context.CancelFunc // the running stream
}

func (s *server) me() string {
	if s.help.auth == nil {
		return ""
	}
	return s.help.auth.Login
}

func (s *server) client() *helpClient {
	c := &helpClient{base: relayBase()}
	if s.help.auth != nil {
		c.token = s.help.auth.Token
	}
	return c
}

// helpInit loads what the previous run left, before the initialize answer.
func (s *server) helpInit() {
	s.smu.Lock()
	defer s.smu.Unlock()
	s.help.targets = map[string]replyTarget{}
	if relayBase() == "" || s.dataDir == "" {
		return
	}
	s.help.targets = loadTargets(s.dataDir)
	if a, ok := loadAuth(s.dataDir); ok {
		s.help.auth = &a
	}
}

// helpStart connects, after the initialize answer.
func (s *server) helpStart() {
	if relayBase() == "" || s.dataDir == "" {
		return
	}
	s.smu.Lock()
	if s.help.auth != nil {
		s.startStream()
	}
	s.smu.Unlock()
	s.refreshSessions()
}

// startStream opens the event stream in the background. The caller holds smu.
func (s *server) startStream() {
	if s.help.cancel != nil {
		s.help.cancel()
	}
	ctx, cancel := context.WithCancel(s.ctx)
	s.help.cancel, s.help.notMember = cancel, false
	c := s.client()
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		err := c.stream(ctx, s.onEvent)
		if ctx.Err() != nil || err == nil {
			return
		}
		s.smu.Lock()
		defer s.smu.Unlock()
		if errors.Is(err, errUnauthorized) {
			s.signedOut()
		} else {
			s.help.notMember, s.help.err = true, err.Error()
		}
		s.paintAll()
	}()
}

// signedOut forgets the account. The caller holds smu.
func (s *server) signedOut() {
	if s.help.cancel != nil {
		s.help.cancel()
		s.help.cancel = nil
	}
	dropAuth(s.dataDir)
	s.help.auth, s.help.reqs, s.help.people, s.help.notMember = nil, nil, nil, false
}

// loadSessions replaces the cached sessions with the host's list; false when it could not be read.
func (s *server) loadSessions() bool {
	raw, err := s.request(s.ctx, "sessions/list", node{})
	var list []struct {
		Agent, Session string
		Title          *string
		State          string
	}
	if err != nil || json.Unmarshal(raw, &list) != nil {
		return false
	}
	out := make([]sessInfo, 0, len(list))
	for _, e := range list {
		si := sessInfo{Agent: e.Agent, Session: e.Session, State: e.State}
		if e.Title != nil {
			si.Title = *e.Title
		}
		out = append(out, si)
	}
	s.smu.Lock()
	s.help.sess, s.help.sessLoaded = out, true
	s.paintAll()
	s.smu.Unlock()
	return true
}

func (s *server) refreshSessions() {
	s.wg.Add(1)
	go func() { defer s.wg.Done(); s.loadSessions() }()
}

func (s *server) sessionState(p json.RawMessage) {
	var e struct{ Agent, Session, State string }
	if json.Unmarshal(p, &e) != nil {
		return
	}
	s.smu.Lock()
	defer s.smu.Unlock()
	for i := range s.help.sess {
		if s.help.sess[i].Agent == e.Agent && s.help.sess[i].Session == e.Session {
			s.help.sess[i].State = e.State
			s.paintAll()
			return
		}
	}
	s.help.sess = append(s.help.sess, sessInfo{Agent: e.Agent, Session: e.Session, State: e.State})
	s.paintAll()
}

func (s *server) notify(title, body string) {
	s.call("notify", node{"title": title, "body": body})
}

func (s *server) onEvent(event string, data []byte) {
	s.smu.Lock()
	defer s.smu.Unlock()
	if s.help.reqs == nil {
		s.help.reqs = map[string]helpapi.Request{}
	}
	switch event {
	case helpapi.EvSync:
		var list []helpapi.Request
		if json.Unmarshal(data, &list) != nil {
			return
		}
		old := s.help.reqs
		s.help.reqs = map[string]helpapi.Request{}
		for _, r := range list {
			s.help.reqs[r.ID] = r
			// ponytail: a request waiting at wai start notifies again; there is no "seen" store.
			if _, known := old[r.ID]; !known {
				s.notifyIncoming(r)
			}
		}
		s.paintAll()
		go s.refreshSessions()
	case helpapi.EvNew:
		var r helpapi.Request
		if json.Unmarshal(data, &r) != nil {
			return
		}
		if _, known := s.help.reqs[r.ID]; !known {
			s.help.reqs[r.ID] = r
			s.notifyIncoming(r)
		}
		s.paintAll()
	case helpapi.EvUpdated:
		var u helpapi.Update
		if json.Unmarshal(data, &u) != nil {
			return
		}
		s.applyUpdate(u)
		s.paintAll()
	case helpapi.EvPeople:
		var p []helpapi.Person
		if json.Unmarshal(data, &p) != nil {
			return
		}
		s.help.people = p
		s.paintAll()
	}
}

func (s *server) isMe(login string) bool { return strings.EqualFold(login, s.me()) }

func (s *server) notifyIncoming(r helpapi.Request) {
	if s.isMe(r.To) && r.State == helpapi.Waiting {
		s.notify("@"+r.From+" asks for help", firstLine(r.Text))
	}
}

// applyUpdate moves a known request to its new state, and tells the sender what happened.
func (s *server) applyUpdate(u helpapi.Update) {
	r, ok := s.help.reqs[u.ID]
	if !ok {
		return
	}
	changed := r.State != u.State
	r.State, r.Reply, r.Reason, r.Updated = u.State, u.Reply, u.Reason, time.Now()
	s.help.reqs[u.ID] = r
	if changed && s.isMe(r.From) {
		switch u.State {
		case helpapi.Replied:
			s.notify("@"+r.To+" replied", firstLine(u.Reply))
		case helpapi.Declined:
			s.notify("@"+r.To+" declined", firstLine(u.Reason))
		}
	}
}

// helpCounts: incoming requests waiting, and replies not yet sent to a session. The caller holds smu.
func (s *server) helpCounts() (waiting, replies int) {
	for id, r := range s.help.reqs {
		switch {
		case r.State == helpapi.Waiting && s.isMe(r.To):
			waiting++
		case r.State == helpapi.Replied && s.isMe(r.From) && !s.help.targets[id].Sent:
			replies++
		}
	}
	return
}

// fail shows an error in the Help tab and repaints.
func (s *server) fail(err error) {
	s.smu.Lock()
	s.help.err = err.Error()
	s.paintAll()
	s.smu.Unlock()
}

func (s *server) helpSignIn() {
	s.smu.Lock()
	if s.help.signing != nil || relayBase() == "" {
		s.smu.Unlock()
		return
	}
	s.help.signing, s.help.err = &deviceStart{}, ""
	s.paintAll()
	c := s.client()
	s.smu.Unlock()
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		a, err := c.signIn(s.ctx, func(d deviceStart) {
			s.smu.Lock()
			s.help.signing = &d
			s.paintAll()
			s.smu.Unlock()
		})
		s.smu.Lock()
		defer s.smu.Unlock()
		s.help.signing = nil
		if err != nil {
			if s.ctx.Err() == nil {
				s.help.err = err.Error()
			}
		} else if err := saveAuth(s.dataDir, a); err != nil {
			s.help.err = "Could not save the sign-in: " + err.Error()
		} else {
			s.help.auth = &a
			s.startStream()
		}
		s.paintAll()
	}()
}

func (s *server) helpSignOut() {
	s.smu.Lock()
	c := s.client()
	s.signedOut()
	s.help.err = ""
	s.paintAll()
	s.smu.Unlock()
	s.wg.Add(1)
	go func() { defer s.wg.Done(); c.signOut(s.ctx) }()
}

func (s *server) helpRetry() {
	s.smu.Lock()
	defer s.smu.Unlock()
	if s.help.auth != nil {
		s.help.err = ""
		s.startStream()
		s.paintAll()
	}
}

func (s *server) helpSend(values map[string]string) {
	to := strings.TrimPrefix(strings.TrimSpace(values["help:to"]), "@")
	text := strings.TrimSpace(values["help:text"])
	choice := values["help:session"]
	s.smu.Lock()
	s.help.to, s.help.text, s.help.session, s.help.err = to, values["help:text"], choice, ""
	var tgt *replyTarget
	for _, si := range s.help.sess {
		if si.label() == choice {
			tgt = &replyTarget{Agent: si.Agent, Session: si.Session}
			break
		}
	}
	switch {
	case to == "" || text == "":
		s.help.err = "Write who to ask and what you need."
	case len([]rune(text)) > helpapi.MaxText:
		s.help.err = fmt.Sprintf("The request is over %d characters.", helpapi.MaxText)
	}
	if s.help.err != "" || s.help.sending {
		s.paintAll()
		s.smu.Unlock()
		return
	}
	s.help.sending = true
	s.paintAll()
	c := s.client()
	s.smu.Unlock()
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		p := projectInfo(s.ctx, s.project)
		r, err := c.create(s.ctx, to, p, text)
		s.smu.Lock()
		defer s.smu.Unlock()
		s.help.sending = false
		defer s.refreshSessions()
		if err != nil {
			s.help.err = err.Error()
		} else {
			s.help.to, s.help.text, s.help.session = "", "", ""
			if s.help.reqs == nil {
				s.help.reqs = map[string]helpapi.Request{}
			}
			s.help.reqs[r.ID] = r
			if tgt != nil {
				s.help.targets[r.ID] = *tgt
				saveTargets(s.dataDir, s.help.targets)
			}
		}
		s.paintAll()
	}()
}

// helpAct sends cancel, accept, decline or reply for a request, then does what the action needs.
func (s *server) helpAct(action, id, text string) {
	s.smu.Lock()
	r, ok := s.help.reqs[id]
	c := s.client()
	s.smu.Unlock()
	if !ok {
		return
	}
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		resp, err := c.act(s.ctx, id, action, text)
		if err != nil {
			s.fail(err)
			return
		}
		s.smu.Lock()
		s.help.err = ""
		s.applyUpdate(helpapi.Update{ID: id, State: resp.State, Reply: resp.Reply, Reason: resp.Reason})
		s.paintAll()
		s.smu.Unlock()
		s.refreshSessions()
		if action == "accept" {
			s.call("sessions/open", node{"prompt": acceptPrompt(r), "send": false})
		}
	}()
}

// replyText is what Send to session types into the requester's session.
func replyText(r helpapi.Request) string {
	return "@" + r.To + " replied to your request: " + r.Reply
}

func (s *server) markSent(id string) {
	t := s.help.targets[id]
	t.Sent = true
	s.help.targets[id] = t
	saveTargets(s.dataDir, s.help.targets)
}

// helpToSession sends the reply to its session when that is still open and idle. It asks the host
// first: a closed session would drop the prompt and the reply would be lost.
func (s *server) helpToSession(id string) {
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		if !s.loadSessions() {
			s.fail(errors.New("Could not check the open sessions"))
			return
		}
		s.smu.Lock()
		defer s.smu.Unlock()
		r, ok := s.help.reqs[id]
		t := s.help.targets[id]
		if !ok || r.State != helpapi.Replied || t.Sent || s.sessionOf(t) != "idle" {
			return
		}
		s.call("sessions/prompt", node{"agent": t.Agent, "session": t.Session, "text": replyText(r)})
		s.markSent(id)
		s.paintAll()
	}()
}

// helpNewSession opens the reply in a new, unsent session.
func (s *server) helpNewSession(id string) {
	s.smu.Lock()
	defer s.smu.Unlock()
	r, ok := s.help.reqs[id]
	if !ok || r.State != helpapi.Replied || s.help.targets[id].Sent {
		return
	}
	s.call("sessions/open", node{"prompt": replyText(r), "send": false})
	s.markSent(id)
	s.paintAll()
}

// sessionOf is the state of a reply's session: "gone" when the open sessions do not list it.
func (s *server) sessionOf(t replyTarget) string {
	if t.Session == "" {
		return "gone"
	}
	for _, si := range s.help.sess {
		if si.Agent == t.Agent && si.Session == t.Session {
			return si.State
		}
	}
	if !s.help.sessLoaded {
		return "working"
	}
	return "gone"
}

func (s *server) helpAction(action string, values map[string]string) bool {
	switch action {
	case "help:signin":
		s.helpSignIn()
	case "help:signout":
		s.helpSignOut()
	case "help:retry":
		s.helpRetry()
	case "help:send":
		s.helpSend(values)
	default:
		verb, id, ok := strings.Cut(strings.TrimPrefix(action, "help:"), ":")
		if !ok || !strings.HasPrefix(action, "help:") {
			return false
		}
		switch verb {
		case "cancel", "accept":
			s.helpAct(verb, id, "")
		case "decline":
			s.helpAct(verb, id, values["help:reason:"+id])
		case "reply":
			s.helpAct(verb, id, strings.TrimSpace(values["help:replytext:"+id]))
		case "tosession":
			s.helpToSession(id)
		case "newsession":
			s.helpNewSession(id)
		case "person":
			s.smu.Lock()
			s.help.to = id
			s.paintAll()
			s.smu.Unlock()
			s.refreshSessions()
		default:
			return false
		}
	}
	return true
}

// request is a call to the host that waits for its answer.
func (s *server) request(ctx context.Context, method string, params any) (json.RawMessage, error) {
	id := s.nextID.Add(1)
	ch := make(chan message, 1)
	s.pmu.Lock()
	s.pending[id] = ch
	s.pmu.Unlock()
	defer func() { s.pmu.Lock(); delete(s.pending, id); s.pmu.Unlock() }()
	s.write(node{"jsonrpc": "2.0", "id": id, "method": method, "params": params})
	select {
	case m := <-ch:
		if m.Error != nil {
			return nil, errors.New(m.Error.Message)
		}
		return m.Result, nil
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-time.After(10 * time.Second):
		return nil, errors.New(method + " timed out")
	}
}

func (s *server) deliver(m message) {
	var id int64
	if json.Unmarshal(m.ID, &id) != nil {
		return
	}
	s.pmu.Lock()
	ch := s.pending[id]
	s.pmu.Unlock()
	if ch != nil {
		select {
		case ch <- m:
		default:
		}
	}
}
