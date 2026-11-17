package main

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"unicode/utf8"
)

// fakeGhFixture answers a gh call that carries arg with the fixture; anything else fails.
func fakeGhFixture(t *testing.T, arg, fixture string) {
	t.Helper()
	path, err := filepath.Abs(fixture)
	if err != nil {
		t.Fatal(err)
	}
	fakeCLI(t, "gh", `[ "$1 $2" = "api graphql" ] || exit 2
for a in "$@"; do [ "$a" = '`+arg+`' ] && exec cat '`+path+`'; done
echo "unexpected: $*" >&2; exit 2
`)
}

func TestADFText(t *testing.T) {
	b, err := os.ReadFile("testdata/issue.json")
	if err != nil {
		t.Fatal(err)
	}
	var issue struct {
		Fields struct{ Description json.RawMessage }
	}
	if err := json.Unmarshal(b, &issue); err != nil {
		t.Fatal(err)
	}
	want := "Goal\nFilter emissions by scope.\n• proto field\n• service filter\nmake migrate ENV=stg\nSee https://example.com/doc\n@Ana"
	if got := adfText(issue.Fields.Description); got != want {
		t.Errorf("got %q\nwant %q", got, want)
	}
	for raw, want := range map[string]string{`"plain *wiki* text\nline 2"`: "plain *wiki* text\nline 2", `null`: "", ``: "", `{"type":"doc"}`: "", `42`: ""} {
		if got := adfText(json.RawMessage(raw)); got != want {
			t.Errorf("%s: got %q, want %q", raw, got, want)
		}
	}
	long := adfText(json.RawMessage(`"` + strings.Repeat("é", 5000) + `"`))
	if utf8.RuneCountInString(long) != 4001 || !strings.HasSuffix(long, "…") {
		t.Errorf("not clipped to 4,000: %d runes", utf8.RuneCountInString(long))
	}
}

func TestViewCard(t *testing.T) {
	path, _ := filepath.Abs("testdata/issue.json")
	fakeJira(t, `[ "$*" = "issue view EC-1240 --raw" ] && exec cat '`+path+`'
echo "unexpected: $*" >&2; exit 2
`)
	d, err := viewCard(context.Background(), "EC-1240")
	if err != nil {
		t.Fatal(err)
	}
	if d.Key != "EC-1240" || d.Summary != "New scope filter" || d.Status != "Code Review" || d.Type != "Story" ||
		d.Priority != "High" || d.Assignee != "Ana Souza" || d.ParentKey != "EC-1200" || d.ParentSummary != "Scopes epic" ||
		d.Site != "https://waycarbon.atlassian.net" || !strings.HasPrefix(d.Description, "Goal\n") {
		t.Errorf("detail: %+v", d)
	}
	want := []Card{{"EC-1241", "[proto-repository] Add scope_id", "Done"}, {"EC-1242", "[waycarbon-inventories] Filter by scope", "Development"}}
	if len(d.Subtasks) != 2 || d.Subtasks[0] != want[0] || d.Subtasks[1] != want[1] {
		t.Errorf("subtasks: %+v", d.Subtasks)
	}
	if _, err := viewCard(context.Background(), "EC-1240 --raw"); err == nil {
		t.Error("a key that is not one must not reach jira")
	}
}

// Failed checks come first, the others keep GitHub's order; resolved threads are left out.
func TestPRDetailFailedFirst(t *testing.T) {
	fakeGhFixture(t, "url="+prURL+"88", "testdata/prdetail.json")
	d, err := prDetail(context.Background(), prURL+"88")
	if err != nil {
		t.Fatal(err)
	}
	if d.Additions != 120 || d.Deletions != 8 || d.Files != 5 || d.Author != "ana" {
		t.Errorf("detail: %+v", d)
	}
	var checks []string
	for _, c := range d.Checks {
		checks = append(checks, c.Name+"="+c.State)
	}
	if got := strings.Join(checks, ","); got != "SonarCloud=FAILURE,test=FAILURE,build=SUCCESS,lint=IN_PROGRESS" {
		t.Errorf("checks: %s", got)
	}
	if d.Checks[1].URL != "https://github.com/x/3" {
		t.Errorf("check url: %+v", d.Checks[1])
	}
	if len(d.Reviews) != 1 || d.Reviews[0].By != "joao" || d.Reviews[0].State != "CHANGES_REQUESTED" || d.Reviews[0].Body != "Please add a test." {
		t.Errorf("reviews: %+v", d.Reviews)
	}
	if len(d.Threads) != 2 || d.Threads[0] != (Thread{"joao", "Rename this.", "repo/scope.go", 42}) || d.Threads[1].Body != "Outdated note." {
		t.Errorf("threads: %+v", d.Threads)
	}
	if _, err := prDetail(context.Background(), "http://x"); err == nil {
		t.Error("a link that is not https must not reach gh")
	}
}

// Every PR of the org whose title has the query, any state, newest first.
func TestSearchPRs(t *testing.T) {
	fakeGhFixture(t, "q=EC-1240 in:title is:pr org:waycarbon", "testdata/search.json")
	prs, err := searchPRs(context.Background(), "waycarbon", " EC-1240 ")
	if err != nil {
		t.Fatal(err)
	}
	var got []string
	for _, p := range prs {
		got = append(got, p.Repo+"#"+p.State)
	}
	if strings.Join(got, ",") != "waycarbon/waycarbon-front#OPEN,waycarbon/waycarbon-bff#CLOSED,waycarbon/proto-repository#MERGED" {
		t.Errorf("prs: %v", got)
	}
	if !prs[2].Merged || prs[0].Merged || prs[0].CI != "FAILURE" {
		t.Errorf("merged/CI: %+v", prs)
	}
}
