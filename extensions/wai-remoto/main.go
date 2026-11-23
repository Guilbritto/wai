// wai-remoto: a wai extension (line-delimited JSON-RPC 2.0 on stdio) that lets a paired phone
// follow and drive wai's sessions through an HTTP API on 127.0.0.1, reached through a Tailscale
// tunnel.
package main

import (
	"bufio"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"strings"
	"sync"
	"time"
)

const (
	panelID = "remoto"
	// hostTimeout: how long a call to wai may take before the HTTP request gives up.
	hostTimeout = 10 * time.Second
)

// errNoAnswer: wai did not answer a call within the server's timeout.
var errNoAnswer = errors.New("wai did not answer")

// rpcError is an error answer from wai, such as "No open session <agent> <session>".
type rpcError struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
}

func (e *rpcError) Error() string { return e.Message }

type message struct {
	ID     json.RawMessage `json:"id,omitempty"`
	Method string          `json:"method,omitempty"`
	Params json.RawMessage `json:"params,omitempty"`
	Result json.RawMessage `json:"result,omitempty"`
	Error  *rpcError       `json:"error,omitempty"`
}

type node = map[string]any

type server struct {
	wmu sync.Mutex // serialises writes
	out io.Writer
	now func() time.Time

	dir string // the data folder; empty until initialize

	// Calls to wai waiting for their answer, by id.
	rmu     sync.Mutex
	lastID  int64
	pending map[int64]chan message
	timeout time.Duration

	// Set by initialize.
	store   *store
	pairing *pairing
	guard   *guard

	hub *hub // the open event streams

	lmu  sync.Mutex
	srv  *http.Server // nil while not listening
	addr string

	// The panel's state.
	pmu      sync.Mutex
	msg      string        // the last action's error, shown until the next action
	ticket   int           // the pairing code being counted down
	codeTick time.Duration // how often the time left is redrawn
	done     chan struct{} // closed when wai closes stdin

	smu     sync.Mutex
	states  map[sessionKey]string  // each session's last state, from wai
	prompts map[string][]time.Time // each device's prompts in the last promptWindow
}

func newServer(out io.Writer, now func() time.Time) *server {
	s := &server{
		out: out, now: now, timeout: hostTimeout, hub: newHub(),
		pending:  map[int64]chan message{},
		states:   map[sessionKey]string{},
		prompts:  map[string][]time.Time{},
		codeTick: codeTick,
		done:     make(chan struct{}),
	}
	s.hub.changed = s.render // the connected-device count
	return s
}

func (s *server) write(v any) {
	s.wmu.Lock()
	defer s.wmu.Unlock()
	b, _ := json.Marshal(v)
	fmt.Fprintf(s.out, "%s\n", b)
}

// call sends wai a notification: a host method whose answer we do not need.
func (s *server) call(method string, params any) {
	s.write(node{"jsonrpc": "2.0", "method": method, "params": params})
}

// request calls a host method and waits for its answer: the result, an *rpcError, or errNoAnswer.
func (s *server) request(method string, params any) (json.RawMessage, error) {
	ch := make(chan message, 1)
	s.rmu.Lock()
	s.lastID++
	id := s.lastID
	s.pending[id] = ch
	s.rmu.Unlock()
	s.write(node{"jsonrpc": "2.0", "id": id, "method": method, "params": params})
	select {
	case m := <-ch:
		if m.Error != nil {
			return nil, m.Error
		}
		return m.Result, nil
	case <-time.After(s.timeout):
		s.rmu.Lock()
		delete(s.pending, id)
		s.rmu.Unlock()
		return nil, errNoAnswer
	}
}

// answered hands wai's answer to the call waiting for it.
func (s *server) answered(m message) {
	var id int64
	if json.Unmarshal(m.ID, &id) != nil {
		return
	}
	s.rmu.Lock()
	ch := s.pending[id]
	delete(s.pending, id)
	s.rmu.Unlock()
	if ch != nil {
		ch <- m
	}
}

// notify shows the user a system notification.
func (s *server) notify(title string) {
	s.call("notify", node{"title": title})
}

func (s *server) render() {
	s.call("panel/render", node{"panel": panelID, "tree": s.tree()})
}

func (s *server) handle(m message) {
	switch m.Method {
	case "initialize":
		var p struct {
			DataDir string `json:"data_dir"`
		}
		json.Unmarshal(m.Params, &p)
		s.dir = p.DataDir
		if s.dir == "" {
			s.dir = os.Getenv("WAI_EXT_DATA")
		}
		st, err := openStore(s.dir, s.now)
		if err != nil {
			s.write(node{"jsonrpc": "2.0", "id": m.ID, "error": node{"code": -32000, "message": "Cannot read " + stateFile + ": " + err.Error()}})
			return
		}
		s.store = st
		s.pairing = &pairing{store: st}
		s.guard = &guard{store: st, notify: s.notify}
		s.write(node{"jsonrpc": "2.0", "id": m.ID, "result": node{}})
		s.render()
	case "panel/action":
		s.handleAction(m)
	default:
		if streamed[m.Method] {
			if m.Method == "event/session_state" {
				var p struct{ Agent, Session, State string }
				if json.Unmarshal(m.Params, &p) == nil {
					s.setState(sessionKey{p.Agent, p.Session}, p.State)
				}
			}
			s.hub.publish(strings.TrimPrefix(m.Method, "event/"), compact(m.Params))
		} else if m.ID != nil {
			s.write(node{"jsonrpc": "2.0", "id": m.ID, "error": node{"code": -32601, "message": "Method not found"}})
		}
	}
}

// serve handles wai's messages until stdin closes; its answers go to the calls waiting for them.
func serve(s *server, in io.Reader) {
	defer close(s.done)
	r := bufio.NewReaderSize(in, 1<<20)
	for {
		line, err := r.ReadBytes('\n')
		var m message
		if len(line) > 0 && json.Unmarshal(line, &m) == nil {
			if m.Method != "" {
				s.handle(m)
			} else if m.ID != nil {
				s.answered(m)
			}
		}
		if err != nil {
			return
		}
	}
}

func main() {
	serve(newServer(os.Stdout, time.Now), os.Stdin)
}
