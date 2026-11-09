// wai-waycarbon: a wai extension (line-delimited JSON-RPC 2.0 on stdio) with a "My cards" panel
// over the user's own `jira` CLI.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"strings"
	"sync"
	"unicode"
)

const panelID = "cards"

type message struct {
	ID     json.RawMessage `json:"id,omitempty"`
	Method string          `json:"method,omitempty"`
	Params json.RawMessage `json:"params,omitempty"`
}

type node = map[string]any

type server struct {
	wmu sync.Mutex // serialises writes
	out io.Writer

	smu    sync.Mutex // guards the rest
	nextID int
	gen    int // the latest render; an older result is stale
	cards  map[string]Card
}

func (s *server) write(v any) {
	s.wmu.Lock()
	defer s.wmu.Unlock()
	b, _ := json.Marshal(v)
	fmt.Fprintf(s.out, "%s\n", b)
}

func (s *server) call(method string, params any) {
	s.smu.Lock()
	s.nextID++
	id := s.nextID
	s.smu.Unlock()
	s.write(node{"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

func (s *server) paint(tree node) {
	s.write(node{"jsonrpc": "2.0", "method": "panel/render", "params": node{"panel": panelID, "tree": tree}})
}

// render shows Loading… now and the cards when jira answers, off the message loop.
func (s *server) render() {
	s.smu.Lock()
	s.gen++
	gen := s.gen
	s.paint(node{"type": "text", "text": "Loading…", "muted": true})
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
		s.paint(cardsTree(cards, err))
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
		s.reply(m.ID, node{"panels": []string{}})
		s.render()
	case "panel/action":
		var p struct {
			Action string `json:"action_id"`
		}
		json.Unmarshal(m.Params, &p)
		s.reply(m.ID, nil)
		if p.Action == "refresh" {
			s.render()
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
	s := &server{out: out, cards: map[string]Card{}}
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

func main() { run(os.Stdin, os.Stdout) }
