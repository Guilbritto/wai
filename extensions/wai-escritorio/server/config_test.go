package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

const goodToken = "0123456789abcdef0123456789abcdef"

// writeConfig writes office.json with the given JSON body into a temp dir.
func writeConfig(t *testing.T, body string) string {
	t.Helper()
	p := filepath.Join(t.TempDir(), "office.json")
	if err := os.WriteFile(p, []byte(body), 0o600); err != nil {
		t.Fatal(err)
	}
	return p
}

func wantErr(t *testing.T, path, msg string) {
	t.Helper()
	_, _, err := loadConfig(path)
	if err == nil || !strings.Contains(err.Error(), msg) {
		t.Fatalf("err = %v, want it to contain %q", err, msg)
	}
}

func TestConfigRejectsShortToken(t *testing.T) {
	wantErr(t, writeConfig(t, `{"listen":"127.0.0.1:0","token":"short"}`), "token must be at least 32 characters")
	wantErr(t, writeConfig(t, `{"listen":"127.0.0.1:0","token":"`+goodToken[:31]+`"}`), "token must be at least 32 characters")
	wantErr(t, writeConfig(t, `{"listen":"127.0.0.1:0"}`), "token must be at least 32 characters")
}

func TestConfigRequiresTLSOffLoopback(t *testing.T) {
	for _, l := range []string{"0.0.0.0:7420", ":7420", "192.168.1.5:7420", "example.com:7420"} {
		wantErr(t, writeConfig(t, `{"listen":"`+l+`","token":"`+goodToken+`"}`), "TLS is required off loopback")
	}
	// cert without key is not TLS
	wantErr(t, writeConfig(t, `{"listen":"0.0.0.0:7420","token":"`+goodToken+`","tls_cert":"c.pem"}`), "tls_cert and tls_key")
	c, _, err := loadConfig(writeConfig(t, `{"listen":"0.0.0.0:7420","token":"`+goodToken+`","tls_cert":"c.pem","tls_key":"k.pem"}`))
	if err != nil || !c.tls() {
		t.Fatalf("off loopback with cert and key: %v tls=%v", err, c.tls())
	}
}

func TestConfigAllowsPlainLoopback(t *testing.T) {
	for _, l := range []string{"127.0.0.1:7420", "127.0.0.1:0", "[::1]:7420", "localhost:7420"} {
		c, m, err := loadConfig(writeConfig(t, `{"listen":"`+l+`","token":"`+goodToken+`"}`))
		if err != nil {
			t.Fatalf("%s: %v", l, err)
		}
		if c.Listen != l || m.W == 0 {
			t.Fatalf("%s: config %+v, map %dx%d", l, c, m.W, m.H)
		}
	}
	c, _, err := loadConfig(writeConfig(t, `{"token":"`+goodToken+`"}`))
	if err != nil || c.Listen != "127.0.0.1:7420" {
		t.Fatalf("default listen: %v %q", err, c.Listen)
	}
}

func TestConfigBadMapRefused(t *testing.T) {
	dir := t.TempDir()
	cfg := func(m string) string {
		p := filepath.Join(dir, "office.json")
		os.WriteFile(p, []byte(`{"listen":"127.0.0.1:0","token":"`+goodToken+`","map":"`+m+`"}`), 0o600)
		return p
	}
	wantErr(t, cfg("nope.map"), "map file missing")
	os.WriteFile(filepath.Join(dir, "nospawn.map"), []byte("###\n#.#\n###\n"), 0o600)
	wantErr(t, cfg("nospawn.map"), "map has no spawn point")
	os.WriteFile(filepath.Join(dir, "bad.map"), []byte("####\n#S?#\n####\n"), 0o600)
	wantErr(t, cfg("bad.map"), "unknown tile '?' at 2,1")
	os.WriteFile(filepath.Join(dir, "ok.map"), []byte("####\n#S.#\n####\n"), 0o600)
	_, m, err := loadConfig(cfg("ok.map"))
	if err != nil || m.Spawn.X != 1 || m.Spawn.Y != 1 {
		t.Fatalf("good custom map: %v %+v", err, m.Spawn)
	}
}
