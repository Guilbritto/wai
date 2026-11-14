package main

import (
	"context"
	"crypto"
	"crypto/rand"
	"crypto/rsa"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"strings"
	"sync"
	"time"
)

type Config struct {
	Addr, Data, Org, ClientID, AppID, InstallationID string
	AppKey                                           *rsa.PrivateKey
	API, Web                                         string
}

func ConfigFromEnv() (Config, error) {
	env := func(k, def string) string {
		if v := os.Getenv(k); v != "" {
			return v
		}
		return def
	}
	c := Config{
		Addr:           env("RELAY_ADDR", ":8080"),
		Data:           os.Getenv("RELAY_DATA"),
		Org:            os.Getenv("RELAY_ORG"),
		ClientID:       os.Getenv("RELAY_GITHUB_CLIENT_ID"),
		AppID:          os.Getenv("RELAY_GITHUB_APP_ID"),
		InstallationID: os.Getenv("RELAY_GITHUB_INSTALLATION_ID"),
		API:            env("RELAY_GITHUB_API", "https://api.github.com"),
		Web:            env("RELAY_GITHUB_WEB", "https://github.com"),
	}
	for _, r := range []struct{ name, v string }{
		{"RELAY_DATA", c.Data}, {"RELAY_ORG", c.Org}, {"RELAY_GITHUB_CLIENT_ID", c.ClientID},
		{"RELAY_GITHUB_APP_ID", c.AppID}, {"RELAY_GITHUB_INSTALLATION_ID", c.InstallationID},
		{"RELAY_GITHUB_APP_KEY", os.Getenv("RELAY_GITHUB_APP_KEY")},
	} {
		if r.v == "" {
			return Config{}, fmt.Errorf("%s is required", r.name)
		}
	}
	key, err := loadKey(os.Getenv("RELAY_GITHUB_APP_KEY"))
	if err != nil {
		return Config{}, fmt.Errorf("RELAY_GITHUB_APP_KEY: %w", err)
	}
	c.AppKey = key
	return c, nil
}

func loadKey(path string) (*rsa.PrivateKey, error) {
	b, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	blk, _ := pem.Decode(b)
	if blk == nil {
		return nil, errors.New("no PEM block")
	}
	if k, err := x509.ParsePKCS1PrivateKey(blk.Bytes); err == nil {
		return k, nil
	}
	k, err := x509.ParsePKCS8PrivateKey(blk.Bytes)
	if err != nil {
		return nil, errors.New("not a PKCS#1 or PKCS#8 key")
	}
	rk, ok := k.(*rsa.PrivateKey)
	if !ok {
		return nil, errors.New("not an RSA key")
	}
	return rk, nil
}

var ErrPending = errors.New("authorization pending")

type DeviceStart struct {
	DeviceCode      string `json:"device_code"`
	UserCode        string `json:"user_code"`
	VerificationURI string `json:"verification_uri"`
	Interval        int    `json:"interval"`
}

type GitHub struct {
	cfg    Config
	client *http.Client

	mu      sync.Mutex
	instTok string
	instExp time.Time
}

func NewGitHub(c Config, client *http.Client) *GitHub {
	return &GitHub{cfg: c, client: client}
}

// do sends the request and decodes a JSON body into out (nil to skip). It returns the status code.
func (g *GitHub) do(req *http.Request, out any) (int, error) {
	req.Header.Set("Accept", "application/json")
	resp, err := g.client.Do(req)
	if err != nil {
		return 0, err
	}
	defer resp.Body.Close()
	b, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return 0, err
	}
	if out != nil && len(b) > 0 {
		if err := json.Unmarshal(b, out); err != nil {
			return resp.StatusCode, fmt.Errorf("github: bad response (%d)", resp.StatusCode)
		}
	}
	return resp.StatusCode, nil
}

func (g *GitHub) postForm(ctx context.Context, path string, form url.Values, out any) (int, error) {
	req, err := http.NewRequestWithContext(ctx, "POST", g.cfg.Web+path, strings.NewReader(form.Encode()))
	if err != nil {
		return 0, err
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	return g.do(req, out)
}

func (g *GitHub) DeviceCode(ctx context.Context) (DeviceStart, error) {
	var ds DeviceStart
	code, err := g.postForm(ctx, "/login/device/code", url.Values{"client_id": {g.cfg.ClientID}}, &ds)
	if err != nil {
		return DeviceStart{}, err
	}
	if code != 200 || ds.DeviceCode == "" {
		return DeviceStart{}, fmt.Errorf("github: device code failed (%d)", code)
	}
	return ds, nil
}

func (g *GitHub) PollToken(ctx context.Context, deviceCode string) (string, error) {
	var r struct {
		AccessToken string `json:"access_token"`
		Error       string `json:"error"`
	}
	form := url.Values{"client_id": {g.cfg.ClientID}, "device_code": {deviceCode}, "grant_type": {"urn:ietf:params:oauth:grant-type:device_code"}}
	if _, err := g.postForm(ctx, "/login/oauth/access_token", form, &r); err != nil {
		return "", err
	}
	switch {
	case r.Error == "authorization_pending" || r.Error == "slow_down":
		return "", ErrPending
	case r.Error != "":
		return "", fmt.Errorf("github: %s", r.Error)
	case r.AccessToken == "":
		return "", errors.New("github: no access token")
	}
	return r.AccessToken, nil
}

func (g *GitHub) User(ctx context.Context, token string) (int64, string, error) {
	req, err := http.NewRequestWithContext(ctx, "GET", g.cfg.API+"/user", nil)
	if err != nil {
		return 0, "", err
	}
	req.Header.Set("Authorization", "Bearer "+token)
	var u struct {
		ID    int64  `json:"id"`
		Login string `json:"login"`
	}
	code, err := g.do(req, &u)
	if err != nil {
		return 0, "", err
	}
	if code != 200 || u.ID == 0 || u.Login == "" {
		return 0, "", fmt.Errorf("github: user lookup failed (%d)", code)
	}
	return u.ID, u.Login, nil
}

func b64(b []byte) string { return base64.RawURLEncoding.EncodeToString(b) }

func (g *GitHub) appJWT() (string, error) {
	now := time.Now()
	claims, _ := json.Marshal(map[string]any{"iat": now.Add(-60 * time.Second).Unix(), "exp": now.Add(9 * time.Minute).Unix(), "iss": g.cfg.AppID})
	signing := b64([]byte(`{"alg":"RS256","typ":"JWT"}`)) + "." + b64(claims)
	sum := sha256.Sum256([]byte(signing))
	sig, err := rsa.SignPKCS1v15(rand.Reader, g.cfg.AppKey, crypto.SHA256, sum[:])
	if err != nil {
		return "", err
	}
	return signing + "." + b64(sig), nil
}

// installationToken returns a cached installation token, renewing it one minute before expiry.
func (g *GitHub) installationToken(ctx context.Context) (string, error) {
	g.mu.Lock()
	defer g.mu.Unlock()
	if g.instTok != "" && time.Now().Before(g.instExp.Add(-time.Minute)) {
		return g.instTok, nil
	}
	jwt, err := g.appJWT()
	if err != nil {
		return "", err
	}
	req, err := http.NewRequestWithContext(ctx, "POST", g.cfg.API+"/app/installations/"+url.PathEscape(g.cfg.InstallationID)+"/access_tokens", nil)
	if err != nil {
		return "", err
	}
	req.Header.Set("Authorization", "Bearer "+jwt)
	var r struct {
		Token     string    `json:"token"`
		ExpiresAt time.Time `json:"expires_at"`
	}
	code, err := g.do(req, &r)
	if err != nil {
		return "", err
	}
	if code != 201 || r.Token == "" {
		return "", fmt.Errorf("github: installation token failed (%d)", code)
	}
	g.instTok, g.instExp = r.Token, r.ExpiresAt
	return g.instTok, nil
}

func (g *GitHub) IsMember(ctx context.Context, login string) (bool, error) {
	tok, err := g.installationToken(ctx)
	if err != nil {
		return false, err
	}
	req, err := http.NewRequestWithContext(ctx, "GET", g.cfg.API+"/orgs/"+url.PathEscape(g.cfg.Org)+"/members/"+url.PathEscape(login), nil)
	if err != nil {
		return false, err
	}
	req.Header.Set("Authorization", "Bearer "+tok)
	code, err := g.do(req, nil)
	if err != nil {
		return false, err
	}
	switch code {
	case 204:
		return true, nil
	case 404:
		return false, nil
	}
	return false, fmt.Errorf("github: membership check failed (%d)", code)
}
