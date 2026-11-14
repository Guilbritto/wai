package main

import (
	"encoding/json"
	"fmt"
	"sync"

	"wai-waycarbon/helpapi"
)

const streamBuffer = 16

// Hub fans events out to each user's open streams. Send never blocks: a
// stream whose buffer is full is dropped and the client reconnects (and resyncs).
type Hub struct {
	mu       sync.Mutex
	streams  map[int64]map[chan []byte]struct{}
	onToggle func(userID int64) // called, lock-free, when a user goes online or offline
}

func NewHub() *Hub { return &Hub{streams: map[int64]map[chan []byte]struct{}{}} }

func frame(event string, data any) []byte {
	b, _ := json.Marshal(data)
	return []byte(fmt.Sprintf("event: %s\ndata: %s\n\n", event, b))
}

func (h *Hub) Join(userID int64) (<-chan []byte, func(), bool) {
	h.mu.Lock()
	set := h.streams[userID]
	if len(set) >= helpapi.StreamsPerUser {
		h.mu.Unlock()
		return nil, nil, false
	}
	if set == nil {
		set = map[chan []byte]struct{}{}
		h.streams[userID] = set
	}
	ch := make(chan []byte, streamBuffer)
	set[ch] = struct{}{}
	first := len(set) == 1
	h.mu.Unlock()
	if first && h.onToggle != nil {
		h.onToggle(userID)
	}
	return ch, func() { h.drop(userID, ch) }, true
}

// drop closes and forgets one stream; safe to call twice.
func (h *Hub) drop(userID int64, ch chan []byte) {
	h.mu.Lock()
	set := h.streams[userID]
	_, ok := set[ch]
	if ok {
		close(ch)
		delete(set, ch)
		if len(set) == 0 {
			delete(h.streams, userID)
		}
	}
	gone := ok && len(set) == 0
	h.mu.Unlock()
	if gone && h.onToggle != nil {
		h.onToggle(userID)
	}
}

func (h *Hub) Send(userID int64, event string, data any) {
	msg := frame(event, data)
	var full []chan []byte
	h.mu.Lock()
	for ch := range h.streams[userID] {
		select {
		case ch <- msg:
		default:
			full = append(full, ch)
		}
	}
	h.mu.Unlock()
	for _, ch := range full {
		h.drop(userID, ch)
	}
}

func (h *Hub) Online(userID int64) bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	return len(h.streams[userID]) > 0
}

// Connected lists the users with at least one open stream.
func (h *Hub) Connected() []int64 {
	h.mu.Lock()
	defer h.mu.Unlock()
	out := make([]int64, 0, len(h.streams))
	for id := range h.streams {
		out = append(out, id)
	}
	return out
}

// Kick closes every stream of a user.
func (h *Hub) Kick(userID int64) {
	h.mu.Lock()
	var chs []chan []byte
	for ch := range h.streams[userID] {
		chs = append(chs, ch)
	}
	h.mu.Unlock()
	for _, ch := range chs {
		h.drop(userID, ch)
	}
}
