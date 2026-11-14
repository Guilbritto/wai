package main

import (
	"strconv"
)

// counts are what needs the user, per group; Tasks that load PRs and help requests set them.
type counts struct{ NeedsYou, Reviews, Cards, Help, Replies int }

func (c counts) total() int { return c.NeedsYou + c.Reviews + c.Cards + c.Help + c.Replies }

// card is a kv of the non-zero groups.
func (c counts) card() node {
	var items []node
	for _, g := range []struct {
		key string
		n   int
	}{{"Needs you", c.NeedsYou}, {"Review requests", c.Reviews}, {"Cards to update", c.Cards}, {"Help requests", c.Help}, {"Replies to send", c.Replies}} {
		if g.n > 0 {
			items = append(items, node{"key": g.key, "value": strconv.Itoa(g.n)})
		}
	}
	return node{"type": "kv", "items": items}
}

func tab(id, title string, badge int, body node) node {
	t := node{"id": id, "title": title, "children": []node{body}}
	if badge > 0 {
		t["badge"] = strconv.Itoa(badge)
	}
	return t
}

func rootTree(work, help node, workBadge, helpBadge int) node {
	return node{"type": "column", "children": []node{
		{"type": "tabs", "id": "tab", "tabs": []node{tab("work", "Work", workBadge, work), tab("help", "Help", helpBadge, help)}}}}
}

// workBody is the Work tab: the plugin alert, then the cards (or Loading…).
func (s *server) workBody() node {
	kids := []node{}
	switch {
	case s.installing:
		kids = append(kids, node{"type": "spinner", "label": "Installing the WayCarbon plugin…"})
	case s.pluginMissing:
		kids = append(kids, node{"type": "alert", "tone": "warning",
			"text": "The WayCarbon Claude plugin is not installed, so the /waycarbon commands will not work."})
		if s.pluginErr != "" {
			kids = append(kids, node{"type": "alert", "tone": "danger", "text": s.pluginErr})
		}
		kids = append(kids, node{"type": "button", "id": "plugin:install", "label": "Install plugin", "style": "primary"})
	}
	if s.loading {
		kids = append(kids, node{"type": "text", "text": "Loading…", "muted": true})
	} else {
		kids = append(kids, cardsTree(s.cardList, s.cardsErr)["children"].([]node)...)
	}
	return node{"type": "column", "children": kids}
}

func helpBody() node {
	return node{"type": "text", "text": "Help requests need the WayCarbon relay, which is not set up yet.", "muted": true}
}

// paintAll is the only painter: the panel and the bar item. The caller holds smu.
func (s *server) paintAll() {
	c := s.counts
	s.write(node{"jsonrpc": "2.0", "method": "panel/render", "params": node{"panel": panelID,
		"tree": rootTree(s.workBody(), helpBody(), c.NeedsYou+c.Reviews+c.Cards, c.Help+c.Replies)}})
	bar := node{"item": "attention"}
	if n := c.total(); n > 0 {
		bar = node{"item": "attention", "text": strconv.Itoa(n), "icon": "bell", "tone": "warning", "card": c.card()}
	}
	// ponytail: compared as JSON so an unchanged bar sends nothing; the item starts hidden.
	if key := jsonString(bar); key != s.lastBar {
		s.lastBar = key
		s.call("item/update", bar)
	}
}
