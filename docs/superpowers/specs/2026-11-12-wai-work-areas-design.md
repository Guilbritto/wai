# wai: work areas (áreas de trabalho)

> 2026-11-12. Asked for by the owner (item 1.12 of `docs/superpowers/plans/2026-11-12-wai-execucao-pendencias.md`). Mockup the owner picked: option **A** of `/Users/guilherme.brito/Project/Guilherme/wai/.superpowers/brainstorm/85564-1794503108/content/work-areas-v1.html` (git-ignored; he hesitated with B before choosing A). Builds on the sidebar redesign (`feat/sidebar-redesign`, spec `docs/superpowers/specs/2026-11-12-wai-sidebar-redesign-design.md` on that branch): plan it only after that branch is merged.

## Intent

The owner works on several things at once, each with a few Claude sessions side by side, e.g. "Evolução BB" = three sessions in three panes. Today a click on any session row puts that session in the **focused pane**, replacing what was there, so a carefully arranged triple screen falls apart the moment he looks at another session. Groups don't help: they only fold the sidebar list.

Owner, verbatim: *"Área de trabalho: é uma área onde tem alguns chats abertos que ficam da forma que eu organizei. Grupo: é um agrupamento de sessão, onde não respeita as telas e como foram abertas."* And: *"se eu criar uma área de trabalho eu posso mudar/organizar as abas e depois se mudar a área de trabalho ou clicar em uma sessão fora da área de trabalho, a área de trabalho atual 'esconde' e é mostrado a nova área de trabalho ou a sessão que foi clicada (de forma unitária na tela)"*.

## Goals

1. A **work area** is a named, user-created set of sessions with their screen arrangement (which session in which pane, up to `MAX_PANES` = 4, and which pane has focus).
2. Showing an area shows its panes exactly as they were left.
3. Clicking a session outside the shown area, or another area, **hides** the current area intact. Another area shows as its panes; an outside session shows **alone, in one pane**.
4. Areas are listed in the sidebar's Sessions panel, in their own section.

## Non-goals

- Changing what groups do. Groups keep folding the Open list and never touch panes.
- More than `MAX_PANES` panes in an area.
- Areas spanning projects/windows, sharing areas, area templates.

## Anti-goals (a failure even if it works)

- A click anywhere silently rearranging or dropping a session from an area.
- Hiding an area ends, pauses or restarts its sessions, or loses a waiting permission/question.
- An area that needs you while hidden, with no sign of it.
- Idle CPU above 0.5 %: a hidden area draws nothing and runs no timer.
- Losing a sidebar feature (drag to pane, Open to the side, ×, rename, groups, History).

## Decided by the owner

- **Layout A:** a **"Work areas"** section in the Sessions panel, above **Open**. Each area is one row: layout icon (▥), name, number of panes; the shown area is highlighted. Groups stay inside Open with their folder look.
- **Create:** a **+** on the "Work areas" section header creates an area.
- **Fill:** drag session rows onto the area, the same way sessions are dragged into a group.
- **Organize:** inside a shown area the owner moves, opens and closes panes; the area keeps the result.
- **Switch:** as in Goal 3.

## My calls (not confirmed by the owner; check them while reading)

1. The **+** creates an empty area and asks for its name inline, like New group. An empty area shows one empty pane.
2. A session dropped on an area becomes a new pane to the right of its panes. With 4 panes, the drop is refused with the hint `A work area holds up to 4 sessions`.
3. A session belongs to **at most one area**. Its row moves from Open to under its area's row, which expands and folds like a group.
4. Closing a pane (×) inside an area, or dragging the session's row out of the area, **takes it out of the area** and back to Open. The session is not ended. Ending a session (Open row ×) also removes it from its area.
5. Deleting an area (row menu) returns its sessions to Open. Nothing is ended.
6. Clicking a session that belongs to another area shows that area, with the focus on that session's pane.
7. A hidden area's row shows the sidebar redesign's "needs you" badge when one of its sessions waits for you.
8. Areas (name, sessions in pane order, focused pane, folded state) are saved in `AppState` and come back on relaunch. The shown area is restored too.
9. In the loose view (no area shown), a clicked session shows alone. "Open to the side" still splits as today, and those splits are not saved anywhere.
10. While an area is shown, opening a file, Settings or a new-session draft uses a pane as today. That pane is not saved in the area. If it takes an area session's pane, that session stays in the area and comes back the next time the area is shown.

## Current state (facts to build on)

- `crates/wai/src/workspace.rs`: `panes: Vec<Pane>` (`Pane::Session/Editor/Settings/Empty/Draft/Preparing…`), `focused_pane`, `MAX_PANES = 4`. `show(session, side)` reuses a pane already showing the session, else `session_pane()` (the focused pane unless it is the editor). `focus_pane`, `drop_pane`, `close_pane`, `save_state`, `follow_session`.
- `crates/wai-settings/src/lib.rs`: `SessionGroup { name, collapsed, sessions: Vec<String> }` (by session id), persisted in `AppState.groups`.
- `crates/wai/src/sidebar.rs`: groups (`new_group`, collapse, rename by double click, drag a row onto a group header). The sidebar redesign rewrites this panel (activity bar, Sessions panel with Open and History sections, filter). Build on its version.
- Session ids are per agent; groups key by id alone (`ponytail` note in `SessionGroup`). Areas can do the same.

## Left to the builder

- The data model, e.g. `WorkArea { name, collapsed, panes: Vec<String>, focused: usize }` next to `SessionGroup`, and where the hidden areas' pane layout lives at runtime.
- Pane sizes, if panes become resizable.
- Test ids: follow the sidebar redesign's naming (`work-area-*`, `new-work-area`).
