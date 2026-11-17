# Private Extension Indexes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A WayCarbon developer adds `waycarbon/wai-waycarbon` once with **Add index…** and installs/updates the private extension with one click, using their `gh` login.

**Architecture:** `owner/repo` expands to the latest release's `index-entry.toml` URL and is appended to the user's `settings.toml`. Index and package GETs stay anonymous; a 404 on a GitHub URL retries with the user's token (raw: same URL; release asset: through the GitHub API, whose signed redirect is followed without the token). The shared release workflow gains a `latest` input so the private repo's newest release is "latest".

**Tech Stack:** Rust (gpui / gpui_kit, hyper 1, toml_edit, serde_json), GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-11-17-private-extension-indexes-design.md`

## Global Constraints

- The token goes only to `api.github.com` and `raw.githubusercontent.com`, only on the first request of a GET; never on a redirect hop, never in logs, errors or the UI.
- Token order: `GH_TOKEN`, then `GITHUB_TOKEN` (an empty value counts as unset), then `gh auth token --hostname github.com` (stdin closed, killed after 5 s). wai stores no token.
- Indexes stay `https://` only and user-settings only; `settings.toml` stores the expanded URL.
- Shorthand: `owner/repo` → `https://github.com/<owner>/<repo>/releases/latest/download/index-entry.toml`.
- Error copy (verbatim): no token → `not found. If it is private, sign in with gh auth login.`; token, still 404 → `not found with your GitHub login (gh auth status shows which account).`
- No new crate dependencies. Tests never touch the network (`https://test.invalid/<path>` in tests).
- Commits: `feat:`/`fix:`/`chore:`/`docs:`, no scope, no Co-Authored-By. Workflow and extension-repo changes are `chore:`.
- Gate per task: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and the task's tests.
- Another session edits `extensions/wai-bichinho/` in the main checkout. Work in a worktree: `git worktree add ../wai-private-indexes -b feat/private-extension-indexes development`, and build with `CARGO_TARGET_DIR=/Users/guilherme.brito/Project/Guilherme/wai/target` (the disk is nearly full).

## Review Focus

1. **A public index or package that is a real 404** (a typo'd URL on any host) must fail as today, with no token lookup and no `gh` spawn. Only GitHub-shaped URLs fall back. Task 3 pins this with `classify` returning `None` for non-GitHub URLs.
2. **A redirect after an authenticated request** (API asset → `release-assets.githubusercontent.com`) must not carry `Authorization`. Task 2 pins this with `headers_go_on_the_first_request_only`.
3. **Pasted input with spaces or a trailing slash** (` waycarbon/wai-waycarbon `, `waycarbon/wai-waycarbon/`) should be accepted after trimming, or rejected. It must never be written as a broken URL. Task 1 pins this in `index_url_shorthand`.
4. **Adding the same index twice** must not duplicate it in `settings.toml`. Task 1 pins this in `add_index_appends_once`.
5. **A `settings.toml` that does not parse** must be left alone and the user told. `write_change` already refuses it; Task 4's `add_index` pushes the warning, the same as `Workspace::write_setting`.

---

### Task 1: The shorthand and the settings write

**Files:**
- Modify: `crates/wai-ext/src/index.rs` (next to `https_url`, ~line 92; tests in `mod tests`, ~169)
- Modify: `crates/wai-settings/src/edit.rs` (`enum Change`, `write_change`'s `match`; tests in `mod tests`)

**Interfaces:**
- Produces: `wai_ext::index::index_url(input: &str) -> Option<String>`. Produces `wai_settings::Change::AddIndex(String)`, already re-exported as `wai_settings::Change`.

- [ ] **Step 1: Set up the worktree** (see Global Constraints), then write the failing tests.

```rust
// crates/wai-ext/src/index.rs, mod tests
#[test]
fn index_url_shorthand() {
    let latest = "https://github.com/waycarbon/wai-waycarbon/releases/latest/download/index-entry.toml";
    assert_eq!(index_url("waycarbon/wai-waycarbon").as_deref(), Some(latest));
    assert_eq!(index_url("  waycarbon/wai-waycarbon \n").as_deref(), Some(latest));
    assert_eq!(index_url("a.b/c_d-1").as_deref(),
        Some("https://github.com/a.b/c_d-1/releases/latest/download/index-entry.toml"));
    assert_eq!(index_url("https://example.com/i.toml").as_deref(), Some("https://example.com/i.toml"));
    for bad in ["", "a", "a/", "/b", "a/b/", "a/b/c", "a b/c", "http://x/i.toml", "a/b?c", "../b"] {
        assert_eq!(index_url(bad), None, "{bad:?}");
    }
}
```

```rust
// crates/wai-settings/src/edit.rs, mod tests
#[test]
fn add_index_appends_once() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("settings.toml");
    std::fs::write(&p, "# mine\ntheme = \"light\"\n").unwrap();
    write_change(d.path(), &Change::AddIndex("https://a/i.toml".into())).unwrap();
    write_change(d.path(), &Change::AddIndex("https://b/i.toml".into())).unwrap();
    write_change(d.path(), &Change::AddIndex("https://a/i.toml".into())).unwrap();
    assert!(std::fs::read_to_string(&p).unwrap().starts_with("# mine\ntheme = \"light\"\n"));
    let (s, _) = Settings::load(d.path(), None).unwrap();
    assert_eq!(s.extensions.indexes, ["https://a/i.toml", "https://b/i.toml"]);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p wai-ext index_url_shorthand; cargo test -p wai-settings add_index_appends_once`
Expected: compile errors (`index_url` / `AddIndex` not found).

- [ ] **Step 3: Implement**

`index_url`: trim the input. If it is an `https_url`, return it as is. If it is exactly two non-empty segments split by one `/`, each segment only ASCII letters, digits, `-`, `_`, `.`, and neither segment `.` or `..`, return the shorthand URL. Otherwise return `None`.

`Change::AddIndex(url)`: get `table(root, "extensions")`, then its `indexes` as a `toml_edit::Array`. Create the array when it is missing; replace it when it is not an array. Push `url` unless an equal string is already there.

- [ ] **Step 4: Run them to see them pass**

Run: `cargo test -p wai-ext; cargo test -p wai-settings`
Expected: PASS (the old tests too).

- [ ] **Step 5: Commit**

```bash
git add crates/wai-ext/src/index.rs crates/wai-settings/src/edit.rs
git commit -m "feat: owner/repo index shorthand and a settings change that adds an index"
```

---

### Task 2: Extra headers on the first request, a readable HTTP status

**Files:**
- Modify: `crates/wai/src/update/download.rs` (`get_into`, `get`, `download_with`; tests)
- Modify: `crates/wai/src/marketplace.rs:73` (the `get_into` call gains `&[]`)

**Interfaces:**
- Produces:
  - `pub(crate) struct HttpStatus(pub u16)` in `crate::update::download`. It implements `Display` as `HTTP {0}` and `std::error::Error`. `get` returns it (`Err(HttpStatus(status).into())`) where it used to `bail!("HTTP {status}")`, so the error text stays the same.
  - `pub(crate) fn get_into(url: &str, headers: &[(&str, &str)], out: &mut impl Write, limits: Limits) -> anyhow::Result<()>`.
  - `headers` go on the first request only, and each replaces a default header of the same name (`HeaderMap::insert`, not `append`).
  - An `authorization` value is marked sensitive.
  - `download_with` keeps its signature and passes `&[]`.

- [ ] **Step 1: Write the failing test**

```rust
// crates/wai/src/update/download.rs, mod tests
#[test]
fn headers_go_on_the_first_request_only() {
    let extra = [("authorization", "Bearer t"), ("accept", "application/vnd.github+json")];
    let first = request("https://api.github.com/x", 0, &extra).unwrap();
    assert_eq!(first.headers()["authorization"], "Bearer t");
    assert!(first.headers()["authorization"].is_sensitive());
    assert_eq!(first.headers().get_all("accept").iter().count(), 1);
    assert_eq!(first.headers()["accept"], "application/vnd.github+json");
    let hop = request("https://release-assets.githubusercontent.com/x", 1, &extra).unwrap();
    assert!(hop.headers().get("authorization").is_none());
    assert_eq!(hop.headers()["accept"], "application/octet-stream");
    assert_eq!(anyhow::Error::from(HttpStatus(404)).to_string(), "HTTP 404");
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p wai headers_go_on_the_first_request_only`
Expected: compile error (`request` / `HttpStatus` not found).

- [ ] **Step 3: Implement**

Add `fn request(url: &str, hop: usize, headers: &[(&str, &str)]) -> anyhow::Result<hyper::Request<String>>`. It builds today's request (User-Agent, `Accept: application/octet-stream`, empty body). When `hop == 0`, it then inserts `headers`. `get` loops `for hop in 0..=MAX_REDIRECTS` and calls `request(&url, hop, headers)`. `get_into` and `get` take `headers` through to it.

- [ ] **Step 4: Run it to see it pass**

Run: `cargo test -p wai download`
Expected: PASS (`expected_sum_formats` and `allowed_hosts` too).

- [ ] **Step 5: Commit**

```bash
git add crates/wai/src/update/download.rs crates/wai/src/marketplace.rs
git commit -m "feat: send extra headers on a download's first request only"
```

---

### Task 3: The GitHub fallback for indexes and packages

**Files:**
- Create: `crates/wai/src/marketplace/github.rs`
- Modify: `crates/wai/src/marketplace.rs` (`mod github;`, non-test `mod net`, `fetch_all`'s `from` and error label)
- Modify: `crates/wai/src/claude_plugins.rs:83` (`fn run` → `pub(crate) fn run`, reused for `gh`)

**Interfaces:**
- Consumes: `get_into(url, headers, out, limits)`, `HttpStatus`, `Limits` (Task 2). Consumes `claude_plugins::run(cmd: Command, timeout: Duration) -> Result<(ExitStatus, String, String), String>`.
- Produces in `marketplace::github`:
  - `#[derive(Debug, PartialEq)] pub(crate) enum Gh<'a> { Raw, Release { owner: &'a str, repo: &'a str, tag: Option<&'a str>, name: &'a str } }`. A `tag` of `None` means the latest release.
  - `pub(crate) fn classify(url: &str) -> Option<Gh<'_>>`
  - `pub(crate) fn repo_label(url: &str) -> Option<String>`: `"<owner>/<repo>"` for a `Release`.
  - `pub(crate) fn asset_id(json: &str, name: &str) -> Option<u64>`: the `id` of the entry in `assets` whose `name` matches.
  - `#[cfg(not(test))] pub(crate) fn get_private(url: &str, gh: Gh, out: &mut impl Write, limits: Limits) -> anyhow::Result<()>`
  - `#[cfg(not(test))] fn token() -> Option<String>`
- `fetch_all`'s error line becomes `Could not load {label}: {e:#}`, where `label = repo_label(url)` or else `url`. Task 4's test relies on that.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/wai/src/marketplace/github.rs, mod tests
#[test]
fn classifies_github_urls() {
    assert_eq!(classify("https://raw.githubusercontent.com/o/r/main/index.toml"), Some(Gh::Raw));
    assert_eq!(classify("https://github.com/o/r/releases/download/v1.0/a.tar.gz"),
        Some(Gh::Release { owner: "o", repo: "r", tag: Some("v1.0"), name: "a.tar.gz" }));
    assert_eq!(classify("https://github.com/o/r/releases/latest/download/index-entry.toml"),
        Some(Gh::Release { owner: "o", repo: "r", tag: None, name: "index-entry.toml" }));
    for url in [
        "https://github.com/o/r",
        "https://github.com/o/r/archive/main.zip",
        "https://github.com/o/r/releases/download/v1/a?x=1",
        "https://github.com/o/r/releases/download/v1/a/b",
        "https://github.com.evil.example/o/r/releases/download/v1/a",
        "https://example.com/o/r/releases/download/v1/a",
        "https://raw.githubusercontent.com/",
        "http://github.com/o/r/releases/download/v1/a",
    ] {
        assert_eq!(classify(url), None, "{url}");
    }
    assert_eq!(repo_label("https://github.com/o/r/releases/latest/download/x").as_deref(), Some("o/r"));
    assert_eq!(repo_label("https://raw.githubusercontent.com/o/r/main/i.toml"), None);
}

#[test]
fn finds_the_asset_id() {
    let json = r#"{"tag_name":"v1","assets":[{"id":1,"name":"a"},{"id":22,"name":"index-entry.toml"}]}"#;
    assert_eq!(asset_id(json, "index-entry.toml"), Some(22));
    assert_eq!(asset_id(json, "b"), None);
    assert_eq!(asset_id("not json", "a"), None);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p wai marketplace::github`
Expected: compile error (module missing).

- [ ] **Step 3: Implement `github.rs`**

- `classify`. Two cases match:
  - `https://raw.githubusercontent.com/<non-empty>` is `Raw`.
  - `https://github.com/` followed by exactly six `/`-separated non-empty segments is a `Release`: either `o/r/releases/download/<tag>/<name>` or `o/r/releases/latest/download/<name>`.

  Any `?` or `#` in the URL means `None`. Hosts match exactly; reuse no host-suffix logic.
- `asset_id`: parse with `serde_json::Value`.
- `token()`: check `GH_TOKEN`, then `GITHUB_TOKEN`; a non-empty, trimmed value wins. Otherwise call `claude_plugins::run` with `gh auth token --hostname github.com`, `stdin(Stdio::null())` and a 5 s timeout. Exit 0 with non-empty trimmed stdout gives the token; anything else gives `None`. Never log it.
- `get_private`:
  1. With no token: `bail!` with the no-token copy.
  2. Let `auth = format!("Bearer {token}")`. Map any `HttpStatus(404)` below to the "still 404" copy.
  3. `Raw`: `get_into(url, &[("authorization", &auth)], out, limits)`.
  4. `Release`:
     - GET `https://api.github.com/repos/{owner}/{repo}/releases/tags/{tag}`, or `…/releases/latest` when `tag` is `None`. Send `authorization` and `accept: application/vnd.github+json`. Write into a `Vec`, with `Limits { max: 1024 * 1024, ..limits }`.
     - Look up `asset_id(name)`. If there is none, `bail!("the release has no {name}")`.
     - GET `https://api.github.com/repos/{owner}/{repo}/releases/assets/{id}` with `authorization` and `accept: application/octet-stream` into `out`. Task 2 already drops the headers on the redirect.

- [ ] **Step 4: Wire it into `marketplace.rs`**

In the non-test `mod net`, add `fn get(url: &str, out: &mut impl Write, limits: Limits) -> anyhow::Result<()>`:

```rust
match get_into(url, &[], out, limits) {
    Err(e) if e.downcast_ref::<HttpStatus>().is_some_and(|s| s.0 == 404) => match github::classify(url) {
        Some(gh) => github::get_private(url, gh, out, limits),
        None => Err(e),
    },
    other => other,
}
```

A 404 writes nothing to `out`, so the retry can reuse it.

- `get_index` reads through `get`.
- `get_package` creates the file, writes through `get`, and removes the file on error, as `download_with` does.
- `fetch_all`:
  - `from` for a user index becomes `github::repo_label(url)`, or else `https_host(url)`, or else `url`.
  - The error line uses the label (see Interfaces).

- [ ] **Step 5: Run the tests**

Run: `cargo test -p wai marketplace; cargo test -p wai extensions_view`
Expected: PASS, including `installs_and_updates_from_an_index`.

- [ ] **Step 6: Check the real fallback by hand** (network, not a test)

Run a scratch `#[test] #[ignore]` (not committed), or a one-off `cargo run`, to call `net::get_index` on `https://github.com/waycarbon/wai-waycarbon/releases/latest/download/index-entry.toml`, with the work account active in `gh`.

Expected: the text starts with `[[extensions]]` and contains `id = "wai-waycarbon"`. With `GH_TOKEN=bad`, the error is the "still 404" copy. Delete the scratch code.

- [ ] **Step 7: Commit**

```bash
git add crates/wai/src/marketplace.rs crates/wai/src/marketplace/github.rs crates/wai/src/claude_plugins.rs
git commit -m "feat: read private GitHub indexes and packages with the gh login"
```

---

### Task 4: Add index… on the Extensions page

**Files:**
- Modify: `crates/wai/src/extensions_view.rs`:
  - the view struct (~99) and `new` (~112);
  - a new `add_index_dialog` / `add_index` (near `refresh`, ~380);
  - the AVAILABLE header (~709, beside `ext-refresh`);
  - tests (near `installs_and_updates_from_an_index`, ~1180).
- Modify: `docs/user-guide.md` (Extensions → **Indexes.**, ~522–536; and the summary line ~372)

**Interfaces:**
- Consumes: `index_url` and `Change::AddIndex` (Task 1). Consumes `wai_settings::write_change(user_dir: &Path, change: &Change)`. The view's `config_dir` is the user dir: `Workspace::extensions` passes `self.user_dir`.
- Produces, as element ids for tests:
  - `ext-add-index`: the button beside Refresh, `ghost().xsmall()`, label `Add index…`.
  - `index-add`: the dialog's primary button `Add`, disabled while `index_url(input)` is `None`.
  - `index-cancel`.
  - The dialog's input is the view field `index: Entity<InputState>`, placeholder `owner/repo or https://…`.

- [ ] **Step 1: Write the failing test**

```rust
// crates/wai/src/extensions_view.rs, tests
/// Add index writes the expanded shorthand to the user's settings once and reads the indexes
/// again; an invalid input writes nothing.
#[gpui_kit::test]
fn adds_an_index(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    std::fs::create_dir_all(&config).unwrap();
    let (workspace, cx) = workspace(&config, cx);
    let view = cx.update(|window, cx| workspace.update(cx, |w, cx| w.show_extensions(window, cx)));
    let type_in = |text: &str, cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            let input = view.read(cx).index.clone();
            input.update(cx, |s, cx| s.set_value(text, window, cx));
        });
    };
    click("ext-add-index", cx);
    type_in("not a repo", cx);
    click("index-add", cx);
    assert!(!config.join("settings.toml").exists(), "an invalid index is not written");
    type_in("waycarbon/wai-waycarbon", cx);
    click("index-add", cx);
    let (s, _) = Settings::load(&config, None).unwrap();
    assert_eq!(s.extensions.indexes,
        ["https://github.com/waycarbon/wai-waycarbon/releases/latest/download/index-entry.toml"]);
    let errors = view.read_with(cx, |v, _| match &v.available {
        super::Available::Loaded(l) => l.errors.clone(),
        _ => vec![],
    });
    assert!(errors.iter().any(|e| e.starts_with("Could not load waycarbon/wai-waycarbon:")), "{errors:?}");
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p wai adds_an_index`
Expected: compile error (no `index` field).

- [ ] **Step 3: Implement**

- `fn add_index_dialog(&mut self, window, cx)` opens the dialog with `window.open_dialog`, following the consent dialog's pattern (~245).
  - Title: `Add an extension index`.
  - Body: `Input::new(&index)`, then a muted `text_xs` line, `A GitHub repo (owner/repo) or an https URL.`
  - Footer: `index-cancel` and `index-add`. `index-add` reads the input on each render to set `disabled`.
  - On click, `index-add` closes the dialog and calls `add_index`.
- `fn add_index(&mut self, input: &str, window, cx)`:
  - Return if `index_url(input)` is `None`.
  - Call `write_change(&self.config_dir, &Change::AddIndex(url))`.
  - On `Err`, push `Notification::warning(format!("Could not save settings: {e}"))`.
  - On `Ok`, clear the input and call `self.refresh(cx)`.

- [ ] **Step 4: Run it to see it pass**

Run: `cargo test -p wai extensions_view`
Expected: PASS.

- [ ] **Step 5: Update `docs/user-guide.md`**

- In the **Indexes.** paragraph, lead with **Add index…**: type `owner/repo` (the latest release's `index-entry.toml`) or an `https://` URL. It is saved to your `settings.toml`, and the `indexes = [...]` example stays.
- Add a bullet: a private GitHub repo loads with your `gh` login (`GH_TOKEN`, `GITHUB_TOKEN`, or `gh auth token`). The account must have access; `gh auth switch` changes it.
- Its listings show `from owner/repo`.
- Add **Add index…** to the summary line (~372).

- [ ] **Step 6: Commit**

```bash
git add crates/wai/src/extensions_view.rs docs/user-guide.md
git commit -m "feat: add an extension index from the Extensions page"
```

---

### Task 5: The release workflow's `latest` input

**Files:**
- Modify: `.github/workflows/extension-release.yml` (`inputs`, ~20; the header comment; `Publish the release`, ~143–148)
- Modify: `docs/extensions.md` (**Your own index**, ~790–792)

- [ ] **Step 1: Add the input**

```yaml
      latest:
        description: Mark the release as the repository's latest (a private index reads the latest release's index-entry.toml)
        type: boolean
        default: false
```

In the publish step, replace `--latest=false` with `--latest=${{ inputs.latest }}`. Keep the comment about wai's updater, reworded as the reason the default is `false`.

- [ ] **Step 2: Check the YAML**

Run: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/extension-release.yml'))" && grep -n "latest" .github/workflows/extension-release.yml`
Expected: no error. `--latest=${{ inputs.latest }}` appears once, and `--latest=false` does not appear.

- [ ] **Step 3: Update `docs/extensions.md`**

Under **Your own index**, add a short **A private index** part covering three points:
- Keep the extension in a private GitHub repo and release it with `with: latest: true`.
- Users add it with **Add index…** → `owner/repo`; their `gh` login needs access.
- Only people with access see it.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/extension-release.yml docs/extensions.md
git commit -m "chore: let an extension release mark itself latest"
```

---

### Task 6: Release and roll out (needs the owner's go-ahead at each push)

**Files:**
- Spec: `docs/superpowers/specs/2026-11-17-private-extension-indexes-design.md` and this plan, committed with the branch.
- In `waycarbon/wai-waycarbon` (a separate clone, pushed with the work account):
  - `.github/workflows/release.yml`: add `with:` / `latest: true`.
  - `README.md`: the install steps.

- [ ] **Step 1: Run the whole gate**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all pass.

- [ ] **Step 2: Commit the spec and plan**

```bash
git add docs/superpowers/specs/2026-11-17-private-extension-indexes-design.md docs/superpowers/plans/2026-11-17-private-extension-indexes.md
git commit -m "docs: private extension indexes design and plan"
```

- [ ] **Step 3: Release wai**, following CLAUDE.md "Release". Ask the owner first.
  - Run `git log --oneline -5 development`.
  - Merge the branch into `development`.
  - `git branch -f main development`, only when `main` is an ancestor.
  - Push both.
  - Expected: the `release` workflow publishes a new minor version.

- [ ] **Step 4: Update the private repo**. Do it after Step 3 lands on `main`: the reusable workflow is read from `@main`, and an unknown input fails the call.
  - `release.yml` gets `with:` / `latest: true`.
  - `README.md` gets three lines: `gh auth login` with an account in the waycarbon org; wai → Extensions → **Add index…** → `waycarbon/wai-waycarbon`; **Install**.
  - Commit as `chore:` and push to `main` with the work account's credential.

- [ ] **Step 5: End-to-end on this Mac**
  - Once the installed wai has updated to the new release (or a local build, per CLAUDE.md "Install a local build"), go to Extensions → **Add index…** → `waycarbon/wai-waycarbon`.
  - Expected: `WayCarbon` lists `… · from waycarbon/wai-waycarbon`, and **Install** shows the consent dialog and installs 0.6.0.
  - Remove the worktree afterwards: `git worktree remove ../wai-private-indexes`.
