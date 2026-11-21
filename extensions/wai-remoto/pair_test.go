package main

import (
	"encoding/base64"
	"errors"
	"strings"
	"testing"
	"time"
)

// PAIR-09: a valid code and a device name give a new device token, and the code is used.
func TestPairIssuesTokenAndUsesCode(t *testing.T) {
	c := &clock{t0}
	s := openTest(t, t.TempDir(), c)
	p := &pairing{store: s}
	code, expires := p.issue()
	if raw, err := base64.RawURLEncoding.DecodeString(code); err != nil || len(raw) != 16 || len(code) != 22 {
		t.Errorf("code %q: want 16 random bytes in base64url (%v)", code, err)
	}
	if !expires.Equal(t0.Add(5 * time.Minute)) {
		t.Errorf("expires %v, want 5 minutes from now", expires)
	}
	if next, _ := p.issue(); next == code {
		t.Error("a new code repeats the previous one")
	} else {
		code = next
	}

	c.now = t0.Add(5*time.Minute - time.Second)
	token, err := p.redeem(code, "Pixel 9")
	if err != nil || token == "" {
		t.Fatalf("redeem: %q, %v", token, err)
	}
	d, ok := s.lookup(token)
	if !ok || d.Name != "Pixel 9" || !d.Paired.Equal(c.now) {
		t.Errorf("paired device %+v, %v", d, ok)
	}
	if again, err := p.redeem(code, "Other"); !errors.Is(err, errBadCode) || again != "" {
		t.Errorf("the code worked twice: %q, %v", again, err)
	}
	if len(s.devices()) != 1 {
		t.Errorf("devices: %+v", s.devices())
	}

	// A name longer than 64 characters is cut.
	code, _ = p.issue()
	token, _ = p.redeem(code, strings.Repeat("é", 70))
	if d, _ := s.lookup(token); d.Name != strings.Repeat("é", 64) {
		t.Errorf("name %q", d.Name)
	}
}

// PAIR-10: an unknown, expired, used or replaced code issues no token and pairs nothing.
func TestPairRejectsUnknownExpiredUsedCode(t *testing.T) {
	c := &clock{t0}
	s := openTest(t, t.TempDir(), c)
	p := &pairing{store: s}
	refused := func(what, code string) {
		t.Helper()
		if token, err := p.redeem(code, "Phone"); !errors.Is(err, errBadCode) || token != "" {
			t.Errorf("%s code: token %q, error %v", what, token, err)
		}
	}
	refused("no issued", "")
	code, _ := p.issue()
	refused("an unknown", "AAAAAAAAAAAAAAAAAAAAAA")
	refused("an empty", "")

	c.now = t0.Add(5 * time.Minute)
	refused("an expired", code)

	code, _ = p.issue()
	if _, err := p.redeem(code, "Phone"); err != nil {
		t.Fatal(err)
	}
	refused("a used", code)

	old, _ := p.issue()
	p.issue()
	refused("a replaced", old)

	if got := names(s.devices()); got != "Phone" {
		t.Errorf("only the one good code pairs: %q", got)
	}
}
