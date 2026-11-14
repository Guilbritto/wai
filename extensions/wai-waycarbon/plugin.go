package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
)

const (
	pluginName        = "waycarbon"
	pluginMarketplace = "waycarbon/claude-code"
	updateEvery       = 24 * time.Hour
)

// pluginInstalled reads `claude plugin list --json` for the first `waycarbon@…` id.
func pluginInstalled(ctx context.Context) (id string, ok bool, err error) {
	out, err := runCLI(ctx, "claude", "plugin", "list", "--json")
	if err != nil {
		return "", false, err
	}
	var list []struct {
		ID string `json:"id"`
	}
	if err := json.Unmarshal([]byte(out), &list); err != nil {
		return "", false, errors.New("claude plugin list: unreadable output")
	}
	for _, p := range list {
		if strings.HasPrefix(p.ID, pluginName+"@") {
			return p.ID, true, nil
		}
	}
	return "", false, nil
}

// installPlugin is the line wai core runs; never `-y`.
func installPlugin(ctx context.Context) error {
	ctx, cancel := context.WithTimeout(ctx, 5*time.Minute)
	defer cancel()
	_, err := runCLI(ctx, "claude", "plugin", "install", pluginName, "--marketplace", pluginMarketplace, "--scope", "user")
	return err
}

// updatePluginDaily refreshes the marketplace and the plugin at most once a day. Nothing runs when
// the plugin is not installed. The time is written only after a successful update, so a failed or
// cancelled one is tried again at the next start.
func updatePluginDaily(ctx context.Context, dataDir string, now time.Time) error {
	stamp := filepath.Join(dataDir, "plugin-updated")
	if b, err := os.ReadFile(stamp); err == nil {
		if last, err := time.Parse(time.RFC3339, strings.TrimSpace(string(b))); err == nil && now.Sub(last) < updateEvery {
			return nil
		}
	}
	id, ok, err := pluginInstalled(ctx)
	if err != nil || !ok {
		return err
	}
	market := id[strings.Index(id, "@")+1:]
	_, err = runCLI(ctx, "claude", "plugin", "marketplace", "update", market)
	if err == nil {
		_, err = runCLI(ctx, "claude", "plugin", "update", id)
	}
	if err != nil {
		return err
	}
	if err := os.WriteFile(stamp, []byte(now.Format(time.RFC3339)), 0o644); err != nil {
		fmt.Fprintln(os.Stderr, "wai-waycarbon: plugin update stamp:", err)
	}
	return nil
}
