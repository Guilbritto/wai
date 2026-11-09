//! The opt-in update check: once per launch, when the user's `updates.check` is on and
//! `updates.repo` names a GitHub repository, one GET of its latest release. A newer wai shows a
//! notice whose button opens the release page; nothing is ever downloaded. Every failure is only
//! logged.

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
use wai_settings::UpdateSettings;

const TIMEOUT: Duration = Duration::from_secs(5);
/// GitHub's answer is a few KB; one past this is not read.
const MAX_BODY: usize = 1024 * 1024;
/// Characters of a version the notice shows.
const MAX_SHOWN: usize = 32;

/// A fetch's HTTP status and body (at most `MAX_BODY` bytes).
type Fetched = anyhow::Result<(u16, Vec<u8>)>;

/// The latest release, when it is newer than this wai.
#[derive(Debug, PartialEq)]
pub struct Release {
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
    })
}

/// A semver version's ordering key: `major.minor.patch`, whether it is a release (above a
/// pre-release), then the pre-release's dot-separated parts (numbers below words).
type Version<'a> = ([u64; 3], bool, Vec<(bool, u64, &'a str)>);

/// `tag` as a `Version`; `v` and build metadata (`+…`) are allowed, the metadata ignored. `None`
/// for anything else, so a tag that parses is ASCII `[0-9A-Za-z.+-]` only.
fn version(tag: &str) -> Option<Version<'_>> {
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
    use std::cell::Cell;

    use super::*;

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
                page: page.into()
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
        assert_eq!(
            check(&UpdateSettings::default(), "0.1.0", never).unwrap(),
            None
        );
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

    /// The real fetch, without the network: a plain-HTTP URL is refused before any connection
    /// (and the 5 s timer runs inside its runtime instead of panicking).
    #[test]
    fn fetch_is_https_only() {
        let refused = format!("{:#}", fetch("http://127.0.0.1:9/").unwrap_err());
        assert!(refused.ends_with("unsupported scheme http"), "{refused}");
    }
}
