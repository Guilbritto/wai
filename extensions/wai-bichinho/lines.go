package main

import "strings"

var fallback = map[string][]string{
	"needs_you":   {"Ei! Precisam de você."},
	"error":       {"Ih… deu erro."},
	"pr_fix":      {"Tem PR pedindo conserto."},
	"pr_approved": {"PR aprovado! Bora dar merge?"},
	"level_up":    {"Subi de nível!"},
	"evolve":      {"Evoluí!"},
	"egg":         {"Achei um ovo!"},
	"hatch":       {"Oi! Eu sou {name}."},
	"pet":         {"Hehe ♥"},
	"feed":        {"Nham!"},
	"full":        {"Tô cheio."},
	"turn_done":   {"Pronto!"},
	"hungry":      {"Tô com fome…"},
	"lonely":      {"Cadê você?"},
	"wake":        {"Hm? Acordei."},
	"chat_done":   {"Terminei, dá uma olhada."},
	"chat_error":  {"Ih, não consegui. Tenta de novo?"},
	"chat_busy":   {"Calma, um de cada vez."},
}

// kinds lists every kind with its priority, stickiness, chatter flag and a one-line meaning for the agent prompt.
var kinds = []struct {
	Kind            string
	Prio            int
	Sticky, Chatter bool
	Means           string
}{
	{"needs_you", 3, true, false, "a session is waiting for the user"},
	{"error", 3, false, false, "the focused session hit an error"},
	{"pr_fix", 2, true, false, "a pull request needs a fix"},
	{"pr_approved", 2, true, false, "a pull request was approved"},
	{"level_up", 2, false, false, "the pet leveled up after a turn"},
	{"evolve", 2, false, false, "the pet just evolved into a new form"},
	{"egg", 2, false, false, "the pet found a new egg after a turn"},
	{"hatch", 2, false, false, "the pet just hatched; {name} is its name"},
	{"pet", 1, false, false, "the user petted it"},
	{"feed", 1, false, false, "the user fed it"},
	{"full", 1, false, false, "the user fed it while it was full"},
	{"turn_done", 1, false, true, "an agent turn ended normally"},
	{"hungry", 1, false, true, "it got hungry"},
	{"lonely", 1, false, true, "it feels lonely"},
	{"wake", 1, false, true, "the first event after it slept"},
	{"chat_done", 3, true, false, "the pet finished what the user asked; a click shows its session"},
	{"chat_error", 3, false, false, "the pet's own session failed or was cancelled"},
	{"chat_busy", 3, false, false, "the user sent the pet more messages than it can hold"},
}

// pick returns a line for kind, {name} replaced.
func (s *server) pick(kind string) string {
	pool := s.st.Lines[kind]
	if len(pool) == 0 {
		pool = fallback[kind]
	}
	if len(pool) == 0 {
		return ""
	}
	text := pool[s.rng.IntN(len(pool))]
	name := ""
	if s.st.Pet != nil {
		name = s.st.Pet.Name
	}
	return strings.ReplaceAll(text, "{name}", name)
}

// say queues kind's line with its priority, stickiness and chatter flag from kinds.
func (s *server) say(kind string) { s.sayURL(kind, "") }

// sayURL says a line whose bubble opens url when clicked.
func (s *server) sayURL(kind, url string) {
	for _, k := range kinds {
		if k.Kind == kind {
			if text := s.pick(kind); text != "" {
				s.talk.say(line{Kind: kind, Text: text, Prio: k.Prio, Sticky: k.Sticky, Chatter: k.Chatter, URL: url}, s.now(), s.asleep)
			}
			return
		}
	}
}
