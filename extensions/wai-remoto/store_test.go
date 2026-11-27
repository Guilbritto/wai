package main

import (
	"crypto/sha256"
	"encoding/hex"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"
)

var t0 = time.Date(2026, 11, 21, 12, 0, 0, 0, time.UTC)

// clock is a settable time for the store's now.
type clock struct{ now time.Time }

func (c *clock) get() time.Time { return c.now }

func openTest(t *testing.T, dir string, c *clock) *store {
	t.Helper()
	s, err := openStore(dir, c.get)
	if err != nil {
		t.Fatal(err)
	}
	return s
}

func addDevice(t *testing.T, s *store, name string) (string, Device) {
	t.Helper()
	token, d, err := s.add(name)
	if err != nil {
		t.Fatal(err)
	}
	return token, d
}

func names(ds []Device) string {
	var n []string
	for _, d := range ds {
		n = append(n, d.Name)
	}
	return strings.Join(n, ",")
}

// Token storage: state.json (0600) holds the SHA-256 of each token, never the token.
func TestStoreKeepsOnlyTokenHash(t *testing.T) {
	dir := t.TempDir()
	c := &clock{t0}
	s := openTest(t, dir, c)
	if s.st.Port != 7717 || s.st.TunnelURL != "" || len(s.st.Devices) != 0 {
		t.Errorf("a new store: %+v", s.st)
	}
	token, d := addDevice(t, s, "Pixel")
	sum := sha256.Sum256([]byte(token))
	hash := hex.EncodeToString(sum[:])
	if len(token) != 43 || d.TokenHash != hash || len(d.ID) != 32 || d.Name != "Pixel" || !d.Paired.Equal(t0) || !d.LastUsed.Equal(t0) {
		t.Errorf("device %+v for token %q", d, token)
	}

	path := filepath.Join(dir, "state.json")
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(b), token) || !strings.Contains(string(b), hash) {
		t.Errorf("state.json must hold the hash only: %s", b)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	// Windows has no group or other bits: a file written 0600 reads back as 0666 (not read-only).
	want := os.FileMode(0o600)
	if runtime.GOOS == "windows" {
		want = 0o666
	}
	if info.Mode().Perm() != want || !info.Mode().IsRegular() {
		t.Errorf("state.json mode %v, want %v", info.Mode(), want)
	}

	// A fresh store reads it back: the token still works, the hash itself is no token.
	again := openTest(t, dir, c)
	if got, ok := again.lookup(token); !ok || got.ID != d.ID {
		t.Errorf("token after reload: %+v %v", got, ok)
	}
	if _, ok := again.lookup(hash); ok {
		t.Error("the stored hash works as a token")
	}
	if _, ok := again.lookup(""); ok {
		t.Error("an empty token works")
	}
	token2, _ := addDevice(t, again, "iPhone")
	if token2 == token {
		t.Error("two devices got the same token")
	}
}

// DEV-04: a token unused for 30 days is refused and its device leaves the list.
func TestTokenExpiresAfter30DaysIdle(t *testing.T) {
	dir := t.TempDir()
	c := &clock{t0}
	s := openTest(t, dir, c)
	old, _ := addDevice(t, s, "Old")
	c.now = t0.Add(10 * 24 * time.Hour)
	fresh, _ := addDevice(t, s, "Fresh")

	c.now = t0.Add(30*24*time.Hour - time.Second)
	if got := names(s.devices()); got != "Old,Fresh" {
		t.Errorf("a second before 30 days: %q", got)
	}
	c.now = t0.Add(30 * 24 * time.Hour)
	if got := names(s.devices()); got != "Fresh" {
		t.Errorf("after 30 days idle the list is %q, want Fresh", got)
	}
	if _, ok := s.lookup(old); ok {
		t.Error("a token idle for 30 days still works")
	}
	if _, ok := s.lookup(fresh); !ok {
		t.Error("a token used 20 days ago stopped working")
	}
	// Removed on disk too.
	if got := names(openTest(t, dir, c).st.Devices); got != "Fresh" {
		t.Errorf("state.json devices %q", got)
	}
}

// DEV-05: an authenticated request restarts the token's 30-day expiry.
func TestTokenUseRenewsExpiry(t *testing.T) {
	dir := t.TempDir()
	c := &clock{t0}
	s := openTest(t, dir, c)
	token, _ := addDevice(t, s, "Pixel")
	used := t0.Add(29 * 24 * time.Hour)
	c.now = used
	d, ok := s.lookup(token)
	if !ok || !d.LastUsed.Equal(used) || !d.Paired.Equal(t0) {
		t.Fatalf("lookup at 29 days: %+v %v", d, ok)
	}
	// Saved: a restart keeps the renewal.
	s = openTest(t, dir, c)
	if got := s.st.Devices[0].LastUsed; !got.Equal(used) {
		t.Errorf("last_used on disk %v, want %v", got, used)
	}
	c.now = used.Add(30*24*time.Hour - time.Second)
	if _, ok := s.lookup(token); !ok {
		t.Error("59 days after pairing, 30 days less a second after its last use, the token stopped working")
	}
	c.now = c.now.Add(30 * 24 * time.Hour)
	if _, ok := s.lookup(token); ok {
		t.Error("30 days after its last use the token still works")
	}
}

// DEV-02, DEV-03: revoke deletes one device's token; sign out all deletes every one.
func TestRevokeOneAndAll(t *testing.T) {
	dir := t.TempDir()
	c := &clock{t0}
	s := openTest(t, dir, c)
	a, da := addDevice(t, s, "A")
	b, _ := addDevice(t, s, "B")
	cc, _ := addDevice(t, s, "C")

	if !s.revoke(da.ID) {
		t.Fatal("revoke of a paired device failed")
	}
	if s.revoke(da.ID) || s.revoke("nope") {
		t.Error("revoke of an unknown device succeeded")
	}
	if _, ok := s.lookup(a); ok {
		t.Error("a revoked token works")
	}
	if _, ok := s.lookup(b); !ok {
		t.Error("revoking A stopped B")
	}
	if got := names(s.devices()); got != "B,C" {
		t.Errorf("after revoking A: %q", got)
	}
	if got := names(openTest(t, dir, c).st.Devices); got != "B,C" {
		t.Errorf("state.json after revoking A: %q", got)
	}

	s.revokeAll()
	for _, tok := range []string{b, cc} {
		if _, ok := s.lookup(tok); ok {
			t.Error("a token works after sign out all")
		}
	}
	again := openTest(t, dir, c)
	if len(s.devices()) != 0 || len(again.st.Devices) != 0 || again.st.Port != 7717 {
		t.Errorf("after sign out all: %+v, on disk %+v", s.devices(), again.st)
	}
}
