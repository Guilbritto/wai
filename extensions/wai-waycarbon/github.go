package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"os"
	"os/exec"
	"regexp"
	"strings"
	"time"
)

// PR is one pull request, with the nulls of the GraphQL answer turned into empty values.
type PR struct {
	Number                   int
	Title, URL, Repo, Branch string
	Draft                    bool
	Review                   string // reviewDecision
	Mergeable                string
	MergeState               string // mergeStateStatus: BLOCKED when something required stops the merge
	CI                       string // statusCheckRollup.state, or ""
	Labels                   []string
	Merged                   bool
	LastReview               Review // the newest review by someone other than the author
}

// Review is one submitted review: approved, changes requested or commented.
type Review struct {
	By, State string
	At        time.Time
}

// Snapshot is one GitHub refresh. Warn is a GraphQL error that came with usable data.
type Snapshot struct {
	Login                          string
	Mine, Requested, Queue, Merged []PR
	Warn                           string
}

// errSignedOut: gh is installed but has no login.
var errSignedOut = errors.New("gh is signed out")

const query = `query($mine: String!, $requested: String!, $queue: String!, $merged: String!) {
  viewer { login }
  mine: search(query: $mine, type: ISSUE, first: 50) { nodes { ...pr } }
  requested: search(query: $requested, type: ISSUE, first: 50) { nodes { ...pr } }
  queue: search(query: $queue, type: ISSUE, first: 50) { nodes { ...pr } }
  merged: search(query: $merged, type: ISSUE, first: 50) { nodes { ...pr } }
}
fragment pr on PullRequest {
  number title url isDraft headRefName
  author { login }
  repository { nameWithOwner }
  reviewDecision mergeable mergeStateStatus
  latestReviews(last: 20) { nodes { state submittedAt author { login } } }
  labels(first: 10) { nodes { name } }
  commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
}`

type gqlPR struct {
	Number           int
	Title, URL       string
	IsDraft          bool
	HeadRefName      string
	Author           *struct{ Login string }
	Repository       *struct{ NameWithOwner string }
	ReviewDecision   string
	Mergeable        string
	MergeStateStatus string
	Labels           *struct{ Nodes []*struct{ Name string } }
	LatestReviews    *struct {
		Nodes []*struct {
			State       string
			SubmittedAt time.Time
			Author      *struct{ Login string }
		}
	}
	Commits *struct {
		Nodes []*struct {
			Commit *struct{ StatusCheckRollup *struct{ State string } }
		}
	}
}

func (g *gqlPR) pr(merged bool) (PR, bool) {
	// A search hit that is not a pull request is an empty object; a link must be https.
	if g == nil || !strings.HasPrefix(g.URL, "https://") {
		return PR{}, false
	}
	p := PR{Number: g.Number, Title: g.Title, URL: g.URL, Branch: g.HeadRefName, Draft: g.IsDraft,
		Review: g.ReviewDecision, Mergeable: g.Mergeable, MergeState: g.MergeStateStatus, Merged: merged}
	if g.Repository != nil {
		p.Repo = g.Repository.NameWithOwner
	}
	if g.Labels != nil {
		for _, l := range g.Labels.Nodes {
			if l != nil {
				p.Labels = append(p.Labels, l.Name)
			}
		}
	}
	// A repo that does not require reviews has no reviewDecision: the reviews decide it, as on GitHub.
	approved, changes := false, false
	if g.LatestReviews != nil {
		for _, r := range g.LatestReviews.Nodes {
			if r == nil || r.Author == nil || g.Author != nil && r.Author.Login == g.Author.Login {
				continue
			}
			switch r.State {
			case "APPROVED", "CHANGES_REQUESTED", "COMMENTED":
			default:
				continue
			}
			approved, changes = approved || r.State == "APPROVED", changes || r.State == "CHANGES_REQUESTED"
			if r.SubmittedAt.After(p.LastReview.At) {
				p.LastReview = Review{By: r.Author.Login, State: r.State, At: r.SubmittedAt}
			}
		}
	}
	if p.Review == "" && changes {
		p.Review = "CHANGES_REQUESTED"
	} else if p.Review == "" && approved {
		p.Review = "APPROVED"
	}
	if g.Commits != nil {
		for _, c := range g.Commits.Nodes {
			if c != nil && c.Commit != nil && c.Commit.StatusCheckRollup != nil {
				p.CI = c.Commit.StatusCheckRollup.State
			}
		}
	}
	return p, true
}

// parseSnapshot reads the GraphQL answer; `data` must be present.
func parseSnapshot(out string) (Snapshot, error) {
	var r struct {
		Data *struct {
			Viewer                         *struct{ Login string }
			Mine, Requested, Queue, Merged *struct{ Nodes []*gqlPR }
		}
		Errors []struct{ Message string }
	}
	if err := json.Unmarshal([]byte(out), &r); err != nil || r.Data == nil {
		return Snapshot{}, errors.New("GitHub answered with unreadable data")
	}
	var s Snapshot
	if r.Data.Viewer != nil {
		s.Login = r.Data.Viewer.Login
	}
	if len(r.Errors) > 0 {
		s.Warn = r.Errors[0].Message
	}
	read := func(from *struct{ Nodes []*gqlPR }, merged bool) (prs []PR) {
		if from == nil {
			return nil
		}
		for _, g := range from.Nodes {
			if p, ok := g.pr(merged); ok {
				prs = append(prs, p)
			}
		}
		return prs
	}
	s.Mine, s.Requested, s.Queue, s.Merged = read(r.Data.Mine, false), read(r.Data.Requested, false), read(r.Data.Queue, false), read(r.Data.Merged, true)
	return s, nil
}

func org() string {
	if o := os.Getenv("WAI_WAYCARBON_ORG"); o != "" {
		return o
	}
	return "waycarbon"
}

// fetchGitHub is one GraphQL call. The searches go in as variables, so an org name cannot change the
// query. gh exits non-zero when the answer carries GraphQL errors (an org behind SAML the token is
// not authorized for) yet still prints `data`: that is used, with the first error in Warn.
func fetchGitHub(ctx context.Context, org string, since time.Time) (Snapshot, error) {
	out, err := runCLI(ctx, "gh", "api", "graphql", "-f", "query="+query,
		"-f", "mine=is:pr is:open author:@me org:"+org,
		"-f", "requested=is:pr is:open review-requested:@me org:"+org,
		"-f", "queue=is:pr is:open label:review org:"+org+" -author:@me",
		"-f", "merged=is:pr is:merged author:@me org:"+org+" merged:>="+since.Format("2006-01-02"))
	if err == nil {
		return parseSnapshot(out)
	}
	if s, perr := parseSnapshot(out); perr == nil {
		if s.Warn == "" {
			s.Warn = err.Error()
		}
		return s, nil
	}
	var nf ErrNotFound
	if errors.As(err, &nf) {
		return Snapshot{}, err
	}
	if _, aerr := runCLI(ctx, "gh", "auth", "status"); aerr != nil {
		return Snapshot{}, errSignedOut
	}
	return Snapshot{}, err
}

var codeRe = regexp.MustCompile(`one-time code: (\S+)`)

// ghLogin starts the device-flow login, stdin null. gh prints the code on stderr and waits; the code
// channel gets it once, the exit channel the result (a start failure included). Kill it through the
// command's Process or by cancelling ctx.
func ghLogin(ctx context.Context) (*exec.Cmd, <-chan string, <-chan error) {
	code, exit := make(chan string, 1), make(chan error, 1)
	fail := func(err error) (*exec.Cmd, <-chan string, <-chan error) {
		close(code)
		exit <- err
		return nil, code, exit
	}
	path, err := exec.LookPath("gh")
	if err != nil {
		return fail(ErrNotFound{"gh"})
	}
	cmd := exec.CommandContext(ctx, path, "auth", "login", "--web", "--hostname", "github.com", "--git-protocol", "https", "--skip-ssh-key")
	cmd.Env = append(os.Environ(), "GIT_TERMINAL_PROMPT=0")
	cmd.WaitDelay = 2 * time.Second
	stderr, err := cmd.StderrPipe()
	if err != nil {
		return fail(err)
	}
	if err := cmd.Start(); err != nil {
		return fail(err)
	}
	go func() {
		var last string
		sc := bufio.NewScanner(stderr)
		sent := false
		for sc.Scan() {
			line := strings.TrimSpace(sc.Text())
			if line != "" {
				last = line
			}
			if m := codeRe.FindStringSubmatch(line); m != nil && !sent {
				sent = true
				code <- m[1]
			}
		}
		close(code)
		err := cmd.Wait()
		if err != nil && last != "" {
			err = errors.New(last)
		}
		exit <- err
	}()
	return cmd, code, exit
}
