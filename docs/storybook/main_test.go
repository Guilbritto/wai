package main

import (
	"bytes"
	"encoding/json"
	"strings"
	"testing"
)

// Every node type of extension API 1.5 (docs/extensions.md, "Trees").
var allTypes = strings.Fields(`column row card section tabs divider spacer text markdown badge icon
	link alert kv progress spinner list table log button menu input select checkbox`)

func walk(v any, depth int, f func(n map[string]any, depth int)) {
	n, ok := v.(map[string]any)
	if !ok {
		return
	}
	f(n, depth)
	for _, c := range asList(n["children"]) {
		walk(c, depth+1, f)
	}
	for _, t := range asList(n["tabs"]) {
		for _, c := range asList(t.(map[string]any)["children"]) {
			walk(c, depth+1, f)
		}
	}
	for _, r := range asList(n["rows"]) {
		for _, c := range asList(r.(map[string]any)["cells"]) {
			walk(c, depth+1, f)
		}
	}
}

func asList(v any) []any { l, _ := v.([]any); return l }

func TestStorybook(t *testing.T) {
	var out bytes.Buffer
	b, err := newBook(&out)
	if err != nil {
		t.Fatal(err)
	}
	b.handle(message{ID: json.RawMessage("0"), Method: "initialize"})
	b.handle(message{ID: json.RawMessage("1"), Method: "panel/action",
		Params: json.RawMessage(`{"panel":"stories","action_id":"bar-update","values":{"bar-text":"Hi","bar-icon":"rocket","bar-tone":"success","bar-card":false}}`)})

	var msgs []struct {
		Method string         `json:"method"`
		Params map[string]any `json:"params"`
	}
	for _, line := range strings.Split(strings.TrimSpace(out.String()), "\n") {
		var m struct {
			Method string         `json:"method"`
			Params map[string]any `json:"params"`
		}
		if err := json.Unmarshal([]byte(line), &m); err != nil {
			t.Fatalf("not one JSON message per line: %q", line)
		}
		msgs = append(msgs, m)
	}
	var methods []string
	for _, m := range msgs {
		methods = append(methods, m.Method)
	}
	if got := strings.Join(methods, ","); got != ",item/update,panel/render,,item/update,panel/render" {
		t.Fatalf("messages: %s", got)
	}
	if item := msgs[1].Params; item["card"] == nil || item["text"] != "Storybook" {
		t.Errorf("first item/update: %v", item)
	}
	if item := msgs[4].Params; item["card"] != nil || item["text"] != "Hi" || item["tone"] != "success" {
		t.Errorf("item/update from the form: %v", item)
	}

	tree := msgs[5].Params["tree"]
	seen := map[string]bool{}
	ids := map[string]bool{}
	nodes, deepest := 0, 0
	walk(tree, 1, func(n map[string]any, depth int) {
		nodes++
		deepest = max(deepest, depth)
		typ, _ := n["type"].(string)
		seen[typ] = true
		switch typ {
		case "button", "input", "select", "checkbox", "section", "tabs":
			if id, ok := n["id"].(string); ok {
				if ids[id] {
					t.Errorf("id %q repeats: wai shows the node as unsupported", id)
				}
				ids[id] = true
			}
		}
	})
	for _, typ := range allTypes {
		if !seen[typ] {
			t.Errorf("no story renders %q", typ)
		}
	}
	if nodes > 2000 || deepest > 32 {
		t.Errorf("tree too large for wai: %d nodes, %d levels", nodes, deepest)
	}
	if !strings.Contains(out.String(), `panel/action  bar-update`) {
		t.Error("the action is not in the Actions log")
	}
}
