//! Private GitHub indexes and packages: an anonymous GET that ends in a 404 on a GitHub URL is
//! tried again with the user's GitHub login (`GH_TOKEN`, `GITHUB_TOKEN`, then `gh auth token`).
//! The token goes to `api.github.com` and `raw.githubusercontent.com` only, on the first request
//! of each GET: a release asset's signed redirect is followed without it.

#[cfg(not(test))]
use std::io::Write;

#[cfg(not(test))]
use crate::update::download::{HttpStatus, Limits, get_into};

/// A GitHub URL that can be fetched again with a token.
#[derive(Debug, PartialEq)]
pub(crate) enum Gh<'a> {
    /// `raw.githubusercontent.com`: the same URL, with the token.
    Raw,
    /// A release asset, through the API; `tag` `None` is the latest release.
    Release {
        owner: &'a str,
        repo: &'a str,
        tag: Option<&'a str>,
        name: &'a str,
    },
}

pub(crate) fn classify(url: &str) -> Option<Gh<'_>> {
    if url.contains(['?', '#']) {
        return None;
    }
    if let Some(rest) = url.strip_prefix("https://raw.githubusercontent.com/") {
        return (!rest.is_empty()).then_some(Gh::Raw);
    }
    let parts: Vec<&str> = url
        .strip_prefix("https://github.com/")?
        .split('/')
        .collect();
    if parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let [owner, repo, "releases", a, b, name] = parts[..] else {
        return None;
    };
    let tag = match (a, b) {
        ("download", tag) => Some(tag),
        ("latest", "download") => None,
        _ => return None,
    };
    Some(Gh::Release {
        owner,
        repo,
        tag,
        name,
    })
}

/// `owner/repo` for a release URL: what the Extensions page shows for its index.
pub(crate) fn repo_label(url: &str) -> Option<String> {
    match classify(url)? {
        Gh::Release { owner, repo, .. } => Some(format!("{owner}/{repo}")),
        Gh::Raw => None,
    }
}

/// The id of the asset called `name` in an API release.
pub(crate) fn asset_id(json: &str, name: &str) -> Option<u64> {
    let release: serde_json::Value = serde_json::from_str(json).ok()?;
    let assets = release["assets"].as_array()?;
    assets.iter().find(|a| a["name"] == name)?["id"].as_u64()
}

#[cfg(not(test))]
const NOT_SIGNED_IN: &str = "not found. If it is private, sign in with gh auth login.";
#[cfg(not(test))]
const NO_ACCESS: &str = "not found with your GitHub login (gh auth status shows which account).";

/// GET `url`, which `classify`d as `gh`, with the user's GitHub login.
#[cfg(not(test))]
pub(crate) fn get_private(
    url: &str,
    gh: Gh,
    out: &mut impl Write,
    limits: Limits,
) -> anyhow::Result<()> {
    let token = token().ok_or_else(|| anyhow::anyhow!(NOT_SIGNED_IN))?;
    let auth = format!("Bearer {token}");
    let auth = ("authorization", auth.as_str());
    let not_found = |e: anyhow::Error| match e.downcast_ref::<HttpStatus>() {
        Some(HttpStatus(404)) => anyhow::anyhow!(NO_ACCESS),
        _ => e,
    };
    let Gh::Release {
        owner,
        repo,
        tag,
        name,
    } = gh
    else {
        return get_into(url, &[auth], out, limits).map_err(not_found);
    };
    let api = format!("https://api.github.com/repos/{owner}/{repo}/releases");
    let release = match tag {
        Some(tag) => format!("{api}/tags/{tag}"),
        None => format!("{api}/latest"),
    };
    let mut json = Vec::new();
    let json_limits = Limits {
        max: 1024 * 1024,
        ..limits
    };
    let accept = ("accept", "application/vnd.github+json");
    get_into(&release, &[auth, accept], &mut json, json_limits).map_err(not_found)?;
    let id = asset_id(&String::from_utf8_lossy(&json), name)
        .ok_or_else(|| anyhow::anyhow!("the release has no {name}"))?;
    let accept = ("accept", "application/octet-stream");
    get_into(&format!("{api}/assets/{id}"), &[auth, accept], out, limits).map_err(not_found)
}

/// The user's GitHub token; never stored, logged or shown.
#[cfg(not(test))]
fn token() -> Option<String> {
    let set = |name| {
        let value = std::env::var(name).ok()?.trim().to_string();
        (!value.is_empty()).then_some(value)
    };
    set("GH_TOKEN").or_else(|| set("GITHUB_TOKEN")).or_else(|| {
        let mut gh = std::process::Command::new("gh");
        gh.args(["auth", "token", "--hostname", "github.com"])
            .stdin(std::process::Stdio::null());
        let five = std::time::Duration::from_secs(5);
        let (status, out, _) = crate::claude_plugins::run(gh, five).ok()?;
        let token = out.trim().to_string();
        (status.success() && !token.is_empty()).then_some(token)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_github_urls() {
        assert_eq!(
            classify("https://raw.githubusercontent.com/o/r/main/index.toml"),
            Some(Gh::Raw)
        );
        assert_eq!(
            classify("https://github.com/o/r/releases/download/v1.0/a.tar.gz"),
            Some(Gh::Release {
                owner: "o",
                repo: "r",
                tag: Some("v1.0"),
                name: "a.tar.gz"
            })
        );
        assert_eq!(
            classify("https://github.com/o/r/releases/latest/download/index-entry.toml"),
            Some(Gh::Release {
                owner: "o",
                repo: "r",
                tag: None,
                name: "index-entry.toml"
            })
        );
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
        assert_eq!(
            repo_label("https://github.com/o/r/releases/latest/download/x").as_deref(),
            Some("o/r")
        );
        assert_eq!(
            repo_label("https://raw.githubusercontent.com/o/r/main/i.toml"),
            None
        );
    }

    #[test]
    fn finds_the_asset_id() {
        let json = r#"{"tag_name":"v1","assets":[{"id":1,"name":"a"},{"id":22,"name":"index-entry.toml"}]}"#;
        assert_eq!(asset_id(json, "index-entry.toml"), Some(22));
        assert_eq!(asset_id(json, "b"), None);
        assert_eq!(asset_id("not json", "a"), None);
    }
}
