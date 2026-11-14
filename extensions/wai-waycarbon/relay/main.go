package main

import (
	"context"
	"log"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"
)

type statusWriter struct {
	http.ResponseWriter
	code int
}

func (w *statusWriter) WriteHeader(c int) {
	w.code = c
	w.ResponseWriter.WriteHeader(c)
}

func (w *statusWriter) Flush() { w.ResponseWriter.(http.Flusher).Flush() }

// logged logs method, path and status only: never bodies, tokens or codes.
func logged(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		sw := &statusWriter{ResponseWriter: w, code: http.StatusOK}
		next.ServeHTTP(sw, r)
		log.Printf("%s %s %d", r.Method, r.URL.Path, sw.code)
	})
}

func main() {
	cfg, err := ConfigFromEnv()
	if err != nil {
		log.Fatal(err)
	}
	st, err := OpenStore(cfg.Data, time.Now)
	if err != nil {
		log.Fatal(err)
	}
	s := NewServer(st, NewGitHub(cfg, &http.Client{Timeout: 15 * time.Second}), cfg)
	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGTERM, os.Interrupt)
	defer stop()
	go s.Loop(ctx)
	srv := &http.Server{Addr: cfg.Addr, Handler: logged(s.Routes()), ReadHeaderTimeout: 10 * time.Second}
	go func() {
		<-ctx.Done()
		sc, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		srv.Shutdown(sc)
	}()
	log.Printf("relay listening on %s", cfg.Addr)
	if err := srv.ListenAndServe(); err != http.ErrServerClosed {
		log.Fatal(err)
	}
}
