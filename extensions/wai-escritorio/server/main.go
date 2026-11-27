// Command wai-escritorio-server is the office server: it owns who is where
// and fans changes out to every connected wai-escritorio extension.
package main

import (
	"context"
	"crypto/tls"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"log"
	"net"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/coder/websocket"

	"wai-escritorio/office"
)

const (
	readLimit    = 16 << 10 // bytes per message (OFC-25)
	rateLimit    = 30       // messages acted on per second (OFC-24)
	helloTimeout = 10 * time.Second
	writeTimeout = 10 * time.Second
)

// Ping timings. A peer that stops answering leaves within pingEvery +
// pongWithin = 10 s (OFC-10). Tests shorten them before newServer.
var (
	pingEvery  = 3 * time.Second
	pongWithin = 7 * time.Second
)

func main() {
	cfgPath := flag.String("config", "", "path to office.json")
	flag.Parse()
	if *cfgPath == "" {
		fmt.Fprintln(os.Stderr, "usage: wai-escritorio-server -config office.json")
		os.Exit(2)
	}
	cfg, m, err := loadConfig(*cfgPath)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	if err := run(ctx, cfg, m); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

// run listens, logs the bound address on one stderr line, and serves until
// ctx ends.
func run(ctx context.Context, c Config, m office.Map) error {
	hs := &http.Server{}
	if c.tls() {
		cert, err := tls.LoadX509KeyPair(c.TLSCert, c.TLSKey)
		if err != nil {
			return err
		}
		hs.TLSConfig = &tls.Config{Certificates: []tls.Certificate{cert}, MinVersion: tls.VersionTLS12}
	}
	ln, err := net.Listen("tcp", c.Listen)
	if err != nil {
		return err
	}
	log.Printf("listening on %s", ln.Addr())
	srv := newServer(ctx, c.Token, m)
	defer srv.h.stop()
	hs.Handler = srv.handler()
	go func() { <-ctx.Done(); hs.Close() }()
	if c.tls() {
		err = hs.ServeTLS(ln, "", "")
	} else {
		err = hs.Serve(ln)
	}
	if errors.Is(err, http.ErrServerClosed) {
		return nil
	}
	return err
}

type server struct {
	ctx        context.Context
	h          *hub
	pingEvery  time.Duration
	pongWithin time.Duration
}

func newServer(ctx context.Context, token string, m office.Map) *server {
	return &server{ctx: ctx, h: newHub(token, m), pingEvery: pingEvery, pongWithin: pongWithin}
}

func (srv *server) handler() http.Handler { return http.HandlerFunc(srv.handle) }

func (srv *server) handle(w http.ResponseWriter, r *http.Request) {
	c, err := websocket.Accept(w, r, nil)
	if err != nil {
		return
	}
	defer c.CloseNow()
	c.SetReadLimit(readLimit)
	ctx, cancel := context.WithCancel(srv.ctx)
	defer cancel()

	hello, err := readHello(ctx, c)
	if err != nil {
		srv.h.logf("refused addr=%s reason=no hello", r.RemoteAddr)
		c.Close(websocket.StatusPolicyViolation, "hello expected")
		return
	}
	s := newSession(r.RemoteAddr)
	srv.h.join(s, hello)
	written := make(chan struct{})
	go func() { defer close(written); srv.write(c, s) }()
	go srv.ping(ctx, c)

	srv.read(ctx, c, s)
	srv.h.leave(s)
	select {
	case <-written:
	case <-srv.ctx.Done():
	}
}

func readHello(ctx context.Context, c *websocket.Conn) (office.Hello, error) {
	ctx, cancel := context.WithTimeout(ctx, helloTimeout)
	defer cancel()
	_, data, err := c.Read(ctx)
	if err != nil {
		return office.Hello{}, err
	}
	var h office.Hello
	return h, json.Unmarshal(data, &h)
}

// read acts on client messages until the connection ends. Messages past
// rateLimit in a one-second window are dropped.
func (srv *server) read(ctx context.Context, c *websocket.Conn, s *session) {
	var windowStart time.Time
	n := 0
	for {
		_, data, err := c.Read(ctx)
		if err != nil {
			return
		}
		if now := time.Now(); now.Sub(windowStart) >= time.Second {
			windowStart, n = now, 0
		}
		if n++; n > rateLimit {
			continue
		}
		var head struct{ T string }
		if json.Unmarshal(data, &head) != nil {
			continue
		}
		switch head.T {
		case office.TMove:
			var m office.Move
			if json.Unmarshal(data, &m) == nil {
				srv.h.move(s, m)
			}
		case office.TStatus:
			var st office.SetStatus
			if json.Unmarshal(data, &st) == nil {
				srv.h.status(s, st)
			}
		}
	}
}

// write sends what the hub queues, then closes the connection once the hub
// closes the session.
func (srv *server) write(c *websocket.Conn, s *session) {
	failed := false
	for {
		select {
		case m, ok := <-s.out:
			if !ok {
				c.Close(websocket.StatusNormalClosure, "")
				return
			}
			if failed {
				continue
			}
			data, err := json.Marshal(m)
			if err == nil {
				ctx, cancel := context.WithTimeout(srv.ctx, writeTimeout)
				err = c.Write(ctx, websocket.MessageText, data)
				cancel()
			}
			if err != nil {
				failed = true
				c.CloseNow()
			}
		case <-srv.ctx.Done():
			return
		}
	}
}

// ping closes the connection when the peer stops answering.
func (srv *server) ping(ctx context.Context, c *websocket.Conn) {
	t := time.NewTicker(srv.pingEvery)
	defer t.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-t.C:
			pctx, cancel := context.WithTimeout(ctx, srv.pongWithin)
			err := c.Ping(pctx)
			cancel()
			if err != nil {
				c.CloseNow()
				return
			}
		}
	}
}
