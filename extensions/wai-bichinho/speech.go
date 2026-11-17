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
	if sp.cur != nil && sp.cur.Text == l.Text {
		return
	}
	for _, q := range sp.queue {
		if q.Text == l.Text {
			return
		}
	}
	if l.Chatter {
		if asleep || now.Sub(sp.chatted) < chatterGap {
			return
		}
		sp.chatted = now
	}
	if l.Prio == 3 && sp.cur != nil && !sp.cur.Sticky && sp.cur.Prio < 3 {
		sp.show(l, now)
		return
	}
	at := 0
	for i, q := range sp.queue {
		if q.Prio >= l.Prio {
			at = i + 1
		}
	}
	sp.queue = append(sp.queue, line{})
	copy(sp.queue[at+1:], sp.queue[at:])
	sp.queue[at] = l
	if len(sp.queue) > maxQueue {
		low := 0
		for i, q := range sp.queue {
			if q.Prio < sp.queue[low].Prio {
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

func (sp *speech) drop(kind string) {
	if sp.cur != nil && sp.cur.Kind == kind {
		sp.cur = nil
	}
	keep := sp.queue[:0]
	for _, q := range sp.queue {
		if q.Kind != kind {
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
