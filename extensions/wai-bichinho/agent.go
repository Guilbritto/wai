package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
	"unicode/utf8"
)

type Note struct {
	At   time.Time `json:"at"`
	Text string    `json:"text"`
}

const (
	maxDiary      = 30
	diaryInPrompt = 15
	linesEvery    = 6 * time.Hour
	genBackoff    = 30 * time.Minute
	genTimeout    = 90 * time.Second
	maxLineLen    = 80
	model         = "claude-opus-5-5" // ponytail: Haiku 5.5 ("claude-haiku-5-5") is the cheaper swap
)

// runner runs one Claude request and returns the CLI's stdout.
type runner func(ctx context.Context, system, prompt, schema string) ([]byte, error)

type pools struct {
	lines map[string][]string
	born  time.Time
	level int
	err   error
}

// traits: what a stat at 7 or more, and at 3 or less, adds to the persona.
var traits = [4]struct{ high, low string }{
	{"terse, on-task", "easily distracted, drifts into random thoughts"},
	{"soothing, unbothered even by errors", "dramatic: small things are big events"},
	{`mischievous: puns, jokes, playful denial ("Quebrou tudo. Não fui eu.")`, "orderly, likes things tidy"},
	{"affectionate and warm", "grumpy and sarcastic, never mean"},
}

func persona(p *Pet) string {
	sp := speciesByID(p.Species)
	var b strings.Builder
	fmt.Fprintf(&b, "You are %s, a %s, a %s creature from Brazilian fauna or folklore; let its nature show.", p.Name, sp.Name, strings.ToLower(rarities[sp.Rarity].name))
	switch sp.Rarity {
	case Epic:
		b.WriteString(" You are proud.")
	case Legendary:
		b.WriteString(" You are grandiose and speak of yourself as a legend.")
	}
	if p.Shiny {
		b.WriteString(" You are a little vain about your shine.")
	}
	for i, st := range p.Stats {
		switch {
		case st >= 7:
			fmt.Fprintf(&b, " Your %s is high: you are %s.", statNames[i], traits[i].high)
		case st <= 3:
			fmt.Fprintf(&b, " Your %s is low: you are %s.", statNames[i], traits[i].low)
		}
	}
	switch lv := level(p.XP); {
	case lv <= 2:
		b.WriteString(" You are very young: use baby talk, 1 to 3 words per line.")
	case lv <= 5:
		b.WriteString(" You are growing: use short simple sentences.")
	default:
		b.WriteString(" You are grown: use full short sentences that may recall your diary.")
	}
	b.WriteString("\n\nRules: write in Brazilian Portuguese. At most 60 characters per line. No emoji except ♥, and no quotes. Never mention being an AI and never give coding advice. Keep it friendly.")
	return b.String()
}

func request(p *Pet, mood string, diary []Note) string {
	var b strings.Builder
	fmt.Fprintf(&b, "Pet: %s, a %s, level %d. ", p.Name, speciesByID(p.Species).Name, level(p.XP))
	fmt.Fprintf(&b, "Stats (1-10): %s %d, %s %d, %s %d, %s %d. Mood now: %s.\n", statNames[0], p.Stats[0], statNames[1], p.Stats[1], statNames[2], p.Stats[2], statNames[3], p.Stats[3], mood)
	if len(diary) > 0 {
		b.WriteString("\nYour diary, oldest first:\n")
		for _, n := range diary {
			fmt.Fprintf(&b, "- %s: %s\n", n.At.Format("2006-01-02"), n.Text)
		}
	}
	b.WriteString("\nWrite 6 different lines for each event kind below, in character. Kind: when it is said.\n")
	for _, k := range kinds {
		fmt.Fprintf(&b, "- %s: %s\n", k.Kind, k.Means)
	}
	return b.String()
}

func schema() string {
	props := map[string]any{}
	var req []string
	for _, k := range kinds {
		props[k.Kind] = map[string]any{"type": "array", "items": map[string]any{"type": "string"}}
		req = append(req, k.Kind)
	}
	b, _ := json.Marshal(map[string]any{"type": "object", "properties": props, "required": req, "additionalProperties": false})
	return string(b)
}

// parseLines reads the CLI's JSON: structured_output when present, else the result decoded as JSON.
// Unknown kinds, empty lines and lines over maxLineLen are dropped.
func parseLines(out []byte) (map[string][]string, error) {
	var env struct {
		IsError bool            `json:"is_error"`
		Result  string          `json:"result"`
		Struct  json.RawMessage `json:"structured_output"`
	}
	if err := json.Unmarshal(out, &env); err != nil {
		return nil, err
	}
	if env.IsError {
		return nil, errors.New("claude: " + env.Result)
	}
	raw := []byte(env.Result)
	if len(env.Struct) > 0 && string(env.Struct) != "null" {
		raw = env.Struct
	}
	var got map[string][]string
	if err := json.Unmarshal(raw, &got); err != nil {
		return nil, err
	}
	lines := map[string][]string{}
	for _, k := range kinds {
		for _, l := range got[k.Kind] {
			if l = strings.TrimSpace(l); l != "" && utf8.RuneCountInString(l) <= maxLineLen {
				lines[k.Kind] = append(lines[k.Kind], l)
			}
		}
	}
	return lines, nil
}

func findClaude() string {
	if p, err := exec.LookPath("claude"); err == nil {
		return p
	}
	if home, err := os.UserHomeDir(); err == nil {
		p := filepath.Join(home, ".local", "bin", "claude")
		if fi, err := os.Stat(p); err == nil && fi.Mode()&0o111 != 0 {
			return p
		}
	}
	return ""
}

func claudeRunner(bin, dir string) runner {
	return func(ctx context.Context, system, prompt, schema string) ([]byte, error) {
		cmd := exec.CommandContext(ctx, bin, "-p", "--safe-mode", "--model", model, "--effort", "low",
			"--output-format", "json", "--json-schema", schema, "--system-prompt", system,
			"--tools", "", "--no-session-persistence", "--max-budget-usd", "0.10", prompt)
		cmd.Dir = dir
		return cmd.Output()
	}
}

// note adds a line to the diary, keeping the last maxDiary.
func (s *server) note(text string) {
	s.st.Diary = append(s.st.Diary, Note{At: s.now(), Text: text})
	if n := len(s.st.Diary); n > maxDiary {
		s.st.Diary = append([]Note(nil), s.st.Diary[n-maxDiary:]...)
	}
}

// maybeGenerate starts one background run when the pools are missing, stale or for another pet.
// The goroutine touches nothing of the server but s.gen.
func (s *server) maybeGenerate(now time.Time) {
	p := s.st.Pet
	if s.run == nil || s.genBusy || p == nil || now.Sub(s.genFailed) < genBackoff {
		return
	}
	lv := level(p.XP)
	if s.st.LinesBorn.Equal(p.Born) && s.st.LinesLevel == lv && now.Sub(s.st.LinesAt) < linesEvery {
		return
	}
	pet := *p
	mood, _ := s.mood(now)
	diary := s.st.Diary[max(0, len(s.st.Diary)-diaryInPrompt):]
	diary = append([]Note(nil), diary...)
	run, gen := s.run, s.gen
	s.genBusy = true
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), genTimeout)
		defer cancel()
		res := pools{born: pet.Born, level: lv}
		out, err := run(ctx, persona(&pet), request(&pet, mood, diary), schema())
		if err == nil {
			res.lines, err = parseLines(out)
		}
		res.err = err
		gen <- res
	}()
}

// gotPools takes a finished run on the serve goroutine.
func (s *server) gotPools(p pools, now time.Time) {
	s.genBusy = false
	if s.st.Pet == nil || !s.st.Pet.Born.Equal(p.born) {
		return
	}
	if p.err != nil || len(p.lines) == 0 {
		s.genFailed = now
		if p.err != nil {
			fmt.Fprintln(os.Stderr, "wai-bichinho: lines:", p.err)
		}
		return
	}
	s.st.Lines, s.st.LinesBorn, s.st.LinesLevel, s.st.LinesAt = p.lines, p.born, p.level, now
	s.save()
}
