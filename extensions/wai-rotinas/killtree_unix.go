//go:build unix

package main

import (
	"errors"
	"os/exec"
	"strconv"
	"strings"
	"syscall"
)

// killTree SIGKILLs everything below pid, then pid. The snapshot can miss a process forked
// meanwhile, so it repeats (up to 3 passes) until a pass finds no descendants.
func killTree(pid int) error {
	for range 3 {
		var rows [][2]int
		if out, err := exec.Command("ps", "-A", "-o", "pid=,ppid=").Output(); err == nil {
			for _, line := range strings.Split(string(out), "\n") {
				if f := strings.Fields(line); len(f) == 2 {
					p, e1 := strconv.Atoi(f[0])
					pp, e2 := strconv.Atoi(f[1])
					if e1 == nil && e2 == nil {
						rows = append(rows, [2]int{p, pp})
					}
				}
			}
		}
		kids := descendants(rows, pid)
		for _, k := range kids {
			syscall.Kill(k, syscall.SIGKILL)
		}
		if len(kids) == 0 {
			break
		}
	}
	if err := syscall.Kill(pid, syscall.SIGKILL); err != nil && !errors.Is(err, syscall.ESRCH) {
		return err
	}
	return nil
}
