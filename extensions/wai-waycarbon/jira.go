package main

import (
	"context"
	"errors"
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
func myCards(ctx context.Context) ([]Card, error) {
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
	cards, err := listCards(ctx, "-a", user, "-s~Done", "--plain", "--no-headers", "--columns", "key,summary,status")
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
	_, err := jira(ctx, "issue", "move", key, done)
	return err
}
