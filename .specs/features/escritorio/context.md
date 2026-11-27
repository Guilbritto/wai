# Escritório Context

**Gathered:** 2026-11-26
**Spec:** `.specs/features/escritorio/spec.md`
**Status:** Ready for design

---

## Feature Boundary

A wai extension for a virtual office with Gather's mechanics (2D map, avatars, proximity audio/video, rooms, desks, status, chat). Its name, art and look are original: Gather's assets and identity are not copied.

---

## Implementation Decisions

### Where it runs

- Native inside wai. The user picked "Nativo dentro do wai" over the browser page and the panel-only versions. This needs changes to wai's core (a colour map node, keyboard input, audio/video), because today a panel draws only one-colour `pixels`, gets clicks without coordinates, gets no keyboard input and has no camera or mic.

### Server

- A self-hosted Go server that a team member runs on a VM or machine. The user picked "Servidor Go próprio" over LiveKit Cloud and over one wai hosting on the LAN.

### MVP scope

- The user put all four in the first version: "Mapa + avatares andando, Áudio/vídeo por proximidade, Salas privadas + mesas, Status + chat de texto".

### Slicing

- Four deliveries in sequence, each with its own spec, usable alone (user: "4 entregas em sequência"):
  1. `escritorio` (this spec): map + avatars + Go server + presence panel + agent status, plus the extension API for scene and keyboard.
  2. `escritorio-av`: proximity audio/video.
  3. `escritorio-salas`: private rooms + desks.
  4. `escritorio-chat`: text chat + manual status (available/busy/away).

### Joining

- Name + server address + team token on first use, no external account (user: "Nome + servidor + token").
- User asked: "Depois se precisarmos mudar, fica fácil?" Answer: yes, if the server checks identity in one place and avatars key on a server-assigned id, never on the token. Swapping in GitHub/Google login later changes that one check and the setup form. Recorded as OFC-27.

### Mic and camera

- Off when entering the office (user: "Desligados"). Binds delivery 2.

### Architecture

- Extension + new public API (user: "Extensão + API nova"): the office is a Go extension like wai-bichinho; wai gains generic capabilities (colour scene node, keyboard input, later an A/V call) any extension can use.

### Scene format

- The scene node carries a palette and character patterns in the JSON; wai paints the map once and tweens sprites (user: "Paleta + padrões no JSON"), over a PNG tileset and an incremental scene protocol. AD-002.

### wai integration

- "Status pelos agentes": an agent's session state shows on its owner's avatar.
- "Painel de presença": a wai panel lists who is online and who is near you, with a way to enter the office.
- The pet following the avatar was not picked.

### Agent's Discretion

(none yet)

### Declined / Undiscussed Gray Areas → Assumptions

(pending)

---

## Specific References

- Gather (gather.town): "quero literalmente fazer bem parecido com eles". Mechanics are the reference; art, name and look are not copied.
- Zed's calls run on LiveKit inside GPUI: precedent for native audio in wai (see the memory note "Follow other IDEs").

---

## Deferred Ideas

- The wai-bichinho pet following the avatar (offered, not picked).
