package main

import (
	"encoding/json"
	"fmt"
	"strconv"
	"strings"
	"time"
)

// codeTick: how often the panel redraws the pairing code's time left.
const codeTick = time.Second

// serveCommand is what the user runs to reach the port from the tailnet, over HTTPS on 443
// (tailscale.com/kb/1242/tailscale-serve: --bg keeps it running; a bare port is a local target).
func serveCommand(port int) string { return "tailscale serve --bg " + strconv.Itoa(port) }

// tree is the remoto panel: settings, Start/Stop, the serve command, pairing, devices (PAIR-02,
// PAIR-08, PAIR-11, DEV-01).
func (s *server) tree() node {
	if s.store == nil {
		return node{"type": "column", "children": []node{{"type": "text", "text": "Not listening", "muted": true}}}
	}
	port, tunnel := s.store.settings()
	status, on := s.status()
	s.pmu.Lock()
	msg := s.msg
	s.pmu.Unlock()

	var kids []node
	if msg != "" {
		kids = append(kids, node{"type": "alert", "tone": "danger", "text": msg})
	}
	head := []node{{"type": "text", "text": status, "style": "strong", "muted": !on}}
	if on {
		head = append(head, node{"type": "spacer"}, node{"type": "badge", "text": fmt.Sprintf("%d devices connected", s.hub.connected()), "tone": "info"})
	}
	kids = append(kids,
		node{"type": "row", "children": head},
		node{"type": "input", "id": "port", "placeholder": "Port", "value": strconv.Itoa(port)},
		node{"type": "input", "id": "tunnel", "placeholder": "Tunnel URL (https://…)", "value": tunnel},
		node{"type": "text", "text": "Reach the port from your tailnet with:", "muted": true},
		node{"type": "text", "text": serveCommand(port), "mono": true},
	)
	if on {
		kids = append(kids, node{"type": "button", "id": "stop", "label": "Stop", "icon": "square"})
	} else {
		kids = append(kids, node{"type": "button", "id": "start", "label": "Start", "style": "primary", "icon": "play"})
	}

	if on {
		kids = append(kids, node{"type": "divider"}, node{"type": "button", "id": "pair", "label": "Pair a device", "icon": "qr-code"})
		if code, expires, ok := s.pairing.live(); ok {
			rows, err := qrRows(pairPayload(tunnel, code))
			if err != nil {
				kids = append(kids, node{"type": "alert", "tone": "danger", "text": err.Error()})
			} else {
				left := expires.Sub(s.now()).Round(time.Second)
				kids = append(kids,
					node{"type": "pixels", "rows": rows, "scale": 3},
					node{"type": "text", "text": fmt.Sprintf("Scan it in wai-mobile. Expires in %d:%02d", int(left.Minutes()), int(left.Seconds())%60), "muted": true},
				)
			}
		}
	}

	kids = append(kids, node{"type": "divider"}, node{"type": "text", "text": "Devices", "style": "label"})
	devices := s.store.devices()
	if len(devices) == 0 {
		kids = append(kids, node{"type": "text", "text": "No paired devices", "muted": true})
		return node{"type": "column", "children": kids}
	}
	var rows []node
	for _, d := range devices {
		rows = append(rows, node{"cells": []any{
			d.Name,
			d.Paired.Local().Format("2 Jan 2006 15:04"),
			node{"type": "button", "id": "revoke:" + d.ID, "label": "Revoke", "style": "danger",
				"confirm": node{"title": "Revoke " + d.Name + "?", "text": "Its token stops working at once.", "confirm_label": "Revoke"}},
		}})
	}
	kids = append(kids,
		node{"type": "table", "columns": []node{{"title": "Name"}, {"title": "Paired", "width": 130}, {"title": "", "width": 80}}, "rows": rows},
		node{"type": "button", "id": "signout", "label": "Sign out all devices", "style": "danger",
			"confirm": node{"title": "Sign out all devices?", "text": "Every paired device must pair again.", "confirm_label": "Sign out"}},
	)
	return node{"type": "column", "children": kids}
}

// action handles a click in the panel; the form's fields arrive in values.
func (s *server) action(id string, values map[string]any) {
	text := func(k string) string { v, _ := values[k].(string); return strings.TrimSpace(v) }
	msg := ""
	switch {
	case id == "start":
		port, err := strconv.Atoi(text("port"))
		if err != nil {
			port = 0 // refused by the range check
		}
		if err := s.start(port, text("tunnel")); err != nil {
			msg = err.Error()
		}
	case id == "stop":
		s.stop()
	case id == "pair":
		if _, on := s.status(); !on {
			msg = "Start wai-remoto before pairing a device"
			break
		}
		s.pairing.issue()
		s.pmu.Lock()
		s.ticket++
		ticket := s.ticket
		s.pmu.Unlock()
		go s.countDown(ticket)
	case strings.HasPrefix(id, "revoke:"):
		s.revoke(strings.TrimPrefix(id, "revoke:"))
	case id == "signout":
		s.signOutAll()
	default:
		return
	}
	s.pmu.Lock()
	s.msg = msg
	s.pmu.Unlock()
	s.render()
}

// countDown redraws the time left while pairing code ticket is shown, and once more when it
// expires or is used. A newer code stops it.
func (s *server) countDown(ticket int) {
	for {
		s.pmu.Lock()
		every, current := s.codeTick, s.ticket == ticket
		s.pmu.Unlock()
		select {
		case <-time.After(every):
		case <-s.done:
			return
		}
		if !current {
			return
		}
		s.render()
		if _, _, ok := s.pairing.live(); !ok {
			return
		}
	}
}

// handleAction answers a panel/action request, then acts on it.
func (s *server) handleAction(m message) {
	var p struct {
		Panel    string         `json:"panel"`
		ActionID string         `json:"action_id"`
		Values   map[string]any `json:"values"`
	}
	json.Unmarshal(m.Params, &p)
	if m.ID != nil {
		s.write(node{"jsonrpc": "2.0", "id": m.ID, "result": nil})
	}
	if p.Panel == panelID {
		s.action(p.ActionID, p.Values)
	}
}
