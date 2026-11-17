package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
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
		promptFor("merge", u):      "Merge " + u + " once its required checks pass, then move its Jira card.",
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
	s := newServer(context.Background(), io.Discard)
	s.work, s.wst = w, WorkState{Login: "dev", Updated: time.Now()}
	n, _ := s.workTab()
	tree := asMap(n)
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
	if hits != 2 {
		t.Errorf("the title appears %d times, want 2 (the list card's and the detail card's titles)", hits)
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
	tree := asMap(workList(w))
	for id, want := range map[string][2]string{"sel:work:pr:" + prURL + "1": {"Conflict", "danger"}, "sel:work:card:EC-7": {"PR merged", "warning"}} {
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
	// A card with several PRs gets a group item above them; a lone PR or one with no card stays flat.
	tree := asMap(prList(w))
	var order []string
	var walk func(n map[string]any)
	walk = func(n map[string]any) {
		if n["type"] == "card" {
			order = append(order, strings.TrimPrefix(fmt.Sprintf("%v|%v", n["id"], n["badge"]), "sel:prs:"))
		}
		kids, _ := n["children"].([]any)
		for _, k := range kids {
			walk(k.(map[string]any))
		}
	}
	walk(tree)
	want := []string{"group:mine:EC-1|2 PRs", "pr:" + prURL + "1|<nil>", "pr:" + prURL + "2|Waiting", "pr:" + prURL + "4|Waiting",
		"pr:" + prURL + "3|Waiting", "group:req:EC-9|2 PRs", "pr:" + prURL + "20|<nil>", "pr:" + prURL + "21|<nil>"}
	if strings.Join(order, ",") != strings.Join(want, ",") {
		t.Errorf("items:\n got %v\nwant %v", order, want)
	}
	if g := findID(tree, "sel:prs:group:mine:EC-1"); g["title"] != "EC-1 · Filter by user" {
		t.Errorf("group title: %v", g["title"])
	}

	// Selecting a group shows its PRs with Review all.
	s := newServer(context.Background(), io.Discard)
	s.work = w
	s.sel["prs"] = "group:req:EC-9"
	n, id := s.prsTab()
	if b := findID(asMap(n), "act:prs:reviewall:req:EC-9"); id != "group:req:EC-9" || b == nil || b["label"] != "Review all 2" {
		t.Errorf("group detail %s: %v", id, b)
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

// The board keeps the Interstellar order, then statuses it does not know; an empty column is a strip.
func TestBoardOrder(t *testing.T) {
	b := asMap(board([]Card{{"EC-1", "a", "Test"}, {"EC-2", "b", "Weird"}, {"EC-3", "c", "to do"}, {"EC-4", "d", ""}, {"EC-5", "e", "Test"}},
		map[string]int{"EC-1": 2}))
	var got []string
	for _, c := range b["children"].([]any) {
		col := c.(map[string]any)
		kids := col["children"].([]any)
		head := kids[0].(map[string]any)["text"].(string)
		if col["width"] != nil {
			if col["width"] != float64(28) || len(kids) != 2 || head != "0" {
				t.Errorf("strip: %v", col)
			}
			head = strings.ReplaceAll(kids[1].(map[string]any)["text"].(string), "\n", "") + " · 0"
		}
		got = append(got, head)
	}
	want := "Tech Refinement · 0,To Do · 1,Ready For Dev · 0,Development · 0,Development Done · 0,Code Review · 0," +
		"Code Review Done · 0,Ready For Test · 0,Test · 2,Test Done · 0,Weird · 1,No status · 1"
	if strings.Join(got, ",") != want {
		t.Errorf("columns:\n got %s\nwant %s", strings.Join(got, ","), want)
	}
	if c := findID(b, "sel:jira:card:EC-1"); c == nil || c["badge"] != "2 PRs" || findID(b, "sel:jira:card:EC-5")["badge"] != nil {
		t.Errorf("PR count badge: %v", c)
	}
}

// The selection stays on its item across repaints and refreshes, and falls back to the first item
// when its item is gone.
func TestSelectionSurvivesRefresh(t *testing.T) {
	cards := []Card{{"EC-7", "x", ""}, {"EC-8", "x", ""}, {"EC-5", "x", ""}}
	s := newServer(context.Background(), io.Discard)
	s.work = buildWork(fixture(t), cards)
	if _, id := s.workTab(); id != "pr:"+prURL+"1" {
		t.Errorf("default: %s", id)
	}
	s.sel["work"] = "pr:" + prURL + "3"
	s.work = buildWork(fixture(t), cards)
	n, id := s.workTab()
	tree := asMap(n)
	if id != "pr:"+prURL+"3" || findID(tree, "sel:work:pr:"+prURL+"3")["selected"] != true || findID(tree, "sel:work:pr:"+prURL+"1")["selected"] != nil {
		t.Errorf("kept: %s", id)
	}
	if b := findID(tree, "act:work:pr:"+prURL+"3"); b == nil || b["label"] != "Fix review" {
		t.Errorf("detail action: %v", b)
	}
	s.sel["work"] = "pr:https://github.com/o/r/pull/999"
	if _, id := s.workTab(); id != "pr:"+prURL+"1" {
		t.Errorf("fallback: %s", id)
	}
	s.work = Work{}
	if n, id := s.workTab(); id != "" || !strings.Contains(textOf(asMap(n)), "Nothing needs you right now.") {
		t.Errorf("empty: %q", id)
	}
}

// A detail fetched before a refresh cleared it is dropped; the next load fetches again.
func TestStaleDetailDropped(t *testing.T) {
	s := newServer(context.Background(), io.Discard)
	m := map[string]*detail[int]{}
	release := make(chan struct{})
	s.smu.Lock()
	load(s, m, "k", func() (int, error) { <-release; return 1, nil })
	stale := m["k"]
	load(s, m, "k", func() (int, error) { t.Error("a second fetch while one runs"); return 0, nil })
	clear(m)
	s.smu.Unlock()
	close(release)
	s.wg.Wait()
	if m["k"] != nil || !stale.Busy {
		t.Errorf("the stale result must be dropped: %v %+v", m["k"], stale)
	}
	s.smu.Lock()
	load(s, m, "k", func() (int, error) { return 2, fmt.Errorf("boom") })
	s.smu.Unlock()
	s.wg.Wait()
	s.smu.Lock()
	defer s.smu.Unlock()
	if d := m["k"]; d == nil || d.Busy || d.Data != 2 || d.Err != "boom" {
		t.Errorf("fresh load: %+v", d)
	}
}

// reviewedJSON is one PR of mine in a repo without required reviews: felipe approved, then the author
// replied, and the CI still runs.
func reviewedJSON(at string) string {
	return `{"data":{"viewer":{"login":"dev"},"mine":{"nodes":[{"number":12,"title":"EC-12 approved, CI running",
  "url":"https://github.com/waycarbon/front/pull/12","author":{"login":"dev"},"reviewDecision":null,"mergeable":"MERGEABLE",
  "latestReviews":{"nodes":[{"state":"APPROVED","submittedAt":"` + at + `","author":{"login":"felipe"}},
    {"state":"COMMENTED","submittedAt":"2026-11-16T15:00:00Z","author":{"login":"dev"}},{"state":"PENDING","submittedAt":null,"author":{"login":"ana"}}]},
  "commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"PENDING"}}}]}}]}}}`
}

func TestReviewsDecideAndNotify(t *testing.T) {
	s, err := parseSnapshot(reviewedJSON("2026-11-16T14:00:00Z"))
	if err != nil {
		t.Fatal(err)
	}
	p := s.Mine[0]
	if p.Review != "APPROVED" || reasonOf(p) != ReadyToMerge || p.LastReview.By != "felipe" || p.LastReview.State != "APPROVED" {
		t.Fatalf("an approval decides a repo without a reviewDecision, the author's reply does not count: %+v", p)
	}
	if reasonOf(PR{Review: "APPROVED", CI: "FAILURE"}) != CIFailed || reasonOf(PR{Review: "APPROVED", Draft: true}) != Waiting {
		t.Error("a failing CI or a draft still wins over an approval")
	}
	if reasonOf(PR{Review: "APPROVED", CI: "FAILURE", MergeState: "BLOCKED"}) != CIFailed {
		t.Error("a failing required check blocks the merge")
	}
	for _, ms := range []string{"UNSTABLE", "BEHIND"} {
		if reasonOf(PR{Review: "APPROVED", CI: "FAILURE", MergeState: ms}) != ReadyToMerge {
			t.Errorf("a failing optional check (%s) must not hide the approval", ms)
		}
	}
	if reasonOf(PR{Review: "REVIEW_REQUIRED", LastReview: Review{State: "APPROVED"}}) != Waiting {
		t.Error("a required second approval keeps the PR waiting")
	}

	seen := map[string]time.Time{}
	if got := newReviews(seen, s.Mine); len(got) != 1 {
		t.Fatalf("a first review is new: %v", got)
	}
	if got := newReviews(seen, s.Mine); len(got) != 0 {
		t.Errorf("the same review again is not: %v", got)
	}
	title, body := reviewNote([]PR{p})
	if title != "@felipe approved #12" || body != "EC-12 approved, CI running" {
		t.Errorf("note: %q %q", title, body)
	}
	q := PR{Number: 13, LastReview: Review{By: "ana", State: "CHANGES_REQUESTED"}}
	if title, body := reviewNote([]PR{p, q}); title != "2 PRs reviewed" || body != "@felipe approved #12, @ana requested changes on #13" {
		t.Errorf("notes: %q %q", title, body)
	}
}
