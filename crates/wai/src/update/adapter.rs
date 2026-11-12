//! The adapter check: one GET of an npm package's latest version, to tell when the `npx` command
//! pinned in the settings runs an older one. Only logged on failure; never installs anything.

use anyhow::{bail, ensure};

use super::{Fetched, fetch, newer, version};

/// The package and version of an `npx` command's `<pkg>@<version>` word (a scoped name keeps its
/// leading `@`); `None` for anything else.
pub fn pinned(command: &str) -> Option<(String, String)> {
    let mut words = command.split_whitespace();
    (words.next()? == "npx").then_some(())?;
    words.find_map(|word| {
        let (pkg, ver) = word.rsplit_once('@')?;
        (!pkg.is_empty() && version(ver).is_some()).then(|| (pkg.to_string(), ver.to_string()))
    })
}

fn valid_pkg(pkg: &str) -> bool {
    let name = pkg.strip_prefix('@').unwrap_or(pkg);
    !name.is_empty()
        && !name.contains("..")
        && name.matches('/').count() <= 1
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-'))
}

/// `Ok(Some(latest))` only when npm's latest is newer than `pinned`. `fetch` runs only for a
/// valid `pkg`.
pub fn check(
    pkg: &str,
    pinned: &str,
    fetch: impl FnOnce(&str) -> Fetched,
) -> anyhow::Result<Option<String>> {
    ensure!(valid_pkg(pkg), "not an npm package name: {pkg:?}");
    let (status, body) = fetch(&format!("https://registry.npmjs.org/{pkg}/latest"))?;
    ensure!(status == 200, "HTTP {status}");
    let value: serde_json::Value = serde_json::from_slice(&body)?;
    let Some(latest) = value.get("version").and_then(|v| v.as_str()) else {
        bail!("no version in the answer");
    };
    ensure!(version(latest).is_some(), "bad version {latest:?}");
    Ok(newer(pinned, latest).then(|| latest.to_string()))
}

/// `check` over the network.
pub fn run(pkg: &str, pinned: &str) -> anyhow::Result<Option<String>> {
    check(pkg, pinned, fetch)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PKG: &str = "@agentclientprotocol/claude-agent-acp";

    fn answer(status: u16, body: &str) -> impl FnOnce(&str) -> Fetched {
        let body = body.as_bytes().to_vec();
        move |_| Ok((status, body))
    }

    #[test]
    fn pinned_reads_the_npx_word() {
        let command = format!("npx -y {PKG}@0.103.0");
        assert_eq!(pinned(&command), Some((PKG.into(), "0.103.0".into())));
        for none in ["codex-acp", "npx foo", "npx foo@latest"] {
            assert_eq!(pinned(none), None, "{none}");
        }
    }

    #[test]
    fn adapter_check_handles_failures() {
        let ok = |body| check(PKG, "0.103.0", answer(200, body));
        assert_eq!(
            ok(r#"{"version":"0.104.0"}"#).unwrap(),
            Some("0.104.0".into())
        );
        for none in [
            r#"{"version":"0.103.0"}"#,
            r#"{"version":"0.102.0"}"#,
            r#"{"version":"0.104.0-beta.1"}"#,
        ] {
            assert_eq!(ok(none).unwrap(), None, "{none}");
        }
        for body in ["{}", r#"{"version":"x"}"#] {
            assert!(ok(body).is_err(), "{body}");
        }
        for status in [404, 429] {
            let got = check(PKG, "0.103.0", answer(status, r#"{"version":"0.104.0"}"#));
            assert!(got.is_err(), "{status}");
        }
        assert!(check(PKG, "0.103.0", |_| Err(anyhow::anyhow!("offline"))).is_err());
    }

    #[test]
    fn adapter_url_is_fixed() {
        let mut url = String::new();
        check(PKG, "0.103.0", |u| {
            url = u.to_string();
            Ok((404, Vec::new()))
        })
        .ok();
        assert_eq!(
            url,
            "https://registry.npmjs.org/@agentclientprotocol/claude-agent-acp/latest"
        );
        for bad in ["a/../b", "a b", "a/b/c", "@@a"] {
            let got = check(bad, "1.0.0", |_| panic!("fetched {bad}"));
            assert!(got.is_err(), "{bad}");
        }
    }
}
