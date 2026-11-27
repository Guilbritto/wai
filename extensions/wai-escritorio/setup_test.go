package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func goodSetup() Setup {
	return Setup{Name: "Ana", Server: "wss://office.example.com", Token: strings.Repeat("t", 32), Avatar: 3}
}

func TestSetupRejectsBadName(t *testing.T) {
	for _, name := range []string{"", strings.Repeat("a", 33), "line\nbreak", "tab\there", "nul\x00"} {
		s := goodSetup()
		s.Name = name
		if got := validate(s); got != "Name must be 1 to 32 characters" {
			t.Errorf("name %q: got %q", name, got)
		}
	}
	for _, name := range []string{"A", strings.Repeat("a", 32), "José"} {
		s := goodSetup()
		s.Name = name
		if got := validate(s); got != "" {
			t.Errorf("name %q: got %q", name, got)
		}
	}
}

func TestSetupSavedPrivate(t *testing.T) {
	dir := t.TempDir()
	want := goodSetup()
	if err := saveSetup(dir, want); err != nil {
		t.Fatal(err)
	}
	fi, err := os.Stat(filepath.Join(dir, "setup.json"))
	if err != nil {
		t.Fatal(err)
	}
	if fi.Mode().Perm() != 0o600 {
		t.Errorf("mode %v, want 0600", fi.Mode().Perm())
	}
	got, err := loadSetup(dir)
	if err != nil || got == nil || *got != want {
		t.Fatalf("load: %+v, %v", got, err)
	}
	// Saving over a looser file keeps it private.
	p := filepath.Join(dir, "setup.json")
	os.Chmod(p, 0o644)
	if err := saveSetup(dir, want); err != nil {
		t.Fatal(err)
	}
	if fi, _ := os.Stat(p); fi.Mode().Perm() != 0o600 {
		t.Errorf("mode after overwrite %v", fi.Mode().Perm())
	}
	if none, err := loadSetup(t.TempDir()); none != nil || err != nil {
		t.Errorf("missing file: %+v, %v", none, err)
	}
}

func TestSetupRefusesPlainRemote(t *testing.T) {
	const want = "Use wss:// for a server that is not on this computer"
	for _, addr := range []string{"ws://office.example.com:7420", "ws://10.0.0.5", "ws://192.168.1.2:7420"} {
		s := goodSetup()
		s.Server = addr
		if got := validate(s); got != want {
			t.Errorf("%s: got %q", addr, got)
		}
	}
	for _, addr := range []string{"ws://127.0.0.1:7420", "ws://localhost:7420", "ws://[::1]:7420", "wss://office.example.com"} {
		s := goodSetup()
		s.Server = addr
		if got := validate(s); got != "" {
			t.Errorf("%s: got %q", addr, got)
		}
	}
	s := goodSetup()
	s.Server = "office.example.com"
	if validate(s) == "" {
		t.Error("an address with no scheme must be refused")
	}
}
