package main

import (
	"encoding/json"
	"net"
	"net/http"
	"reflect"
	"strconv"
	"strings"
	"testing"
	"time"
)

// click sends wai's panel/action for id with the form's values and checks its answer; the
// redrawn tree follows (see waitTree).
func (h *host) click(id string, values map[string]any) {
	h.t.Helper()
	b, _ := json.Marshal(node{"jsonrpc": "2.0", "id": "click", "method": "panel/action",
		"params": node{"panel": "remoto", "action_id": id, "values": values}})
	h.send(string(b))
	for {
		m := h.next()
		if m["id"] == "click" {
			if _, ok := m["result"]; !ok || m["error"] != nil {
				h.t.Errorf("panel/action %s answered %v", id, m)
			}
			return
		}
	}
}

// waitTree reads the panel's renders until one satisfies ok, and returns it.
func (h *host) waitTree(what string, ok func(map[string]any) bool) map[string]any {
	h.t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	var last map[string]any
	for time.Now().Before(deadline) {
		m := h.next()
		if m["method"] != "panel/render" {
			continue
		}
		p, _ := m["params"].(map[string]any)
		if p["panel"] != "remoto" {
			h.t.Fatalf("render of panel %v", p["panel"])
		}
		last, _ = p["tree"].(map[string]any)
		if ok(last) {
			return last
		}
	}
	h.t.Fatalf("%s: no such render; the last one: %v", what, last)
	return nil
}

// nodes are every node of a tree, table cells included; a string cell is a text node.
func nodes(n any) []map[string]any {
	var out []map[string]any
	switch v := n.(type) {
	case string:
		out = append(out, map[string]any{"type": "text", "text": v})
	case map[string]any:
		out = append(out, v)
		for _, k := range []string{"children", "cells", "rows"} {
			if kids, ok := v[k].([]any); ok {
				for _, kid := range kids {
					out = append(out, nodes(kid)...)
				}
			}
		}
	}
	return out
}

// find is the node of type typ with id ("" for the first of that type), or nil.
func find(tree map[string]any, typ, id string) map[string]any {
	for _, n := range nodes(tree) {
		if n["type"] == typ && (id == "" || n["id"] == id) {
			return n
		}
	}
	return nil
}

// shows: some text, badge or alert of the tree reads exactly s.
func shows(tree map[string]any, s string) bool {
	for _, n := range nodes(tree) {
		if (n["type"] == "text" || n["type"] == "badge" || n["type"] == "alert") && n["text"] == s {
			return true
		}
	}
	return false
}

// drawn is the tree s would render now, as wai reads it.
func drawn(s *server) map[string]any {
	b, _ := json.Marshal(s.tree())
	var tree map[string]any
	json.Unmarshal(b, &tree)
	return tree
}

func form(port, tunnel string) map[string]any { return map[string]any{"port": port, "tunnel": tunnel} }

// PAIR-02, PAIR-03, PAIR-04, PAIR-05, PAIR-06: the panel's port and tunnel URL fields, Start, the
// tailscale serve command for the port, and what Start shows.
func TestPanelShowsSettingsAndServeCommand(t *testing.T) {
	a := startAPI(t)
	a.stopAtEnd()
	tree := drawn(a.h.s)
	if in := find(tree, "input", "port"); in == nil || in["value"] != "7717" || in["placeholder"] != "Port" {
		t.Errorf("port field: %v", in)
	}
	if in := find(tree, "input", "tunnel"); in == nil || in["value"] != "" {
		t.Errorf("tunnel URL field: %v", in)
	}
	if b := find(tree, "button", "start"); b == nil || b["label"] != "Start" {
		t.Errorf("Start button: %v", b)
	}
	if !shows(tree, "tailscale serve --bg 7717") || !shows(tree, "Not listening") || find(tree, "button", "stop") != nil {
		t.Errorf("before Start: %v", tree)
	}

	refusedWith := func(what string, values map[string]any, msg string) {
		t.Helper()
		a.h.click("start", values)
		tree := a.h.waitTree(what, func(tr map[string]any) bool { return find(tr, "alert", "") != nil })
		if al := find(tree, "alert", ""); al["text"] != msg || al["tone"] != "danger" {
			t.Errorf("%s: alert %v, want %q", what, al, msg)
		}
		if find(tree, "button", "start") == nil || find(tree, "button", "stop") != nil || !shows(tree, "Not listening") {
			t.Errorf("%s: Start is gone or it listens: %v", what, tree)
		}
	}
	refusedWith("port abc", form("abc", tunnelURL), "Port must be between 1024 and 65535")
	refusedWith("port 80", form("80", tunnelURL), "Port must be between 1024 and 65535")
	refusedWith("an http tunnel", form("7717", "http://my-mac.ts.net"), "The tunnel URL must start with https://")
	taken, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer taken.Close()
	busy := taken.Addr().(*net.TCPAddr).Port
	refusedWith("a taken port", form(strconv.Itoa(busy), tunnelURL), "Port "+strconv.Itoa(busy)+" is in use")

	port := strconv.Itoa(freePort(t))
	a.h.click("start", form(port, tunnelURL))
	tree = a.h.waitTree("started", func(tr map[string]any) bool { return shows(tr, "Listening on 127.0.0.1:"+port) })
	if find(tree, "alert", "") != nil || find(tree, "button", "start") != nil || find(tree, "button", "stop") == nil {
		t.Errorf("after Start: %v", tree)
	}
	if !shows(tree, "tailscale serve --bg "+port) || find(tree, "input", "port")["value"] != port || find(tree, "input", "tunnel")["value"] != tunnelURL {
		t.Errorf("after Start the command and fields follow the new settings: %v", tree)
	}

	a.h.click("stop", form(port, tunnelURL))
	tree = a.h.waitTree("stopped", func(tr map[string]any) bool { return shows(tr, "Not listening") })
	if find(tree, "button", "start") == nil || find(tree, "button", "stop") != nil {
		t.Errorf("after Stop: %v", tree)
	}
	refused(t, "after Stop", "127.0.0.1:"+port)
}

// PAIR-08: "Pair a device" shows a QR code of the tunnel URL and a new one-time code, and the
// time left, counted down until it expires.
func TestPairShowsQRWithURLAndCode(t *testing.T) {
	a := startAPI(t)
	a.stopAtEnd()
	a.h.s.pmu.Lock()
	a.h.s.codeTick = 20 * time.Millisecond
	a.h.s.pmu.Unlock()
	port := strconv.Itoa(freePort(t))
	a.h.click("start", form(port, tunnelURL))
	tree := a.h.waitTree("started", func(tr map[string]any) bool { return find(tr, "button", "pair") != nil })
	if b := find(tree, "button", "pair"); b["label"] != "Pair a device" || find(tree, "pixels", "") != nil {
		t.Errorf("before pairing: %v", tree)
	}

	a.h.click("pair", form(port, tunnelURL))
	tree = a.h.waitTree("the QR code", func(tr map[string]any) bool { return find(tr, "pixels", "") != nil })
	code, _, ok := a.h.s.pairing.live()
	if !ok {
		t.Fatal("no live code")
	}
	want, _ := qrRows(pairPayload(tunnelURL, code))
	if !strings.Contains(pairPayload(tunnelURL, code), "url=https%3A%2F%2Fmy-mac.tail0123ab.ts.net") || !strings.HasSuffix(pairPayload(tunnelURL, code), "&code="+code) {
		t.Errorf("payload %q", pairPayload(tunnelURL, code))
	}
	var rows []string
	for _, r := range find(tree, "pixels", "")["rows"].([]any) {
		rows = append(rows, r.(string))
	}
	if !reflect.DeepEqual(rows, want) {
		t.Errorf("the QR rows are not the tunnel URL and the code's")
	}
	if !shows(tree, "Scan it in wai-mobile. Expires in 5:00") {
		t.Errorf("time left: %v", tree)
	}

	a.h.advance(2*time.Minute + 30*time.Second)
	a.h.waitTree("2:30 left", func(tr map[string]any) bool { return shows(tr, "Scan it in wai-mobile. Expires in 2:30") })

	// A new code replaces the shown one.
	a.h.click("pair", form(port, tunnelURL))
	a.h.waitTree("a new code", func(tr map[string]any) bool { return shows(tr, "Scan it in wai-mobile. Expires in 5:00") })
	if again, _, _ := a.h.s.pairing.live(); again == code {
		t.Error("Pair a device kept the old code")
	}

	a.h.advance(5 * time.Minute)
	tree = a.h.waitTree("expired", func(tr map[string]any) bool { return find(tr, "pixels", "") == nil })
	if find(tree, "button", "pair") == nil {
		t.Errorf("after the code expired: %v", tree)
	}
}

// PAIR-11: a device that pairs shows in the panel's list with its name and pairing date.
func TestPanelListsPairedDevice(t *testing.T) {
	h := startHost(t)
	h.initialize(dataDir(t))
	h.next()
	tree := h.waitTree("the first render", func(map[string]any) bool { return true })
	if !shows(tree, "No paired devices") || find(tree, "table", "") != nil {
		t.Errorf("no device yet: %v", tree)
	}
	code, _ := h.s.pairing.issue()
	r := httptestPair(h, code, "Pixel 9")
	if r != http.StatusOK {
		t.Fatalf("pair: %d", r)
	}
	tree = h.waitTree("the paired device", func(tr map[string]any) bool { return find(tr, "table", "") != nil })
	d := h.s.store.devices()[0]
	table := find(tree, "table", "")
	rows := table["rows"].([]any)
	if len(rows) != 1 {
		t.Fatalf("rows %v", rows)
	}
	cells := rows[0].(map[string]any)["cells"].([]any)
	if cells[0] != "Pixel 9" || cells[1] != d.Paired.Local().Format("2 Jan 2006 15:04") || cells[1] != t0.Local().Format("2 Jan 2006 15:04") {
		t.Errorf("device row %v", cells)
	}
	if shows(tree, "No paired devices") {
		t.Error("still says no paired devices")
	}
}

// httptestPair redeems code through POST /v1/pair and returns the status.
func httptestPair(h *host, code, device string) int {
	r, _ := http.NewRequest("POST", "/v1/pair", strings.NewReader(`{"code":`+quote(code)+`,"device":`+quote(device)+`}`))
	r.Host = "127.0.0.1:7717"
	w := serveTo(h.s.handler(), r)
	return w.Code
}

// DEV-01: while started, the panel shows "<n> devices connected", counting the devices with an
// open event stream.
func TestPanelCountsConnectedDevices(t *testing.T) {
	a := startAPI(t)
	a.stopAtEnd()
	port := freePort(t)
	addr := "127.0.0.1:" + strconv.Itoa(port)
	a.h.click("start", form(strconv.Itoa(port), tunnelURL))
	a.h.waitTree("0 connected", func(tr map[string]any) bool { return shows(tr, "0 devices connected") })

	other, _, _ := a.h.s.store.add("iPhone")
	base := "http://" + addr
	mine1 := openSSEAt(t, base, addr, a.token)
	a.h.waitTree("1 connected", func(tr map[string]any) bool { return shows(tr, "1 devices connected") })
	openSSEAt(t, base, addr, a.token) // a second stream of the same device counts once
	theirs := openSSEAt(t, base, addr, other)
	a.h.waitTree("2 connected", func(tr map[string]any) bool { return shows(tr, "2 devices connected") })
	if n := a.h.s.hub.connected(); n != 2 {
		t.Errorf("connected %d with three streams of two devices", n)
	}

	theirs.resp.Body.Close()
	a.h.waitTree("1 connected after a stream closed", func(tr map[string]any) bool { return shows(tr, "1 devices connected") })
	mine1.resp.Body.Close()
	a.h.waitTree("still 1: the device has another stream", func(tr map[string]any) bool { return a.h.s.hub.connected() == 1 && shows(tr, "1 devices connected") })

	a.h.click("stop", form(strconv.Itoa(port), tunnelURL))
	tree := a.h.waitTree("stopped", func(tr map[string]any) bool { return shows(tr, "Not listening") })
	for _, n := range nodes(tree) {
		if s, _ := n["text"].(string); strings.HasSuffix(s, "devices connected") {
			t.Errorf("not started, the panel shows %q", s)
		}
	}
}

// DEV-02, DEV-03: Revoke removes the device and closes its streams within 1 second; "Sign out
// all devices" removes every device and closes every stream within 1 second.
func TestPanelRevokeAndSignOutAll(t *testing.T) {
	a := startAPI(t)
	srv := a.listen()
	other, od, _ := a.h.s.store.add("iPhone")
	mine, theirs := openSSE(t, srv, a.token), openSSE(t, srv, other)
	tree := drawn(a.h.s)
	b := find(tree, "button", "revoke:"+a.device.ID)
	if b == nil || b["label"] != "Revoke" || b["confirm"] == nil || find(tree, "button", "revoke:"+od.ID) == nil {
		t.Fatalf("Revoke buttons: %v", tree)
	}
	if so := find(tree, "button", "signout"); so == nil || so["label"] != "Sign out all devices" || so["style"] != "danger" || so["confirm"] == nil {
		t.Errorf("Sign out all devices: %v", so)
	}

	began := time.Now()
	a.h.click("revoke:"+a.device.ID, form("7717", ""))
	mine.ends("the revoked device's stream", time.Second)
	if took := time.Since(began); took > time.Second {
		t.Errorf("revoke took %v", took)
	}
	tree = a.h.waitTree("Pixel gone", func(tr map[string]any) bool { return find(tr, "button", "revoke:"+a.device.ID) == nil })
	if find(tree, "button", "revoke:"+od.ID) == nil || shows(tree, "Pixel") || !shows(tree, "iPhone") {
		t.Errorf("after Revoke: %v", tree)
	}
	if _, ok := a.h.s.store.lookup(a.token); ok {
		t.Error("the revoked token still works")
	}
	if theirs.resp.StatusCode != http.StatusOK {
		t.Fatal("the other stream")
	}

	began = time.Now()
	a.h.click("signout", form("7717", ""))
	theirs.ends("a stream after sign out all", time.Second)
	if took := time.Since(began); took > time.Second {
		t.Errorf("sign out all took %v", took)
	}
	tree = a.h.waitTree("no devices", func(tr map[string]any) bool { return shows(tr, "No paired devices") })
	if find(tree, "table", "") != nil || find(tree, "button", "signout") != nil {
		t.Errorf("after sign out all: %v", tree)
	}
	if _, ok := a.h.s.store.lookup(other); ok {
		t.Error("a token works after sign out all")
	}
}
