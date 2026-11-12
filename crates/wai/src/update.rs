//! The opt-in update check: once per launch, when the user's `updates.check` is on and
//! `updates.repo` names a GitHub repository, one GET of its latest release. A newer wai is
//! announced; when this copy can install it, `run` also downloads the release's build for this OS
//! and `SHA256SUMS.txt`, checks the build against its listed hash and stages it in the cache.
//! Only `DEFAULT_REPO`'s releases are ever installed: a build is code that runs with the user's
//! rights, and a configurable repository is a configurable source of that code, so any other
//! repository is only announced. Every failure is only logged.

// ponytail: `run`, `Outcome`, `verify`, `cache_dir` and the download have no caller until the UI
// wiring (check_on_launch replaced); remove this allow then.
#![allow(dead_code)]

mod download;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, anyhow, bail, ensure};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::notification::Notification;
use gpui_kit::{AnyWindowHandle, App, AppContext as _};
use http_body_util::{BodyExt as _, Limited};
use hyper::header::{ACCEPT, USER_AGENT};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde::Deserialize;
use wai_ext::panel::cut;
use wai_settings::{DEFAULT_REPO, UpdateSettings};

const TIMEOUT: Duration = Duration::from_secs(5);
/// GitHub's answer is a few KB; one past this is not read.
const MAX_BODY: usize = 1024 * 1024;
/// Characters of a version the notice shows.
const MAX_SHOWN: usize = 32;

/// A fetch's HTTP status and body (at most `MAX_BODY` bytes).
type Fetched = anyhow::Result<(u16, Vec<u8>)>;

/// The build's file-name ending for this OS; nothing is installed where it is empty.
#[cfg(target_os = "macos")]
const ASSET_SUFFIX: &str = "_aarch64.app.zip";
#[cfg(windows)]
const ASSET_SUFFIX: &str = "_x64-setup.exe";
#[cfg(not(any(target_os = "macos", windows)))]
const ASSET_SUFFIX: &str = "";

/// The release asset that lists the SHA-256 of every build.
const SUMS_ASSET: &str = "SHA256SUMS.txt";
/// The sums file is a few lines.
const MAX_SUMS: u64 = 64 * 1024;

/// A file attached to a release.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Asset {
    pub name: String,
    #[serde(rename = "browser_download_url")]
    pub url: String,
    pub size: u64,
}

/// The latest release, when it is newer than this wai.
#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub assets: Vec<Asset>,
    /// Its tag: a semver version (`v` allowed), so ASCII `[0-9A-Za-z.+-]` only (see `version`).
    pub tag: String,
    /// `https://github.com/<repo>/releases/…`.
    pub page: String,
}

impl Release {
    /// `wai <version> is available`, the version clamped.
    pub fn text(&self) -> String {
        let version = self.tag.strip_prefix('v').unwrap_or(&self.tag);
        format!("wai {} is available", cut(version, MAX_SHOWN))
    }
}

/// Once per launch (`app::run`), off the main thread: a newer release shows a notice whose button
/// opens its page. A failure is only logged.
pub fn check_on_launch(settings: UpdateSettings, window: AnyWindowHandle, cx: &mut App) {
    let current = env!("CARGO_PKG_VERSION");
    let found = cx.background_spawn(async move {
        // A panic in a background task aborts wai: whatever the network sends, it stops here.
        let checked = std::panic::catch_unwind(|| check(&settings, current, fetch));
        checked.unwrap_or_else(|_| Err(anyhow!("panicked")))
    });
    cx.spawn(async move |cx| {
        let release = match found.await {
            Ok(Some(release)) => release,
            Ok(None) => return,
            Err(e) => return eprintln!("wai: update check failed: {}", logged(&e)),
        };
        let notice = Notification::info(release.text()).action(move |_, _, cx| {
            let (page, notice) = (release.page.clone(), cx.entity());
            Button::new("update-open")
                .label("Open release page")
                .on_click(move |_, window, cx| {
                    cx.open_url(&page);
                    notice.update(cx, |n, cx| n.dismiss(window, cx));
                })
        });
        let shown = window.update(cx, |_, window, cx| window.push_notification(notice, cx));
        shown.ok();
    })
    .detach();
}

/// `Ok(None)`: off, no repository set, or no newer release. `fetch` is called only when the check
/// is on and the repository is valid. `Err` is what to log.
pub fn check(
    settings: &UpdateSettings,
    current: &str,
    fetch: impl FnOnce(&str) -> Fetched,
) -> anyhow::Result<Option<Release>> {
    let (true, Some(repo)) = (settings.check, &settings.repo) else {
        return Ok(None);
    };
    ensure!(valid_repo(repo), "updates.repo is not owner/repo: {repo:?}");
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let (status, body) = fetch(&url)?;
    match status {
        200 => {}
        403 | 429 => bail!("HTTP {status} (rate limited?)"),
        _ => bail!("HTTP {status}"),
    }
    let release = parse_release(&body, repo)?;
    Ok(newer(current, &release.tag).then_some(release))
}

/// A downloaded build whose hash matched `SHA256SUMS.txt`, waiting in the cache.
#[derive(Debug, Clone, PartialEq)]
pub struct Staged {
    pub tag: String,
    pub file: PathBuf,
    /// Lowercase hex.
    pub sha256: String,
}

impl Staged {
    /// `wai <version> is ready`, the version clamped.
    pub fn text(&self) -> String {
        let version = self.tag.strip_prefix('v').unwrap_or(&self.tag);
        format!("wai {} is ready", cut(version, MAX_SHOWN))
    }
}

/// What a check-and-stage found.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Checking is off or no repository is set.
    Off,
    UpToDate,
    /// Newer, but this copy will not install it; `why` is the sentence Settings shows.
    Available {
        release: Release,
        why: Option<String>,
    },
    Ready(Staged),
}

/// Where builds are staged: wiped and refilled by each `run` that stages one.
pub fn cache_dir() -> PathBuf {
    let base = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
    base.join("wai-updates")
}

/// The first asset `want` accepts whose name is one plain file name (it is joined onto the cache)
/// and whose URL `download` may fetch.
fn asset(release: &Release, want: impl Fn(&str) -> bool) -> Option<&Asset> {
    let plain = |n: &str| {
        let ok = |c: char| c.is_ascii_alphanumeric() || "-_.+".contains(c);
        !n.is_empty() && n != "." && n != ".." && n.chars().all(ok)
    };
    let usable = |a: &&Asset| plain(&a.name) && download::allowed(&a.url) && want(&a.name);
    release.assets.iter().find(usable)
}

/// `check`, then, for a newer release this copy may install (`DEFAULT_REPO`, no `blocker`, a build
/// for this OS and the sums file): download both into `cache` and verify the build against the
/// sums. `download(url, dest)` writes `dest`; on any `Err` the cache holds no build.
pub fn run(
    settings: &UpdateSettings,
    current: &str,
    blocker: Option<String>,
    cache: &Path,
    fetch: impl FnOnce(&str) -> Fetched,
    download: impl Fn(&str, &Path) -> anyhow::Result<()>,
) -> anyhow::Result<Outcome> {
    let Some(release) = check(settings, current, fetch)? else {
        let asked = settings.check && settings.repo.is_some();
        return Ok(if asked {
            Outcome::UpToDate
        } else {
            Outcome::Off
        });
    };
    let repo = settings.repo.as_deref().unwrap_or_default();
    let why = match repo.eq_ignore_ascii_case(DEFAULT_REPO) {
        true => blocker,
        false => Some(format!(
            "wai installs updates only from {DEFAULT_REPO}, not {repo}"
        )),
    };
    if why.is_some() {
        return Ok(Outcome::Available { release, why });
    }
    let build = (!ASSET_SUFFIX.is_empty())
        .then(|| asset(&release, |n| n.ends_with(ASSET_SUFFIX)))
        .flatten();
    let sums = asset(&release, |n| n == SUMS_ASSET);
    let (Some(build), Some(sums)) = (build, sums) else {
        let why = format!("this release has no {ASSET_SUFFIX} build with {SUMS_ASSET}");
        return Ok(Outcome::Available {
            release,
            why: Some(why),
        });
    };
    let staged = stage(&release.tag, build, sums, cache, &download)?;
    Ok(Outcome::Ready(staged))
}

/// Empties `cache`, downloads the sums and the build into it, and keeps the build only when its
/// hash is the one listed.
fn stage(
    tag: &str,
    build: &Asset,
    sums: &Asset,
    cache: &Path,
    download: &impl Fn(&str, &Path) -> anyhow::Result<()>,
) -> anyhow::Result<Staged> {
    match std::fs::remove_dir_all(cache) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    std::fs::create_dir_all(cache)?;
    let sums_file = cache.join(SUMS_ASSET);
    download(&sums.url, &sums_file).context("downloading the checksums")?;
    ensure!(
        std::fs::metadata(&sums_file)?.len() <= MAX_SUMS,
        "the checksums file is too large"
    );
    let listed = std::fs::read_to_string(&sums_file).context("reading the checksums")?;
    let file = cache.join(&build.name);
    let staged = (|| {
        download(&build.url, &file).context("downloading the update")?;
        let sha256 = download::sha256_file(&file)?;
        let want = download::expected_sum(&listed, &build.name);
        ensure!(
            want.as_deref() == Some(&sha256),
            "the downloaded update does not match its checksum"
        );
        anyhow::Ok(sha256)
    })();
    match staged {
        Ok(sha256) => Ok(Staged {
            tag: tag.to_string(),
            file,
            sha256,
        }),
        Err(e) => {
            std::fs::remove_file(&file).ok();
            Err(e)
        }
    }
}

/// Re-hashes the staged build, so a file changed since staging is never installed.
pub fn verify(staged: &Staged) -> anyhow::Result<()> {
    let now = download::sha256_file(&staged.file).ok();
    ensure!(
        now.as_deref() == Some(&staged.sha256),
        "the downloaded update changed on disk; check for updates again"
    );
    Ok(())
}

/// The PowerShell one-liner that waits for wai (`pid`) to exit, then runs the NSIS installer
/// silently (`/S`) and relaunches wai (`/R`). Waiting lets wai quit cleanly (state saved, agents
/// stopped) instead of being killed by the installer. Pure so it is tested on every OS.
pub(crate) fn installer_script(setup: &Path, pid: u32) -> String {
    // PowerShell also reads the curly quotes U+2018..U+201B as single quotes; double each as itself.
    let setup: String = setup
        .display()
        .to_string()
        .chars()
        .flat_map(|c| {
            let n = if matches!(c, '\'' | '\u{2018}'..='\u{201B}') {
                2
            } else {
                1
            };
            std::iter::repeat_n(c, n)
        })
        .collect();
    format!(
        "Wait-Process -Id {pid} -ErrorAction SilentlyContinue; \
         Start-Process -FilePath '{setup}' -ArgumentList '/S','/R'"
    )
}

/// GitHub's `owner/repo`: one slash, letters, digits, `-`, `_` and `.`, no `.`/`..` part.
fn valid_repo(repo: &str) -> bool {
    let part = |p: &str| {
        let safe = |c: char| c.is_ascii_alphanumeric() || "-_.".contains(c);
        !p.is_empty() && p.len() <= 100 && p != "." && p != ".." && p.chars().all(safe)
    };
    repo.split_once('/')
        .is_some_and(|(owner, name)| part(owner) && part(name))
}

#[derive(Deserialize)]
struct Latest {
    tag_name: String,
    html_url: Option<String>,
    #[serde(default)]
    assets: Vec<Asset>,
}

/// The `/releases/latest` answer for `repo`: its tag, which must be a version, and its page when
/// that is one of `repo`'s releases (else the releases list).
fn parse_release(body: &[u8], repo: &str) -> anyhow::Result<Release> {
    let latest: Latest = serde_json::from_slice(body).context("unexpected answer")?;
    let tag = latest.tag_name;
    ensure!(
        version(&tag).is_some(),
        "not a version: {:?}",
        cut(&tag, MAX_SHOWN)
    );
    let releases = format!("https://github.com/{repo}/releases");
    let own = |url: &&String| {
        let rest = url
            .strip_prefix(&releases)
            .and_then(|r| r.strip_prefix('/'));
        let safe = |c: char| c.is_ascii_alphanumeric() || "-_./+".contains(c);
        url.len() <= 300 && rest.is_some_and(|r| r.chars().all(safe) && !r.contains(".."))
    };
    let page = latest.html_url.as_ref().filter(own).cloned();
    Ok(Release {
        tag,
        page: page.unwrap_or(releases),
        assets: latest.assets,
    })
}

/// A semver version's ordering key: `major.minor.patch`, whether it is a release (above a
/// pre-release), then the pre-release's dot-separated parts (numbers below words).
type Version<'a> = ([u64; 3], bool, Vec<(bool, u64, &'a str)>);

/// `tag` as a `Version`; `v` and build metadata (`+…`) are allowed, the metadata ignored. `None`
/// for anything else, so a tag that parses is ASCII `[0-9A-Za-z.+-]` only.
pub(crate) fn version(tag: &str) -> Option<Version<'_>> {
    let tag = tag.strip_prefix('v').unwrap_or(tag);
    let (tag, build) = match tag.split_once('+') {
        Some((tag, build)) => (tag, Some(build)),
        None => (tag, None),
    };
    let (core, pre) = match tag.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (tag, None),
    };
    let number = |n: &str| match n.bytes().all(|b| b.is_ascii_digit()) {
        true => n.parse::<u64>().ok(),
        false => None,
    };
    let mut parts = core.split('.').map(number);
    let core = [parts.next()??, parts.next()??, parts.next()??];
    if parts.next().is_some() {
        return None;
    }
    // Pre-release and build identifiers: dot-separated, each non-empty `[0-9A-Za-z-]`.
    let id = |i: &str| !i.is_empty() && i.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if ![pre, build]
        .into_iter()
        .flatten()
        .all(|ids| ids.split('.').all(id))
    {
        return None;
    }
    let ids = pre.into_iter().flat_map(|pre| pre.split('.'));
    let key = ids.map(|i| match number(i) {
        Some(n) => (false, n, ""),
        None => (true, 0, i),
    });
    Some((core, pre.is_none(), key.collect()))
}

/// Semver: whether `latest` is newer than `current`. A pre-release counts only when `current` is
/// one.
fn newer(current: &str, latest: &str) -> bool {
    let (Some(current), Some(latest)) = (version(current), version(latest)) else {
        return false;
    };
    let (current_is_release, latest_is_release) = (current.1, latest.1);
    if current_is_release && !latest_is_release {
        return false;
    }
    latest > current
}

/// Characters of a failure the log shows.
const MAX_LOGGED: usize = 200;

/// What a failed check logs: the error with its causes, clamped (serde quotes what it got).
fn logged(error: &anyhow::Error) -> String {
    cut(&format!("{error:#}"), MAX_LOGGED)
}

/// GET `url` over HTTPS: its status and at most `MAX_BODY` bytes, within `TIMEOUT` in all.
// ponytail: blocks one GPUI background thread for up to 5 s, once per launch. If launch work ever
// queues behind it, run it on its own `std::thread` and send the result back through a channel.
fn fetch(url: &str) -> Fetched {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    // The timer needs the runtime: it is made inside it.
    let got = runtime.block_on(async { tokio::time::timeout(TIMEOUT, get(url)).await });
    // A DNS lookup still blocked past the timeout must not hold this thread.
    runtime.shutdown_background();
    got.map_err(|_| anyhow!("no answer in {} s", TIMEOUT.as_secs()))?
}

async fn get(url: &str) -> Fetched {
    // ponytail: the native roots (keychain, schannel) load synchronously, so the 5 s timer cannot
    // cut that short. If it ever stalls, load them in `spawn_blocking` inside the timer.
    // ponytail: the crypto provider is the one rustls' features pick (ring, the only one built).
    // Should anything also enable aws-lc-rs, this panics (caught and logged); then make it explicit
    // with `with_provider_and_native_roots(rustls::crypto::ring::default_provider())` (rustls as a
    // direct dependency).
    // ponytail: no `HTTPS_PROXY`: behind a proxy the check logs a connect error. Upgrade:
    // hyper-util's `client-proxy` (`Tunnel`) built from the environment.
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()?
        .https_only()
        .enable_http1()
        .build();
    let client = Client::builder(TokioExecutor::new()).build(https);
    // GitHub refuses requests without a User-Agent.
    let request = hyper::Request::get(url)
        .header(USER_AGENT, concat!("wai/", env!("CARGO_PKG_VERSION")))
        .header(ACCEPT, "application/vnd.github+json")
        .body(String::new())?;
    // ponytail: redirects are not followed, so a renamed repository logs `HTTP 301`. Upgrade:
    // follow one `Location` on https://api.github.com, or have the user update `updates.repo`.
    let response = client.request(request).await?;
    let status = response.status().as_u16();
    let body = Limited::new(response.into_body(), MAX_BODY).collect().await;
    let body = body.map_err(|e| anyhow!("reading the answer: {e}"))?;
    Ok((status, body.to_bytes().to_vec()))
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;
    use download::sha256_file;
    use wai_settings::DEFAULT_REPO;

    fn on(repo: &str) -> UpdateSettings {
        UpdateSettings {
            check: true,
            repo: Some(repo.into()),
        }
    }

    fn body(tag: &str, html_url: &str) -> Vec<u8> {
        serde_json::json!({
            "url": "https://api.github.com/repos/owner/wai/releases/1",
            "html_url": html_url,
            "tag_name": tag,
            "name": "wai",
            "draft": false,
            "prerelease": false,
            "body": "Notes",
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn parses_latest_release() {
        let page = "https://github.com/owner/wai/releases/tag/v0.2.0";
        let release = parse_release(&body("v0.2.0", page), "owner/wai").unwrap();
        assert_eq!(
            release,
            Release {
                tag: "v0.2.0".into(),
                page: page.into(),
                assets: vec![],
            }
        );
        assert_eq!(release.text(), "wai 0.2.0 is available");

        // Only a page of the configured repository's releases opens; otherwise its releases list.
        let releases = "https://github.com/owner/wai/releases";
        for html_url in [
            "https://evil.example/owner/wai/releases/tag/v0.2.0",
            "https://github.com/owner/wai-evil/releases/tag/v0.2.0",
            "https://github.com/other/wai/releases/tag/v0.2.0",
            "http://github.com/owner/wai/releases/tag/v0.2.0",
            "https://github.com/owner/wai/releases/../../evil/x",
            "https://github.com/owner/wai/releases/tag/v0.2.0?x=\"><script>",
            "javascript:alert(1)",
        ] {
            let release = parse_release(&body("v0.2.0", html_url), "owner/wai").unwrap();
            assert_eq!(release.page, releases, "{html_url}");
        }
        let no_url = br#"{"tag_name": "0.2.0"}"#;
        let release = parse_release(no_url, "owner/wai").unwrap();
        assert_eq!(
            (release.tag.as_str(), release.page.as_str()),
            ("0.2.0", releases)
        );
    }

    #[test]
    fn compares_versions() {
        for (current, latest, want) in [
            ("0.1.0", "0.2.0", true),
            ("0.1.0", "v0.1.1", true),
            ("0.1.0", "1.0.0", true),
            ("0.9.0", "0.10.0", true),
            ("0.1.0", "0.1.0", false),
            ("0.1.0", "v0.1.0", false),
            ("0.2.0", "0.1.9", false),
            ("1.0.0", "0.99.99", false),
            // A pre-release is offered only to a pre-release.
            ("0.1.0", "0.2.0-beta.1", false),
            ("0.2.0-beta.1", "0.2.0-beta.2", true),
            ("0.2.0-beta.2", "0.2.0-beta.10", true),
            ("0.2.0-beta.2", "0.2.0-alpha", false),
            ("0.2.0-beta.1", "0.2.0", true),
            ("0.2.0-rc.1", "0.1.9", false),
            ("0.2.0-beta", "0.2.0-beta.1", true),
            ("0.2.0-1", "0.2.0-alpha", true),
            // Build metadata does not count.
            ("0.1.0", "0.1.0+build.5", false),
            ("0.1.0", "0.1.1+build.5", true),
            // Not versions.
            ("0.1.0", "", false),
            ("0.1.0", "latest", false),
            ("0.1.0", "1.2", false),
            ("0.1.0", "1.2.3.4", false),
            ("0.1.0", "+1.2.3", false),
            ("0.1.0", "1.+2.3", false),
            ("0.1.0", "1.2.3-", false),
            ("0.1.0", "1.2.3-a..b", false),
            ("0.1.0", "99999999999999999999999.0.0", false),
            ("0.1.0", "vv1.0.0", false),
            ("0.1.0", "1.0.0-\u{202e}gpj", false),
            // Build metadata is checked like a pre-release: the whole tag can be shown.
            ("0.1.0", "1.0.0+\u{202e}x", false),
            ("0.1.0", "1.0.0+a\nb", false),
            ("0.1.0", "1.0.0+", false),
            ("0.1.0", "1.0.0+a..b", false),
            ("0.1.0", "1.0.0+a+b", false),
            ("garbage", "1.0.0", false),
        ] {
            assert_eq!(newer(current, latest), want, "{current} → {latest}");
        }
    }

    /// Review Focus #2: offline, rate-limited, malformed or hostile answers end in a logged error
    /// and no notice; shown text is clamped.
    #[test]
    fn update_check_handles_failures() {
        let settings = on("owner/wai");
        let answer = |status: u16, body: Vec<u8>| move |_: &str| -> Fetched { Ok((status, body)) };
        let page = "https://github.com/owner/wai/releases/tag/v9.0.0";

        let offline = check(&settings, "0.1.0", |_| anyhow::bail!("dns error"));
        assert_eq!(offline.unwrap_err().to_string(), "dns error");
        for (status, want) in [
            (403, "HTTP 403 (rate limited?)"),
            (429, "HTTP 429 (rate limited?)"),
            (404, "HTTP 404"),
            (500, "HTTP 500"),
        ] {
            let failed = check(&settings, "0.1.0", answer(status, body("v9.0.0", page)));
            assert_eq!(failed.unwrap_err().to_string(), want);
        }
        for malformed in [
            b"".to_vec(),
            b"<html>".to_vec(),
            b"{\"tag_name\": ".to_vec(),
            br#"{"html_url": "https://github.com/owner/wai/releases/tag/v9.0.0"}"#.to_vec(),
            br#"{"tag_name": 9}"#.to_vec(),
            br#"[]"#.to_vec(),
            body("not a version", page),
            body(&format!("v9.0.0-{}", "\u{202e}".repeat(10)), page),
            body(&format!("{}.0.0", "9".repeat(10 * 1024)), page),
            body("v9.0.0+\u{202e}x", page),
            serde_json::to_vec(&"x".repeat(10 * 1024)).unwrap(),
        ] {
            let failed = check(&settings, "0.1.0", answer(200, malformed.clone()));
            let shown = String::from_utf8_lossy(&malformed);
            assert!(failed.is_err(), "{shown}");
            // What is logged: the whole chain (`{:#}`), clamped.
            let error = failed.unwrap_err();
            let (chain, log) = (format!("{error:#}"), logged(&error));
            assert!(chain.starts_with(log.trim_end_matches('…')), "{log}");
            assert!(
                log.chars().count() <= MAX_LOGGED + 1,
                "the log is clamped: {log}"
            );
        }
        // The 10 KB JSON string is in serde's error: the clamp is what keeps it out of the log.
        let string = check(
            &settings,
            "0.1.0",
            answer(200, serde_json::to_vec(&"x".repeat(10 * 1024)).unwrap()),
        );
        assert!(format!("{:#}", string.unwrap_err()).len() > 10 * 1024);

        // A 10 KB name is never shown; a 10 KB pre-release tag is clamped.
        let mut huge = serde_json::from_slice::<serde_json::Value>(&body("v9.0.0", page)).unwrap();
        huge["name"] = "x".repeat(10 * 1024).into();
        huge["body"] = "\u{202e}<script>".repeat(1024).into();
        let huge = huge.to_string().into_bytes();
        let release = check(&settings, "0.1.0", answer(200, huge))
            .unwrap()
            .unwrap();
        assert_eq!(release.text(), "wai 9.0.0 is available");
        let long = format!("v9.0.0-{}", "a".repeat(10 * 1024));
        let release = check(&settings, "0.1.0-beta", answer(200, body(&long, page)));
        let text = release.unwrap().unwrap().text();
        assert!(
            text.starts_with("wai 9.0.0-aaa") && text.ends_with("… is available"),
            "{text}"
        );
        assert!(text.chars().count() <= 60, "{text}");

        // Up to date: no notice, nothing to log.
        let current = check(&settings, "9.0.0", answer(200, body("v9.0.0", page)));
        assert_eq!(current.unwrap(), None);
    }

    #[test]
    fn disabled_means_no_request() {
        let never = |url: &str| -> Fetched { panic!("no request when off, got {url}") };
        let off = |repo: Option<&str>| UpdateSettings {
            check: false,
            repo: repo.map(Into::into),
        };
        assert_eq!(check(&off(None), "0.1.0", never).unwrap(), None);
        assert_eq!(
            check(&off(Some("owner/wai")), "0.1.0", never).unwrap(),
            None
        );
        let unset = UpdateSettings {
            check: true,
            repo: None,
        };
        assert_eq!(check(&unset, "0.1.0", never).unwrap(), None);
        // A slug that is not `owner/repo` never becomes a URL.
        for repo in [
            "",
            "owner",
            "owner/",
            "/wai",
            "owner/wai/x",
            "../wai",
            "owner/..",
            "owner/wai?x=1",
            "owner/wai#x",
            "owner wai/x",
            "evil.com:443/wai",
            "own\u{e9}r/wai",
        ] {
            let failed = check(&on(repo), "0.1.0", never);
            assert!(failed.is_err(), "{repo:?}");
        }

        let calls = Cell::new(0);
        let fetch = |url: &str| -> Fetched {
            calls.set(calls.get() + 1);
            assert_eq!(
                url,
                "https://api.github.com/repos/Owner-1/w.ai_x/releases/latest"
            );
            Ok((200, body("v0.1.0", "")))
        };
        assert_eq!(check(&on("Owner-1/w.ai_x"), "0.1.0", fetch).unwrap(), None);
        assert_eq!(calls.get(), 1);
    }

    const SUMS: &str = "SHA256SUMS.txt";

    fn asset_name() -> String {
        format!("wai_9.0.0{ASSET_SUFFIX}")
    }

    fn url_of(name: &str) -> String {
        format!("https://github.com/Guilbritto/wai/releases/download/v9.0.0/{name}")
    }

    fn with_assets(names: &[&str]) -> Vec<u8> {
        let assets: Vec<_> = names
            .iter()
            .map(|n| serde_json::json!({"name": n, "browser_download_url": url_of(n), "size": 7}))
            .collect();
        let mut v = serde_json::from_slice::<serde_json::Value>(&body("v9.0.0", "")).unwrap();
        v["assets"] = assets.into();
        v.to_string().into_bytes()
    }

    fn latest(names: &[&str]) -> impl FnOnce(&str) -> Fetched {
        let body = with_assets(names);
        move |_| Ok((200, body))
    }

    fn never_download(_: &str, _: &Path) -> anyhow::Result<()> {
        panic!("nothing may be downloaded")
    }

    #[test]
    fn parses_assets() {
        let release = parse_release(&with_assets(&["a.zip", "b.exe"]), "owner/wai").unwrap();
        assert_eq!(
            release.assets,
            vec![
                Asset {
                    name: "a.zip".into(),
                    url: url_of("a.zip"),
                    size: 7
                },
                Asset {
                    name: "b.exe".into(),
                    url: url_of("b.exe"),
                    size: 7
                },
            ]
        );
        let bare = parse_release(&body("v9.0.0", ""), "owner/wai").unwrap();
        assert!(bare.assets.is_empty());
    }

    #[test]
    fn only_the_default_repo_installs() {
        let cache = tempfile::tempdir().unwrap();
        let names = [asset_name(), SUMS.to_string()];
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let out = run(
            &on("other/wai"),
            "0.1.0",
            None,
            cache.path(),
            latest(&names),
            never_download,
        )
        .unwrap();
        let Outcome::Available { why: Some(why), .. } = out else {
            panic!("{out:?}")
        };
        assert!(why.contains("other/wai"), "{why}");

        #[cfg(any(target_os = "macos", windows))]
        {
            let calls = Cell::new(0);
            let download = |_: &str, _: &Path| {
                calls.set(calls.get() + 1);
                anyhow::bail!("stop")
            };
            let out = run(
                &on("guilbritto/WAI"),
                "0.1.0",
                None,
                cache.path(),
                latest(&names),
                download,
            );
            assert!(out.is_err());
            assert_eq!(calls.get(), 1);
        }
    }

    #[test]
    #[cfg(any(target_os = "macos", windows))]
    fn release_without_asset_only_notifies() {
        let cache = tempfile::tempdir().unwrap();
        let (asset, sums) = (asset_name(), SUMS);
        for names in [vec![sums], vec![asset.as_str()], vec!["other.zip", sums]] {
            let out = run(
                &on(DEFAULT_REPO),
                "0.1.0",
                None,
                cache.path(),
                latest(&names),
                never_download,
            )
            .unwrap();
            assert!(
                matches!(out, Outcome::Available { why: Some(_), .. }),
                "{out:?}"
            );
        }
    }

    #[test]
    fn blocked_copy_only_notifies() {
        let cache = tempfile::tempdir().unwrap();
        let names = [asset_name(), SUMS.to_string()];
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let out = run(
            &on(DEFAULT_REPO),
            "0.1.0",
            Some("x".into()),
            cache.path(),
            latest(&names),
            never_download,
        )
        .unwrap();
        let Outcome::Available { why, .. } = out else {
            panic!("{out:?}")
        };
        assert_eq!(why.as_deref(), Some("x"));
    }

    #[test]
    fn off_and_current_need_no_download() {
        let cache = tempfile::tempdir().unwrap();
        let off = UpdateSettings {
            check: false,
            repo: None,
        };
        let never = |_: &str| -> Fetched { panic!("no request when off") };
        let out = run(&off, "0.1.0", None, cache.path(), never, never_download);
        assert_eq!(out.unwrap(), Outcome::Off);
        let out = run(
            &on(DEFAULT_REPO),
            "9.0.0",
            None,
            cache.path(),
            latest(&[]),
            never_download,
        );
        assert_eq!(out.unwrap(), Outcome::UpToDate);
    }

    /// A `download` that serves `bytes` for the asset and `sums` for the sums file.
    #[cfg(any(target_os = "macos", windows))]
    fn serve<'a>(
        bytes: &'a [u8],
        sums: &'a str,
        seen: &'a RefCell<Vec<String>>,
    ) -> impl Fn(&str, &Path) -> anyhow::Result<()> + 'a {
        move |url, dest| {
            seen.borrow_mut().push(url.to_string());
            let body = if url.ends_with(SUMS) {
                sums.as_bytes()
            } else {
                bytes
            };
            std::fs::write(dest, body)?;
            Ok(())
        }
    }

    #[cfg(any(target_os = "macos", windows))]
    fn stage(cache: &Path, bytes: &[u8], sums: &str) -> anyhow::Result<Outcome> {
        let names = [asset_name(), SUMS.to_string()];
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let seen = RefCell::new(vec![]);
        let download = serve(bytes, sums, &seen);
        run(
            &on(DEFAULT_REPO),
            "0.1.0",
            None,
            cache,
            latest(&names),
            download,
        )
    }

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    #[cfg(any(target_os = "macos", windows))]
    fn stages_a_verified_asset() {
        let cache = tempfile::tempdir().unwrap();
        std::fs::write(cache.path().join("old.zip"), b"old").unwrap();
        let sums = format!("{ABC}  {}\n", asset_name());
        let out = stage(cache.path(), b"abc", &sums).unwrap();
        let Outcome::Ready(staged) = out else {
            panic!("{out:?}")
        };
        assert_eq!(staged.tag, "v9.0.0");
        assert_eq!(staged.sha256, ABC);
        assert_eq!(staged.file, cache.path().join(asset_name()));
        assert_eq!(staged.text(), "wai 9.0.0 is ready");
        assert!(!cache.path().join("old.zip").exists());
        verify(&staged).unwrap();
    }

    #[test]
    #[cfg(any(target_os = "macos", windows))]
    fn rejects_a_wrong_checksum() {
        let cache = tempfile::tempdir().unwrap();
        let sums = format!("{}  {}\n", "0".repeat(64), asset_name());
        let err = stage(cache.path(), b"abc", &sums).unwrap_err();
        assert!(format!("{err:#}").contains("checksum"), "{err:#}");
        assert!(!cache.path().join(asset_name()).exists());
        // No line for the asset at all is refused the same way.
        let err = stage(cache.path(), b"abc", "").unwrap_err();
        assert!(format!("{err:#}").contains("checksum"), "{err:#}");
        assert!(!cache.path().join(asset_name()).exists());
    }

    #[test]
    fn staged_file_is_hashed_again() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("wai.zip");
        std::fs::write(&file, b"abc").unwrap();
        let staged = Staged {
            tag: "v9.0.0".into(),
            file: file.clone(),
            sha256: sha256_file(&file).unwrap(),
        };
        verify(&staged).unwrap();
        std::fs::write(&file, b"abd").unwrap();
        let err = verify(&staged).unwrap_err();
        assert_eq!(
            err.to_string(),
            "the downloaded update changed on disk; check for updates again"
        );
        std::fs::write(&file, b"ab").unwrap();
        assert!(verify(&staged).is_err());
        std::fs::remove_file(&file).unwrap();
        assert!(verify(&staged).is_err());
    }

    /// The real fetch, without the network: a plain-HTTP URL is refused before any connection
    /// (and the 5 s timer runs inside its runtime instead of panicking).
    #[test]
    fn fetch_is_https_only() {
        let refused = format!("{:#}", fetch("http://127.0.0.1:9/").unwrap_err());
        assert!(refused.ends_with("unsupported scheme http"), "{refused}");
    }

    #[test]
    fn installer_script_quotes_the_path() {
        let setup = Path::new(r"C:\Users\o'neil\AppData\Local\wai-updates\wai_1_x64-setup.exe");
        assert_eq!(
            installer_script(setup, 42),
            r"Wait-Process -Id 42 -ErrorAction SilentlyContinue; Start-Process -FilePath 'C:\Users\o''neil\AppData\Local\wai-updates\wai_1_x64-setup.exe' -ArgumentList '/S','/R'"
        );
        let curly = Path::new("C:\\Users\\O\u{2019}Neil\\x\u{2018}\u{201A}\u{201B}.exe");
        assert!(installer_script(curly, 1).contains("'C:\\Users\\O\u{2019}\u{2019}Neil\\x\u{2018}\u{2018}\u{201A}\u{201A}\u{201B}\u{201B}.exe'"));
    }
}
