package main

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// fakeCLI puts an executable `name` first on PATH; it runs the shell script.
func fakeCLI(t *testing.T, name, script string) {
	t.Helper()
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, name), []byte("#!/bin/sh\n"+script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", dir+":"+os.Getenv("PATH"))
}

// fakeClaude logs every call (argv, then GIT_TERMINAL_PROMPT) and lists the plugin as installed.
func fakeClaude(t *testing.T) (log string) {
	t.Helper()
	log = filepath.Join(t.TempDir(), "claude.log")
	fakeCLI(t, "claude", `echo "$* GIT_TERMINAL_PROMPT=$GIT_TERMINAL_PROMPT" >> '`+log+`'
[ "$1 $2" = "plugin list" ] && echo '[{"id":"other@x"},{"id":"waycarbon@waycarbon"}]'
exit 0
`)
	return log
}

func calls(log string) []string {
	b, _ := os.ReadFile(log)
	return strings.Split(strings.TrimSpace(string(b)), "\n")
}

func TestPluginInstalledParsesList(t *testing.T) {
	fakeClaude(t)
	id, ok, err := pluginInstalled(context.Background())
	if err != nil || !ok || id != "waycarbon@waycarbon" {
		t.Fatalf("got %q %v %v", id, ok, err)
	}
	fakeCLI(t, "claude", `echo '[{"id":"other@x"}]'`)
	if _, ok, err := pluginInstalled(context.Background()); ok || err != nil {
		t.Fatalf("want missing, got %v %v", ok, err)
	}
}

func TestInstallPluginArgv(t *testing.T) {
	log := fakeClaude(t)
	if err := installPlugin(context.Background()); err != nil {
		t.Fatal(err)
	}
	got := calls(log)
	want := "plugin install waycarbon --marketplace waycarbon/claude-code --scope user GIT_TERMINAL_PROMPT=0"
	if len(got) != 1 || got[0] != want {
		t.Errorf("got %q, want %q", got, want)
	}
	if strings.Contains(got[0], "-y") {
		t.Error("never -y")
	}
}

func TestUpdateOncePerDay(t *testing.T) {
	log := fakeClaude(t)
	dir := t.TempDir()
	now := time.Date(2026, 11, 14, 9, 0, 0, 0, time.UTC)
	updates := func() string {
		var u []string
		for _, l := range calls(log) {
			if strings.HasPrefix(l, "plugin marketplace update") || strings.HasPrefix(l, "plugin update") {
				u = append(u, strings.TrimSuffix(l, " GIT_TERMINAL_PROMPT=0"))
			}
		}
		return strings.Join(u, "|")
	}
	if err := updatePluginDaily(context.Background(), dir, now); err != nil {
		t.Fatal(err)
	}
	const once = "plugin marketplace update waycarbon|plugin update waycarbon@waycarbon"
	if got := updates(); got != once {
		t.Fatalf("first: %q", got)
	}
	updatePluginDaily(context.Background(), dir, now.Add(time.Hour))
	if got := updates(); got != once {
		t.Fatalf("1 h later must run nothing: %q", got)
	}
	updatePluginDaily(context.Background(), dir, now.Add(25*time.Hour))
	if got := updates(); got != once+"|"+once {
		t.Fatalf("25 h later must run again: %q", got)
	}
}

func TestUpdateSkippedWithoutPlugin(t *testing.T) {
	fakeCLI(t, "claude", `echo '[]'`)
	dir := t.TempDir()
	if err := updatePluginDaily(context.Background(), dir, time.Now()); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(dir, "plugin-updated")); err == nil {
		t.Error("nothing to update, nothing recorded")
	}
}

func TestUpdateFailureStillRecordsTime(t *testing.T) {
	fakeCLI(t, "claude", `[ "$2" = list ] && { echo '[{"id":"waycarbon@m"}]'; exit 0; }
echo "network down" >&2; exit 1
`)
	dir := t.TempDir()
	err := updatePluginDaily(context.Background(), dir, time.Now())
	if err == nil || !strings.Contains(err.Error(), "network down") {
		t.Fatalf("want the stderr line, got %v", err)
	}
	if _, err := os.Stat(filepath.Join(dir, "plugin-updated")); err != nil {
		t.Error("the time must be written on failure")
	}
}
