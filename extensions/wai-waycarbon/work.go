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

// GhSignIn is a sign-in in progress (Code, once gh printed it) or one that failed (Err).
type GhSignIn struct{ Code, Err string }

type WorkState struct {
	GhMissing, GhSignedOut, JiraMissing bool
	JiraProblem                         string
	SignIn                              *GhSignIn
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

func prCard(id string, p PR, badge string) node {
	return node{"type": "card", "id": id, "title": oneLine(fmt.Sprintf("#%d %s", p.Number, p.Title)),
		"subtitle": oneLine(p.Repo), "badge": badge, "children": []node{ghLink(p.URL)}}
}

func section(id, title string, open bool, kids []node) node {
	return node{"type": "section", "id": id, "title": title, "open": open, "badge": strconv.Itoa(len(kids)), "children": kids}
}

func cardRow(c CardRow) node {
	title := oneLine(c.Summary)
	if title == "" {
		title = c.Key
	}
	n := node{"type": "card", "id": "card:" + c.Key, "title": title, "subtitle": strings.TrimRight(oneLine(c.Key+" · "+c.Status), " ·")}
	switch {
	case c.MergedNoOpen:
		n["badge"] = "PR merged"
		n["children"] = []node{{"type": "button", "id": "done:" + c.Key, "label": "Move to Done",
			"confirm": node{"title": "Move " + c.Key + " to Done?"}}}
	case len(c.Open) > 0:
		n["badge"] = "#" + strconv.Itoa(c.Open[0].Number)
		n["children"] = []node{ghLink(c.Open[0].URL)}
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
	switch {
	case st.JiraMissing:
		kids = append(kids, node{"type": "alert", "tone": "warning", "text": "jira CLI not found"},
			node{"type": "log", "text": "brew install ankitpokhrel/jira-cli/jira-cli", "follow": false},
			node{"type": "text", "text": "Then run jira init in a terminal.", "muted": true})
	case st.JiraProblem != "":
		kids = append(kids, node{"type": "alert", "tone": "warning", "text": oneLine(st.JiraProblem)})
	}
	return kids
}

func workTree(w Work, st WorkState) node {
	head := "updated " + st.Updated.Format("15:04")
	if st.Login != "" {
		head = "@" + oneLine(st.Login) + " · " + head
	}
	kids := []node{{"type": "row", "children": []node{
		{"type": "text", "text": head, "muted": true},
		{"type": "button", "id": "work:refresh", "label": "Refresh", "icon": "refresh-cw"}}}}
	kids = append(kids, problems(st)...)
	var needs, reviews, cards, waiting []node
	for _, p := range w.NeedsYou {
		needs = append(needs, prCard("pr:"+p.URL, p, badges[reasonOf(p)]))
	}
	for _, p := range w.Reviews {
		badge := "Review"
		if p.Requested {
			badge = "Requested"
		}
		reviews = append(reviews, prCard("review:"+p.URL, p.PR, badge))
	}
	for _, c := range w.Cards {
		cards = append(cards, cardRow(c))
	}
	for _, p := range w.Waiting {
		state := p.CI
		if p.Draft {
			state = "Draft"
		}
		if state == "" {
			state = "Waiting"
		}
		waiting = append(waiting, prCard("pr:"+p.URL, p, oneLine(strings.ToUpper(state[:1])+strings.ToLower(state[1:]))))
	}
	for _, s := range []struct {
		id, title string
		open      bool
		kids      []node
	}{{"needs", "Needs you", true, needs}, {"reviews", "Review requests", true, reviews},
		{"cards", "My cards", true, cards}, {"waiting", "Waiting on others", false, waiting}} {
		if len(s.kids) > 0 {
			kids = append(kids, section(s.id, s.title, s.open, s.kids))
		}
	}
	if len(kids) == 1 {
		kids = append(kids, node{"type": "text", "text": "Nothing needs you right now.", "muted": true})
	}
	return node{"type": "column", "children": kids}
}
