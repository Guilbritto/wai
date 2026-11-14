// wai-waycarbon: a wai extension (line-delimited JSON-RPC 2.0 on stdio) with one tabbed "WayCarbon" panel,
// a bar item and upkeep of the waycarbon Claude plugin.
package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"
	"sync"
	"time"
	"unicode"
)

const panelID = "waycarbon"

type message struct {
	ID     json.RawMessage `json:"id,omitempty"`
	Method string          `json:"method,omitempty"`
	Params json.RawMessage `json:"params,omitempty"`
}

type node = map[string]any

type server struct {
	wmu sync.Mutex // serialises writes
	out io.Writer

	smu      sync.Mutex // guards the rest
	nextID   int
	gen      int // the latest render; an older result is stale
	cards    map[string]Card
	cardList []Card
	cardsErr error
	loading  bool

	ctx context.Context // cancelled at EOF; plugin work stops with it
	wg  sync.WaitGroup  // plugin goroutines, drained before run returns

	dataDir, project string
	pluginMissing    bool
	installing       bool
	pluginErr        string
	counts           counts
	lastBar          string
}

func jsonString(v any) string {
	b, _ := json.Marshal(v)
	return string(b)
}

func (s *server) write(v any) {
	s.wmu.Lock()
	defer s.wmu.Unlock()
	b, _ := json.Marshal(v)
	fmt.Fprintf(s.out, "%s\n", b)
}

// call sends a request; the caller must not hold smu.
func (s *server) call(method string, params any) {
	s.smu.Lock()
	s.nextID++
	id := s.nextID
	s.smu.Unlock()
	s.write(node{"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

// refreshWork shows Loading… now and the cards when jira answers, off the message loop.
func (s *server) refreshWork() {
	s.smu.Lock()
	s.gen++
	gen := s.gen
	s.loading = true
	s.paintAll()
	s.smu.Unlock()
	go func() {
		cards, err := myCards()
		s.smu.Lock()
		defer s.smu.Unlock()
		if gen != s.gen {
			return
		}
		s.cards = map[string]Card{}
		for _, c := range cards {
			s.cards[c.Key] = c
		}
		s.cardList, s.cardsErr, s.loading = cards, err, false
		s.paintAll()
	}()
}

// checkPlugin looks for the plugin and repaints. A missing claude counts as a missing plugin.
func (s *server) checkPlugin() {
	_, ok, err := pluginInstalled(s.ctx)
	var nf ErrNotFound
	s.smu.Lock()
	defer s.smu.Unlock()
	if err == nil || errors.As(err, &nf) {
		s.pluginMissing = !ok
	}
	s.installing = false
	s.paintAll()
}

func (s *server) installPluginClick() {
	s.smu.Lock()
	if s.installing {
		s.smu.Unlock()
		return
	}
	s.installing, s.pluginErr = true, ""
	s.paintAll()
	s.smu.Unlock()
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		err := installPlugin(s.ctx)
		s.smu.Lock()
		if err != nil {
			s.pluginErr = err.Error()
		}
		s.smu.Unlock()
		s.checkPlugin()
	}()
}

func cardsTree(cards []Card, err error) node {
	refresh := node{"type": "button", "id": "refresh", "label": "Refresh"}
	switch {
	case err != nil:
		return node{"type": "column", "children": []node{{"type": "text", "text": err.Error()}, refresh}}
	case len(cards) == 0:
		return node{"type": "column", "children": []node{{"type": "text", "text": "No open cards assigned to you."}, refresh}}
	}
	items := make([]node, 0, len(cards))
	for _, c := range cards {
		title := c.Summary
		if title == "" {
			title = c.Key
		}
		item := node{"id": c.Key, "title": title}
		if c.Status != "" {
			item["subtitle"] = c.Status
		}
		items = append(items, item)
	}
	return node{"type": "column", "children": []node{
		{"type": "row", "children": []node{
			{"type": "text", "text": "My cards"},
			{"type": "badge", "text": fmt.Sprintf("%d open", len(cards))}}},
		{"type": "list", "fill": true, "items": items},
		refresh}}
}

// prompt is the pre-filled session text; control characters of the summary become spaces.
func prompt(c Card) string {
	clean := strings.Map(func(r rune) rune {
		if unicode.IsControl(r) {
			return ' '
		}
		return r
	}, c.Summary)
	clean = strings.Join(strings.Fields(clean), " ")
	if clean == "" {
		return "Work on " + c.Key
	}
	return "Work on " + c.Key + ": " + clean
}

func (s *server) reply(id json.RawMessage, result any) {
	s.write(node{"jsonrpc": "2.0", "id": id, "result": result})
}

func (s *server) handle(m message) {
	switch m.Method {
	case "initialize":
		var p struct {
			DataDir string `json:"data_dir"`
			Project string `json:"project"`
		}
		json.Unmarshal(m.Params, &p)
		s.smu.Lock()
		s.dataDir, s.project = p.DataDir, p.Project
		s.smu.Unlock()
		s.reply(m.ID, node{})
		s.refreshWork()
		s.wg.Add(2)
		go func() { defer s.wg.Done(); s.checkPlugin() }()
		go func() {
			defer s.wg.Done()
			if p.DataDir == "" {
				return
			}
			if err := updatePluginDaily(s.ctx, p.DataDir, time.Now()); err != nil && s.ctx.Err() == nil {
				fmt.Fprintln(os.Stderr, "wai-waycarbon: plugin update:", err)
			}
		}()
	case "item/action":
		if m.ID != nil {
			s.reply(m.ID, nil)
		}
		s.call("panel/open", node{"panel": panelID})
	case "panel/action":
		var p struct {
			Action string `json:"action_id"`
		}
		json.Unmarshal(m.Params, &p)
		s.reply(m.ID, nil)
		switch p.Action {
		case "refresh":
			s.refreshWork()
			return
		case "plugin:install":
			s.installPluginClick()
			return
		}
		s.smu.Lock()
		c, ok := s.cards[p.Action]
		s.smu.Unlock()
		if ok {
			s.call("sessions/open", node{"prompt": prompt(c), "send": false})
		}
	default:
		if m.ID != nil {
			s.write(node{"jsonrpc": "2.0", "id": m.ID, "error": node{"code": -32601, "message": "Method not found"}})
		}
	}
}

// run reads until EOF. Responses to our own calls (no method) are ignored.
func run(in io.Reader, out io.Writer) {
	ctx, cancel := context.WithCancel(context.Background())
	s := &server{ctx: ctx, out: out, cards: map[string]Card{}, lastBar: jsonString(node{"item": "attention"})}
	r := bufio.NewReaderSize(in, 1<<20)
	for {
		line, err := r.ReadBytes('\n')
		var m message
		if len(line) > 0 && json.Unmarshal(line, &m) == nil && m.Method != "" {
			s.handle(m)
		}
		if err != nil {
			cancel()
			s.wg.Wait()
			return
		}
	}
}

func main() { run(os.Stdin, os.Stdout) }
