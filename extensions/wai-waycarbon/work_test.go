package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

const prURL = "https://github.com/waycarbon/inventories/pull/"

func fixture(t *testing.T) Snapshot {
	t.Helper()
	b, err := os.ReadFile("testdata/graphql.json")
	if err != nil {
		t.Fatal(err)
	}
	s, err := parseSnapshot(string(b))
	if err != nil {
		t.Fatal(err)
	}
	return s
}

func byNumber(t *testing.T, s Snapshot, n int) PR {
	t.Helper()
	for _, p := range append(append([]PR{}, s.Mine...), s.Merged...) {
		if p.Number == n {
			return p
		}
	}
	t.Fatalf("no PR %d", n)
	return PR{}
}

func TestReasons(t *testing.T) {
	s := fixture(t)
	if s.Login != "dev" {
		t.Errorf("login %q", s.Login)
	}
	want := map[int]Reason{1: Conflict, 2: CIFailed, 3: ChangesRequested, 4: ReadyToMerge, 5: Waiting, 6: Waiting, 7: Waiting, 9: Waiting}
	for n, r := range want {
		if got := reasonOf(byNumber(t, s, n)); got != r {
			t.Errorf("PR %d: reason %d, want %d", n, got, r)
		}
	}
	if p := byNumber(t, s, 9); p.Repo != "" || len(p.Labels) != 0 {
		t.Errorf("null repository and labels must be empty: %+v", p)
	}
	if p := byNumber(t, s, 7); p.CI != "" || p.Mergeable != "UNKNOWN" {
		t.Errorf("null rollup: %+v", p)
	}
}

func TestKeysOf(t *testing.T) {
	got := keysOf(PR{Title: "EC-1 and EC-2 fix", Branch: "feat/EC-2-x-AB1-3"})
	if strings.Join(got, ",") != "EC-1,EC-2,AB1-3" {
		t.Errorf("got %v", got)
	}
}

func TestBuildWork(t *testing.T) {
	s := fixture(t)
	cards := []Card{{"EC-7", "Shipped", "In Review"}, {"EC-8", "Two parts", "In Progress"}, {"EC-5", "Nothing yet", "To Do"}}
	w := buildWork(s, cards)
	var order []int
	for _, p := range w.NeedsYou {
		order = append(order, p.Number)
	}
	if len(order) != 4 || order[0] != 1 || order[1] != 2 || order[2] != 3 || order[3] != 4 {
		t.Errorf("needs you order: %v", order)
	}
	if len(w.Waiting) != 4 {
		t.Errorf("waiting: %d", len(w.Waiting))
	}
	if len(w.Reviews) != 2 || w.Reviews[0].Number != 20 || !w.Reviews[0].Requested || w.Reviews[1].Number != 21 || w.Reviews[1].Requested {
		t.Errorf("reviews: %+v", w.Reviews)
	}
	row := map[string]CardRow{}
	for _, c := range w.Cards {
		row[c.Key] = c
	}
	if !row["EC-7"].MergedNoOpen || len(row["EC-7"].Open) != 0 {
		t.Errorf("EC-7: %+v", row["EC-7"])
	}
	if row["EC-8"].MergedNoOpen || len(row["EC-8"].Open) != 1 || row["EC-8"].Open[0].Number != 7 {
		t.Errorf("EC-8: %+v", row["EC-8"])
	}
	if row["EC-5"].MergedNoOpen || len(row["EC-5"].Open) != 0 {
		t.Errorf("EC-5: %+v", row["EC-5"])
	}
}

func TestPrompts(t *testing.T) {
	u := "https://github.com/o/r/pull/1"
	for got, want := range map[string]string{
		promptFor("conflict", u):   "Resolve the merge conflicts on " + u + ".",
		promptFor("ci", u):         "The CI fails on " + u + ". Find out why and fix it.",
		promptFor("changes", u):    "/waycarbon:fix-review " + u,
		promptFor("merge", u):      "Merge " + u + ", then move its Jira card.",
		promptFor("review", u):     "/waycarbon:code-review " + u,
		promptFor("solve", "EC-1"): "/waycarbon:solve-card EC-1",
		promptFor("deploy", ""):    "/waycarbon:deploy",
		promptFor("recheck", u):    "/waycarbon:recheck-review " + u,
	} {
		if got != want {
			t.Errorf("got %q, want %q", got, want)
		}
	}
}

func TestWorkTreeTextIsPlain(t *testing.T) {
	evil := "[x](javascript:alert(1))"
	w := Work{NeedsYou: []PR{{Number: 1, Title: evil, URL: prURL + "1", Repo: "o/r", Mergeable: "CONFLICTING"}}}
	var tree map[string]any
	b, _ := json.Marshal(workTree(w, WorkState{Login: "dev", Updated: time.Now()}))
	json.Unmarshal(b, &tree)
	var hits int
	var walk func(n map[string]any)
	walk = func(n map[string]any) {
		if n["type"] == "markdown" {
			t.Error("a markdown node")
		}
		for k, v := range n {
			if s, ok := v.(string); ok && strings.Contains(s, evil) {
				hits++
				if _, isCard := n["subtitle"]; k != "title" || !isCard {
					t.Errorf("title text in %q of %v", k, n["type"])
				}
			}
		}
		kids, _ := n["children"].([]any)
		for _, k := range kids {
			walk(k.(map[string]any))
		}
	}
	walk(tree)
	if hits != 1 {
		t.Errorf("the title appears %d times, want 1 (the card title)", hits)
	}
}

func TestClickPrompts(t *testing.T) {
	s := fixture(t)
	w := buildWork(s, []Card{{"EC-7", "x", ""}, {"EC-8", "x", ""}, {"EC-5", "x", ""}})
	for id, want := range map[string]string{
		"pr:" + prURL + "3":      "/waycarbon:fix-review " + prURL + "3",
		"pr:" + prURL + "5":      "/waycarbon:recheck-review " + prURL + "5",
		"review:" + prURL + "20": "/waycarbon:code-review " + prURL + "20",
		"card:EC-5":              "/waycarbon:solve-card EC-5",
		"card:EC-7":              "/waycarbon:deploy",
		"card:EC-8":              "/waycarbon:recheck-review " + prURL + "7",
	} {
		if got, ok := w.clickPrompt(id); !ok || got != want {
			t.Errorf("%s: got %q %v, want %q", id, got, ok, want)
		}
	}
	for _, id := range []string{"pr:https://evil/1", "card:EC-99", "review:" + prURL + "3", "nope"} {
		if got, ok := w.clickPrompt(id); ok {
			t.Errorf("%s must not open a session, got %q", id, got)
		}
	}
}

func TestFetchGitHub(t *testing.T) {
	log := filepath.Join(t.TempDir(), "gh.log")
	data, _ := filepath.Abs("testdata/graphql.json")
	fakeCLI(t, "gh", `echo "$*" >> '`+log+`'
case "$1 $2" in
  "api graphql") cat '`+data+`'; echo "SAML enforcement" >&2; exit 1 ;;
  "auth status") exit 0 ;;
esac
`)
	s, err := fetchGitHub(context.Background(), "acme", time.Date(2026, 11, 14, 0, 0, 0, 0, time.UTC))
	if err != nil || len(s.Mine) != 8 || s.Warn != "SAML enforcement" {
		t.Fatalf("partial data: %v %q %d", err, s.Warn, len(s.Mine))
	}
	got := string(mustRead(log))
	for _, part := range []string{"api graphql", "is:pr is:open author:@me org:acme", "is:pr is:merged author:@me org:acme merged:>=2026-11-14"} {
		if !strings.Contains(got, part) {
			t.Errorf("argv %q lacks %q", got, part)
		}
	}

	fakeCLI(t, "gh", `echo "gh: HTTP 401" >&2; exit 1`)
	if _, err := fetchGitHub(context.Background(), "acme", time.Now()); err != errSignedOut {
		t.Errorf("signed out: %v", err)
	}
	fakeCLI(t, "gh", `[ "$1" = auth ] && exit 0; echo "gh: HTTP 502" >&2; exit 1`)
	if _, err := fetchGitHub(context.Background(), "acme", time.Now()); err == nil || err.Error() != "gh: HTTP 502" {
		t.Errorf("failure: %v", err)
	}
}

func TestToneBadges(t *testing.T) {
	w := Work{NeedsYou: []PR{{Number: 1, Title: "t", URL: prURL + "1", Repo: "o/r", Mergeable: "CONFLICTING"}},
		Cards: []CardRow{{Card: Card{Key: "EC-7", Summary: "s"}, MergedNoOpen: true}}}
	var tree map[string]any
	b, _ := json.Marshal(workTree(w, WorkState{Updated: time.Now()}))
	json.Unmarshal(b, &tree)
	for id, want := range map[string][2]string{"pr:" + prURL + "1": {"Conflict", "danger"}, "card:EC-7": {"PR merged", "warning"}} {
		c := findID(tree, id)
		if c["badge"] != nil {
			t.Errorf("%s: a toned card has no header badge: %v", id, c["badge"])
		}
		if bd := find(c, "badge"); bd == nil || bd["text"] != want[0] || bd["tone"] != want[1] {
			t.Errorf("%s: badge node %v, want %v", id, bd, want)
		}
	}
}

func asMap(n node) map[string]any {
	var m map[string]any
	b, _ := json.Marshal(n)
	json.Unmarshal(b, &m)
	return m
}

func TestPRsTabGroupsByCard(t *testing.T) {
	w := buildWork(Snapshot{
		Mine: []PR{
			{Number: 1, Title: "EC-1 proto", URL: prURL + "1", Mergeable: "CONFLICTING"},
			{Number: 2, Title: "service part", Branch: "feat/EC-1-service", URL: prURL + "2"},
			{Number: 3, Title: "bump deps", URL: prURL + "3"},
			{Number: 4, Title: "EC-2 alone", URL: prURL + "4"},
		},
		Requested: []PR{{Number: 20, Title: "EC-9 a", URL: prURL + "20"}, {Number: 21, Title: "EC-9 b", URL: prURL + "21"}},
	}, []Card{{"EC-1", "Filter by user", "In Progress"}})
	tree := asMap(prsTree(w, WorkState{Updated: time.Now()}))
	for id, want := range map[string]string{
		"prs:mine:EC-1": "EC-1 · Filter by user|2 PRs|true|Review all 2",
		"prs:mine:EC-2": "EC-2 · EC-2 alone|1 PR|false|",
		"prs:mine:none": "No card|1 PR|false|",
		"prs:req:EC-9":  "EC-9 · EC-9 a|2 PRs|true|Review all 2",
	} {
		g := findID(tree, id)
		if g == nil {
			t.Errorf("no group %s", id)
			continue
		}
		label := ""
		if b := find(g, "button"); b != nil {
			label = b["label"].(string)
		}
		if got := fmt.Sprintf("%v|%v|%v|%s", g["title"], g["badge"], g["open"], label); got != want {
			t.Errorf("%s: got %q, want %q", id, got, want)
		}
	}
	head := "Review the 2 PRs of EC-1 as one change: run /waycarbon:code-review on each, then check they fit each other (proto, service, BFF and front contracts).\n"
	if got, ok := w.clickPrompt("reviewall:mine:EC-1"); !ok || got != head+prURL+"1\n"+prURL+"2" {
		t.Errorf("review all: %q %v", got, ok)
	}
	if _, ok := w.clickPrompt("reviewall:req:EC-9"); !ok {
		t.Error("review all of the requested EC-9")
	}
	for _, id := range []string{"reviewall:mine:EC-2", "reviewall:mine:", "reviewall:req:EC-1", "reviewall:queue:EC-9", "reviewall:"} {
		if got, ok := w.clickPrompt(id); ok {
			t.Errorf("%s must not open a session, got %q", id, got)
		}
	}
}

func TestJiraTabByStatus(t *testing.T) {
	w := buildWork(Snapshot{}, []Card{{"EC-1", "a", "In Progress"}, {"EC-2", "b", "To Do"}, {"EC-3", "c", "In Progress"}})
	var got []string
	for _, k := range asMap(jiraTree(w, WorkState{Updated: time.Now()}))["children"].([]any) {
		if s := k.(map[string]any); s["type"] == "section" {
			got = append(got, fmt.Sprintf("%v:%v", s["title"], s["badge"]))
		}
	}
	if strings.Join(got, ",") != "In Progress:2,To Do:1" {
		t.Errorf("sections: %v", got)
	}
}
