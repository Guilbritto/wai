package main

import (
	"encoding/json"
	"io"
	"net/http"
	"strings"
	"sync"
	"time"
)

const (
	maxStreams   = 4                // EDGE-05: open event streams per device
	pingInterval = 15 * time.Second // a comment line that keeps proxies from closing a quiet stream
	streamBuffer = 256              // frames a slow stream may hold before it is closed
)

// streamed are the wai events /v1/events forwards; the SSE event name drops the "event/".
var streamed = map[string]bool{
	"event/session_state":        true,
	"event/turn_ended":           true,
	"event/session_update":       true,
	"event/permission_requested": true,
	"event/permission_resolved":  true,
	"event/question_requested":   true,
	"event/question_resolved":    true,
}

// stream is one open GET /v1/events.
type stream struct {
	device string
	out    chan []byte
	gone   chan struct{} // closed when the stream must end
}

// hub fans wai's events out to the open streams.
type hub struct {
	changed func() // the open streams changed; nil for none

	mu      sync.Mutex
	ping    time.Duration
	streams map[*stream]bool
}

func newHub() *hub {
	return &hub{ping: pingInterval, streams: map[*stream]bool{}}
}

func (h *hub) notify() {
	if h.changed != nil {
		h.changed()
	}
}

// open adds a stream for device; nil when it already has maxStreams.
func (h *hub) open(device string) (*stream, time.Duration) {
	h.mu.Lock()
	n := 0
	for st := range h.streams {
		if st.device == device {
			n++
		}
	}
	if n >= maxStreams {
		h.mu.Unlock()
		return nil, 0
	}
	st := &stream{device: device, out: make(chan []byte, streamBuffer), gone: make(chan struct{})}
	h.streams[st] = true
	ping := h.ping
	h.mu.Unlock()
	h.notify()
	return st, ping
}

// endLocked ends a stream; under h.mu. It reports whether the stream was open.
func (h *hub) endLocked(st *stream) bool {
	if !h.streams[st] {
		return false
	}
	delete(h.streams, st)
	close(st.gone)
	return true
}

// end ends the streams for which drop is true.
func (h *hub) end(drop func(*stream) bool) {
	h.mu.Lock()
	ended := false
	for st := range h.streams {
		if drop(st) && h.endLocked(st) {
			ended = true
		}
	}
	h.mu.Unlock()
	if ended {
		h.notify()
	}
}

// kick ends every stream of device (DEV-02).
func (h *hub) kick(device string) { h.end(func(st *stream) bool { return st.device == device }) }

// closeAll ends every stream (DEV-03, Stop).
func (h *hub) closeAll() { h.end(func(*stream) bool { return true }) }

// connected counts the devices with an open stream (DEV-01).
func (h *hub) connected() int {
	h.mu.Lock()
	defer h.mu.Unlock()
	devices := map[string]bool{}
	for st := range h.streams {
		devices[st.device] = true
	}
	return len(devices)
}

// publish sends one wai event to every stream. A stream too slow to take it is ended: the app
// reconnects and reads the session again.
func (h *hub) publish(name string, data json.RawMessage) {
	frame := []byte("event: " + name + "\ndata: " + string(data) + "\n\n")
	h.mu.Lock()
	ended := false
	for st := range h.streams {
		select {
		case st.out <- frame:
		default:
			ended = h.endLocked(st) || ended
		}
	}
	h.mu.Unlock()
	if ended {
		h.notify()
	}
}

// compact puts an event's params on one line, as an SSE data field must be.
func compact(params json.RawMessage) json.RawMessage {
	var b strings.Builder
	if len(params) == 0 {
		return json.RawMessage("null")
	}
	var v any
	if json.Unmarshal(params, &v) != nil {
		return json.RawMessage("null")
	}
	enc := json.NewEncoder(&b)
	enc.SetEscapeHTML(false)
	enc.Encode(v)
	return json.RawMessage(strings.TrimSuffix(b.String(), "\n"))
}

// GET /v1/events: wai's session events as Server-Sent Events, one `event:` and one `data:` line
// each, and a `: ping` comment every ping interval (REMO-02, REMO-04). 429 past maxStreams.
func (s *server) events(w http.ResponseWriter, r *http.Request) {
	st, ping := s.hub.open(deviceOf(r).ID)
	if st == nil {
		refuse(w, http.StatusTooManyRequests, "At most 4 event streams per device")
		return
	}
	defer s.hub.end(func(x *stream) bool { return x == st })
	flusher, _ := w.(http.Flusher)
	flush := func() {
		if flusher != nil {
			flusher.Flush()
		}
	}
	w.Header().Set("Content-Type", "text/event-stream")
	w.WriteHeader(http.StatusOK)
	flush()
	tick := time.NewTicker(ping)
	defer tick.Stop()
	for {
		var err error
		select {
		case frame := <-st.out:
			_, err = w.Write(frame)
		case <-tick.C:
			_, err = io.WriteString(w, ": ping\n\n")
		case <-st.gone:
			return
		case <-r.Context().Done():
			return
		}
		if err != nil {
			return
		}
		flush()
	}
}
