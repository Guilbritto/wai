package main

import (
	"fmt"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"
	"unicode"
)

// Reason is why a PR of mine needs attention, in the order the table lists them.
type Reason int

const (
	Conflict Reason = iota
	CIFailed
	ChangesRequested
	ReadyToMerge
	Waiting
)

func reasonOf(p PR) Reason {
	switch {
	case p.Mergeable == "CONFLICTING":
		return Conflict
	case p.CI == "FAILURE" || p.CI == "ERROR":
		return CIFailed
	case p.Review == "CHANGES_REQUESTED":
		return ChangesRequested
	case p.Review == "APPROVED" && p.CI == "SUCCESS" && !p.Draft:
		return ReadyToMerge
	}
	return Waiting
}

var badges = map[Reason]string{Conflict: "Conflict", CIFailed: "CI failed", ChangesRequested: "Changes requested", ReadyToMerge: "Ready to merge"}

var prKeyRe = regexp.MustCompile(`[A-Z][A-Z0-9]+-[0-9]+`)

// keysOf are the Jira keys in the title and the branch, once each.
func keysOf(p PR) []string {
	var keys []string
	seen := map[string]bool{}
	for _, k := range prKeyRe.FindAllString(p.Title+" "+p.Branch, -1) {
		if !seen[k] {
			seen[k] = true
			keys = append(keys, k)
		}
	}
	return keys
}

type ReviewPR struct {
	PR
	Requested bool
}

type CardRow struct {
	Card
	Open         []PR // by reason, first reason first
	MergedNoOpen bool
}

type Work struct {
	NeedsYou []PR
	Reviews  []ReviewPR
	Cards    []CardRow
	Waiting  []PR
}

func buildWork(s Snapshot, cards []Card) Work {
	var w Work
	mine := append([]PR(nil), s.Mine...)
	sort.SliceStable(mine, func(i, j int) bool { return reasonOf(mine[i]) < reasonOf(mine[j]) })
	for _, p := range mine {
		if reasonOf(p) == Waiting {
			w.Waiting = append(w.Waiting, p)
		} else {
			w.NeedsYou = append(w.NeedsYou, p)
		}
	}
	seen := map[string]bool{}
	for _, p := range s.Requested {
		if !seen[p.URL] {
			seen[p.URL] = true
			w.Reviews = append(w.Reviews, ReviewPR{p, true})
		}
	}
	for _, p := range s.Queue {
		if !seen[p.URL] {
			seen[p.URL] = true
			w.Reviews = append(w.Reviews, ReviewPR{p, false})
		}
	}
	has := func(prs []PR, key string) (out []PR) {
		for _, p := range prs {
			for _, k := range keysOf(p) {
				if k == key {
					out = append(out, p)
					break
				}
			}
		}
		return out
	}
	for _, c := range cards {
		row := CardRow{Card: c, Open: has(mine, c.Key)}
		row.MergedNoOpen = len(row.Open) == 0 && len(has(s.Merged, c.Key)) > 0
		w.Cards = append(w.Cards, row)
	}
	return w
}

func (w Work) merged() (n int) {
	for _, c := range w.Cards {
		if c.MergedNoOpen {
			n++
		}
	}
	return n
}

// requested counts the reviews asked of the user; the team queue does not count.
func (w Work) requested() (n int) {
	for _, p := range w.Reviews {
		if p.Requested {
			n++
		}
	}
	return n
}

// promptFor is the text a click pre-fills; it carries only a URL or a Jira key.
func promptFor(action, arg string) string {
	switch action {
	case "conflict":
		return "Resolve the merge conflicts on " + arg + "."
	case "ci":
		return "The CI fails on " + arg + ". Find out why and fix it."
	case "changes":
		return "/waycarbon:fix-review " + arg
	case "merge":
		return "Merge " + arg + ", then move its Jira card."
	case "review":
		return "/waycarbon:code-review " + arg
	case "solve":
		return "/waycarbon:solve-card " + arg
	case "deploy":
		return "/waycarbon:deploy"
	}
	return "/waycarbon:recheck-review " + arg
}

func promptForPR(p PR) string {
	return promptFor([]string{"conflict", "ci", "changes", "merge", "recheck"}[reasonOf(p)], p.URL)
}

// clickPrompt maps a clicked card id to its prompt. The id must name something in w.
func (w Work) clickPrompt(id string) (string, bool) {
	kind, rest, _ := strings.Cut(id, ":")
	switch kind {
	case "pr":
		all := append(append([]PR(nil), w.NeedsYou...), w.Waiting...)
		for _, c := range w.Cards {
			all = append(all, c.Open...)
		}
		for _, p := range all {
			if p.URL == rest {
				return promptForPR(p), true
			}
		}
	case "review":
		for _, p := range w.Reviews {
			if p.URL == rest {
				return promptFor("review", p.URL), true
			}
		}
	case "reviewall":
		sec, key, _ := strings.Cut(rest, ":")
		for _, s := range w.prSections() {
			for _, g := range groupByCard(s.PRs) {
				if s.ID == sec && g.Key != "" && g.Key == key && len(g.PRs) > 1 {
					return reviewAllPrompt(g.Key, g.PRs), true
				}
			}
		}
	case "card":
		for _, c := range w.Cards {
			switch {
			case c.Key != rest:
			case len(c.Open) > 0:
				return promptForPR(c.Open[0]), true
			case c.MergedNoOpen:
				return promptFor("deploy", ""), true
			default:
				return promptFor("solve", c.Key), true
			}
		}
	}
	return "", false
}

// CardSearch is the Jira tab's search: running (Busy), failed (Err) or done (Cards).
type CardSearch struct {
	Query, Err string
	Busy       bool
	Cards      []Card
}

// clickPrompt: a found card that is not one of mine opens solve-card; mine go through Work.clickPrompt.
func (cs *CardSearch) clickPrompt(id string) (string, bool) {
	key, ok := strings.CutPrefix(id, "card:")
	if cs == nil || !ok {
		return "", false
	}
	for _, c := range cs.Cards {
		if c.Key == key {
			return promptFor("solve", key), true
		}
	}
	return "", false
}

// GhSignIn is a sign-in in progress (Code, once gh printed it) or one that failed (Err).
type GhSignIn struct{ Code, Err string }

type WorkState struct {
	GhMissing, GhSignedOut, JiraMissing bool
	JiraProblem                         string
	SignIn                              *GhSignIn
	Search                              *CardSearch
	Err                                 string
	Login                               string
	Updated                             time.Time
}

// oneLine keeps text from other people on one line, as the panel protocol wants.
func oneLine(s string) string {
	s = strings.Map(func(r rune) rune {
		if unicode.IsControl(r) {
			return ' '
		}
		return r
	}, s)
	s = strings.Join(strings.Fields(s), " ")
	if r := []rune(s); len(r) > 200 {
		s = string(r[:200]) + "…"
	}
	return s
}

func ghLink(url string) node { return node{"type": "link", "text": "Open on GitHub", "url": url} }

var tones = map[Reason]string{Conflict: "danger", CIFailed: "danger", ChangesRequested: "warning", ReadyToMerge: "success"}

// prCard: a reason with a tone puts a toned badge node beside the link (the header badge has no
// tone); any other badge stays in the header.
func prCard(id string, p PR, badge, tone string) node {
	n := node{"type": "card", "id": id, "title": oneLine(fmt.Sprintf("#%d %s", p.Number, p.Title)),
		"subtitle": oneLine(p.Repo), "children": []node{ghLink(p.URL)}}
	if tone == "" {
		n["badge"] = badge
	} else {
		n["children"] = []node{{"type": "row", "children": []node{{"type": "badge", "text": badge, "tone": tone}, ghLink(p.URL)}}}
	}
	return n
}

func section(id, title string, open bool, kids []node) node {
	return node{"type": "section", "id": id, "title": title, "open": open, "badge": strconv.Itoa(len(kids)), "children": kids}
}

// cardRow is a Jira card; its Move to Done button id is done:KEY with the prefix in front, so the
// same card can show on two tabs.
func cardRow(c CardRow, prefix string) node {
	title := oneLine(c.Summary)
	if title == "" {
		title = c.Key
	}
	n := node{"type": "card", "id": "card:" + c.Key, "title": title, "subtitle": strings.TrimRight(oneLine(c.Key+" · "+c.Status), " ·")}
	switch {
	case c.MergedNoOpen:
		n["children"] = []node{{"type": "row", "children": []node{
			{"type": "badge", "text": "PR merged", "tone": "warning"},
			{"type": "button", "id": prefix + "done:" + c.Key, "label": "Move to Done",
				"confirm": node{"title": "Move " + c.Key + " to Done?"}}}}}
	case len(c.Open) > 0:
		p := c.Open[0]
		n["children"] = []node{ghLink(p.URL)}
		if r := reasonOf(p); tones[r] != "" {
			n["children"] = []node{{"type": "row", "children": []node{
				{"type": "badge", "text": "#" + strconv.Itoa(p.Number) + " " + badges[r], "tone": tones[r]}, ghLink(p.URL)}}}
		} else {
			n["badge"] = "#" + strconv.Itoa(p.Number)
		}
	default:
		n["badge"] = oneLine(c.Status)
	}
	return n
}

// problems are the alerts at the top of the tab.
func problems(st WorkState) []node {
	var kids []node
	if st.Err != "" {
		kids = append(kids, node{"type": "alert", "tone": "danger", "text": oneLine(st.Err)})
	}
	switch {
	case st.GhMissing:
		kids = append(kids, node{"type": "alert", "tone": "warning", "text": "GitHub CLI not found"},
			node{"type": "log", "text": "brew install gh", "follow": false})
	case st.GhSignedOut:
		si := st.SignIn
		if si != nil && si.Err == "" {
			code := si.Code
			if code == "" {
				code = "waiting for the code…"
			}
			kids = append(kids, node{"type": "text", "text": "Enter this code on GitHub:"},
				node{"type": "text", "text": code, "mono": true},
				node{"type": "link", "text": "github.com/login/device", "url": "https://github.com/login/device"},
				node{"type": "spinner", "label": "Waiting for GitHub…"},
				node{"type": "button", "id": "gh:cancel", "label": "Cancel"})
			break
		}
		if si != nil {
			kids = append(kids, node{"type": "alert", "tone": "danger", "text": oneLine(si.Err)})
		}
		kids = append(kids, node{"type": "alert", "tone": "warning", "text": "Signed out of GitHub"},
			node{"type": "button", "id": "gh:signin", "label": "Sign in with GitHub", "style": "primary"})
	}
	return append(kids, jiraProblems(st)...)
}

// jiraProblems hold no buttons, so the Work and Jira tabs can both show them.
func jiraProblems(st WorkState) []node {
	switch {
	case st.JiraMissing:
		return []node{{"type": "alert", "tone": "warning", "text": "jira CLI not found"},
			{"type": "log", "text": "brew install ankitpokhrel/jira-cli/jira-cli", "follow": false},
			{"type": "text", "text": "Then run jira init in a terminal.", "muted": true}}
	case st.JiraProblem != "":
		return []node{{"type": "alert", "tone": "warning", "text": oneLine(st.JiraProblem)}}
	}
	return nil
}

// ghProblems are the GitHub alerts for the PRs tab; signing in stays on the Work tab.
func ghProblems(st WorkState) []node {
	var kids []node
	if st.Err != "" {
		kids = append(kids, node{"type": "alert", "tone": "danger", "text": oneLine(st.Err)})
	}
	switch {
	case st.GhMissing:
		kids = append(kids, node{"type": "alert", "tone": "warning", "text": "GitHub CLI not found"})
	case st.GhSignedOut:
		kids = append(kids, node{"type": "alert", "tone": "warning", "text": "Signed out of GitHub: sign in on the Work tab"})
	}
	return kids
}

// headRow is the muted "updated" line with a Refresh button; each tab needs its own button id.
func headRow(text, id string) node {
	return node{"type": "row", "children": []node{
		{"type": "text", "text": text, "muted": true},
		{"type": "button", "id": id, "label": "Refresh", "icon": "refresh-cw"}}}
}

func (st WorkState) head() string {
	head := "updated " + st.Updated.Format("15:04")
	if st.Login != "" {
		head = "@" + oneLine(st.Login) + " · " + head
	}
	return head
}

// mineCard is a PR of mine: its reason, or its state while it waits on others.
func mineCard(p PR) node {
	if r := reasonOf(p); r != Waiting {
		return prCard("pr:"+p.URL, p, badges[r], tones[r])
	}
	state := p.CI
	if p.Draft {
		state = "Draft"
	}
	if state == "" {
		state = "Waiting"
	}
	return prCard("pr:"+p.URL, p, oneLine(strings.ToUpper(state[:1])+strings.ToLower(state[1:])), "")
}

func reviewCard(p ReviewPR) node {
	if p.Requested {
		return prCard("review:"+p.URL, p.PR, "Requested", "")
	}
	return prCard("review:"+p.URL, p.PR, "Review", "")
}

// workTree is the Work tab, an inbox: what needs the user now, from GitHub and Jira.
func workTree(w Work, st WorkState) node {
	kids := append([]node{headRow(st.head(), "work:refresh")}, problems(st)...)
	var needs []node
	for _, p := range w.NeedsYou {
		needs = append(needs, mineCard(p))
	}
	queue := 0
	for _, p := range w.Reviews {
		if p.Requested {
			needs = append(needs, reviewCard(p))
		} else {
			queue++
		}
	}
	for _, c := range w.Cards {
		if c.MergedNoOpen {
			needs = append(needs, cardRow(c, "inbox:"))
		}
	}
	if len(needs) > 0 {
		kids = append(kids, section("needs", "Needs you", true, needs))
	} else {
		kids = append(kids, node{"type": "text", "text": "Nothing needs you right now.", "muted": true})
	}
	kids = append(kids, node{"type": "text", "muted": true,
		"text": fmt.Sprintf("Jira: %d cards · PRs: %d waiting, %d in the team queue", len(w.Cards), len(w.Waiting), queue)})
	return node{"type": "column", "children": kids}
}

// searchRow is the search field; its value follows the search shown, so Clear empties it.
func searchRow(cs *CardSearch) node {
	q := ""
	if cs != nil {
		q = cs.Query
	}
	kids := []node{{"type": "input", "id": "jira:q", "placeholder": "Search any card: EC-1234 or text", "value": q},
		{"type": "button", "id": "jira:search", "label": "Search", "icon": "search"}}
	if cs != nil {
		kids = append(kids, node{"type": "button", "id": "jira:clear", "label": "Clear"})
	}
	return node{"type": "row", "children": kids}
}

// searchTree is the search's results in place of the statuses; a card of mine keeps its PR and Move to Done.
func searchTree(w Work, cs *CardSearch) []node {
	switch {
	case cs.Busy:
		return []node{{"type": "spinner", "label": "Searching…"}}
	case cs.Err != "":
		return []node{{"type": "alert", "tone": "danger", "text": oneLine(cs.Err)}}
	case len(cs.Cards) == 0:
		return []node{{"type": "text", "text": "No card matches " + oneLine(cs.Query) + ".", "muted": true}}
	}
	mine := map[string]CardRow{}
	for _, c := range w.Cards {
		mine[c.Key] = c
	}
	var rows []node
	for _, c := range cs.Cards {
		row, ok := mine[c.Key]
		if !ok {
			row = CardRow{Card: c}
		}
		rows = append(rows, cardRow(row, ""))
	}
	return []node{section("jira:results", "Results", true, rows)}
}

// jiraTree is the Jira tab: my cards by status, statuses in the order jira lists them, or a search's results.
func jiraTree(w Work, st WorkState) node {
	probs := jiraProblems(st)
	kids := append([]node{headRow(fmt.Sprintf("%d cards assigned to you · updated %s", len(w.Cards), st.Updated.Format("15:04")), "jira:refresh"),
		searchRow(st.Search)}, probs...)
	if st.Search != nil {
		return node{"type": "column", "children": append(kids, searchTree(w, st.Search)...)}
	}
	if len(w.Cards) == 0 && len(probs) == 0 {
		return node{"type": "column", "children": append(kids, node{"type": "text", "text": "No cards assigned to you.", "muted": true})}
	}
	var order []string
	by := map[string][]node{}
	for _, c := range w.Cards {
		if by[c.Status] == nil {
			order = append(order, c.Status)
		}
		by[c.Status] = append(by[c.Status], cardRow(c, ""))
	}
	for _, s := range order {
		title := oneLine(s)
		if title == "" {
			title = "No status"
		}
		kids = append(kids, section("jira:"+s, title, true, by[s]))
	}
	return node{"type": "column", "children": kids}
}

// prGroup is the PRs of one Jira card; Key is "" for PRs that name no card.
type prGroup struct {
	Key string
	PRs []PR
}

// groupByCard groups PRs by their first Jira key, in the order they come; PRs with none go last.
func groupByCard(prs []PR) []prGroup {
	var out []prGroup
	var none []PR
	at := map[string]int{}
	for _, p := range prs {
		keys := keysOf(p)
		if len(keys) == 0 {
			none = append(none, p)
			continue
		}
		i, ok := at[keys[0]]
		if !ok {
			i, at[keys[0]] = len(out), len(out)
			out = append(out, prGroup{Key: keys[0]})
		}
		out[i].PRs = append(out[i].PRs, p)
	}
	if len(none) > 0 {
		out = append(out, prGroup{PRs: none})
	}
	return out
}

type prSection struct {
	ID, Title string
	Open      bool
	PRs       []PR
	Card      func(PR) node
}

// prSections are the PRs tab: mine, the reviews asked of me, the team queue.
func (w Work) prSections() []prSection {
	var req, queue []PR
	for _, p := range w.Reviews {
		if p.Requested {
			req = append(req, p.PR)
		} else {
			queue = append(queue, p.PR)
		}
	}
	return []prSection{
		{"mine", "Mine", true, append(append([]PR(nil), w.NeedsYou...), w.Waiting...), mineCard},
		{"req", "Review requests", true, req, func(p PR) node { return reviewCard(ReviewPR{p, true}) }},
		{"queue", "Team queue", false, queue, func(p PR) node { return reviewCard(ReviewPR{p, false}) }},
	}
}

// reviewAllPrompt asks one session to review a card's PRs together: each one, then how they fit.
func reviewAllPrompt(key string, prs []PR) string {
	text := fmt.Sprintf("Review the %d PRs of %s as one change: run /waycarbon:code-review on each, "+
		"then check they fit each other (proto, service, BFF and front contracts).", len(prs), key)
	for _, p := range prs {
		text += "\n" + p.URL
	}
	return text
}

func plural(n int, one string) string {
	if n == 1 {
		return "1 " + one
	}
	return strconv.Itoa(n) + " " + one + "s"
}

// prsTree is the PRs tab: each section groups its PRs by Jira card, and a card with several PRs
// gets a Review all button.
func prsTree(w Work, st WorkState) node {
	kids := append([]node{headRow(st.head(), "prs:refresh")}, ghProblems(st)...)
	summary := map[string]string{}
	for _, c := range w.Cards {
		summary[c.Key] = c.Summary
	}
	for _, s := range w.prSections() {
		if len(s.PRs) == 0 {
			continue
		}
		var groups []node
		for _, g := range groupByCard(s.PRs) {
			var cards []node
			open := s.ID == "req"
			for _, p := range g.PRs {
				cards = append(cards, s.Card(p))
				open = open || s.ID == "mine" && reasonOf(p) != Waiting
			}
			title, id := "No card", "none"
			if g.Key != "" {
				title, id = g.Key+" · "+summary[g.Key], g.Key
				if summary[g.Key] == "" {
					title = g.Key + " · " + g.PRs[0].Title
				}
			}
			if g.Key != "" && len(g.PRs) > 1 {
				cards = append(cards, node{"type": "button", "id": "reviewall:" + s.ID + ":" + g.Key,
					"label": fmt.Sprintf("Review all %d", len(g.PRs)), "style": "primary"})
			}
			groups = append(groups, node{"type": "section", "id": "prs:" + s.ID + ":" + id, "title": oneLine(title),
				"open": open, "badge": plural(len(g.PRs), "PR"), "children": cards})
		}
		kids = append(kids, node{"type": "section", "id": "prs:" + s.ID, "title": s.Title, "open": s.Open,
			"badge": strconv.Itoa(len(s.PRs)), "children": groups})
	}
	if len(kids) == 1 {
		kids = append(kids, node{"type": "text", "text": "No open PRs.", "muted": true})
	}
	return node{"type": "column", "children": kids}
}
