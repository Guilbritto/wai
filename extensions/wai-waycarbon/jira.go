package main

import (
	"context"
	"encoding/json"
	"errors"
	"net/url"
	"os"
	"regexp"
	"strings"
)

// Card is one row of `jira issue list --plain`.
type Card struct{ Key, Summary, Status string }

var keyRe = regexp.MustCompile(`^[A-Z][A-Z0-9]*-[0-9]+$`)

// parseCards reads tab-separated `key summary status` lines. jira pads the columns with extra tabs
// to align them, so the status is the last column. Lines whose first column is not an issue key
// (warnings, blanks) are dropped; an empty summary column is kept as empty.
func parseCards(out string) []Card {
	var cards []Card
	for _, line := range strings.Split(out, "\n") {
		cols := strings.Split(strings.TrimRight(line, "\r"), "\t")
		key := strings.TrimSpace(cols[0])
		if !keyRe.MatchString(key) {
			continue
		}
		c := Card{Key: key}
		if len(cols) > 1 {
			c.Summary = strings.TrimSpace(cols[1])
		}
		if len(cols) > 2 {
			c.Status = strings.TrimSpace(cols[len(cols)-1])
		}
		cards = append(cards, c)
	}
	return cards
}

// jira runs the user's own CLI; it holds the credentials, we never see them.
func jira(ctx context.Context, args ...string) (string, error) {
	out, err := runCLI(ctx, "jira", args...)
	var nf ErrNotFound
	if errors.As(err, &nf) {
		return "", Problem("jira CLI not found — install it and run jira init")
	}
	return out, err
}

// myCards is R45: `jira me` first, then the open cards assigned to that user.
func myCards(ctx context.Context) ([]Card, error) { return assigned(ctx) }

// sprintCards is myCards limited to the open sprints.
func sprintCards(ctx context.Context) ([]Card, error) {
	return assigned(ctx, "-q", "sprint in openSprints()")
}

func assigned(ctx context.Context, extra ...string) ([]Card, error) {
	notLoggedIn := Problem("jira is not logged in — run jira init in a terminal")
	me, err := jira(ctx, "me")
	if err != nil {
		if errors.Is(err, errSilent) {
			return nil, notLoggedIn
		}
		var p Problem
		if errors.As(err, &p) {
			return nil, err
		}
		return nil, Problem(err.Error())
	}
	user := strings.TrimSpace(me)
	if user == "" {
		return nil, notLoggedIn
	}
	args := append(append([]string{"-a", user, "-s~Done"}, extra...), "--plain", "--no-headers", "--columns", "key,summary,status")
	cards, err := listCards(ctx, args...)
	if err != nil {
		if errors.Is(err, errSilent) {
			return nil, Problem("jira issue list failed")
		}
		return nil, Problem("jira issue list failed: " + err.Error())
	}
	return cards, nil
}

// listCards runs `jira issue list`. jira exits 1 with "No result found" when nothing matches: no cards.
func listCards(ctx context.Context, args ...string) ([]Card, error) {
	out, err := jira(ctx, append([]string{"issue", "list"}, args...)...)
	if err != nil && strings.Contains(err.Error(), "No result found") {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return parseCards(out), nil
}

// searchCards finds any card of the project, Done included: by key when q is one, else by text as
// Jira's search box does. At most 50.
func searchCards(ctx context.Context, q string) ([]Card, error) {
	args := []string{"--plain", "--no-headers", "--columns", "key,summary,status", "--paginate", "50"}
	if k := strings.ToUpper(q); keyRe.MatchString(k) {
		args = append(args, "-q", "key = "+k)
	} else {
		args = append(args, "--", q)
	}
	cards, err := listCards(ctx, args...)
	if errors.Is(err, errSilent) {
		return nil, Problem("jira search failed")
	}
	return cards, err
}

// moveCard moves a card to the done status (WAI_WAYCARBON_DONE, default Done).
func moveCard(ctx context.Context, key string) error {
	done := os.Getenv("WAI_WAYCARBON_DONE")
	if done == "" {
		done = "Done"
	}
	return moveTo(ctx, key, done)
}

func moveTo(ctx context.Context, key, status string) error {
	if !keyRe.MatchString(key) {
		return Problem("not a card key: " + key)
	}
	_, err := jira(ctx, "issue", "move", key, status)
	if errors.Is(err, errSilent) {
		return Problem("jira issue move failed")
	}
	return err
}

// CardDetail is what the Jira tab shows for the selected card. Site is https://<host> of the instance.
type CardDetail struct {
	Key, Summary, Status, Type, Priority, Assignee, ParentKey, ParentSummary, Description, Site string
	Subtasks                                                                                    []Card
}

type named = *struct{ Name string }

func nameOf(n named) string {
	if n == nil {
		return ""
	}
	return n.Name
}

// viewCard reads `jira issue view KEY --raw`, the issue's JSON.
func viewCard(ctx context.Context, key string) (CardDetail, error) {
	if !keyRe.MatchString(key) {
		return CardDetail{}, Problem("not a card key: " + key)
	}
	out, err := jira(ctx, "issue", "view", key, "--raw")
	if errors.Is(err, errSilent) {
		return CardDetail{}, Problem("jira issue view failed")
	}
	if err != nil {
		return CardDetail{}, err
	}
	var r struct {
		Key, Self string
		Fields    struct {
			Summary                     string
			Status, Issuetype, Priority named
			Assignee                    *struct{ DisplayName string }
			Parent                      *struct {
				Key    string
				Fields struct{ Summary string }
			}
			Subtasks []*struct {
				Key    string
				Fields struct {
					Summary string
					Status  named
				}
			}
			Description json.RawMessage
		}
	}
	if err := json.Unmarshal([]byte(out), &r); err != nil {
		return CardDetail{}, Problem("jira answered with unreadable data")
	}
	f := r.Fields
	d := CardDetail{Key: r.Key, Summary: f.Summary, Status: nameOf(f.Status), Type: nameOf(f.Issuetype),
		Priority: nameOf(f.Priority), Description: adfText(f.Description)}
	if f.Assignee != nil {
		d.Assignee = f.Assignee.DisplayName
	}
	if f.Parent != nil {
		d.ParentKey, d.ParentSummary = f.Parent.Key, f.Parent.Fields.Summary
	}
	for _, st := range f.Subtasks {
		if st != nil && keyRe.MatchString(st.Key) {
			d.Subtasks = append(d.Subtasks, Card{st.Key, st.Fields.Summary, nameOf(st.Fields.Status)})
		}
	}
	if u, err := url.Parse(r.Self); err == nil && u.Scheme == "https" && u.Host != "" {
		d.Site = "https://" + u.Host
	}
	return d, nil
}

type adfNode struct {
	Type, Text string
	Attrs      struct{ Text, URL string }
	Content    []adfNode
}

func (n adfNode) write(b *strings.Builder) {
	switch n.Type {
	case "text":
		b.WriteString(n.Text)
		return
	case "hardBreak":
		b.WriteString("\n")
		return
	case "mention", "emoji":
		b.WriteString(n.Attrs.Text)
		return
	case "inlineCard":
		b.WriteString(n.Attrs.URL)
		return
	case "listItem":
		b.WriteString("• ")
	}
	// ponytail: an unknown node (table, panel, media) gives its children's text, never its JSON.
	for _, c := range n.Content {
		c.write(b)
	}
	switch n.Type {
	case "paragraph", "heading", "codeBlock":
		b.WriteString("\n")
	}
}

// adfText is a description as plain text: a string as is (older sites send wiki markup), an Atlassian
// document walked for its text. Blank lines are dropped; at most 4,000 runes.
func adfText(raw json.RawMessage) string {
	var s string
	if json.Unmarshal(raw, &s) != nil {
		var n adfNode
		var te *json.UnmarshalTypeError
		if err := json.Unmarshal(raw, &n); err != nil && !errors.As(err, &te) {
			return ""
		}
		var b strings.Builder
		n.write(&b)
		s = b.String()
	}
	var lines []string
	for _, l := range strings.Split(s, "\n") {
		if l = strings.TrimRight(l, " \t\r"); strings.TrimSpace(l) != "" {
			lines = append(lines, l)
		}
	}
	return clip(strings.Join(lines, "\n"), 4000)
}
