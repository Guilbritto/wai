package main

import (
	"fmt"
	"time"
	"unicode/utf8"
)

type line struct {
	ID         string
	Kind, Text string
	Prio       int
	Sticky     bool
	Chatter    bool
	URL        string // the PR a click opens; "" for none
}

const (
	lineGap    = 250 * time.Millisecond
	maxQueue   = 3
	chatterGap = 15 * time.Minute
	stuckAfter = 30 * time.Second
	charTime   = 30 * time.Millisecond
)

type speech struct {
	cur     *line
	curAt   time.Time
	queue   []line
	nextAt  time.Time
	chatted time.Time
	seq     int
}

func (sp *speech) show(l line, now time.Time) {
	sp.seq++
	l.ID = fmt.Sprintf("say-%d", sp.seq)
	sp.cur, sp.curAt = &l, now
}

func (sp *speech) say(l line, now time.Time, asleep bool) {
	// A PR's line is said once per PR, whatever text it picked.
	same := func(q line) bool {
		return q.URL == l.URL && (q.Text == l.Text || l.URL != "" && q.Kind == l.Kind)
	}
	if sp.cur != nil && same(*sp.cur) {
		return
	}
	for _, q := range sp.queue {
		if same(q) {
			return
		}
	}
	if l.Chatter {
		if asleep || now.Sub(sp.chatted) < chatterGap {
			return
		}
		sp.chatted = now
	}
	if l.Prio == 3 && sp.cur != nil && sp.cur.Prio < 3 {
		// A sticky line steps aside and comes back after.
		if sp.cur.Sticky {
			sp.insert(*sp.cur, true)
		}
		sp.show(l, now)
		return
	}
	sp.insert(l, false)
}

// insert queues l after the higher priorities, and after its own unless first.
func (sp *speech) insert(l line, first bool) {
	at := 0
	for i, q := range sp.queue {
		if q.Prio > l.Prio || q.Prio == l.Prio && !first {
			at = i + 1
		}
	}
	sp.queue = append(sp.queue, line{})
	copy(sp.queue[at+1:], sp.queue[at:])
	sp.queue[at] = l
	if len(sp.queue) > maxQueue {
		low := 0
		for i, q := range sp.queue {
			// A requeued line was said before its peers: on a tie, the newest goes.
			if q.Prio < sp.queue[low].Prio || first && q.Prio == sp.queue[low].Prio {
				low = i
			}
		}
		sp.queue = append(sp.queue[:low], sp.queue[low+1:]...)
	}
}

func (sp *speech) next(now time.Time) {
	if sp.cur != nil || len(sp.queue) == 0 || now.Before(sp.nextAt) {
		return
	}
	l := sp.queue[0]
	sp.queue = sp.queue[1:]
	sp.show(l, now)
}

func (sp *speech) done(id string, now time.Time) {
	if sp.cur != nil && sp.cur.ID == id {
		sp.cur = nil
		sp.nextAt = now.Add(lineGap)
	}
}

func (sp *speech) unfocus() {
	if sp.cur != nil && !sp.cur.Sticky {
		sp.cur = nil
	}
}

func (sp *speech) drop(match func(line) bool) {
	if sp.cur != nil && match(*sp.cur) {
		sp.cur = nil
	}
	keep := sp.queue[:0]
	for _, q := range sp.queue {
		if !match(q) {
			keep = append(keep, q)
		}
	}
	sp.queue = keep
}

func (sp *speech) expire(now time.Time) {
	if sp.cur != nil && !sp.cur.Sticky && now.Sub(sp.curAt) > stuckAfter {
		sp.cur = nil
	}
}

func (sp *speech) talking(now time.Time) bool {
	if sp.cur == nil {
		return false
	}
	return now.Before(sp.curAt.Add(charTime*time.Duration(utf8.RuneCountInString(sp.cur.Text)) + 200*time.Millisecond))
}
