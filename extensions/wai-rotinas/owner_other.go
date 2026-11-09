//go:build !unix

package main

import (
	"errors"
	"os"
)

// owned cannot tell who may write a file here (Windows uses ACLs, not mode bits), so it refuses.
// ponytail: refuses every script on Windows; read the ACL with golang.org/x/sys/windows when needed.
func owned(path string, fi os.FileInfo) error {
	return errors.New("wai-rotinas cannot check who may change a file on this system; routines run on macOS and Linux only")
}
