package main

import (
	"encoding/json"
	"errors"
	"net"
	"net/url"
	"os"
	"path/filepath"

	"wai-escritorio/office"
)

const setupFile = "setup.json"

// Setup is what the user entered to join: it lives in the data folder.
type Setup struct {
	Name   string `json:"name"`
	Server string `json:"server"`
	Token  string `json:"token"`
	Avatar int    `json:"avatar"`
}

const (
	msgName   = "Name must be 1 to 32 characters"
	msgServer = "Server address must start with ws:// or wss://"
	msgPlain  = "Use wss:// for a server that is not on this computer"
	msgToken  = "Enter the team token"
	msgAvatar = "Pick an avatar"
)

// validate returns the message to show for a bad setup, or "".
func validate(s Setup) string {
	if !office.ValidName(s.Name) {
		return msgName
	}
	u, err := url.Parse(s.Server)
	if err != nil || (u.Scheme != "ws" && u.Scheme != "wss") || u.Hostname() == "" {
		return msgServer
	}
	if u.Scheme == "ws" && !loopback(u.Hostname()) {
		return msgPlain
	}
	if s.Token == "" {
		return msgToken
	}
	if s.Avatar < 0 || s.Avatar >= office.Avatars {
		return msgAvatar
	}
	return ""
}

func loopback(host string) bool {
	if host == "localhost" {
		return true
	}
	ip := net.ParseIP(host)
	return ip != nil && ip.IsLoopback()
}

// loadSetup reads dir/setup.json; a missing file is no setup (nil, nil).
func loadSetup(dir string) (*Setup, error) {
	b, err := os.ReadFile(filepath.Join(dir, setupFile))
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var s Setup
	if err := json.Unmarshal(b, &s); err != nil {
		return nil, err
	}
	return &s, nil
}

// saveSetup writes dir/setup.json readable only by the user (OFC-13).
func saveSetup(dir string, s Setup) error {
	b, err := json.Marshal(s)
	if err != nil {
		return err
	}
	f, err := os.CreateTemp(dir, setupFile+".*")
	if err != nil {
		return err
	}
	defer os.Remove(f.Name())
	if err := f.Chmod(0o600); err != nil {
		f.Close()
		return err
	}
	if _, err := f.Write(b); err != nil {
		f.Close()
		return err
	}
	if err := f.Close(); err != nil {
		return err
	}
	return os.Rename(f.Name(), filepath.Join(dir, setupFile))
}
