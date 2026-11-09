package main

import (
	"bytes"
	"context"
	"errors"
	"os/exec"
	"regexp"
	"strings"
	"time"
)

// Card is one row of `jira issue list --plain`.
type Card struct{ Key, Summary, Status string }

const jiraTimeout = 30 * time.Second

// Problem is something the user can fix; it is shown in the panel as is.
type Problem string

func (p Problem) Error() string { return string(p) }

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

// errSilent: the CLI failed without saying why on stderr.
var errSilent = errors.New("jira failed without a message")

// jira runs the user's own CLI with argv only (never a shell); it holds the credentials, we never see them.
func jira(args ...string) (string, error) {
	path, err := exec.LookPath("jira")
	if err != nil {
		return "", Problem("jira CLI not found — install it and run jira init")
	}
	ctx, cancel := context.WithTimeout(context.Background(), jiraTimeout)
	defer cancel()
	var stdout, stderr bytes.Buffer
	cmd := exec.CommandContext(ctx, path, args...)
	cmd.Stdout, cmd.Stderr = &stdout, &stderr
	// A grandchild holding the pipes cannot hang Run after the timeout kills jira.
	cmd.WaitDelay = 2 * time.Second
	if err := cmd.Run(); err != nil {
		if ctx.Err() != nil {
			return "", Problem("jira timed out")
		}
		msg := strings.TrimSpace(stderr.String())
		if msg == "" {
			return "", errSilent
		}
		return "", errors.New(firstLine(msg))
	}
	return stdout.String(), nil
}

func firstLine(s string) string { return strings.SplitN(s, "\n", 2)[0] }

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
