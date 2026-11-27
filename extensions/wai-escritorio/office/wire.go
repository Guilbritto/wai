package office

// Message types, the value of the "t" field of every frame.
const (
	THello    = "hello"
	TMove     = "move"
	TStatus   = "status"
	TWelcome  = "welcome"
	TJoined   = "joined"
	TMoved    = "moved"
	TLeft     = "left"
	TPosition = "position"
	TRefused  = "refused"
)

// Refusal reasons.
const (
	ReasonToken    = "token"
	ReasonFull     = "full"
	ReasonReplaced = "replaced"
)

// client to server

type Hello struct {
	T      string `json:"t"`
	Token  string `json:"token"`
	Name   string `json:"name"`
	Avatar int    `json:"avatar"`
}

type Move struct {
	T string `json:"t"`
	X int    `json:"x"`
	Y int    `json:"y"`
}

type SetStatus struct {
	T      string `json:"t"`
	Status Status `json:"status"`
}

// server to client

type Person struct {
	ID     string `json:"id"`
	Name   string `json:"name"`
	Avatar int    `json:"avatar"`
	X      int    `json:"x"`
	Y      int    `json:"y"`
	Status Status `json:"status"`
}

type Welcome struct {
	T      string   `json:"t"`
	You    string   `json:"you"`
	Map    []string `json:"map"`
	People []Person `json:"people"`
}

type Joined struct {
	T      string `json:"t"`
	Person Person `json:"person"`
}

type Moved struct {
	T  string `json:"t"`
	ID string `json:"id"`
	X  int    `json:"x"`
	Y  int    `json:"y"`
}

type Left struct {
	T  string `json:"t"`
	ID string `json:"id"`
}

type StatusChanged struct {
	T      string `json:"t"`
	ID     string `json:"id"`
	Status Status `json:"status"`
}

// Position corrects a client whose move the server refused.
type Position struct {
	T string `json:"t"`
	X int    `json:"x"`
	Y int    `json:"y"`
}

type Refused struct {
	T      string `json:"t"`
	Reason string `json:"reason"`
}
