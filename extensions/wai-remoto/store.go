package main

import (
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"time"
)

const (
	stateFile   = "state.json"
	defaultPort = 7717
	// DEV-04: a token unused this long stops working.
	idleExpiry = 30 * 24 * time.Hour
)

// State is state.json: the settings and the paired devices. It never holds a token, only its
// SHA-256, so a copy of the file pairs nothing.
type State struct {
	Port      int      `json:"port"`
	TunnelURL string   `json:"tunnel_url"`
	Devices   []Device `json:"devices"`
}

type Device struct {
	ID        string    `json:"id"`         // 16 random bytes, hex
	Name      string    `json:"name"`       // sent by the app at pairing
	TokenHash string    `json:"token_hash"` // hex SHA-256 of the token
	Paired    time.Time `json:"paired"`
	LastUsed  time.Time `json:"last_used"` // expires idleExpiry after it
}

type store struct {
	path string
	now  func() time.Time

	mu sync.Mutex
	st State
}

// openStore reads state.json from dir; a missing file is a new install.
func openStore(dir string, now func() time.Time) (*store, error) {
	s := &store{path: filepath.Join(dir, stateFile), now: now, st: State{Port: defaultPort}}
	b, err := os.ReadFile(s.path)
	if errors.Is(err, os.ErrNotExist) {
		return s, nil
	}
	if err != nil {
		return nil, err
	}
	if err := json.Unmarshal(b, &s.st); err != nil {
		return nil, err
	}
	return s, nil
}

// save writes state.json 0600, through a temporary file so a crash never leaves half of it.
func (s *store) save() error {
	b, err := json.MarshalIndent(s.st, "", "  ")
	if err != nil {
		return err
	}
	tmp := s.path + ".tmp"
	if err := os.WriteFile(tmp, b, 0o600); err != nil {
		return err
	}
	if err := os.Chmod(tmp, 0o600); err != nil { // WriteFile's mode is cut by the umask only
		return err
	}
	return os.Rename(tmp, s.path)
}

// persist saves, or logs why it could not: the change holds in memory until the next save.
func (s *store) persist() {
	if err := s.save(); err != nil {
		fmt.Fprintln(os.Stderr, "wai-remoto: cannot save", s.path+":", err)
	}
}

func random(n int) []byte {
	b := make([]byte, n)
	rand.Read(b) // never fails (crypto/rand panics instead)
	return b
}

func hashToken(token string) string {
	sum := sha256.Sum256([]byte(token))
	return hex.EncodeToString(sum[:])
}

func (s *store) expired(d Device) bool {
	return !s.now().Before(d.LastUsed.Add(idleExpiry))
}

// prune drops the devices idle for idleExpiry; under s.mu. It reports whether any went.
func (s *store) prune() bool {
	kept := s.st.Devices[:0]
	for _, d := range s.st.Devices {
		if !s.expired(d) {
			kept = append(kept, d)
		}
	}
	gone := len(kept) != len(s.st.Devices)
	s.st.Devices = kept
	return gone
}

// settings are the port and the tunnel URL.
func (s *store) settings() (int, string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.st.Port, s.st.TunnelURL
}

// add pairs a device: a new 32-byte token, returned once and stored as its hash.
func (s *store) add(name string) (string, Device, error) {
	token := base64.RawURLEncoding.EncodeToString(random(32))
	now := s.now()
	d := Device{ID: hex.EncodeToString(random(16)), Name: name, TokenHash: hashToken(token), Paired: now, LastUsed: now}
	s.mu.Lock()
	defer s.mu.Unlock()
	s.prune()
	s.st.Devices = append(s.st.Devices, d)
	if err := s.save(); err != nil {
		s.st.Devices = s.st.Devices[:len(s.st.Devices)-1]
		return "", Device{}, err
	}
	return token, d, nil
}

// lookup finds the device of a token and renews its expiry (DEV-05). An expired device is
// removed (DEV-04).
func (s *store) lookup(token string) (Device, bool) {
	if token == "" {
		return Device{}, false
	}
	hash := []byte(hashToken(token))
	s.mu.Lock()
	defer s.mu.Unlock()
	gone := s.prune()
	for i, d := range s.st.Devices {
		if subtle.ConstantTimeCompare(hash, []byte(d.TokenHash)) == 1 {
			s.st.Devices[i].LastUsed = s.now()
			s.persist()
			return s.st.Devices[i], true
		}
	}
	if gone {
		s.persist()
	}
	return Device{}, false
}

// devices are the paired devices, the expired ones removed.
func (s *store) devices() []Device {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.prune() {
		s.persist()
	}
	return append([]Device(nil), s.st.Devices...)
}

// revoke deletes one device and its token; false when no device has that id.
func (s *store) revoke(id string) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	for i, d := range s.st.Devices {
		if d.ID == id {
			s.st.Devices = append(s.st.Devices[:i], s.st.Devices[i+1:]...)
			s.persist()
			return true
		}
	}
	return false
}

// revokeAll deletes every device and token ("Sign out all devices").
func (s *store) revokeAll() {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.st.Devices = nil
	s.persist()
}
