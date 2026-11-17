package main

import (
	"fmt"
	"math"
	"math/rand/v2"
	"strings"
	"time"
)

type Rarity int

const (
	Common Rarity = iota
	Uncommon
	Rare
	Epic
	Legendary
)

// rarities: the odds of an egg (weight out of 100), the badge tone, and the lowest stat roll.
var rarities = [...]struct {
	name, tone    string
	weight, floor int
}{
	{"Common", "neutral", 60, 1},
	{"Uncommon", "success", 25, 2},
	{"Rare", "info", 10, 3},
	{"Epic", "danger", 4, 4},
	{"Legendary", "warning", 1, 5},
}

// shinyOdds: one egg in shinyOdds hatches shiny, at any rarity.
const shinyOdds = 40

// Species art is three lines; E is an eye, M the mouth, and * twinkles.
type Species struct {
	ID, Name string
	Rarity   Rarity
	Art      [3]string
}

var species = []Species{
	{"tatu", "Tatu", Common, [3]string{
		`  .-===-.`,
		` ( E M E )`,
		"  `^^-^^'"}},
	{"sapo", "Sapo-cururu", Common, [3]string{
		`  (E)_(E)`,
		` (   M   )`,
		`  ^^   ^^`}},
	{"jabuti", "Jabuti", Common, [3]string{
		`  _/###\_`,
		` ( E M E )`,
		`  "-' '-"`}},
	{"vagalume", "Vagalume", Common, [3]string{
		`  \   /`,
		` (E M E)`,
		`  /)*(\`}},
	{"quati", "Quati", Uncommon, [3]string{
		`  /\_/\`,
		` (E M E)`,
		`  (___)=#=#`}},
	{"tucano", "Tucano", Uncommon, [3]string{
		`   .--.`,
		`  ( E  )==>`,
		`   )  (`}},
	{"preguica", "Preguiça", Uncommon, [3]string{
		` ~~~~~~~~~`,
		`  \(EME)/`,
		`   (   )`}},
	{"lobo-guara", "Lobo-guará", Rare, [3]string{
		`  /\   /\`,
		` ( E M E )`,
		`   || ||`}},
	{"boto", "Boto-cor-de-rosa", Rare, [3]string{
		`    __`,
		`  <(E M)~~`,
		` ~~~~~~~~~~`}},
	{"mico", "Mico-leão-dourado", Epic, [3]string{
		` \\|||//`,
		` (E M E)`,
		` //|||\\`}},
	{"curupira", "Curupira", Epic, [3]string{
		`  ^^^^^`,
		` (E M E)`,
		`  <| |<`}},
	{"boitata", "Boitatá", Legendary, [3]string{
		` *~(E E)~*`,
		`  ~\_M_/~`,
		` *~~~~~~~*`}},
}

var eggArt = [3]string{
	`   .-.`,
	`  (   )`,
	"   `-'",
}

func speciesByID(id string) *Species {
	for i := range species {
		if species[i].ID == id {
			return &species[i]
		}
	}
	return nil
}

var names = []string{
	"Bolota", "Pipoca", "Farofa", "Paçoca", "Jujuba", "Tapioca", "Cocada", "Quindim",
	"Pudim", "Caju", "Pitanga", "Acerola", "Cuscuz", "Pamonha", "Canjica", "Mingau",
	"Bombom", "Fubá", "Tutu", "Feijão", "Biju", "Sagu", "Pequi", "Açaí",
}

var statNames = [4]string{"Focus", "Calm", "Chaos", "Charm"}

type Pet struct {
	Name    string    `json:"name"`
	Species string    `json:"species"`
	Shiny   bool      `json:"shiny"`
	Born    time.Time `json:"born"`
	XP      int       `json:"xp"`
	FedAt   time.Time `json:"fed_at"`
	Love    float64   `json:"love"`     // at LovedAt
	LovedAt time.Time `json:"loved_at"` // it fades from there
	Stats   [4]int    `json:"stats"`
}

// State is what pet.json keeps.
type State struct {
	Pet   *Pet            `json:"pet,omitempty"` // nil: an egg waits
	Eggs  int             `json:"eggs"`
	Turns int             `json:"turns"`
	Seen  map[string]bool `json:"seen"` // species hatched; true once one was shiny
	Past  []Pet           `json:"past,omitempty"`
}

const (
	hungerHours = 8  // full to starving
	loveHours   = 24 // all love to none
	petLove     = 20
	turnsPerEgg = 25
	maxEggs     = 3
	maxPast     = 20
	xpPerTurn   = 10
)

func newState() State { return State{Eggs: 1, Seen: map[string]bool{}} }

func clamp(v float64) float64 { return math.Max(0, math.Min(100, v)) }

// Hunger is 0 (full) to 100 (starving).
func (p *Pet) Hunger(now time.Time) float64 {
	return clamp(now.Sub(p.FedAt).Hours() / hungerHours * 100)
}

func (p *Pet) Affection(now time.Time) float64 {
	return clamp(p.Love - now.Sub(p.LovedAt).Hours()/loveHours*100)
}

func (p *Pet) Pet(now time.Time) {
	p.Love, p.LovedAt = clamp(p.Affection(now)+petLove), now
}

// Feed says false when the pet was not hungry.
func (p *Pet) Feed(now time.Time) bool {
	if p.Hunger(now) < 10 {
		return false
	}
	p.FedAt = now
	return true
}

// level: 1, then n once xp reaches 25·(n-1)²: 25, 100, 225, 400…
func level(xp int) int { return 1 + int(math.Sqrt(float64(xp)/25)) }

func xpFor(level int) int { return 25 * (level - 1) * (level - 1) }

func rollRarity(rng *rand.Rand) Rarity {
	n := rng.IntN(100)
	for i, r := range rarities {
		if n < r.weight {
			return Rarity(i)
		}
		n -= r.weight
	}
	return Common
}

// Hatch opens an egg; the current pet, if any, moves to Past.
func (st *State) Hatch(rng *rand.Rand, now time.Time) *Pet {
	tier := rollRarity(rng)
	var pool []Species
	for _, s := range species {
		if s.Rarity == tier {
			pool = append(pool, s)
		}
	}
	sp := pool[rng.IntN(len(pool))]
	p := &Pet{
		Name: names[rng.IntN(len(names))], Species: sp.ID, Shiny: rng.IntN(shinyOdds) == 0,
		Born: now, FedAt: now, Love: 60, LovedAt: now,
	}
	floor := rarities[tier].floor
	for i := range p.Stats {
		p.Stats[i] = floor + rng.IntN(11-floor)
	}
	if st.Pet != nil {
		st.Past = append([]Pet{*st.Pet}, st.Past...)
		st.Past = st.Past[:min(len(st.Past), maxPast)]
	}
	if st.Seen == nil {
		st.Seen = map[string]bool{}
	}
	st.Seen[sp.ID] = st.Seen[sp.ID] || p.Shiny
	st.Pet = p
	st.Eggs = max(0, st.Eggs-1)
	return p
}

// TurnEnded counts a finished turn: XP for the pet, and an egg every turnsPerEgg turns.
func (st *State) TurnEnded(reason string) (levelUp, egg bool) {
	st.Turns++
	if st.Turns%turnsPerEgg == 0 && st.Eggs < maxEggs {
		st.Eggs++
		egg = true
	}
	if st.Pet == nil {
		return false, egg
	}
	gain := xpPerTurn
	if reason != "end_turn" {
		gain = 2
	}
	before := level(st.Pet.XP)
	st.Pet.XP += gain
	return level(st.Pet.XP) > before, egg
}

// A mood is the face and what floats beside it; fx cycles one step every `every` frames.
type mood struct {
	eye, mouth string
	fx         []string
	every      int
}

var moods = map[string]mood{
	"idle":     {"o", "_", nil, 1},
	"blink":    {"-", "_", nil, 1},
	"happy":    {"^", "w", []string{"♥", " ♥"}, 1},
	"surprise": {"O", "o", []string{"!"}, 1},
	"working":  {"•", "~", []string{".", "..", "..."}, 1},
	"alert":    {"O", "o", []string{"!", " !"}, 1},
	"error":    {"x", "~", []string{"#"}, 1},
	"hungry":   {"o", "O", nil, 1},
	"sad":      {";", "n", nil, 1},
	"talk":     {"o", "o", nil, 1},
	"sleeping": {"-", "_", []string{"z", "zZ"}, 4},
}

// draw is the pet's three lines for a mood at an animation frame.
func draw(p *Pet, moodName string, frame int) string {
	m := moods[moodName]
	if moodName == "idle" && frame%8 == 7 {
		m = moods["blink"]
	}
	if moodName == "talk" && frame%2 == 1 {
		m.mouth = "_"
	}
	lines := eggArt
	if p != nil {
		if sp := speciesByID(p.Species); sp != nil {
			lines = sp.Art
		}
	}
	twinkle := strings.NewReplacer("*", "·")
	for i, l := range lines {
		if p != nil {
			l = strings.NewReplacer("E", m.eye, "M", m.mouth).Replace(l)
		}
		if frame%2 == 1 {
			l = twinkle.Replace(l)
		}
		// Working bobs; an egg wobbles.
		if (moodName == "working" && frame%2 == 1) || (p == nil && frame/2%2 == 1) {
			l = " " + l
		}
		lines[i] = l
	}
	if len(m.fx) > 0 {
		lines[0] += " " + m.fx[frame/m.every%len(m.fx)]
	}
	if p != nil && p.Shiny {
		lines[2] += []string{" ✦", " ✧"}[frame/4%2]
	}
	return strings.Join(lines[:], "\n")
}

func (p *Pet) Title() string {
	if p.Shiny {
		return p.Name + " ✦"
	}
	return p.Name
}

func percent(v float64) string { return fmt.Sprintf("%d%%", int(math.Round(v))) }
