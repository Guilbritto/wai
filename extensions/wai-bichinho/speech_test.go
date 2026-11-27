package main

import (
	"testing"
	"time"
)

var t0 = time.Date(2026, 11, 17, 12, 0, 0, 0, time.UTC)

func n(kind string, prio int) line { return line{Kind: kind, Text: kind, Prio: prio} }
func chatter(kind string) line     { return line{Kind: kind, Text: kind, Prio: 1, Chatter: true} }

func TestOneLineAtATimeWithGap(t *testing.T) {
	var sp speech
	sp.say(n("a", 2), t0, false)
	sp.say(n("b", 2), t0, false)
	sp.next(t0)
	if sp.cur == nil || sp.cur.Text != "a" {
		t.Fatalf("cur = %v", sp.cur)
	}
	sp.done(sp.cur.ID, t0)
	sp.next(t0.Add(100 * time.Millisecond))
	if sp.cur != nil {
		t.Fatal("promoted inside the gap")
	}
	sp.next(t0.Add(250 * time.Millisecond))
	if sp.cur == nil || sp.cur.Text != "b" {
		t.Fatalf("cur = %v", sp.cur)
	}
}

func TestUrgentJumpsAndReplaces(t *testing.T) {
	var sp speech
	sp.say(n("a", 1), t0, false)
	sp.next(t0)
	sp.say(n("needs_you", 3), t0, false)
	if sp.cur.Kind != "needs_you" || len(sp.queue) != 0 {
		t.Fatalf("cur = %v queue = %v", sp.cur, sp.queue)
	}

	var st speech
	s := n("a", 1)
	s.Sticky = true
	st.say(s, t0, false)
	st.next(t0)
	st.say(n("b", 2), t0, false)
	st.say(n("needs_you", 3), t0, false)
	if st.cur.Kind != "needs_you" || len(st.queue) != 2 || st.queue[0].Kind != "b" || st.queue[1].Kind != "a" {
		t.Fatalf("a sticky line steps aside and comes back after its betters: cur = %v queue = %v", st.cur, st.queue)
	}
}

func TestSameLineTwoPRs(t *testing.T) {
	var sp speech
	pr := func(url string) line { return line{Kind: "pr_fix", Text: "x", Prio: 2, Sticky: true, URL: url} }
	sp.say(pr("https://github.com/o/r/pull/1"), t0, false)
	sp.next(t0)
	sp.say(pr("https://github.com/o/r/pull/2"), t0, false)
	sp.say(pr("https://github.com/o/r/pull/1"), t0, false)
	if sp.cur.URL != "https://github.com/o/r/pull/1" || len(sp.queue) != 1 || sp.queue[0].URL != "https://github.com/o/r/pull/2" {
		t.Fatalf("cur = %v queue = %v", sp.cur, sp.queue)
	}
}

func TestDedupe(t *testing.T) {
	var sp speech
	l := line{Kind: "x", Text: "Nham!", Prio: 2}
	sp.say(l, t0, false)
	sp.say(l, t0, false)
	if len(sp.queue) != 1 {
		t.Fatalf("queue = %v", sp.queue)
	}
	sp.next(t0)
	sp.say(l, t0, false)
	if sp.cur == nil || len(sp.queue) != 0 {
		t.Fatalf("cur = %v queue = %v", sp.cur, sp.queue)
	}
}

func TestQueueCapDropsLowest(t *testing.T) {
	var sp speech
	sp.say(n("busy", 2), t0, false)
	sp.next(t0)
	for _, k := range []string{"a", "b", "c"} {
		sp.say(n(k, 2), t0, false)
	}
	sp.say(n("low", 1), t0, false)
	if len(sp.queue) != 3 || sp.queue[0].Kind != "a" || sp.queue[2].Kind != "c" {
		t.Fatalf("queue = %v", sp.queue)
	}
	sp.say(n("d", 2), t0, false)
	if len(sp.queue) != 3 || sp.queue[0].Kind != "b" || sp.queue[2].Kind != "d" {
		t.Fatalf("queue = %v", sp.queue)
	}
}

func TestChatterLimitedAndQuietAsleep(t *testing.T) {
	var sp speech
	sp.say(chatter("a"), t0, false)
	sp.say(chatter("b"), t0.Add(time.Minute), false)
	if len(sp.queue) != 1 {
		t.Fatalf("queue = %v", sp.queue)
	}
	sp.say(chatter("c"), t0.Add(16*time.Minute), false)
	if len(sp.queue) != 2 {
		t.Fatalf("queue = %v", sp.queue)
	}
	var sl speech
	sl.say(chatter("a"), t0, true)
	sl.say(n("b", 2), t0, true)
	if len(sl.queue) != 1 || sl.queue[0].Kind != "b" {
		t.Fatalf("queue = %v", sl.queue)
	}
}

func TestUnfocusKeepsSticky(t *testing.T) {
	var sp speech
	sp.say(n("a", 2), t0, false)
	sp.next(t0)
	sp.unfocus()
	if sp.cur != nil {
		t.Fatal("normal cur kept")
	}
	s := n("s", 2)
	s.Sticky = true
	sp.say(s, t0, false)
	sp.next(t0.Add(time.Second))
	sp.unfocus()
	if sp.cur == nil {
		t.Fatal("sticky cur dropped")
	}
}

func TestStaleDoneIgnored(t *testing.T) {
	var sp speech
	sp.say(n("a", 1), t0, false)
	sp.next(t0)
	old := sp.cur.ID
	sp.say(n("needs_you", 3), t0, false)
	if sp.cur.ID == old {
		t.Fatal("same id")
	}
	sp.done(old, t0)
	if sp.cur == nil || sp.cur.Kind != "needs_you" {
		t.Fatalf("cur = %v", sp.cur)
	}
}

func TestStuckLineIsDropped(t *testing.T) {
	var sp speech
	sp.say(n("a", 2), t0, false)
	sp.next(t0)
	sp.expire(sp.curAt.Add(31 * time.Second))
	if sp.cur != nil {
		t.Fatal("stuck cur kept")
	}
	s := n("s", 2)
	s.Sticky = true
	sp.say(s, t0, false)
	sp.next(t0.Add(time.Minute))
	sp.expire(sp.curAt.Add(31 * time.Second))
	if sp.cur == nil {
		t.Fatal("sticky cur dropped")
	}
}

func TestTalking(t *testing.T) {
	var sp speech
	sp.say(line{Kind: "a", Text: "0123456789", Prio: 2}, t0, false)
	sp.next(t0)
	if !sp.talking(sp.curAt.Add(499 * time.Millisecond)) {
		t.Fatal("should talk")
	}
	if sp.talking(sp.curAt.Add(500 * time.Millisecond)) {
		t.Fatal("should be done")
	}
}

func TestDropKind(t *testing.T) {
	var sp speech
	sp.say(n("needs_you", 3), t0, false)
	sp.say(n("a", 2), t0, false)
	sp.next(t0)
	sp.drop(func(l line) bool { return l.Kind == "needs_you" })
	if sp.cur != nil || len(sp.queue) != 1 || sp.queue[0].Kind != "a" {
		t.Fatalf("cur = %v queue = %v", sp.cur, sp.queue)
	}
}

func TestRequeuedStickySurvivesFullQueue(t *testing.T) {
	var sp speech
	pr := line{Kind: "pr_fix", Text: "x", Prio: 2, Sticky: true, URL: "https://github.com/o/r/pull/1"}
	sp.say(pr, t0, false)
	sp.next(t0)
	for _, k := range []string{"a", "b", "c"} {
		sp.say(n(k, 2), t0, false)
	}
	sp.say(line{Kind: "needs_you", Text: "!", Prio: 3, Sticky: true}, t0, false)
	if sp.cur.Kind != "needs_you" || len(sp.queue) != 3 || sp.queue[0].URL != pr.URL {
		t.Fatalf("cur = %v queue = %v", sp.cur, sp.queue)
	}
}

func TestSamePRDedupesAcrossTexts(t *testing.T) {
	var sp speech
	url := "https://github.com/o/r/pull/1"
	sp.say(line{Kind: "pr_fix", Text: "x", Prio: 2, Sticky: true, URL: url}, t0, false)
	sp.next(t0)
	sp.say(line{Kind: "pr_fix", Text: "y", Prio: 2, Sticky: true, URL: url}, t0, false)
	if len(sp.queue) != 0 {
		t.Fatalf("queue = %v", sp.queue)
	}
}
