package main

import (
	"crypto/subtle"
	"encoding/base64"
	"errors"
	"sync"
	"time"
)

// codeTTL: a pairing code works for 5 minutes, once.
const codeTTL = 5 * time.Minute

// errBadCode: the code is unknown, expired or used. The pair route answers it with 401.
var errBadCode = errors.New("Unknown, expired or used pairing code")

// pairing holds the one live pairing code, in memory only.
type pairing struct {
	store *store

	mu      sync.Mutex
	code    string // "" when there is none
	expires time.Time
}

// issue makes a new code, 16 random bytes in base64url, replacing the previous one.
func (p *pairing) issue() (string, time.Time) {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.code = base64.RawURLEncoding.EncodeToString(random(16))
	p.expires = p.store.now().Add(codeTTL)
	return p.code, p.expires
}

// redeem pairs a device with the live code and returns its token; the code is then used.
func (p *pairing) redeem(code, name string) (string, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.code == "" || subtle.ConstantTimeCompare([]byte(code), []byte(p.code)) != 1 || !p.store.now().Before(p.expires) {
		return "", errBadCode
	}
	p.code = ""
	if r := []rune(name); len(r) > 64 {
		name = string(r[:64])
	}
	token, _, err := p.store.add(name)
	return token, err
}
