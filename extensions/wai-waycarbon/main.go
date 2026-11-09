// wai-waycarbon: a wai extension (line-delimited JSON-RPC 2.0 on stdio) with a "My cards" panel
// over the user's own `jira` CLI.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"sync"
)

const panelID = "cards"

type message struct {
	ID     json.RawMessage `json:"id,omitempty"`
	Method string          `json:"method,omitempty"`
	Params json.RawMessage `json:"params,omitempty"`
}

type node = map[string]any

type server struct {
	mu     sync.Mutex
	out    io.Writer
	nextID int
	cards  map[string]Card
}

func (s *server) write(v any) {
	s.mu.Lock()
	defer s.mu.Unlock()
	b, _ := json.Marshal(v)
	fmt.Fprintf(s.out, "%s\n", b)
}

func (s *server) call(method string, params any) {
	s.nextID++
	s.write(node{"jsonrpc": "2.0", "id": s.nextID, "method": method, "params": params})
}

func (s *server) render() {
	cards, err := myCards()
	s.cards = map[string]Card{}
	var tree node
	refresh := node{"type": "button", "id": "refresh", "label": "Refresh"}
	switch {
	case err != nil:
		tree = node{"type": "column", "children": []node{
			{"type": "text", "text": err.Error()}, refresh}}
	case len(cards) == 0:
		tree = node{"type": "column", "children": []node{
			{"type": "text", "text": "No open cards assigned to you."}, refresh}}
	default:
		items := make([]node, 0, len(cards))
		for _, c := range cards {
			s.cards[c.Key] = c
			item := node{"id": c.Key, "title": c.Summary}
			if c.Status != "" {
				item["subtitle"] = c.Status
			}
			items = append(items, item)
		}
		tree = node{"type": "column", "children": []node{
			{"type": "row", "children": []node{
				{"type": "text", "text": "My cards"},
				{"type": "badge", "text": fmt.Sprintf("%d open", len(cards))}}},
			{"type": "list", "fill": true, "items": items},
			refresh}}
	}
	s.write(node{"jsonrpc": "2.0", "method": "panel/render", "params": node{"panel": panelID, "tree": tree}})
}

func (s *server) handle(m message) {
	var result any
	switch m.Method {
	case "initialize":
		result = node{"panels": []string{}}
	case "panel/action":
		var p struct {
			Action string `json:"action_id"`
		}
		json.Unmarshal(m.Params, &p)
		defer func() {
			if p.Action == "refresh" {
				s.render()
			} else if c, ok := s.cards[p.Action]; ok {
				s.call("sessions/open", node{"prompt": "Work on " + c.Key + ": " + c.Summary, "send": false})
			}
		}()
	}
	if m.ID != nil {
		s.write(node{"jsonrpc": "2.0", "id": m.ID, "result": result})
	}
	if m.Method == "initialize" {
		s.render()
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
