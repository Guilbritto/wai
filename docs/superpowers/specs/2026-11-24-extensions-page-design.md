# Extensions page: list and detail, like VS Code

Approved mockup: `.superpowers/brainstorm/12078-1795522903/content/extensions-vscode-v3.html`.

## Intent

The Extensions page (Settings → Extensions) stacks everything in one column: install-from-folder and
git URL controls on top, then rows that grow with permissions, errors and plugin buttons. Make it
read like VS Code's Extensions view: a short list on the left and the selected extension's page
on the right.

## Goals

- Left pane (340 px): title "Extensions", Refresh and a `⋯` menu, a search field, then collapsible
  INSTALLED and AVAILABLE sections. A row is a monogram, the name, the version, one truncated line
  of description, and a status (Running / Starting / Crashed / Disabled) or, for a listing, the
  index it came from. On the right of the row: Install (listing), Update (installed, newer version
  listed), or a gear menu with Enable/Disable and Uninstall. Index errors show as one warning box
  under AVAILABLE.
- `⋯` menu: Install from folder…, Install from git URL… (a dialog, like Add index), Add index….
- AVAILABLE lists only listings that are not installed; an installed one with a newer listed
  version shows Update on its INSTALLED row.
- Right pane: a header (large monogram, name, version badge, "x.y.z available" badge, id · index
  · repository link, description, actions Update to x / Enable or Disable / Uninstall, status),
  then tabs:
  - Details: the crash or failure text in a red box, About (the description), and the Claude Code
    plugins it requires or suggests, each with "Install in terminal".
  - Features (installed only): the manifest's contributions grouped by kind (panels, bar items,
    commands, agents, MCP servers, templates, themes); an empty note when there are none.
  - Permissions (installed only): each granted permission in plain words, then the v1 limitation.
  - A meta column: Identifier, Version (→ newer), Folder for an installed one; Identifier,
    Version, Index, Platforms for a listing; Repository and, for GitHub, Releases.
- The Settings nav shows a dot on Extensions when an update is listed.
- The Extensions page drops the Settings page header (title, description); the two panes fill the
  content area.

## Non-goals

- No extension icons in the manifest (monogram colored from the id; an `icon` field can come later).
- No permissions or features for listings: the index does not carry them; the consent dialog
  still shows them before anything installs.
- No README or changelog tab.
- No move out of the Settings modal.

## Constraints

- Consent stays the only way in: every install and update still goes through the consent dialog.
- Manifest and index text still goes through `clamp`.
- Existing tests keep their meaning; ids they click may move to the detail pane.

## Approaches

1. **List and detail inside the Settings modal** (picked): same entry point and modal, smallest move.
2. VS Code's full model: the list in the sidebar, the detail as a tab in the main area. More
   faithful, but it needs a new sidebar tab and a new kind of main-area tab.
3. Polish the single column (cards, spacing). Cheapest, but keeps the stacked rows the page has.

The owner approved the mockup of 1 ("ficou bom").

## Calls made for the builder

- No "Source" row for installed extensions: the store does not record where one came from. The
  index is shown when the id is listed.
- Selection defaults to the first installed extension, else the first listing.
- Monogram palette: lilas, boreal, brasa, ceu, magenta, ok, picked by a hash of the id.
- No "Update all" (it was in the mockup's menu): every update needs its own consent dialog and
  the view stages one at a time, so it would be a queue of dialogs. Update stays per row.
