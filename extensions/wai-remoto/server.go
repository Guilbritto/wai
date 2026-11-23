package main

import (
	"errors"
	"fmt"
	"net"
	"net/http"
	"strconv"
	"strings"
	"syscall"
	"time"
)

const (
	minPort = 1024
	maxPort = 65535
)

// checkSettings validates what Start uses, before anything binds (PAIR-03, PAIR-04).
func checkSettings(port int, tunnel string) error {
	if port < minPort || port > maxPort {
		return errors.New("Port must be between 1024 and 65535")
	}
	if !strings.HasPrefix(tunnel, "https://") {
		return errors.New("The tunnel URL must start with https://")
	}
	return nil
}

// inUse: the port is taken. Windows answers WSAEADDRINUSE (10048), which is not EADDRINUSE.
func inUse(err error) bool {
	var errno syscall.Errno
	return errors.Is(err, syscall.EADDRINUSE) || (errors.As(err, &errno) && errno == 10048)
}

// start saves the settings and listens on 127.0.0.1:<port>, never another address (PAIR-05).
// Nothing listens before it (PAIR-07).
func (s *server) start(port int, tunnel string) error {
	if err := checkSettings(port, tunnel); err != nil {
		return err
	}
	s.lmu.Lock()
	defer s.lmu.Unlock()
	if s.srv != nil {
		return errors.New("Already listening on " + s.addr)
	}
	s.store.setSettings(port, tunnel)
	addr := "127.0.0.1:" + strconv.Itoa(port)
	ln, err := net.Listen("tcp", addr)
	if inUse(err) {
		return fmt.Errorf("Port %d is in use", port) // PAIR-06
	}
	if err != nil {
		return fmt.Errorf("Cannot listen on %s: %v", addr, err)
	}
	srv := &http.Server{Handler: s.handler(), ReadHeaderTimeout: 10 * time.Second}
	s.srv, s.addr = srv, addr
	go srv.Serve(ln)
	return nil
}

// stop closes the listener and every connection at once, event streams included (PAIR-12).
func (s *server) stop() {
	s.lmu.Lock()
	srv := s.srv
	s.srv, s.addr = nil, ""
	s.lmu.Unlock()
	if srv == nil {
		return
	}
	s.hub.closeAll()
	srv.Close()
}

// status is the panel's line: "Listening on 127.0.0.1:<port>" or "Not listening".
func (s *server) status() (string, bool) {
	s.lmu.Lock()
	defer s.lmu.Unlock()
	if s.srv == nil {
		return "Not listening", false
	}
	return "Listening on " + s.addr, true
}
