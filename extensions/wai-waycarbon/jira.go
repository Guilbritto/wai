package main

import (
	"context"
	"errors"
	"regexp"
	"strings"
)

// Card is one row of `jira issue list --plain`.
type Card struct{ Key, Summary, Status string }

var keyRe = regexp.MustCompile(`^[A-Z][A-Z0-9]*-[0-9]+$`)

// parseCards reads tab-separated `key summary status` lines. Lines whose first column is not an
// issue key (warnings, blanks) are dropped; an empty summary column is kept as empty.
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
			c.Status = strings.TrimSpace(cols[2])
		}
		cards = append(cards, c)
	}
	return cards
}

// jira runs the user's own CLI; it holds the credentials, we never see them.
func jira(args ...string) (string, error) {
	out, err := runCLI(context.Background(), "jira", args...)
	var nf ErrNotFound
	if errors.As(err, &nf) {
		return "", Problem("jira CLI not found — install it and run jira init")
	}
	return out, err
}

// myCards is R45: `jira me` first, then the open cards assigned to that user.
func myCards() ([]Card, error) {
	notLoggedIn := Problem("jira is not logged in — run jira init in a terminal")
	me, err := jira("me")
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
	out, err := jira("issue", "list", "-a", user, "-s~Done", "--plain", "--no-headers", "--columns", "key,summary,status")
	if err != nil {
		if errors.Is(err, errSilent) {
			return nil, Problem("jira issue list failed")
		}
		return nil, Problem("jira issue list failed: " + err.Error())
	}
	return parseCards(out), nil
}
