package main

import (
	"fmt"
	"strconv"
	"strings"
)

// counts are what needs the user, per group; Reviews leaves out the team queue. The refresh sets them.
type counts struct{ NeedsYou, Reviews, Cards int }

func (c counts) total() int { return c.NeedsYou + c.Reviews + c.Cards }

// card is a kv of the non-zero groups.
func (c counts) card() node {
	var items []node
	for _, g := range []struct {
		key string
		n   int
	}{{"Needs you", c.NeedsYou}, {"Review requests", c.Reviews}, {"Cards to update", c.Cards}} {
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

func rootTree(tabs ...node) node {
	return node{"type": "column", "children": []node{{"type": "tabs", "id": "tab", "tabs": tabs}}}
}

// body is a tab's tree; the Work tab also shows the plugin alert above it.
func (s *server) body(tree node, plugin bool) node {
	kids := []node{}
	switch {
	case !plugin:
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
	return node{"type": "column", "children": append(kids, tree)}
}

// paintAll is the only painter: the panel and the bar item. Tabs stay under Loading… until the first
// refresh; after a paint, the selected items' details load. The caller holds smu.
func (s *server) paintAll() {
	c := s.counts
	sel := map[string]string{}
	build := func(id string, tree func() (node, string)) node {
		if s.refreshing && s.wst.Updated.IsZero() {
			return node{"type": "text", "text": "Loading…", "muted": true}
		}
		n, item := tree()
		sel[id] = item
		return n
	}
	s.write(node{"jsonrpc": "2.0", "method": "panel/render", "params": node{"panel": panelID,
		"tree": rootTree(tab("work", "Work", c.NeedsYou+c.Reviews+c.Cards, s.body(build("work", s.workTab), true)),
			tab("jira", "Jira", c.Cards, s.body(build("jira", s.jiraTab), false)),
			tab("prs", "PRs", c.NeedsYou+c.Reviews, s.body(build("prs", s.prsTab), false)))}})
	bar := node{"item": "attention"}
	if n := c.total(); n > 0 {
		bar = node{"item": "attention", "text": strconv.Itoa(n), "icon": "bell", "tone": "warning", "card": c.card()}
	}
	// ponytail: compared as JSON so an unchanged bar sends nothing; the item starts hidden.
	if key := jsonString(bar); key != s.lastBar {
		s.lastBar = key
		s.call("item/update", bar)
	}
	// ponytail: a refresh clears the details when it ends, so nothing loads while one runs.
	if s.refreshing {
		return
	}
	for tab, id := range sel {
		kind, rest, _ := strings.Cut(id, ":")
		switch {
		case kind == "pr":
			load(s, s.prDetails, rest, func() (PRDetail, error) { return prDetail(s.ctx, rest) })
		case kind == "card" && tab == "jira":
			load(s, s.cardDetails, rest, s.fetchCard(rest))
		}
	}
}

// detail is one item's detail: loading (Busy), failed (Err) or loaded (Data).
type detail[T any] struct {
	Busy bool
	Err  string
	Data T
}

// load fetches m[key] off the caller, once until a refresh clears m. A result whose entry was
// replaced meanwhile is dropped. The caller holds smu.
func load[T any](s *server, m map[string]*detail[T], key string, fetch func() (T, error)) {
	if m[key] != nil || s.ctx.Err() != nil {
		return
	}
	d := &detail[T]{Busy: true}
	m[key] = d
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		data, err := fetch()
		s.smu.Lock()
		defer s.smu.Unlock()
		if m[key] != d {
			return
		}
		d.Busy, d.Data = false, data
		if err != nil {
			d.Err = err.Error()
		}
		s.paintAll()
	}()
}

// choose marks a tab's selected item in its list: the stored one when the list still has it, else the
// first. Item cards have the id sel:<tab>:<item>; it returns the item, "" for an empty list.
func (s *server) choose(tab string, list node) string {
	prefix := "sel:" + tab + ":"
	var items []node
	var walk func(n node)
	walk = func(n node) {
		if id, _ := n["id"].(string); n["type"] == "card" && strings.HasPrefix(id, prefix) {
			items = append(items, n)
		}
		kids, _ := n["children"].([]node)
		for _, k := range kids {
			walk(k)
		}
	}
	walk(list)
	if len(items) == 0 {
		return ""
	}
	pick := items[0]["id"]
	for _, n := range items {
		if n["id"] == prefix+s.sel[tab] {
			pick = n["id"]
		}
	}
	for _, n := range items {
		if n["id"] == pick {
			n["selected"] = true
		}
	}
	return strings.TrimPrefix(pick.(string), prefix)
}

// col is a column, of a fixed width when width > 0.
func col(width int, kids ...node) node {
	n := node{"type": "column", "children": append([]node{}, kids...)}
	if width > 0 {
		n["width"] = width
	}
	return n
}

// split is a list beside its detail, filling the tab; each side scrolls on its own.
func split(left, right node) node {
	return node{"type": "row", "fill": true, "children": []node{left, right}}
}

// toolbar is a tab's top row: lead (search, toggles), then the muted status and Refresh at the end.
func toolbar(text, refreshID string, lead ...node) node {
	return node{"type": "row", "children": append(lead, node{"type": "spacer"},
		node{"type": "text", "text": text, "muted": true},
		node{"type": "button", "id": refreshID, "label": "Refresh", "icon": "refresh-cw"})}
}

func muted(text string) node { return node{"type": "text", "text": text, "muted": true} }

func badge(text, tone string) node { return node{"type": "badge", "text": text, "tone": tone} }

func alert(tone, text string) node { return node{"type": "alert", "tone": tone, "text": oneLine(text)} }

// pretty turns an API state (CHANGES_REQUESTED) into words (Changes requested).
func pretty(s string) string {
	s = strings.ReplaceAll(strings.ToLower(s), "_", " ")
	if s == "" {
		return "Pending"
	}
	return strings.ToUpper(s[:1]) + s[1:]
}

// prBadges are a PR's state: merged or closed, else draft, review, CI and conflict.
func prBadges(p PR) []node {
	switch {
	case p.Merged || p.State == "MERGED":
		return []node{badge("Merged", "info")}
	case p.State == "CLOSED":
		return []node{badge("Closed", "neutral")}
	}
	var b []node
	if p.Draft {
		b = append(b, badge("Draft", "neutral"))
	}
	switch p.Review {
	case "APPROVED":
		b = append(b, badge("Approved", "success"))
	case "CHANGES_REQUESTED":
		b = append(b, badge("Changes requested", "warning"))
	case "REVIEW_REQUIRED":
		b = append(b, badge("Review required", "neutral"))
	}
	switch p.CI {
	case "SUCCESS":
		b = append(b, badge("CI passed", "success"))
	case "FAILURE", "ERROR":
		b = append(b, badge("CI failed", "danger"))
	case "PENDING", "EXPECTED":
		b = append(b, badge("CI running", "warning"))
	}
	if p.Mergeable == "CONFLICTING" {
		b = append(b, badge("Conflict", "danger"))
	}
	return b
}

func checkTone(state string) string {
	switch {
	case failed[state]:
		return "danger"
	case state == "SUCCESS":
		return "success"
	case state == "NEUTRAL" || state == "SKIPPED":
		return "neutral"
	}
	return "warning"
}

var actLabels = map[Reason]string{Conflict: "Resolve conflict", CIFailed: "Fix CI", ChangesRequested: "Fix review",
	ReadyToMerge: "Merge", Waiting: "Recheck review"}

// findPR finds a listed PR and the action its detail offers: what it needs for mine, a review for
// others' and open search results, none for a closed one.
func (s *server) findPR(url string) (p PR, act, label string, ok bool) {
	for _, p := range append(append([]PR(nil), s.work.NeedsYou...), s.work.Waiting...) {
		if p.URL == url {
			return p, "pr:" + url, actLabels[reasonOf(p)], true
		}
	}
	for _, p := range s.work.Reviews {
		if p.URL == url {
			return p.PR, "review:" + url, "Review", true
		}
	}
	if s.prSearch == nil {
		return PR{}, "", "", false
	}
	for _, p := range s.prSearch.PRs {
		switch {
		case p.URL != url:
		case p.State == "OPEN":
			return p, "review:" + url, "Review", true
		default:
			return p, "", "", true
		}
	}
	return PR{}, "", "", false
}

// prDetailView is a PR's header, its action, then what loaded: size, checks, reviews, open threads.
// Section ids carry the tab, so the Work and PRs tabs can show the same PR.
func (s *server) prDetailView(tab string, p PR, act, label string) node {
	d := s.prDetails[p.URL]
	sub := p.Repo
	if p.Branch != "" {
		sub += " · " + p.Branch
	}
	if d != nil && d.Data.Author != "" {
		sub += " · @" + d.Data.Author
	}
	kids := []node{{"type": "card", "title": oneLine(fmt.Sprintf("#%d %s", p.Number, p.Title)), "subtitle": oneLine(sub),
		"children": []node{{"type": "row", "children": append(prBadges(p), ghLink(p.URL))}}}}
	if act != "" {
		kids = append(kids, node{"type": "row", "children": []node{
			{"type": "button", "id": "act:" + tab + ":" + act, "label": label, "style": "primary"}}})
	}
	switch {
	case d == nil || d.Busy:
		return col(0, append(kids, node{"type": "spinner", "label": "Loading the PR…"})...)
	case d.Err != "":
		return col(0, append(kids, alert("danger", d.Err))...)
	}
	pd := d.Data
	kids = append(kids, node{"type": "kv", "items": []node{{"key": "Changes",
		"value": fmt.Sprintf("+%d −%d · %s", pd.Additions, pd.Deletions, plural(pd.Files, "file"))}}})
	var checks, reviews, threads []node
	for _, c := range pd.Checks {
		name := node{"type": "text", "text": oneLine(c.Name)}
		if c.URL != "" {
			name = node{"type": "link", "text": oneLine(c.Name), "url": c.URL}
		}
		checks = append(checks, node{"type": "row", "children": []node{badge(pretty(c.State), checkTone(c.State)), name}})
	}
	for _, r := range pd.Reviews {
		head := "@" + r.By + " · " + pretty(r.State)
		if !r.At.IsZero() {
			head += " · " + r.At.Local().Format("Jan 2 15:04")
		}
		reviews = append(reviews, muted(oneLine(head)))
		if r.Body != "" {
			reviews = append(reviews, node{"type": "text", "text": r.Body})
		}
	}
	for _, t := range pd.Threads {
		where := t.Path
		if t.Line > 0 {
			where += ":" + strconv.Itoa(t.Line)
		}
		if t.Author != "" {
			where += " · @" + t.Author
		}
		threads = append(threads, muted(oneLine(where)), node{"type": "text", "text": t.Body})
	}
	for _, sec := range []struct {
		id, title string
		kids      []node
	}{{"checks", "Checks", checks}, {"reviews", "Reviews", reviews}, {"threads", "Open threads", threads}} {
		if len(sec.kids) > 0 {
			kids = append(kids, node{"type": "section", "id": tab + ":pr-" + sec.id, "title": sec.title, "children": sec.kids})
		}
	}
	return col(0, kids...)
}
