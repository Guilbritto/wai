// Package helpapi holds the JSON types shared by the relay and the extension.
package helpapi

import "time"

type State string

const (
	Waiting   State = "waiting"
	Accepted  State = "accepted"
	Replied   State = "replied"
	Declined  State = "declined"
	Cancelled State = "cancelled"
	Expired   State = "expired"
)

const (
	MaxText             = 8000
	MaxBody             = 64 << 10
	PerHour             = 20
	WaitingPerRecipient = 10
	StreamsPerUser      = 4
	AcceptWithin        = 24 * time.Hour
	Retention           = 7 * 24 * time.Hour
	TokenLife           = 30 * 24 * time.Hour
)

const (
	EvSync    = "sync"
	EvNew     = "request.new"
	EvUpdated = "request.updated"
	EvPeople  = "people"
)

type Project struct {
	Repo   string `json:"repo,omitempty"`
	Branch string `json:"branch,omitempty"`
}

type Request struct {
	ID      string    `json:"id"`
	From    string    `json:"from"`
	To      string    `json:"to"`
	FromID  int64     `json:"from_id"`
	ToID    int64     `json:"to_id"`
	Project Project   `json:"project"`
	Text    string    `json:"text"`
	State   State     `json:"state"`
	Reply   string    `json:"reply,omitempty"`
	Reason  string    `json:"reason,omitempty"`
	Created time.Time `json:"created"`
	Updated time.Time `json:"updated"`
}

type Person struct {
	Login  string `json:"login"`
	Online bool   `json:"online"`
}

type Update struct {
	ID     string `json:"id"`
	State  State  `json:"state"`
	Reply  string `json:"reply"`
	Reason string `json:"reason"`
}
