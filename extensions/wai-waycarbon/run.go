package main

import (
	"bytes"
	"context"
	"errors"
	"os"
	"os/exec"
	"strings"
	"time"
)

const cliTimeout = 30 * time.Second

// Problem is something the user can fix; it is shown in the panel as is.
type Problem string

func (p Problem) Error() string { return string(p) }

// ErrNotFound: the program is not on PATH.
type ErrNotFound struct{ Name string }

func (e ErrNotFound) Error() string { return e.Name + " not found" }

// errSilent: the CLI failed without saying why on stderr.
var errSilent = errors.New("command failed without a message")

// runCLI runs a program with argv only (never a shell), stdin null. A ctx without a deadline gets
// 30 s. A failure carries the last non-empty line of stderr.
func runCLI(ctx context.Context, name string, args ...string) (string, error) {
	path, err := exec.LookPath(name)
	if err != nil {
		return "", ErrNotFound{name}
	}
	if _, ok := ctx.Deadline(); !ok {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, cliTimeout)
		defer cancel()
	}
	var stdout, stderr bytes.Buffer
	cmd := exec.CommandContext(ctx, path, args...)
	cmd.Stdout, cmd.Stderr = &stdout, &stderr
	cmd.Env = append(os.Environ(), "GIT_TERMINAL_PROMPT=0")
	// A grandchild holding the pipes cannot hang Run after the timeout kills the program.
	cmd.WaitDelay = 2 * time.Second
	if err := cmd.Run(); err != nil {
		if ctx.Err() != nil {
			return "", Problem(name + " timed out")
		}
		if line := lastLine(stderr.String()); line != "" {
			return "", errors.New(line)
		}
		return "", errSilent
	}
	return stdout.String(), nil
}

func lastLine(s string) string {
	lines := strings.Split(strings.TrimSpace(s), "\n")
	return strings.TrimSpace(lines[len(lines)-1])
}
