package main

import (
	"bytes"
	"context"
	"errors"
	"os/exec"
	"strings"
	"time"
)

// Card is one row of `jira issue list --plain`.
type Card struct{ Key, Summary, Status string }

const jiraTimeout = 30 * time.Second

// Problem is something the user can fix; it is shown in the panel as is.
type Problem string

func (p Problem) Error() string { return string(p) }

// parseCards reads tab-separated `key summary status` lines; blank and broken lines are skipped.
func parseCards(out string) []Card {
	var cards []Card
	for _, line := range strings.Split(out, "\n") {
		cols := strings.Split(strings.TrimSpace(line), "\t")
		if len(cols) < 2 || cols[0] == "" {
			continue
		}
		c := Card{Key: strings.TrimSpace(cols[0]), Summary: strings.TrimSpace(cols[1])}
		if len(cols) > 2 {
			c.Status = strings.TrimSpace(cols[2])
		}
		cards = append(cards, c)
	}
	return cards
}

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
	if err := cmd.Run(); err != nil {
		msg := strings.TrimSpace(stderr.String())
		if msg == "" {
			msg = err.Error()
		}
		return "", errors.New(firstLine(msg))
	}
	return stdout.String(), nil
}

func firstLine(s string) string { return strings.SplitN(s, "\n", 2)[0] }

// myCards is R45: `jira me` first, then the open cards assigned to that user.
func myCards() ([]Card, error) {
	me, err := jira("me")
	if err != nil {
		var p Problem
		if errors.As(err, &p) {
			return nil, err
		}
		return nil, Problem("jira is not logged in — run jira init in a terminal")
	}
	user := strings.TrimSpace(me)
	if user == "" {
		return nil, Problem("jira is not logged in — run jira init in a terminal")
	}
	out, err := jira("issue", "list", "-a", user, "-s~Done", "--plain", "--no-headers", "--columns", "key,summary,status")
	if err != nil {
		return nil, Problem("jira issue list failed: " + err.Error())
	}
	return parseCards(out), nil
}
