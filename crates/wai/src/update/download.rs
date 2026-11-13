//! The download half of self-updating: a streamed, size-capped HTTPS GET to a file, the hosts it
//! may be redirected to, and the checksum helper. Extension packages and indexes use the same GET
//! with their own hosts and limits (`marketplace`).

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, bail, ensure};
use http_body_util::BodyExt as _;
use hyper::header::{ACCEPT, LOCATION, USER_AGENT};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

/// The whole download, redirects included.
const TOTAL_TIMEOUT: Duration = Duration::from_secs(600);
/// Bytes a download may write; one past this fails.
const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
/// Where a release asset lives, and the only hosts a redirect may name.
const HOSTS: [&str; 3] = [
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// `url` is `https://` on one of `HOSTS` exactly: no userinfo, no port, no look-alike host.
pub(super) fn allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    HOSTS.iter().any(|h| authority.eq_ignore_ascii_case(h))
}

/// Where a GET may go and how much it may take.
#[derive(Clone, Copy)]
pub(crate) struct Limits {
    /// Checked on the URL and on every redirect.
    pub allowed: fn(&str) -> bool,
    /// Bytes it may write; one past this fails.
    pub max: u64,
    /// The whole GET, redirects included.
    pub timeout: Duration,
}

/// A release asset.
const RELEASE: Limits = Limits {
    allowed,
    max: MAX_DOWNLOAD,
    timeout: TOTAL_TIMEOUT,
};

/// GET a release asset into `dest` (created, or truncated). On any error `dest` is removed.
pub(super) fn download(url: &str, dest: &Path) -> anyhow::Result<()> {
    download_with(url, dest, RELEASE)
}

/// GET `url` into `dest` (created, or truncated) within `limits`. On any error `dest` is removed.
pub(crate) fn download_with(url: &str, dest: &Path, limits: Limits) -> anyhow::Result<()> {
    let result = File::create(dest)
        .map_err(Into::into)
        .and_then(|mut file| get_into(url, &mut file, limits));
    if result.is_err() {
        std::fs::remove_file(dest).ok();
    }
    result
}

/// GET `url` into `out` within `limits`.
// ponytail: blocks one GPUI background thread up to `limits.timeout`, with its own runtime like
// `fetch`. `out` is written with blocking calls between awaits; a slow disk stalls only this
// thread. Upgrade: tokio's `fs` feature if the runtime ever serves more than this one request.
pub(crate) fn get_into(url: &str, out: &mut impl Write, limits: Limits) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let got = runtime
        .block_on(async { tokio::time::timeout(limits.timeout, get(url, out, limits)).await });
    runtime.shutdown_background();
    got.map_err(|_| anyhow!("no download in {} s", limits.timeout.as_secs()))?
}

async fn get(url: &str, out: &mut impl Write, limits: Limits) -> anyhow::Result<()> {
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()?
        .https_only()
        .enable_http1()
        .build();
    let client = Client::builder(TokioExecutor::new()).build(https);
    let mut url = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        ensure!((limits.allowed)(&url), "refusing to download from {url:?}");
        let request = hyper::Request::get(&url)
            .header(USER_AGENT, concat!("wai/", env!("CARGO_PKG_VERSION")))
            .header(ACCEPT, "application/octet-stream")
            .body(String::new())?;
        let response = client.request(request).await?;
        let status = response.status().as_u16();
        match status {
            200 => return write(response.into_body(), out, limits.max).await,
            301 | 302 | 303 | 307 | 308 => {
                let to = response.headers().get(LOCATION);
                url = to
                    .and_then(|l| l.to_str().ok())
                    .ok_or_else(|| anyhow!("HTTP {status} without a Location"))?
                    .to_string();
            }
            _ => bail!("HTTP {status}"),
        }
    }
    bail!("too many redirects")
}

async fn write(
    mut body: hyper::body::Incoming,
    out: &mut impl Write,
    max: u64,
) -> anyhow::Result<()> {
    let mut written = 0u64;
    while let Some(frame) = body.frame().await {
        let Ok(data) = frame?.into_data() else {
            continue;
        };
        written += data.len() as u64;
        ensure!(
            written <= max,
            "the download is over {} MB",
            max / (1024 * 1024)
        );
        out.write_all(&data)?;
    }
    out.flush()?;
    Ok(())
}

/// The hash `sums` (`sha256sum` output: `hash  name` or `hash *name`) lists for exactly `name`,
/// lowercased.
pub(super) fn expected_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, rest) = line.split_once(char::is_whitespace)?;
        let listed = rest.trim_start();
        let listed = listed.strip_prefix('*').unwrap_or(listed);
        (listed == name).then(|| hash.to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_sum_formats() {
        let name = "wai_1_x64-setup.exe";
        assert_eq!(
            expected_sum("ABC  wai_1_x64-setup.exe\n", name),
            Some("abc".into())
        );
        assert_eq!(
            expected_sum("abc *wai_1_x64-setup.exe", name),
            Some("abc".into())
        );
        assert_eq!(expected_sum("abc  other.exe\n", name), None);
        assert_eq!(expected_sum("abc  wai_1_x64-setup.exe.sig", name), None);
        assert_eq!(
            expected_sum("1  a\n2  wai_1_x64-setup.exe\n", name),
            Some("2".into())
        );
    }

    #[test]
    fn allowed_hosts() {
        for url in [
            "https://github.com/Guilbritto/wai/releases/download/v1/x",
            "https://objects.githubusercontent.com/github-production/x?y=1",
            "https://release-assets.githubusercontent.com/x",
        ] {
            assert!(allowed(url), "{url}");
        }
        for url in [
            "http://github.com/Guilbritto/wai/releases/download/v1/x",
            "https://github.com.evil.example/x",
            "https://evil.example/?github.com",
            "https://user@github.com/x",
            "https://github.com:444/x",
            "https://github.com\\@evil.example/x",
            "https://evilgithub.com/x",
            "ftp://github.com/x",
            "",
        ] {
            assert!(!allowed(url), "{url}");
        }
    }
}
