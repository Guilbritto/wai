# wai-bichinho

A small pet that lives in your prompt box. It hatches from an egg, grows with your agents' finished turns, gets hungry and lonely with time, and reacts to the focused session.

## Build and install

```sh
cd extensions/wai-bichinho
go build -o bin/wai-bichinho .
```

Then in wai: Extensions, install from folder, pick this folder, and review the consent (`process`, `panels`, `sessions:read`). It needs a wai with extension API 1.6.

## Playing

- **The egg.** The first one is free: pick **Hatch** in the pet's ⋯ menu. You get another egg every 25 finished turns, up to 3. Hatching a new one moves the current pet to **Past pets**.
- **Rarity.** An egg is Common (60%), Uncommon (25%), Rare (10%), Epic (4%) or Legendary (1%), shown by the level badge's colour, and one in 40 is shiny (✦). Rarer pets roll higher stats.
- **Species.** 12, from Brazilian fauna and folklore: Tatu, Sapo-cururu, Jabuti, Vagalume (common); Quati, Tucano, Preguiça (uncommon); Lobo-guará, Boto-cor-de-rosa (rare); Mico-leão-dourado, Curupira (epic); Boitatá (legendary). The card's **Collection** shows which ones you have hatched.
- **Growing.** Each finished turn in any session gives 10 xp (2 for a cancelled one). Levels come at 25, 100, 225, 400… xp.
- **Care.** It goes from full to starving in 8 hours and loses its affection in a day: **Feed** and **Pet** it from the ⋯ menu or its card. It never dies.
- **Moods.** It works along with the focused session, shouts when any session needs you, frowns at an error, and falls asleep after 20 minutes without activity.

The pet shows only in the focused pane's prompt box, and hides in narrow panes. **Open card** (or the paw in the activity bar) opens its card in the sidebar: stats, care, renaming, the collection and past pets.

State lives in `pet.json` in the extension's data folder (`<config>/extension-data/wai-bichinho/`).
