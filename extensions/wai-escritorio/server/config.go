package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"unicode/utf8"

	"wai-escritorio/office"
)

const minTokenLen = 32

// Config is the office.json file.
type Config struct {
	Listen  string `json:"listen"`
	Token   string `json:"token"`
	TLSCert string `json:"tls_cert"`
	TLSKey  string `json:"tls_key"`
	Map     string `json:"map"`
}

func (c Config) tls() bool { return c.TLSCert != "" && c.TLSKey != "" }

// loadConfig reads and checks the config file and loads its map. A relative
// map path is read from the config file's folder.
func loadConfig(path string) (Config, office.Map, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return Config{}, office.Map{}, err
	}
	c := Config{Listen: "127.0.0.1:7420"}
	if err := json.Unmarshal(data, &c); err != nil {
		return Config{}, office.Map{}, fmt.Errorf("config %s: %w", path, err)
	}
	if utf8.RuneCountInString(c.Token) < minTokenLen {
		return Config{}, office.Map{}, errors.New("token must be at least 32 characters")
	}
	if (c.TLSCert == "") != (c.TLSKey == "") {
		return Config{}, office.Map{}, errors.New("tls_cert and tls_key must be set together")
	}
	if !c.tls() && !loopback(c.Listen) {
		return Config{}, office.Map{}, errors.New("TLS is required off loopback")
	}
	m, err := loadMap(c.Map, filepath.Dir(path))
	if err != nil {
		return Config{}, office.Map{}, err
	}
	return c, m, nil
}

func loadMap(file, dir string) (office.Map, error) {
	if file == "" {
		return office.DefaultMap(), nil
	}
	if !filepath.IsAbs(file) {
		file = filepath.Join(dir, file)
	}
	text, err := os.ReadFile(file)
	if err != nil {
		return office.Map{}, fmt.Errorf("map file missing: %w", err)
	}
	m, err := office.ParseMap(string(text))
	if err != nil {
		return office.Map{}, fmt.Errorf("map %s: %w", file, err)
	}
	return m, nil
}

// loopback reports whether addr (host:port) binds only to this computer.
func loopback(addr string) bool {
	host, _, err := net.SplitHostPort(addr)
	if err != nil {
		return false
	}
	if host == "localhost" {
		return true
	}
	ip := net.ParseIP(host)
	return ip != nil && ip.IsLoopback()
}
