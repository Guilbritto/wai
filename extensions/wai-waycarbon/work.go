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
	// A failing check that is not required (SonarCloud) leaves an approved PR UNSTABLE or BEHIND: it still merges.
	case p.Review == "APPROVED" && !p.Draft && (p.MergeState == "UNSTABLE" || p.MergeState == "BEHIND"):
		return ReadyToMerge
	case p.CI == "FAILURE" || p.CI == "ERROR":
		return CIFailed
	case p.Review == "CHANGES_REQUESTED":
		return ChangesRequested
	// Approved is enough: a failing CI is caught above, a running one is the merge's to wait for.
	case p.Review == "APPROVED" && !p.Draft:
		return ReadyToMerge
	}
	return Waiting
}

var badges = map[Reason]string{Conflict: "Conflict", CIFailed: "CI failed", ChangesRequested: "Changes requested", ReadyToMerge: "Approved"}

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
	Merged       []PR
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
		row := CardRow{Card: c, Open: has(mine, c.Key), Merged: has(s.Merged, c.Key)}
		row.MergedNoOpen = len(row.Open) == 0 && len(row.Merged) > 0
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

// newReviews are my PRs with a review newer than seen has for them; seen, the newest review time per
// PR URL, is updated. A PR seen for the first time counts every review as new.
func newReviews(seen map[string]time.Time, mine []PR) (out []PR) {
	for _, p := range mine {
		if at := p.LastReview.At; at.After(seen[p.URL]) {
			seen[p.URL] = at
			out = append(out, p)
		}
	}
	return out
}

var reviewVerbs = map[string]string{"APPROVED": "approved", "CHANGES_REQUESTED": "requested changes on", "COMMENTED": "commented on"}

// reviewNote is one notification for them all: wai drops a second one within 2 s.
func reviewNote(prs []PR) (title, body string) {
	var lines []string
	for _, p := range prs {
		lines = append(lines, fmt.Sprintf("@%s %s #%d", p.LastReview.By, reviewVerbs[p.LastReview.State], p.Number))
	}
	if len(prs) == 1 {
		return oneLine(lines[0]), oneLine(prs[0].Title)
	}
	return plural(len(prs), "PR") + " reviewed", oneLine(strings.Join(lines, ", "))
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
		return "Merge " + arg + " once its required checks pass, then move its Jira card."
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

// PRSearch is the PRs tab's search, over every PR of the org.
type PRSearch struct {
	Query, Err string
	Busy       bool
	PRs        []PR
}

// clickPrompt: an open PR found by the search opens a review.
func (ps *PRSearch) clickPrompt(id string) (string, bool) {
	url, ok := strings.CutPrefix(id, "review:")
	if ps == nil || !ok {
		return "", false
	}
	for _, p := range ps.PRs {
		if p.URL == url && p.State == "OPEN" {
			return promptFor("review", url), true
		}
	}
	return "", false
}

// GhSignIn is a sign-in in progress (Code, once gh printed it) or one that failed (Err).
type GhSignIn struct{ Code, Err string }

type WorkState struct {
	GhMissing, GhSignedOut, JiraMissing bool
	JiraProblem, SprintErr              string
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
	return clip(strings.Join(strings.Fields(s), " "), 200)
}

// clip keeps the first n runes, with an ellipsis when it cut.
func clip(s string, n int) string {
	if r := []rune(s); len(r) > n {
		return string(r[:n]) + "…"
	}
	return s
}

func ghLink(url string) node { return node{"type": "link", "text": "Open on GitHub", "url": url} }

var tones = map[Reason]string{Conflict: "danger", CIFailed: "danger", ChangesRequested: "warning", ReadyToMerge: "success"}

// prItem is a PR in a tab's list; a toned badge goes below the title (the header badge has no tone).
func prItem(tab string, p PR, text, tone string) node {
	sub := p.Repo
	if keys := keysOf(p); len(keys) > 0 {
		sub += " · " + keys[0]
	}
	n := node{"type": "card", "id": "sel:" + tab + ":pr:" + p.URL, "title": oneLine(fmt.Sprintf("#%d %s", p.Number, p.Title)),
		"subtitle": oneLine(sub)}
	switch {
	case tone != "":
		n["children"] = []node{badge(text, tone)}
	case text != "":
		n["badge"] = text
	}
	return n
}

// mineBadge is why a PR of mine needs me, or its state while it waits on others.
func mineBadge(p PR) (string, string) {
	if r := reasonOf(p); r != Waiting {
		return badges[r], tones[r]
	}
	state := p.CI
	if p.Draft {
		state = "Draft"
	}
	if state == "" {
		state = "Waiting"
	}
	return oneLine(strings.ToUpper(state[:1]) + strings.ToLower(state[1:])), ""
}

func section(id, title string, open bool, kids []node) node {
	return node{"type": "section", "id": id, "title": title, "open": open, "badge": strconv.Itoa(len(kids)), "children": kids}
}

func cardTitle(c Card) string {
	if t := oneLine(c.Summary); t != "" {
		return t
	}
	return c.Key
}

// problems are the alerts at the top of the tab.
func problems(st WorkState) []node {
	var kids []node
	if st.Err != "" {
		kids = append(kids, alert("danger", st.Err))
	}
	switch {
	case st.GhMissing:
		kids = append(kids, alert("warning", "GitHub CLI not found"), node{"type": "log", "text": "brew install gh", "follow": false})
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
			kids = append(kids, alert("danger", si.Err))
		}
		kids = append(kids, alert("warning", "Signed out of GitHub"),
			node{"type": "button", "id": "gh:signin", "label": "Sign in with GitHub", "style": "primary"})
	}
	return append(kids, jiraProblems(st)...)
}

// jiraProblems hold no buttons, so the Work and Jira tabs can both show them.
func jiraProblems(st WorkState) []node {
	switch {
	case st.JiraMissing:
		return []node{alert("warning", "jira CLI not found"),
			{"type": "log", "text": "brew install ankitpokhrel/jira-cli/jira-cli", "follow": false},
			muted("Then run jira init in a terminal.")}
	case st.JiraProblem != "":
		return []node{alert("warning", st.JiraProblem)}
	}
	return nil
}

// ghProblems are the GitHub alerts for the PRs tab; signing in stays on the Work tab.
func ghProblems(st WorkState) []node {
	var kids []node
	if st.Err != "" {
		kids = append(kids, alert("danger", st.Err))
	}
	switch {
	case st.GhMissing:
		kids = append(kids, alert("warning", "GitHub CLI not found"))
	case st.GhSignedOut:
		kids = append(kids, alert("warning", "Signed out of GitHub: sign in on the Work tab"))
	}
	return kids
}

func (st WorkState) head() string {
	head := "updated " + st.Updated.Format("15:04")
	if st.Login != "" {
		head = "@" + oneLine(st.Login) + " · " + head
	}
	return head
}

// workList is the Work tab's inbox: what needs the user now, from GitHub and Jira, then what waits.
func workList(w Work) node {
	var needs, reviews, update, waiting []node
	for _, p := range w.NeedsYou {
		text, tone := mineBadge(p)
		needs = append(needs, prItem("work", p, text, tone))
	}
	for _, p := range w.Reviews {
		if p.Requested {
			reviews = append(reviews, prItem("work", p.PR, "Requested", ""))
		}
	}
	for _, c := range w.Cards {
		if c.MergedNoOpen {
			update = append(update, node{"type": "card", "id": "sel:work:card:" + c.Key, "title": cardTitle(c.Card),
				"subtitle": c.Key, "children": []node{badge("PR merged", "warning")}})
		}
	}
	for _, p := range w.Waiting {
		text, _ := mineBadge(p)
		waiting = append(waiting, prItem("work", p, text, ""))
	}
	var kids []node
	for _, s := range []struct {
		id, title string
		kids      []node
	}{{"needs", "Needs you", needs}, {"requested", "Review requests", reviews}, {"update", "Cards to update", update},
		{"waiting", "Waiting on others", waiting}} {
		if len(s.kids) > 0 {
			kids = append(kids, section("work:"+s.id, s.title, true, s.kids))
		}
	}
	if len(kids) == 0 {
		kids = []node{muted("Nothing needs you right now.")}
	}
	return col(380, kids...)
}

// workTab is the inbox beside the selected item's detail.
func (s *server) workTab() (node, string) {
	list := workList(s.work)
	id := s.choose("work", list)
	kids := append([]node{toolbar(s.wst.head(), "work:refresh")}, problems(s.wst)...)
	return col(0, append(kids, split(list, s.workDetail(id)))...), id
}

func (s *server) workDetail(id string) node {
	kind, rest, _ := strings.Cut(id, ":")
	switch kind {
	case "pr":
		if p, act, label, ok := s.findPR(rest); ok {
			return s.prDetailView("work", p, act, label)
		}
	case "card":
		for _, c := range s.work.Cards {
			if c.Key == rest && c.MergedNoOpen {
				return updateDetail(c)
			}
		}
	}
	return col(0, muted("Select an item."))
}

// updateDetail is a card whose PR merged: deploy it, or move it to Done.
func updateDetail(c CardRow) node {
	row := []node{badge("PR merged", "warning")}
	if c.Status != "" {
		row = append([]node{badge(oneLine(c.Status), "neutral")}, row...)
	}
	kids := []node{{"type": "card", "title": cardTitle(c.Card), "subtitle": c.Key, "children": []node{{"type": "row", "children": row}}}}
	for _, p := range c.Merged {
		kids = append(kids, node{"type": "link", "text": oneLine(fmt.Sprintf("%s#%d %s", p.Repo, p.Number, p.Title)), "url": p.URL})
	}
	return col(0, append(kids, node{"type": "row", "children": []node{
		{"type": "button", "id": "act:work:card:" + c.Key, "label": "Deploy", "style": "primary"},
		{"type": "button", "id": "act:work:done:" + c.Key, "label": "Move to Done", "confirm": node{"title": "Move " + c.Key + " to Done?"}}}})...)
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
	Item      func(PR) node
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
	mine := func(p PR) node { text, tone := mineBadge(p); return prItem("prs", p, text, tone) }
	other := func(p PR) node { return prItem("prs", p, "", "") }
	return []prSection{
		{"mine", "Mine", true, append(append([]PR(nil), w.NeedsYou...), w.Waiting...), mine},
		{"req", "Review requests", true, req, other},
		{"queue", "Team queue", false, queue, other},
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

// groupItem is a card with several PRs: selecting it shows them together, with Review all.
func groupItem(sec string, g prGroup, summary string) node {
	title := g.Key + " · " + summary
	if summary == "" {
		title = g.Key + " · " + g.PRs[0].Title
	}
	return node{"type": "card", "id": "sel:prs:group:" + sec + ":" + g.Key, "icon": "layers", "title": oneLine(title),
		"badge": plural(len(g.PRs), "PR")}
}

// items lists PRs by card: a card with several PRs gets a group item above them.
func items(sec string, prs []PR, summary map[string]string, item func(PR) node) []node {
	var kids []node
	for _, g := range groupByCard(prs) {
		if g.Key != "" && len(g.PRs) > 1 {
			kids = append(kids, groupItem(sec, g, summary[g.Key]))
		}
		for _, p := range g.PRs {
			kids = append(kids, item(p))
		}
	}
	return kids
}

func (w Work) summaries() map[string]string {
	m := map[string]string{}
	for _, c := range w.Cards {
		m[c.Key] = c.Summary
	}
	return m
}

func prList(w Work) node {
	var kids []node
	for _, s := range w.prSections() {
		if len(s.PRs) > 0 {
			kids = append(kids, node{"type": "section", "id": "prs:" + s.ID, "title": s.Title, "open": s.Open,
				"badge": strconv.Itoa(len(s.PRs)), "children": items(s.ID, s.PRs, w.summaries(), s.Item)})
		}
	}
	if len(kids) == 0 {
		kids = []node{muted("No open PRs.")}
	}
	return col(420, kids...)
}

// searchList is a PR search's results by card, newest first, each with its state.
func searchList(ps *PRSearch) node {
	switch {
	case ps.Busy:
		return col(420, node{"type": "spinner", "label": "Searching…"})
	case ps.Err != "":
		return col(420, alert("danger", ps.Err))
	case len(ps.PRs) == 0:
		return col(420, muted("No PR matches "+oneLine(ps.Query)+"."))
	}
	cards := 0
	for _, g := range groupByCard(ps.PRs) {
		if g.Key != "" {
			cards++
		}
	}
	head := muted(fmt.Sprintf("%s, %s for %s", plural(cards, "card"), plural(len(ps.PRs), "PR"), oneLine(ps.Query)))
	state := func(p PR) node { return prItem("prs", p, pretty(p.State), "") }
	return col(420, append([]node{head}, items("search", ps.PRs, nil, state)...)...)
}

func prSearchRow(ps *PRSearch) []node {
	q := ""
	if ps != nil {
		q = ps.Query
	}
	kids := []node{{"type": "input", "id": "prs:q", "placeholder": "Search any PR: EC-1234 or text", "value": q},
		{"type": "button", "id": "prs:search", "label": "Search", "icon": "search"}}
	if ps != nil {
		kids = append(kids, node{"type": "button", "id": "prs:clear", "label": "Clear"})
	}
	return kids
}

// prsTab is the PRs by card, or a search over the org's, beside the selected PR or card.
func (s *server) prsTab() (node, string) {
	list := prList(s.work)
	if s.prSearch != nil {
		list = searchList(s.prSearch)
	}
	id := s.choose("prs", list)
	kids := append([]node{toolbar(s.wst.head(), "prs:refresh", prSearchRow(s.prSearch)...)}, ghProblems(s.wst)...)
	return col(0, append(kids, split(list, s.prsDetail(id)))...), id
}

// group finds the PRs of a group item: a section's or the search's.
func (s *server) group(sec, key string) []PR {
	var prs []PR
	if sec == "search" && s.prSearch != nil {
		prs = s.prSearch.PRs
	}
	for _, ps := range s.work.prSections() {
		if ps.ID == sec {
			prs = ps.PRs
		}
	}
	for _, g := range groupByCard(prs) {
		if key != "" && g.Key == key {
			return g.PRs
		}
	}
	return nil
}

func (s *server) prsDetail(id string) node {
	kind, rest, _ := strings.Cut(id, ":")
	switch kind {
	case "pr":
		if p, act, label, ok := s.findPR(rest); ok {
			return s.prDetailView("prs", p, act, label)
		}
	case "group":
		sec, key, _ := strings.Cut(rest, ":")
		prs := s.group(sec, key)
		if len(prs) == 0 {
			break
		}
		title := key
		if sum := s.work.summaries()[key]; sum != "" {
			title += " · " + sum
		}
		kids := []node{{"type": "card", "title": oneLine(title), "subtitle": plural(len(prs), "PR")}}
		for _, p := range prs {
			kids = append(kids, node{"type": "row", "children": append(prBadges(p),
				node{"type": "link", "text": oneLine(fmt.Sprintf("%s#%d %s", p.Repo, p.Number, p.Title)), "url": p.URL})})
		}
		if sec != "search" {
			kids = append(kids, node{"type": "row", "children": []node{{"type": "button", "id": "act:prs:reviewall:" + sec + ":" + key,
				"label": fmt.Sprintf("Review all %d", len(prs)), "style": "primary"}}})
		}
		return col(0, kids...)
	}
	return col(0, muted("Select a PR."))
}
