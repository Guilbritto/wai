package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"time"
)

const (
	keychainService = "Claude Code-credentials"
	usagePath       = "/api/oauth/usage"
	defaultRetry    = 15 * time.Minute
)

// tokenSource returns Claude Code's OAuth access token, or errNoLogin when there is none.
type tokenSource func(ctx context.Context) (string, error)

// keychain reads the secret from the macOS Keychain; a variable so tests never touch the real one.
var keychain = func(ctx context.Context) (string, error) {
	ctx, cancel := context.WithTimeout(ctx, 60*time.Second)
	defer cancel()
	out, err := exec.CommandContext(ctx, "security", "find-generic-password", "-s", keychainService, "-w").Output()
	return string(out), err
}

func accessToken(secret []byte) string {
	var c struct {
		OAuth struct {
			AccessToken string `json:"accessToken"`
		} `json:"claudeAiOauth"`
	}
	if json.Unmarshal(secret, &c) != nil {
		return ""
	}
	return strings.TrimSpace(c.OAuth.AccessToken)
}

// claudeToken reads Claude Code's login: the Keychain on macOS, then .credentials.json. Read only.
func claudeToken(ctx context.Context) (string, error) {
	if runtime.GOOS == "darwin" && ctx.Err() == nil {
		if secret, err := keychain(ctx); err == nil {
			if tok := accessToken([]byte(secret)); tok != "" {
				return tok, nil
			}
		}
	}
	dir := os.Getenv("CLAUDE_CONFIG_DIR")
	if dir == "" {
		home, err := os.UserHomeDir()
		if err != nil {
			return "", errNoLogin
		}
		dir = filepath.Join(home, ".claude")
	}
	if b, err := os.ReadFile(filepath.Join(dir, ".credentials.json")); err == nil {
		if tok := accessToken(b); tok != "" {
			return tok, nil
		}
	}
	return "", errNoLogin
}

type rateLimited struct{ retry time.Duration }

func (r rateLimited) Error() string { return "rate limited, retry in " + r.retry.String() }

// fetch asks the usage endpoint for the windows. Its errors are built here and never carry the
// token or the response body: the card shows them.
func fetch(ctx context.Context, c *http.Client, usageURL, token string) ([]window, error) {
	ctx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, usageURL, nil)
	if err != nil {
		return nil, errors.New("bad usage URL")
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("anthropic-beta", "oauth-2025-04-20")
	resp, err := c.Do(req)
	if err != nil {
		if ctx.Err() != nil {
			return nil, errors.New("request timed out")
		}
		return nil, errors.New("couldn't reach the server")
	}
	defer resp.Body.Close()
	switch {
	case resp.StatusCode == http.StatusUnauthorized:
		return nil, errExpired
	case resp.StatusCode == http.StatusTooManyRequests:
		retry := defaultRetry
		if s, err := strconv.Atoi(strings.TrimSpace(resp.Header.Get("Retry-After"))); err == nil && s > 0 {
			retry = time.Duration(s) * time.Second
		}
		return nil, rateLimited{retry}
	case resp.StatusCode < 200 || resp.StatusCode > 299:
		return nil, fmt.Errorf("server returned %d", resp.StatusCode)
	}
	body, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return nil, errors.New("couldn't read the response")
	}
	return parseUsage(body)
}

// next is when the next fetch is due, by the wall clock. The base is five minutes after the last
// attempt (a failure counts, or an outage would refetch every minute); a reset after the shown
// values were fetched makes it due 5 s after that reset, even if already past; a 429's retry
// overrides both. The caller compares it with the clock, so now is unused.
func next(v view, retry time.Duration, _ time.Time) time.Time {
	updated, failed := v.updated.Round(0), v.failed.Round(0)
	if retry > 0 && !failed.IsZero() {
		return failed.Add(retry)
	}
	last := updated
	if failed.After(last) {
		last = failed
	}
	due := last.Add(5 * time.Minute)
	for _, w := range v.windows {
		if r := w.Resets.Round(0); r.After(updated) && r.Add(5*time.Second).Before(due) {
			due = r.Add(5 * time.Second)
		}
	}
	return due
}
