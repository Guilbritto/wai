package main

import (
	"fmt"
	"slices"
	"strconv"
	"strings"
	"sync"
)

// boardOrder is the Interstellar board's statuses, left to right; the Move to menu offers them.
var boardOrder = []string{"Tech Refinement", "To Do", "Ready For Dev", "Development", "Development Done",
	"Code Review", "Code Review Done", "Ready For Test", "Test", "Test Done"}

// cardItem is a card in the Jira tab's board or search results.
func cardItem(c Card, badge string) node {
	n := node{"type": "card", "id": "sel:jira:card:" + c.Key, "title": cardTitle(c), "subtitle": c.Key}
	if badge != "" {
		n["badge"] = badge
	}
	return n
}

// board is a column per status in boardOrder, then statuses it does not know in the order first seen.
// Statuses match without case. An empty column is a thin strip: its count and its name, one letter per line.
func board(cards []Card, prs map[string]int) node {
	names := append([]string(nil), boardOrder...)
	by := map[string][]Card{}
	for _, c := range cards {
		k := strings.ToLower(c.Status)
		if !slices.ContainsFunc(names, func(n string) bool { return strings.ToLower(n) == k }) {
			names = append(names, c.Status)
		}
		by[k] = append(by[k], c)
	}
	var cols []node
	for _, name := range names {
		cs := by[strings.ToLower(name)]
		if name = oneLine(name); name == "" {
			name = "No status"
		}
		if len(cs) == 0 {
			cols = append(cols, col(28, muted("0"), muted(strings.Join(strings.Split(name, ""), "\n"))))
			continue
		}
		kids := []node{muted(fmt.Sprintf("%s · %d", name, len(cs)))}
		for _, c := range cs {
			b := ""
			if n := prs[c.Key]; n > 0 {
				b = plural(n, "PR")
			}
			kids = append(kids, cardItem(c, b))
		}
		cols = append(cols, col(0, kids...))
	}
	return node{"type": "row", "children": cols}
}

// cardList is a card search's results, each with its status.
func cardList(cs *CardSearch) node {
	switch {
	case cs.Busy:
		return col(0, node{"type": "spinner", "label": "Searching…"})
	case cs.Err != "":
		return col(0, alert("danger", cs.Err))
	case len(cs.Cards) == 0:
		return col(0, muted("No card matches "+oneLine(cs.Query)+"."))
	}
	kids := []node{muted(plural(len(cs.Cards), "card") + " for " + oneLine(cs.Query))}
	for _, c := range cs.Cards {
		kids = append(kids, cardItem(c, oneLine(c.Status)))
	}
	return col(0, kids...)
}

// searchRow is the card search field; its value follows the search shown, so Clear empties it.
func searchRow(cs *CardSearch) []node {
	q := ""
	if cs != nil {
		q = cs.Query
	}
	kids := []node{{"type": "input", "id": "jira:q", "placeholder": "Search any card: EC-1234 or text", "value": q},
		{"type": "button", "id": "jira:search", "label": "Search", "icon": "search"}}
	if cs != nil {
		kids = append(kids, node{"type": "button", "id": "jira:clear", "label": "Clear"})
	}
	return kids
}

// cards are the board's: the current sprint's, or all assigned to me.
func (s *server) cards() []Card {
	if s.scope == "mine" {
		var cs []Card
		for _, c := range s.work.Cards {
			cs = append(cs, c.Card)
		}
		return cs
	}
	return s.sprint
}

// jiraTab is the board, or a search's results, beside the selected card.
func (s *server) jiraTab() (node, string) {
	cards := s.cards()
	prs := map[string]int{}
	for _, p := range append(append([]PR(nil), s.work.NeedsYou...), s.work.Waiting...) {
		for _, k := range keysOf(p) {
			prs[k]++
		}
	}
	left := board(cards, prs)
	if s.wst.Search != nil {
		left = cardList(s.wst.Search)
	}
	id := s.choose("jira", left)
	scope := func(id, label string, on bool) node {
		b := node{"type": "button", "id": "jira:scope:" + id, "label": label}
		if on {
			b["style"] = "primary"
		}
		return b
	}
	lead := append(searchRow(s.wst.Search), scope("sprint", "Sprint atual", s.scope != "mine"), scope("mine", "Todos os meus", s.scope == "mine"))
	text := fmt.Sprintf("%s · updated %s", plural(len(cards), "card"), s.wst.Updated.Format("15:04"))
	kids := append([]node{toolbar(text, "jira:refresh", lead...)}, jiraProblems(s.wst)...)
	if s.scope != "mine" && s.wst.SprintErr != "" {
		kids = append(kids, alert("warning", "Could not load the current sprint: "+s.wst.SprintErr))
	}
	return col(0, append(kids, split(left, col(420, s.cardDetail(id)...)))...), id
}

// findCard is a listed card: on the board (either scope) or in the search.
func (s *server) findCard(key string) (Card, bool) {
	all := append([]Card(nil), s.sprint...)
	for _, c := range s.work.Cards {
		all = append(all, c.Card)
	}
	if s.wst.Search != nil {
		all = append(all, s.wst.Search.Cards...)
	}
	for _, c := range all {
		if c.Key == key {
			return c, true
		}
	}
	return Card{}, false
}

// cardData is a card's detail and the org's PRs that name it.
type cardData struct {
	CardDetail
	PRs   []PR
	PRErr string
}

// fetchCard loads the card and its PRs in parallel; a PR search failure keeps the card.
func (s *server) fetchCard(key string) func() (cardData, error) {
	return func() (cardData, error) {
		var d cardData
		var err, prErr error
		var wg sync.WaitGroup
		wg.Add(2)
		go func() { defer wg.Done(); d.CardDetail, err = viewCard(s.ctx, key) }()
		go func() { defer wg.Done(); d.PRs, prErr = searchPRs(s.ctx, org(), key) }()
		wg.Wait()
		// GitHub's search matches words: EC-12 also finds EC-12 in "EC-12-1"; keep the PRs that name the card.
		d.PRs = slices.DeleteFunc(d.PRs, func(p PR) bool { return !slices.Contains(keysOf(p), key) })
		if prErr != nil {
			d.PRErr = prErr.Error()
		}
		return d, err
	}
}

// cardDetail is the selected card: its actions, then what loaded.
func (s *server) cardDetail(id string) []node {
	key, _ := strings.CutPrefix(id, "card:")
	c, ok := s.findCard(key)
	if !ok {
		return []node{muted("Select a card.")}
	}
	var moves []node
	for _, st := range boardOrder {
		if !strings.EqualFold(st, c.Status) {
			moves = append(moves, node{"id": "move:" + c.Key + ":" + st, "label": st,
				"confirm": node{"title": "Move " + c.Key + " to " + st + "?"}})
		}
	}
	actions := []node{{"type": "button", "id": "act:jira:solve:" + c.Key, "label": "Solve card", "style": "primary"},
		{"type": "menu", "label": "Move to", "icon": "arrow-right", "items": moves}}
	d := s.cardDetails[c.Key]
	if d != nil && d.Data.Site != "" {
		actions = append(actions, node{"type": "link", "text": "Open in Jira", "url": d.Data.Site + "/browse/" + c.Key})
	}
	head := node{"type": "card", "title": cardTitle(c), "subtitle": c.Key}
	if c.Status != "" {
		head["badge"] = oneLine(c.Status)
	}
	kids := []node{head, {"type": "row", "children": actions}}
	if s.moveErr != "" {
		kids = append(kids, alert("danger", s.moveErr))
	}
	switch {
	case d == nil || d.Busy:
		return append(kids, node{"type": "spinner", "label": "Loading the card…"})
	case d.Err != "":
		return append(kids, alert("danger", d.Err))
	}
	cd := d.Data
	var meta []node
	parent := strings.TrimSuffix(cd.ParentKey+" · "+cd.ParentSummary, " · ")
	for _, kv := range [][2]string{{"Type", cd.Type}, {"Priority", cd.Priority}, {"Parent", strings.TrimPrefix(parent, " · ")}, {"Assignee", cd.Assignee}} {
		if kv[1] != "" {
			meta = append(meta, node{"key": kv[0], "value": oneLine(kv[1])})
		}
	}
	if len(meta) > 0 {
		kids = append(kids, node{"type": "kv", "items": meta})
	}
	if cd.Description != "" {
		kids = append(kids, node{"type": "text", "text": cd.Description})
	}
	if len(cd.Subtasks) > 0 {
		var subs []node
		for _, t := range cd.Subtasks {
			subs = append(subs, node{"key": t.Key, "value": oneLine(strings.TrimSuffix(t.Summary+" · "+t.Status, " · "))})
		}
		kids = append(kids, node{"type": "section", "id": "jira:subtasks", "title": "Sub-tasks",
			"badge": strconv.Itoa(len(subs)), "children": []node{{"type": "kv", "items": subs}}})
	}
	var prs []node
	for _, p := range cd.PRs {
		prs = append(prs, node{"type": "row", "children": append(prBadges(p),
			node{"type": "link", "text": oneLine(fmt.Sprintf("%s#%d %s", p.Repo, p.Number, p.Title)), "url": p.URL})})
	}
	if cd.PRErr != "" {
		prs = append(prs, alert("warning", cd.PRErr))
	}
	if len(prs) > 0 {
		kids = append(kids, node{"type": "section", "id": "jira:prs", "title": "Pull requests", "children": prs})
	}
	return kids
}
