//! The download half of self-updating: a streamed, size-capped HTTPS GET to a file, the hosts it
//! may be redirected to, and the checksum helpers.

use std::fs::File;
use std::io::{Read as _, Write as _};
use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, bail, ensure};
use http_body_util::BodyExt as _;
use hyper::header::{ACCEPT, LOCATION, USER_AGENT};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use sha2::{Digest as _, Sha256};

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

/// GET `url` into `dest` (created, or truncated). On any error `dest` is removed.
// ponytail: blocks one GPUI background thread for up to 10 min, with its own runtime like `fetch`.
// The file is written with blocking `std::fs` calls between awaits; a slow disk stalls only this
// thread. Upgrade: tokio's `fs` feature if the runtime ever serves more than this one request.
pub(super) fn download(url: &str, dest: &Path) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let got = runtime.block_on(async { tokio::time::timeout(TOTAL_TIMEOUT, get(url, dest)).await });
    runtime.shutdown_background();
    let got = got.map_err(|_| anyhow!("no download in {} s", TOTAL_TIMEOUT.as_secs()));
    let result = got.and_then(|r| r);
    if result.is_err() {
        std::fs::remove_file(dest).ok();
    }
    result
}

async fn get(url: &str, dest: &Path) -> anyhow::Result<()> {
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()?
        .https_only()
        .enable_http1()
        .build();
    let client = Client::builder(TokioExecutor::new()).build(https);
    let mut url = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        ensure!(allowed(&url), "refusing to download from {url:?}");
        let request = hyper::Request::get(&url)
            .header(USER_AGENT, concat!("wai/", env!("CARGO_PKG_VERSION")))
            .header(ACCEPT, "application/octet-stream")
            .body(String::new())?;
        let response = client.request(request).await?;
        let status = response.status().as_u16();
        match status {
            200 => return write(response.into_body(), dest).await,
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

async fn write(mut body: hyper::body::Incoming, dest: &Path) -> anyhow::Result<()> {
    let mut file = File::create(dest)?;
    let mut written = 0u64;
    while let Some(frame) = body.frame().await {
        let Ok(data) = frame?.into_data() else {
            continue;
        };
        written += data.len() as u64;
        ensure!(written <= MAX_DOWNLOAD, "the download is over 512 MB");
        file.write_all(&data)?;
    }
    file.flush()?;
    Ok(())
}

/// SHA-256 of the file, lowercase hex.
pub(super) fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        hash.update(&chunk[..n]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
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

    #[test]
    fn hashes_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f");
        std::fs::write(&file, b"abc").unwrap();
        assert_eq!(
            sha256_file(&file).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
