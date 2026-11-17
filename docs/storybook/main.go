// wai-storybook: a wai extension that renders every panel and bar-item component of the
// extension API from stories.json, shows each story's JSON, and logs every action it gets.
package main

import (
	"bufio"
	"bytes"
	_ "embed"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"strings"
	"time"
)

//go:embed stories.json
var storiesJSON []byte

const (
	panelID    = "stories"
	itemID     = "story"
	maxActions = 200
)

type story struct {
	ID    string            `json:"id"`
	Title string            `json:"title"`
	Note  string            `json:"note"`
	Nodes []json.RawMessage `json:"nodes"`
}

type group struct {
	ID      string  `json:"id"`
	Title   string  `json:"title"`
	Stories []story `json:"stories"`
}

type message struct {
	ID     json.RawMessage `json:"id,omitempty"`
	Method string          `json:"method,omitempty"`
	Params json.RawMessage `json:"params,omitempty"`
}

type node = map[string]any

type book struct {
	out     *json.Encoder
	tabs    []any           // the stories, built once
	card    json.RawMessage // the bar item's hover card: the item-card story
	actions []string
	last    string
}

func newBook(out io.Writer) (*book, error) {
	var groups []group
	if err := json.Unmarshal(storiesJSON, &groups); err != nil {
		return nil, fmt.Errorf("stories.json: %w", err)
	}
	b := &book{out: json.NewEncoder(out), last: "none yet"}
	for _, g := range groups {
		cards := make([]any, 0, len(g.Stories))
		for _, s := range g.Stories {
			cards = append(cards, storyCard(s))
			if s.ID == "item-card" {
				b.card = s.Nodes[0]
			}
		}
		b.tabs = append(b.tabs, node{"id": g.ID, "title": g.Title, "badge": fmt.Sprint(len(g.Stories)), "children": cards})
	}
	return b, nil
}

// storyCard is a story's nodes in a card, then their JSON in a closed section.
func storyCard(s story) node {
	children := make([]any, 0, len(s.Nodes)+1)
	src := make([]string, 0, len(s.Nodes))
	for _, n := range s.Nodes {
		children = append(children, n)
		var buf bytes.Buffer
		json.Indent(&buf, n, "", "  ") // keeps the key order of stories.json
		src = append(src, buf.String())
	}
	children = append(children, node{"type": "section", "id": "src:" + s.ID, "title": "JSON", "open": false,
		"children": []any{node{"type": "log", "text": strings.Join(src, ",\n"), "follow": false}}})
	return node{"type": "card", "title": s.Title, "subtitle": s.Note, "children": children}
}

func (b *book) send(m node) {
	m["jsonrpc"] = "2.0"
	b.out.Encode(m)
}

func (b *book) tree() node {
	actions := node{"id": "actions", "title": "Actions", "badge": fmt.Sprint(len(b.actions)), "children": []any{
		node{"type": "log", "fill": true, "text": strings.Join(b.actions, "\n")},
	}}
	return node{"type": "column", "children": []any{
		node{"type": "row", "children": []any{
			node{"type": "icon", "name": "book-open", "tone": "info"},
			node{"type": "text", "text": "Extension API 1.5"},
			node{"type": "spacer"},
			node{"type": "text", "text": "Last action: " + b.last, "mono": true, "muted": true},
		}},
		node{"type": "tabs", "id": "stories", "tabs": append(b.tabs[:len(b.tabs):len(b.tabs)], actions)},
	}}
}

func (b *book) render() {
	b.send(node{"method": "panel/render", "params": node{"panel": panelID, "tree": b.tree()}})
}

func (b *book) record(method, id string, values json.RawMessage) {
	b.last = id
	line := time.Now().Format("15:04:05") + "  " + method + "  " + id
	if len(values) > 0 && string(values) != "null" {
		line += "\n          values " + string(values)
	}
	b.actions = append(b.actions, line)
	if len(b.actions) > maxActions {
		b.actions = b.actions[len(b.actions)-maxActions:]
	}
}

func (b *book) updateItem(text, icon, tone string, card bool) {
	p := node{"item": itemID, "text": text, "icon": icon, "tone": tone, "tooltip": "wai storybook: click to open"}
	if card {
		p["card"] = b.card
	}
	b.send(node{"method": "item/update", "params": p})
}

func (b *book) handle(m message) {
	switch m.Method {
	case "initialize":
		b.send(node{"id": m.ID, "result": node{}})
		b.updateItem("Storybook", "book-open", "info", true)
		b.render()
	case "panel/action":
		b.send(node{"id": m.ID, "result": nil})
		var p struct {
			ActionID string          `json:"action_id"`
			Values   json.RawMessage `json:"values"`
		}
		json.Unmarshal(m.Params, &p)
		b.record("panel/action", p.ActionID, p.Values)
		switch p.ActionID {
		case "bar-update":
			var v struct {
				Text string `json:"bar-text"`
				Icon string `json:"bar-icon"`
				Tone string `json:"bar-tone"`
				Card bool   `json:"bar-card"`
			}
			json.Unmarshal(p.Values, &v)
			b.updateItem(v.Text, v.Icon, v.Tone, v.Card)
		case "bar-hide":
			b.send(node{"method": "item/update", "params": node{"item": itemID}})
		}
		b.render()
	case "item/action", "command/run":
		b.send(node{"id": m.ID, "result": nil})
		b.record(m.Method, string(m.Params), nil)
		b.send(node{"method": "panel/open", "params": node{"panel": panelID}})
		b.render()
	default:
		if m.ID != nil { // a request this extension does not serve
			b.send(node{"id": m.ID, "error": node{"code": -32601, "message": "Method not found: " + m.Method}})
		}
	}
}

func main() {
	b, err := newBook(os.Stdout)
	if err != nil {
		fmt.Fprintln(os.Stderr, "wai-storybook:", err)
		os.Exit(1)
	}
	in := bufio.NewScanner(os.Stdin)
	in.Buffer(make([]byte, 64*1024), 4<<20) // wai sends lines of up to 4 MB
	for in.Scan() {
		var m message
		if json.Unmarshal(in.Bytes(), &m) == nil && m.Method != "" {
			b.handle(m)
		}
	}
}
