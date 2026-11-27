package main

import (
	"context"
	"encoding/json"
	"time"

	"github.com/coder/websocket"

	"wai-escritorio/office"
)

// retryEvery is how long the client waits before it dials again (OFC-04, OFC-60).
const retryEvery = 5 * time.Second

// tUnreachable is the synthetic message for a failed dial or a lost connection.
const tUnreachable = "unreachable"

// serverMsg is any frame from the server, decoded into one flat struct, or tUnreachable.
type serverMsg struct {
	T      string          `json:"t"`
	You    string          `json:"you"`
	Map    []string        `json:"map"`
	People []office.Person `json:"people"`
	Person office.Person   `json:"person"`
	ID     string          `json:"id"`
	X      int             `json:"x"`
	Y      int             `json:"y"`
	Status office.Status   `json:"status"`
	Reason string          `json:"reason"`
	Err    string          `json:"-"`
}

// run connects to the server, sends hello, hands every frame to `to` and writes what arrives on
// `from`. It dials again every retryEvery after a failure or a lost connection, and returns when ctx
// ends or the server refuses the token or says this login was replaced.
func run(ctx context.Context, s Setup, to chan<- serverMsg, from <-chan any) {
	for {
		stop := session(ctx, s, to, from)
		if stop || ctx.Err() != nil {
			return
		}
		select {
		case <-ctx.Done():
			return
		case <-time.After(retryEvery):
		}
	}
}

func deliver(ctx context.Context, to chan<- serverMsg, m serverMsg) bool {
	select {
	case to <- m:
		return true
	case <-ctx.Done():
		return false
	}
}

// session is one connection; it reports whether to stop for good.
func session(parent context.Context, s Setup, to chan<- serverMsg, from <-chan any) bool {
	ctx, cancel := context.WithCancel(parent)
	defer cancel()
	dctx, dcancel := context.WithTimeout(ctx, retryEvery)
	c, _, err := websocket.Dial(dctx, s.Server, nil)
	dcancel()
	if err != nil {
		deliver(ctx, to, serverMsg{T: tUnreachable, Err: err.Error()})
		return false
	}
	defer c.CloseNow()
	c.SetReadLimit(1 << 20)
	hello, _ := json.Marshal(office.Hello{T: office.THello, Token: s.Token, Name: s.Name, Avatar: s.Avatar})
	if err := c.Write(ctx, websocket.MessageText, hello); err != nil {
		deliver(ctx, to, serverMsg{T: tUnreachable, Err: err.Error()})
		return false
	}
	// Anything queued while offline is stale: the server owns the position.
	for drained := false; !drained; {
		select {
		case <-from:
		default:
			drained = true
		}
	}

	type read struct {
		m    serverMsg
		stop bool
		err  error
	}
	reads := make(chan read)
	go func() {
		for {
			_, b, err := c.Read(ctx)
			if err != nil {
				select {
				case reads <- read{err: err}:
				case <-ctx.Done():
				}
				return
			}
			var m serverMsg
			if json.Unmarshal(b, &m) != nil {
				continue
			}
			select {
			case reads <- read{m: m}:
			case <-ctx.Done():
				return
			}
		}
	}()

	for {
		select {
		case <-ctx.Done():
			return true
		case msg := <-from:
			b, err := json.Marshal(msg)
			if err == nil {
				err = c.Write(ctx, websocket.MessageText, b)
			}
			if err != nil {
				deliver(ctx, to, serverMsg{T: tUnreachable, Err: err.Error()})
				return false
			}
		case r := <-reads:
			if r.err != nil {
				deliver(ctx, to, serverMsg{T: tUnreachable, Err: r.err.Error()})
				return false
			}
			if !deliver(ctx, to, r.m) {
				return true
			}
			if r.m.T == office.TRefused {
				return r.m.Reason == office.ReasonToken || r.m.Reason == office.ReasonReplaced
			}
		}
	}
}
