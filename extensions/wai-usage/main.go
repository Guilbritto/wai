// wai-usage: a wai extension (line-delimited JSON-RPC 2.0 on stdio) with one bottom-bar item that
// shows the Claude plan's usage windows. It reads Claude Code's login and never changes it.
package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"sync"
	"time"
)

// maxNap: the scheduler looks at the wall clock at least this often. Go's monotonic clock stops
// while the Mac sleeps, so a fetch due meanwhile happens at most a minute after it wakes.
const maxNap = time.Minute

type message struct {
	ID     json.RawMessage `json:"id,omitempty"`
	Method string          `json:"method,omitempty"`
}

type server struct {
	wmu sync.Mutex // serialises writes
	out io.Writer

	url    string
	client *http.Client
	token  tokenSource
	loc    *time.Location
	zone   string

	ctx       context.Context
	stop      context.CancelFunc
	click     chan struct{} // a click got the fetch slot (busy is set)
	scheduler sync.WaitGroup

	mu    sync.Mutex // guards the rest
	v     view
	retry time.Duration // the last 429's Retry-After, 0 if the last attempt was not a 429
	busy  bool          // a fetch is running or about to
}

func newServer(out io.Writer, url string, c *http.Client, tok tokenSource) *server {
	ctx, stop := context.WithCancel(context.Background())
	return &server{out: out, url: url, client: c, token: tok, loc: time.Local, zone: zoneName(),
		ctx: ctx, stop: stop, click: make(chan struct{}, 1)}
}

func (s *server) write(v any) {
	s.wmu.Lock()
	defer s.wmu.Unlock()
	b, _ := json.Marshal(v)
	fmt.Fprintf(s.out, "%s\n", b)
}

func (s *server) publish() {
	s.mu.Lock()
	params := s.v.update(time.Now().Round(0), s.loc, s.zone)
	s.mu.Unlock()
	s.write(node{"jsonrpc": "2.0", "method": "item/update", "params": params})
}

// fetch runs one round, with the busy slot already taken.
func (s *server) fetch() {
	tok, err := s.token(s.ctx)
	var ws []window
	if err == nil {
		ws, err = fetch(s.ctx, s.client, s.url, tok)
	}
	if s.ctx.Err() != nil {
		return
	}
	now := time.Now().Round(0)
	s.mu.Lock()
	s.retry = 0
	if err == nil {
		s.v = view{windows: ws, updated: now}
	} else {
		s.v.failed, s.v.err = now, err
		var rl rateLimited
		if errors.As(err, &rl) {
			s.retry = rl.retry
		}
	}
	s.busy = false
	s.mu.Unlock()
	if err != nil {
		log.Printf("wai-usage: fetch failed: %v", err) // built by fetch and claudeToken: no token, no body
	}
	s.publish()
}

// schedule fetches when next says, sleeping at most maxNap between looks at the wall clock.
// ponytail: one wakeup a minute; a host "system woke" event would avoid it.
func (s *server) schedule() {
	for s.ctx.Err() == nil {
		s.mu.Lock()
		wait := time.Duration(0)
		if !s.busy {
			due := next(s.v, s.retry, time.Now())
			if wait = due.Sub(time.Now().Round(0)); wait <= 0 {
				s.busy = true
			}
		}
		fetchNow := s.busy
		s.mu.Unlock()
		if fetchNow {
			select { // a click that took the slot before this round is served by it
			case <-s.click:
			default:
			}
			s.fetch()
			continue
		}
		timer := time.NewTimer(min(wait, maxNap))
		select {
		case <-s.click:
			timer.Stop()
			s.fetch()
		case <-timer.C:
		case <-s.ctx.Done():
			timer.Stop()
		}
	}
}

func (s *server) handle(m message) {
	switch m.Method {
	case "initialize":
		s.write(node{"jsonrpc": "2.0", "id": m.ID, "result": node{}})
		s.publish() // loading, before the first fetch
		s.scheduler.Add(1)
		go func() {
			defer s.scheduler.Done()
			s.schedule()
		}()
	case "item/action":
		s.write(node{"jsonrpc": "2.0", "id": m.ID, "result": nil})
		s.mu.Lock()
		take := !s.busy
		s.busy = true
		s.mu.Unlock()
		if take {
			s.click <- struct{}{}
		}
	default:
		if m.ID != nil {
			s.write(node{"jsonrpc": "2.0", "id": m.ID, "error": node{"code": -32601, "message": "Method not found"}})
		}
	}
}

// serve reads until EOF, then cancels a running fetch and waits for it.
func serve(s *server, in io.Reader) {
	defer s.scheduler.Wait()
	defer s.stop()
	r := bufio.NewReaderSize(in, 1<<20)
	for {
		line, err := r.ReadBytes('\n')
		var m message
		if len(line) > 0 && json.Unmarshal(line, &m) == nil && m.Method != "" {
			s.handle(m)
		}
		if err != nil {
			return
		}
	}
}

func main() {
	log.SetFlags(0)
	c := &http.Client{Timeout: 15 * time.Second}
	serve(newServer(os.Stdout, "https://api.anthropic.com"+usagePath, c, claudeToken), os.Stdin)
}
