//go:build unix

package main

import (
	"fmt"
	"os"
	"syscall"
)

// owned: by the user running wai, or by root.
func owned(path string, fi os.FileInfo) error {
	st, ok := fi.Sys().(*syscall.Stat_t)
	if !ok || (int(st.Uid) != os.Getuid() && st.Uid != 0) {
		return fmt.Errorf("%s is owned by another user", path)
	}
	return nil
}
