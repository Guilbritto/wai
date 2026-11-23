# wai-bichinho

A small pet that walks in a dock at the foot of the sidebar. It hatches from an egg, grows with your agents' finished turns, gets hungry and lonely with time, and reacts to the focused session.

## Build and install

```sh
cd extensions/wai-bichinho
go build -o bin/wai-bichinho .
```

Then in wai: Extensions, install from folder, pick this folder, and review the consent (`process`, `panels`, `sessions:read`, `events:wai-waycarbon`). It needs a wai with extension API 1.9.

## Playing

- **The egg.** The first one is free: pick **Hatch** in the pet's ⋯ menu. You get another egg every 25 finished turns, up to 3. Hatching a new one moves the current pet to **Your pets**; nothing is ever deleted.
- **Rarity.** An egg is Common (60%), Uncommon (25%), Rare (10%), Epic (4%) or Legendary (1%), shown by the level badge's colour, and one in 40 is shiny (✦). Rarer pets roll higher stats.
- **Forms.** 25 lines and 40 forms, drawn in pixels: 7 common, 6 uncommon, 4 rare, 4 epic and 4 legendary lines. The card's **Collection** shows which forms you have hatched or evolved into.
- **Evolution.** Most lines evolve at Lv 5 and Lv 15 (three forms) or at Lv 10 (two forms); some never evolve. The card says when the next form comes.
- **Shiny.** One egg in 40 is shiny (✦) at any rarity, with its own colours.
- **Growing.** Each finished turn in any session gives 10 xp (2 for a cancelled one). Levels come at 25, 100, 225, 400… xp.
- **Your pets.** The card lists every stored pet with its form and level; **Use** (or **Switch pet** in the ⋯ menu) swaps it in and stores the current one. Hunger and love freeze while a pet is stored, and pick up where they were when it comes back.
- **Care.** It goes from full to starving in 8 hours and loses its affection in a day: **Feed** and **Pet** it from the ⋯ menu or its card. It never dies.
- **Moods.** It works along with the focused session, shouts when any session needs you, frowns at an error, and falls asleep after 20 minutes without activity.
- **PRs.** With wai-waycarbon installed, it hears its `prs.mine` event: it flashes when one of your PRs is approved or needs a fix (conflict, failing CI, requested changes), then looks alert while any need a fix and happy while any wait to merge. Without wai-waycarbon it just never hears it.
- **Talking.** It speaks in a small speech bubble with a tail, in Brazilian Portuguese. It speaks when it hatches, finds an egg, levels up, is petted or fed, when a session needs you or hits an error, when a PR is approved or needs a fix, and now and then when a turn ends, it gets hungry or lonely, or it wakes up. The bubble waits while the mouse is over it, dims while the prompt box has text, and closes on a click. The `needs_you` line sticks until you deal with it and shrinks to a bouncing "!" after a few seconds.

**Agent and privacy.** The lines come from fixed pt-BR pools. When the `claude` CLI is installed, it also asks Claude to write fresh pools in a personality built from the pet's stats, using `claude -p --safe-mode` with no tools and no saved session. It sends only the pet's name, species, stats, level, mood and its diary (the last 15 of its 30 notes, with dates); it never sends your sessions, prompts, files or PR URLs. Claude Code itself adds your account details (such as your email) to the model's context, so the persona tells the model never to mention your email, account or personal data. Pools refresh at most every 6 hours, with a 30 minute backoff after a failure: about 4 small calls a day, plus one when a new pet hatches or the pet levels up, each capped at $0.10. A pet you switch to uses its fixed lines until the 6 hours are up, so it gets new lines within a few hours. Without `claude`, or when it fails, the pet keeps its fixed lines.

The pet walks in the dock at the foot of the sidebar, so it only shows while the sidebar is open. **Open card** (or the paw in the activity bar) opens its card in the sidebar: stats, care, renaming, the collection and your pets.

Pets from 0.3.x keep working: their old species become the closest line of the new dex (Tatu to Gotinha, Sapo to Larvinha, and so on), and pets stored before the selector had no storage time, so they resume from their last feeding or petting. Pets of the lines 0.5.0 replaced become their successors: Monstro da ferrugem to Kobold, Verme púrpura to Beholder, Sleipnir to Fada, Tarrasque to Tiamat and Singularidade to Goblin.

State lives in `pet.json` in the extension's data folder (`<config>/extension-data/wai-bichinho/`).
