//go:build !unix

package main

import "os"

// killTree: Windows jobs already take the tree down with the extension; this kills the script.
func killTree(pid int) error {
	p, err := os.FindProcess(pid)
	if err != nil {
		return err
	}
	return p.Kill()
}
