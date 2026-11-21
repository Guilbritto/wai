// wai-remoto: a wai extension (line-delimited JSON-RPC 2.0 on stdio) that lets a paired phone
// follow and drive wai's sessions through an HTTP API on 127.0.0.1, reached through a Tailscale
// tunnel.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"sync"
	"time"
)

const panelID = "remoto"

type message struct {
	ID     json.RawMessage `json:"id,omitempty"`
	Method string          `json:"method,omitempty"`
	Params json.RawMessage `json:"params,omitempty"`
}

type node = map[string]any

type server struct {
	wmu sync.Mutex // serialises writes
	out io.Writer
	now func() time.Time

	dir string // the data folder; empty until initialize
}

func newServer(out io.Writer, now func() time.Time) *server {
	return &server{out: out, now: now}
}

func (s *server) write(v any) {
	s.wmu.Lock()
	defer s.wmu.Unlock()
	b, _ := json.Marshal(v)
	fmt.Fprintf(s.out, "%s\n", b)
}

// call sends wai a notification: a host method whose answer we do not need.
func (s *server) call(method string, params any) {
	s.write(node{"jsonrpc": "2.0", "method": method, "params": params})
}

func (s *server) render() {
	s.call("panel/render", node{"panel": panelID, "tree": s.tree()})
}

func (s *server) tree() node {
	return node{"type": "column", "children": []node{{"type": "text", "text": "Not listening", "muted": true}}}
}

func (s *server) handle(m message) {
	switch m.Method {
	case "initialize":
		var p struct {
			DataDir string `json:"data_dir"`
		}
		json.Unmarshal(m.Params, &p)
		s.write(node{"jsonrpc": "2.0", "id": m.ID, "result": node{}})
		s.dir = p.DataDir
		if s.dir == "" {
			s.dir = os.Getenv("WAI_EXT_DATA")
		}
		s.render()
	default:
		if m.ID != nil {
			s.write(node{"jsonrpc": "2.0", "id": m.ID, "error": node{"code": -32601, "message": "Method not found"}})
		}
	}
}

// serve handles wai's messages until stdin closes. Responses to our own calls are ignored.
func serve(s *server, in io.Reader) {
	r := bufio.NewReaderSize(in, 1<<20)
	for {
		line, err := r.ReadBytes('\n')
		var m message
		if len(line) > 0 && json.Unmarshal(line, &m) == nil && m.Method != "" {
			s.handle(m)
		}
		if err != nil {
			return
		}
	}
}

func main() {
	serve(newServer(os.Stdout, time.Now), os.Stdin)
}
