# wai: the extension marketplace

> 2026-11-13. Choices marked **(owner)** are the owner's; **(call)** are mine. No extension API change: the manifest and the protocol stay as they are (API 1.3; the usage branch's 1.4 is independent).

## Intent

Owner: *"precisamos pensar em como vamos montar o marketplace das extensões. Podemos ter dentro do repositório do wai um arquivo indexador onde iremos buscar algumas extensões, e o usuário pode colocar mais links."* And: *"Qualquer um pode publicar a extensão, eu posso fazer a minha e depois publicar, qualquer um pode fazer e publicar"*, *"precisamos documentar isso para que todos saibam como fazer"*.

The usage design deferred exactly this: *"Distribution of the extension's binary (installing from git without a local build): the next design"*.

## Today (from the code)

- **Install** (`crates/wai-ext/src/install.rs`): a folder is copied, or a git repo cloned with depth 1, into `<config>/extensions/.staging/<n>/ext`; links, special files, more than 100 MB or 10,000 entries are refused; the manifest is parsed; the consent dialog (`crates/wai/src/extensions_view.rs`) then moves it to `<config>/extensions/<id>/`. Installing an installed id is an update.
- **Nothing is built.** `extensions/*/bin/` is in `.gitignore`, so installing `wai-waycarbon` from its git URL gives a manifest whose `bin/wai-waycarbon` does not exist.
- **Downloads** exist for the updater (`crates/wai/src/update/download.rs`): a streamed HTTPS GET with redirects, a size cap and SHA-256 helpers, restricted to GitHub's release hosts.
- **Windows:** `std::process::Command` appends `.exe` to a relative program path with no extension, so `command = "bin/x"` runs `bin/x.exe`.

## Goals

1. **An official index** in this repo, `extensions/index.toml`, that wai reads at runtime (no wai release needed to list a new extension) **(owner)**.
2. **More indexes** the user adds (`[extensions] indexes = [...]` in their settings) **(owner: "o usuário pode colocar mais links")**.
3. **Prebuilt packages**, one per platform, downloaded and checked against the SHA-256 the index pins, then installed through the existing consent flow **(owner, from my proposal)**.
4. **Anyone can publish** **(owner)**: binaries live in the author's own GitHub Releases; an entry reaches the official index through a pull request **(owner, the Zed model)**.
5. **A reusable release workflow** so a Go author writes no packaging code, and **a CI check** on index pull requests, so the reviewer only decides whether to trust the author.
6. **Documentation** for authors (publishing) and users (installing, adding indexes) **(owner)**.

## Non-goals

- A hosted marketplace service, accounts, ratings, download counts.
- Building on the user's machine (needs toolchains; `build.rs`, `postinstall` run code before consent).
- Signatures beyond the index's pinned SHA-256 (the index is the trust root; minisign later if needed).
- Automatic extension updates: the list offers **Update**, the user clicks it.
- Search and filters in the list (add when the index outgrows a screen).
- A UI to edit the index list (settings file only, like `updates.repo`).
- Project suggestions by index id (`.wai/extensions.toml` keeps folder/git sources).
- Reusable workflows for languages other than Go (the package format is documented; manifest-only extensions are covered).

## Anti-goals

- A package that installs something other than what the index pinned (the SHA-256 must match before anything is unpacked).
- A listing that installs a different extension than it names (the package's manifest id must equal the listing's id).
- Unpacking outside the staging folder, links, devices, or a decompression bomb filling the disk.
- A project's `.wai/settings.toml` adding an index.
- First-party extension releases breaking wai's updater: the updater reads `/releases/latest`, so extension releases in this repo are made with `--latest=false`.

## Approaches considered

1. **Index + author-published prebuilt packages (chosen, owner).** Index entries pin a URL and SHA-256 per platform; authors publish with a reusable workflow. Language-neutral; no build infrastructure here.
2. **Index + this repo's CI builds every extension (Zed).** Reproducible, but this CI would need every language's toolchain; Zed can do it only because everything is WASM.
3. **Build locally on install.** Needs Go/Rust/Node on every user's machine and runs build code before consent. Refused.

## Design

### The index

TOML, fetched over HTTPS. Unknown fields are ignored (newer indexes must not break older wai); an entry that does not validate is skipped and logged, the rest are listed.

```toml
[[extensions]]
id = "wai-rotinas"                 # a valid extension id
name = "Rotinas"
version = "0.1.0"                  # shown; compared to the installed version by equality
description = "Scheduled prompts"  # optional
repository = "https://github.com/Guilbritto/wai"   # optional: where the source is

[extensions.packages.darwin-arm64]
url = "https://github.com/…/releases/download/wai-rotinas-v0.1.0/wai-rotinas-0.1.0-darwin-arm64.tar.gz"
sha256 = "<64 hex>"
```

- **Platforms:** `darwin-arm64`, `darwin-x64`, `linux-x64`, `linux-arm64`, `windows-x64`, `windows-arm64`, and `any` for a package that runs everywhere (manifest-only, or a script run by a program on PATH). wai picks its own platform, else `any`; an entry with neither shows **Not available for this platform**.
- **Validation per entry:** id valid; name and version non-empty; every URL `https://`; every `sha256` 64 hex characters; platform keys from the list (unknown keys ignored, for future platforms).
- **Sources, in order:** the official index `https://raw.githubusercontent.com/Guilbritto/wai/main/extensions/index.toml`, then the user's `extensions.indexes`. An id already listed by an earlier index is skipped **(call)**: a user index cannot shadow an official entry.
- **Limits:** an index body over 1 MB is refused; 10 s per index.

### The package

- A **`.tar.gz`** whose root holds `wai-extension.toml` (a leading `./` is accepted). zip is not accepted: it loses the executable bit.
- Only regular files and folders. Paths must stay inside (no absolute paths, no `..`). Duplicates are refused.
- At most 100 MB downloaded, and 100 MB and 10,000 entries unpacked, counted while unpacking.
- On macOS and Linux a file with any executable bit in the archive is installed `0755`, others `0644`.
- On Windows the program is `<command>.exe`.

### Installing from a listing

1. **Download** the platform's URL into the staging area: HTTPS only, any host, at most 5 redirects (each HTTPS), 10 min, 100 MB.
2. **Stage** `Source::Package { url, sha256, archive }` in `wai-ext`: the archive's SHA-256 must equal the pinned one, then it is unpacked into `<staging>/<n>/ext` with the rules above, and the manifest parsed. The download is deleted.
3. **Check** that the manifest id equals the listing's id (`expect`, as project suggestions already do).
4. **Consent** as today, its source line `From <package URL>`. An installed id is an update.

### The Extensions page

Below **INSTALLED**, an **AVAILABLE** group with a **Refresh** button:
- `Loading extensions…` while fetching; one muted line per index that failed (`Could not load <url>: <error>`).
- One row per listing: name, version badge, description (one line), `from <host>` when it comes from a user index, and **Install**, **Update to <version>** (installed, other version) or **Installed** (same version, disabled), or **Not available for this platform**.
- Fetched the first time the page renders and on Refresh. Installing a listing goes through `ExtStore::stage_package` (download into the staging area, then `Source::Package`).
```
AVAILABLE 3                                     [Refresh]
Rotinas  0.1.0   Scheduled prompts                [Install]
WayCarbon 0.2.0  Jira "My cards" panel   [Update to 0.2.0]
Navegador 0.1.0  Playwright's MCP server         [Installed]
```

### Settings

```toml
[extensions]
indexes = ["https://example.com/wai-index.toml"]
```
User settings only: a project's `extensions` table is removed with a notice, like `updates`.

### Publishing

- **Reusable workflow** `.github/workflows/extension-release.yml` (`workflow_call`, input `path`): on a tag ending in `v<version>` equal to the manifest's version, it builds a Go extension for every platform (`CGO_ENABLED=0`, `-trimpath`), or one `any` package when the manifest has no `[process]`; packs `wai-extension.toml`, the program at `process.command` (`.exe` on Windows), and `README*`/`LICENSE*`; publishes a GitHub release (`--latest=false`) with the packages, `SHA256SUMS.txt` and `index-entry.toml`, the entry ready to paste.
- **An author's repo** adds a six-line caller workflow and pushes a tag.
- **First-party extensions** use it from `.github/workflows/extensions.yml`, on tags `<id>-v<version>` (`wai-rotinas-v0.1.0`): never `v*`, which semantic-release owns.
- **Index pull requests** run `.github/workflows/index.yml`: `wai-index-check` (a `wai-ext` binary) parses the index with wai's own parser, downloads every package with `curl`, and stages it with wai's own code (hash, unpack rules, manifest), then checks id and version against the entry.
- **Review (call):** the owner merges index PRs after the check passes, judging the author and the permissions; the consent dialog's warning still applies.

## My calls

- TOML for the index (the manifest and settings are TOML).
- First index wins on a duplicate id.
- Versions compared by equality (the manifest's version is free-form).
- `tar` crate (MIT/Apache, used by cargo) for unpacking with limits; `flate2` and `sha2` are already in `Cargo.lock`.
- The page fetches on first open and on Refresh, not at launch: no request until the user looks.
- The official index starts empty: entries need real releases, which need the owner to push tags.
