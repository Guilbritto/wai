package main

import (
	"fmt"
	"math"
	"math/rand/v2"
	"slices"
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

// Form is one stage of a line; Level is where it is reached (the first form is 1).
type Form struct {
	ID, Name string
	Level    int
}

// Line is an evolution line; its id is Forms[0].ID.
type Line struct {
	Rarity Rarity
	Forms  []Form
}

func ln(r Rarity, forms ...Form) Line    { return Line{r, forms} }
func fm(id, name string, level int) Form { return Form{id, name, level} }

// dex lists the lines in the mockup's order: the source of form numbers #001 to #040.
var dex = []Line{
	ln(Common, fm("gotinha", "Gotinha", 1), fm("gosma", "Gosma", 5), fm("gosmao", "Gosmão", 15)),
	ln(Common, fm("larvinha", "Larvinha", 1), fm("bug", "Bug", 5), fm("megabug", "Megabug", 15)),
	ln(Common, fm("fagulha", "Fagulha", 1), fm("fogo", "Fogo-fátuo", 10)),
	ln(Common, fm("bit", "Bit", 1), fm("bot", "Bot", 10)),
	ln(Common, fm("alevino", "Alevino", 1), fm("abissal", "Peixe-abissal", 10)),
	ln(Common, fm("bauzinho", "Bauzinho", 1), fm("mimico", "Mímico", 10)),
	ln(Uncommon, fm("grifinho", "Grifinho", 1), fm("grifo", "Grifo", 10)),
	ln(Uncommon, fm("pedrinha", "Pedrinha", 1), fm("golem", "Golem", 10)),
	ln(Uncommon, fm("corvinho", "Corvinho", 1), fm("corvo", "Corvo de Odin", 10)),
	ln(Uncommon, fm("drone", "Drone", 1)),
	ln(Uncommon, fm("ursinhocoruja", "Ursinho-coruja", 1), fm("ursocoruja", "Urso-coruja", 10)),
	ln(Uncommon, fm("ferrugem", "Monstro da ferrugem", 1)),
	ln(Rare, fm("dragaozinho", "Dragãozinho", 1), fm("dragao", "Dragão", 10)),
	ln(Rare, fm("caozinho", "Cãozinho", 1), fm("cerbero", "Cérbero", 10)),
	ln(Rare, fm("cavalinho", "Cavalo-marinho", 1), fm("hipocampo", "Hipocampo", 10)),
	ln(Rare, fm("cubo", "Cubo gelatinoso", 1)),
	ln(Epic, fm("fenix", "Fênix", 1)),
	ln(Epic, fm("sleipnir", "Sleipnir", 1)),
	ln(Epic, fm("mecha", "Mecha", 1)),
	ln(Epic, fm("verme", "Verme púrpura", 1)),
	ln(Legendary, fm("kraken", "Kraken", 1)),
	ln(Legendary, fm("jormungandr", "Jörmungandr", 1)),
	ln(Legendary, fm("hidra", "Hidra", 1)),
	ln(Legendary, fm("singularidade", "Singularidade", 1)),
	ln(Legendary, fm("tarrasque", "Tarrasque", 1)),
}

// oldSpecies maps the species of 0.3.1 to the line that took their place.
var oldSpecies = map[string]string{"tatu": "gotinha", "sapo": "larvinha", "jabuti": "alevino",
	"vagalume": "fagulha", "quati": "grifinho", "tucano": "corvinho", "preguica": "pedrinha",
	"lobo-guara": "caozinho", "boto": "cavalinho", "mico": "fenix", "curupira": "sleipnir",
	"boitata": "jormungandr"}

// lineByID is nil when id is not the first form of a line.
func lineByID(id string) *Line {
	for i := range dex {
		if dex[i].Forms[0].ID == id {
			return &dex[i]
		}
	}
	return nil
}

func isForm(id string) bool {
	for _, l := range dex {
		for _, f := range l.Forms {
			if f.ID == id {
				return true
			}
		}
	}
	return false
}

// formOf is the last form of p's line whose Level the pet has reached.
func formOf(p *Pet) Form {
	l := lineByID(p.Species)
	if l == nil {
		return Form{ID: "ovo", Name: "Egg", Level: 1}
	}
	f := l.Forms[0]
	for _, c := range l.Forms {
		if c.Level <= level(p.XP) {
			f = c
		}
	}
	return f
}

// pixels is a pixels panel node; id is left out when empty.
func pixels(g grid, scale int, id string) node {
	n := node{"type": "pixels", "rows": g.rows(), "scale": scale}
	if id != "" {
		n["id"] = id
	}
	return n
}

var names = []string{
	"Bolota", "Pipoca", "Farofa", "Paçoca", "Jujuba", "Tapioca", "Cocada", "Quindim",
	"Pudim", "Caju", "Pitanga", "Acerola", "Cuscuz", "Pamonha", "Canjica", "Mingau",
	"Bombom", "Fubá", "Tutu", "Feijão", "Biju", "Sagu", "Pequi", "Açaí",
}

var statNames = [4]string{"Focus", "Calm", "Chaos", "Charm"}

type Pet struct {
	Name    string    `json:"name"`
	Species string    `json:"species"` // the line id
	Shiny   bool      `json:"shiny"`
	Born    time.Time `json:"born"`
	XP      int       `json:"xp"`
	FedAt   time.Time `json:"fed_at"`
	Love    float64   `json:"love"`     // at LovedAt
	LovedAt time.Time `json:"loved_at"` // it fades from there
	Stats   [4]int    `json:"stats"`

	StoredAt time.Time `json:"stored_at,omitempty"` // when it moved to Past; hunger and love freeze from then
}

// State is what pet.json keeps.
type State struct {
	Pet   *Pet            `json:"pet,omitempty"` // nil: an egg waits
	Eggs  int             `json:"eggs"`
	Turns int             `json:"turns"`
	Seen  map[string]bool `json:"seen"` // forms seen; true once one was shiny
	Past  []Pet           `json:"past,omitempty"`

	Lines      map[string][]string `json:"lines,omitempty"` // Claude's pools for the pet born at LinesBorn
	LinesBorn  time.Time           `json:"lines_born,omitempty"`
	LinesLevel int                 `json:"lines_level,omitempty"`
	LinesAt    time.Time           `json:"lines_at,omitempty"`
	Diary      []Note              `json:"diary,omitempty"` // kept across pets
}

const (
	hungerHours = 8  // full to starving
	loveHours   = 24 // all love to none
	petLove     = 20
	turnsPerEgg = 25
	maxEggs     = 3
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

// store moves the current pet to the front of Past.
func (st *State) store(now time.Time) {
	stored := *st.Pet
	stored.StoredAt = now
	st.Past = append([]Pet{stored}, st.Past...)
}

// Use swaps the past pet born at born (UnixNano) with the current one. Hunger and love froze while
// it was stored, so its clocks move forward by the time it spent away. False: no such pet.
func (st *State) Use(born int64, now time.Time) bool {
	i := slices.IndexFunc(st.Past, func(q Pet) bool { return q.Born.UnixNano() == born })
	if i < 0 {
		return false
	}
	q := st.Past[i]
	base := q.StoredAt
	if base.IsZero() {
		base = q.FedAt
		if q.LovedAt.After(base) {
			base = q.LovedAt
		}
	}
	shift := now.Sub(base)
	q.FedAt, q.LovedAt, q.StoredAt = q.FedAt.Add(shift), q.LovedAt.Add(shift), time.Time{}
	st.Past = slices.Delete(st.Past, i, i+1)
	if st.Pet != nil {
		st.store(now)
	}
	st.Pet = &q
	st.Lines = nil
	return true
}

// Hatch opens an egg; the current pet, if any, moves to Past.
func (st *State) Hatch(rng *rand.Rand, now time.Time) *Pet {
	tier := rollRarity(rng)
	var pool []*Line
	for i := range dex {
		if dex[i].Rarity == tier {
			pool = append(pool, &dex[i])
		}
	}
	sp := pool[rng.IntN(len(pool))].Forms[0]
	p := &Pet{
		Name: names[rng.IntN(len(names))], Species: sp.ID, Shiny: rng.IntN(shinyOdds) == 0,
		Born: now, FedAt: now, Love: 60, LovedAt: now,
	}
	floor := rarities[tier].floor
	for i := range p.Stats {
		p.Stats[i] = floor + rng.IntN(11-floor)
	}
	if st.Pet != nil {
		st.store(now)
	}
	if st.Seen == nil {
		st.Seen = map[string]bool{}
	}
	st.Seen[sp.ID] = st.Seen[sp.ID] || p.Shiny
	st.Pet = p
	st.Lines = nil
	st.Eggs = max(0, st.Eggs-1)
	return p
}

// TurnEnded counts a finished turn: XP for the pet, and an egg every turnsPerEgg turns.
func (st *State) TurnEnded(reason string) (levelUp, evolved, egg bool) {
	st.Turns++
	if st.Turns%turnsPerEgg == 0 && st.Eggs < maxEggs {
		st.Eggs++
		egg = true
	}
	if st.Pet == nil {
		return false, false, egg
	}
	gain := xpPerTurn
	if reason != "end_turn" {
		gain = 2
	}
	before, form := level(st.Pet.XP), formOf(st.Pet)
	st.Pet.XP += gain
	if now := formOf(st.Pet); now.ID != form.ID {
		evolved = true
		st.Seen[now.ID] = st.Seen[now.ID] || st.Pet.Shiny
	}
	return level(st.Pet.XP) > before, evolved, egg
}

// nextForm is the form the pet evolves into, and false when it is on its line's last.
func nextForm(p *Pet) (Form, bool) {
	if l := lineByID(p.Species); l != nil {
		for _, f := range l.Forms {
			if f.Level > level(p.XP) {
				return f, true
			}
		}
	}
	return Form{}, false
}

func (p *Pet) Title() string {
	if p.Shiny {
		return p.Name + " ✦"
	}
	return p.Name
}

func percent(v float64) string { return fmt.Sprintf("%d%%", int(math.Round(v))) }

const stageW, stageH = 128, 31

// walk is the pet's place on the stage and where it is going.
type walk struct{ x, dir, pause int }

// step moves one pixel, or waits; it returns true when it moved. At an edge it turns and rests, and
// now and then it stops for a while on its own.
func (w *walk) step(rng *rand.Rand) bool {
	if w.pause > 0 {
		w.pause--
		return false
	}
	w.x += w.dir
	if w.x <= 0 || w.x >= stageW-W {
		w.dir = -w.dir
		w.pause = 12
	} else if rng.Float64() < .025 {
		w.pause = 10 + rng.IntN(16)
	}
	return true
}

// stage puts the sprite at column x of a stageH-row, stageW-column scene, mirrored when flip.
func stage(g grid, x int, flip bool) []string {
	rows := g.rows()
	out := make([]string, stageH)
	for j := range out {
		row := []byte(rows[j])
		if flip {
			slices.Reverse(row)
		}
		out[j] = strings.Repeat(".", x) + string(row) + strings.Repeat(".", stageW-W-x)
	}
	return out
}
