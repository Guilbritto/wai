# Private extension indexes

## Intent

`wai-waycarbon` left the public official index (it now lives in the private repo
`waycarbon/wai-waycarbon`). A WayCarbon developer should point wai at that repo once, then install
and update the extension with one click, the same way other IDEs handle a private marketplace.
Someone without access to the repo sees nothing: no listing, no package, no name.

## Goals

- The Extensions page has **Add index…**. Typing `waycarbon/wai-waycarbon` (or any `https://` index
  URL) adds the index to the user's settings and lists its extensions.
- wai reads an index and downloads packages from a **private GitHub repo** with the user's GitHub
  login (`gh`), with no token stored by wai.
- A new release of the private extension shows up as **Update** after Refresh, with no index file
  to edit by hand.

## Non-goals

- A project suggesting an index to its members (`.wai/extensions.toml`, with consent). Later.
- Removing an index from the UI. The user edits `settings.toml`.
- Private hosts other than GitHub (GitLab, S3, an internal server).
- wai storing or asking for a token; choosing a GitHub account per index.
- Background auto-update of extensions.

## Anti-goals

- The token reaching any host other than `api.github.com` and `raw.githubusercontent.com`,
  including redirect targets, logs, error text and the UI. A design that works but leaks the token
  is a failure.
- A project's `.wai/settings.toml` gaining the power to add an index (stays user-only).

## Constraints (as the code stands)

- Indexes: TOML `[[extensions]]` (`crates/wai-ext/src/index.rs`, `parse_index`). `https://` only.
  The official index is read first; the first index to list an id wins
  (`crates/wai/src/marketplace.rs`, `fetch_all`).
- User indexes come from `[extensions] indexes` in the user's `settings.toml`
  (`crates/wai-settings/src/lib.rs:117`); a project's settings cannot add one (`lib.rs:303`).
- The UI writes `settings.toml` through `wai_settings::edit::write_change(user_dir, &Change)`, which
  keeps the file's comments and layout (`toml_edit`).
- Downloads go through `crates/wai/src/update/download.rs` (`get_into`, `download_with`, hyper,
  `Limits { allowed, max, timeout }`, up to 5 redirects). Package SHA-256 checking happens on
  install and does not change.
- Tests never touch the network: in tests, `marketplace::net` maps `https://test.invalid/<path>` to
  a file. That stays.
- wai imports the login shell's environment at startup (`crates/wai/src/shell_path.rs`), so `gh`,
  `GH_TOKEN` and `GITHUB_TOKEN` are visible to it.
- No new crate dependencies.

### What GitHub does with a private repo (tested 2026-11-17)

| Request | Anonymous | With token |
|---|---|---|
| `raw.githubusercontent.com/<o>/<r>/<ref>/<path>` | 404 | 200 (`Authorization` header) |
| `github.com/<o>/<r>/releases/download/<tag>/<name>` | 404 | 404 |
| `github.com/<o>/<r>/releases/latest/download/<name>` | 404 | 404 |
| `api.github.com/repos/<o>/<r>/releases/tags/<tag>` and `/releases/latest` | 404 | 200, lists the assets with their ids |
| `api.github.com/repos/<o>/<r>/releases/assets/<id>`, `Accept: application/octet-stream` | 404 | 302 to a signed `release-assets.githubusercontent.com` URL that downloads **without** a token |

The release workflow already publishes `index-entry.toml` (a valid one-entry index) on every
release, and `releases/latest/download/index-entry.toml` works for a public repo.

## Approaches

**How the developer points wai at the index** (owner chose **B**)

- A: document a line to paste into `settings.toml`. No code, but every developer edits a file.
- **B: an Add index… button** on the Extensions page that accepts `owner/repo` or a URL and writes
  the setting. One step, discoverable, small UI.
- C: the project repo suggests the index (`.wai/extensions.toml`) and wai asks to add it. Best
  onboarding, but a consent flow; left for later.

**How a private package downloads** (my call: **P1**)

- **P1: wai translates** a `github.com/.../releases/download/...` URL into the API asset call when
  the anonymous GET is a 404. The index format and the release workflow stay as they are; public
  indexes keep working anonymously.
- P2: the workflow writes API asset URLs into `index-entry.toml`. Ties the index format to GitHub's
  API and breaks anonymous public downloads.

**Who keeps the private index current** (my call: **I3**)

- I1: the release workflow commits an `index.toml` to the repo. Needs a write token and commits.
- I2: someone edits an `index.toml` by hand after each release. Gets forgotten.
- **I3: the index is the latest release's `index-entry.toml`.** Every release already publishes it;
  nothing to maintain. Needs the release to be marked "latest" (see Workflow).

**Where the token comes from** (my call: **gh / env**)

- **`GH_TOKEN`, then `GITHUB_TOKEN`, then `gh auth token`.** Every WayCarbon developer already has
  `gh`; nothing new to configure.
- git's credential helper: on the owner's Mac it returns the personal account, not the one in the
  WayCarbon org.
- A token in wai's settings: a secret in a plain file wai would have to protect.

## Design

### 1. Index shorthand

A new `wai_ext::index::index_url(input: &str) -> Option<String>`:

- an `https_url` is returned as is;
- `owner/repo` (GitHub name characters: letters, digits, `-`, `_`, `.`; exactly one `/`; no
  leading/trailing space after trimming) becomes
  `https://github.com/<owner>/<repo>/releases/latest/download/index-entry.toml`;
- anything else is `None`.

`settings.toml` stores the **expanded URL**, so the settings format and its https-only validation do
not change, and a hand-edited setting means the same thing.

### 2. Add index… (Extensions page)

- A button **Add index…** beside **Refresh** in the Available group
  (`crates/wai/src/extensions_view.rs`, near the existing buttons at ~678–716).
- It opens a dialog like **Install from git URL…** (`window.open_dialog`, one `InputState`),
  placeholder `owner/repo or https://…`, and an **Add** button.
- Input that `index_url` rejects: the dialog stays open with a one-line error.
- A valid input: `write_change(user_dir, &Change::AddIndex(url))`, close the dialog, `refresh`.
  Load errors (no access, not found) appear where index errors already appear.
- `Change::AddIndex(String)` (new, `crates/wai-settings/src/edit.rs`): appends to
  `[extensions] indexes`, creating the table and array when missing; a URL already present is left
  alone (no duplicate). The rest of the file keeps its formatting.

```
Available                          [Add index…] [Refresh]
┌ Add an extension index ─────────────────────┐
│ [ waycarbon/wai-waycarbon                 ] │
│ A GitHub repo (owner/repo) or an https URL. │
│                           [Cancel]  [Add]   │
└─────────────────────────────────────────────┘
```

### 3. Where a listing came from

`Entry::from` today is the index's host. For a URL of the shorthand's shape
(`github.com/<o>/<r>/releases/...`), it is `<o>/<r>` instead, so the row reads
`… · from waycarbon/wai-waycarbon` rather than `from github.com`.

### 4. Authenticated GitHub fallback

In `marketplace::net` (the non-test module), for both `get_index` and `get_package`:

1. GET anonymously, as today.
2. If that fails with **HTTP 404** and the URL is one of:
   - `https://raw.githubusercontent.com/<o>/<r>/...`
   - `https://github.com/<o>/<r>/releases/download/<tag>/<name>`
   - `https://github.com/<o>/<r>/releases/latest/download/<name>`

   look up a token (section 5) and retry:
   - **raw**: the same URL with `Authorization: Bearer <token>`.
   - **release asset**: `GET https://api.github.com/repos/<o>/<r>/releases/tags/<tag>` (or
     `/releases/latest`) with `Authorization` and `Accept: application/vnd.github+json`; find the
     asset whose `name` is `<name>`; then `GET https://api.github.com/repos/<o>/<r>/releases/assets/<id>`
     with `Authorization` and `Accept: application/octet-stream`, and follow its redirect
     **without** `Authorization`.
3. Any other failure, or a URL of another shape: the error as today.

The same size and time limits apply as for the anonymous GET. The SHA-256 check on install is
unchanged, so a package fetched this way is verified like any other.

`download.rs` needs: a way to send extra headers on the **first request only** (redirect hops never
carry them), and the HTTP status of a failure readable by the caller (today it is only in the error
text, `HTTP {status}`).

### 5. Token

Looked up only when step 2 needs it, in this order: `GH_TOKEN`, `GITHUB_TOKEN`,
`gh auth token --hostname github.com` (stdin closed, stderr discarded, ~5 s timeout; a missing `gh`
or a non-zero exit means no token). Not cached on disk; never logged, never in an error message.

It is sent only to `api.github.com` and `raw.githubusercontent.com`, and only on the first request
of each GET.

`gh auth token` returns the **active** account's token. A developer whose active `gh` account is
not in the org gets the "not found" error below; switching accounts with `gh auth switch` fixes it.

### 6. Errors

When the fallback cannot help, the line under the index reads, for example:

- no token: `Could not load waycarbon/wai-waycarbon: not found. If it is private, sign in with gh auth login.`
- token, still 404: `Could not load waycarbon/wai-waycarbon: not found with your GitHub login (gh auth status shows which account).`

(`waycarbon/wai-waycarbon` is the shorthand label from section 3; other URLs show as today.)
Package download failures say the same in the install error.

### 7. Release workflow

- `.github/workflows/extension-release.yml` (shared, in this repo) gains an input
  `latest` (boolean, default `false`) that replaces the hard-coded `--latest=false`
  (`--latest=${{ inputs.latest }}`). Default stays `false`: wai's own repo releases the app and its
  updater reads `/releases/latest`.
- `waycarbon/wai-waycarbon`'s `release.yml` passes `latest: true`, so
  `releases/latest/download/index-entry.toml` always points at the newest version. (Today's
  `/releases/latest` already returns v0.6.0 because it is the only release; the second release
  needs the flag.)
- This repo's change to `extension-release.yml` is a wai workflow change (`chore:`); the private
  repo's change is in that repo.

### 8. Docs

- `docs/extensions.md`: publishing a private extension (a private repo, `latest: true`, how users
  add it).
- `docs/user-guide.md`: **Add index…**, the `owner/repo` shorthand, private repos and `gh`.
- `waycarbon/wai-waycarbon` README: install in three lines (`gh auth login`; Extensions → Add
  index… → `waycarbon/wai-waycarbon`; Install).

### 9. Rollout

1. Land the wai change and release it (it is a `feat:`).
2. Land the private repo's `latest: true`.
3. The owner adds `waycarbon/wai-waycarbon` on this Mac and installs from it, as the end-to-end
   check.

## Tests

Offline, like the existing ones:

- `index_url`: URL kept, shorthand expanded, bad input (`a`, `a/b/c`, `a b/c`, `http://…`) rejected.
- `Change::AddIndex`: creates the table, appends, skips a duplicate, keeps comments.
- The URL classification for step 2 (raw / release tag / release latest / other) and the label
  for section 3.
- Redirects drop the extra headers: the smallest check `download.rs` allows (for example a pure
  function that decides the headers for hop n).
- The UI: the existing extensions_view test with `indexes = [...]` keeps passing; one test that Add
  index with `owner/repo` writes the expanded URL.

Manual: section 9, step 3.

## Left to the builder

- How headers and the status reach callers in `download.rs` (new parameter, a small request type,
  a typed error).
- The JSON parsing of the API release (the crate already has `serde`/`serde_json` available through
  the workspace; use what is there).
- Dialog wording and layout details within the sketch above.
- Whether `gh auth token` runs per GET or once per Refresh.
- Percent-encoding a tag in the API path (tags here are `v<version>`).

`Entry::from` stays empty for the official index, as today.
