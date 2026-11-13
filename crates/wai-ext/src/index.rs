//! Extension indexes: TOML files listing extensions and a prebuilt package per platform, each
//! pinned by its SHA-256. wai reads them over HTTPS; installing a listing downloads its package
//! and stages it as a `Source::Package`.
//!
//! Every wai version reads the same official index, so unknown fields are ignored and an entry
//! that does not validate is skipped (with a notice), never the whole index.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::ExtId;

/// The platforms a package can be built for; `any` runs everywhere (no program of its own, or a
/// program found on PATH).
pub const PLATFORMS: [&str; 7] = [
    "darwin-arm64",
    "darwin-x64",
    "linux-x64",
    "linux-arm64",
    "windows-x64",
    "windows-arm64",
    "any",
];

/// This build's platform, as an index names it; `None` on one no package targets.
pub fn platform() -> Option<&'static str> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "windows",
        _ => return None,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        _ => return None,
    };
    let name = format!("{os}-{arch}");
    PLATFORMS.into_iter().find(|p| *p == name)
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Package {
    pub url: String,
    /// Lowercase hex.
    pub sha256: String,
}

/// One extension an index lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub id: ExtId,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    /// Where its source is: `https://` only, else dropped.
    pub repository: Option<String>,
    /// By platform, only those in `PLATFORMS`.
    pub packages: BTreeMap<String, Package>,
}

impl Listing {
    /// The package for this build: its platform's, else `any`.
    pub fn package(&self) -> Option<&Package> {
        self.package_for(platform())
    }

    pub fn package_for(&self, platform: Option<&str>) -> Option<&Package> {
        (platform.and_then(|p| self.packages.get(p))).or_else(|| self.packages.get("any"))
    }
}

#[derive(Deserialize)]
struct RawIndex {
    #[serde(default)]
    extensions: Vec<toml::Value>,
}

#[derive(Deserialize)]
struct RawListing {
    id: String,
    name: String,
    version: String,
    description: Option<String>,
    repository: Option<String>,
    #[serde(default)]
    packages: BTreeMap<String, toml::Value>,
}

/// An `https://` URL with a host and no whitespace or control characters.
pub fn https_url(url: &str) -> bool {
    https_host(url).is_some()
}

/// The host (with any port) of a URL `https_url` accepts.
pub fn https_host(url: &str) -> Option<&str> {
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let authority = url
        .strip_prefix("https://")?
        .split(['/', '?', '#'])
        .next()?;
    (!authority.is_empty()).then_some(authority)
}

fn sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn listing(value: toml::Value) -> Result<Listing, String> {
    let raw = RawListing::deserialize(value).map_err(|e| e.message().to_string())?;
    let id = ExtId::new(&raw.id).map_err(|e| e.to_string())?;
    let named = |what: &str, s: &str| match s.trim().is_empty() {
        true => Err(format!("{}: empty {what}", raw.id)),
        false => Ok(()),
    };
    named("name", &raw.name)?;
    named("version", &raw.version)?;
    let mut packages = BTreeMap::new();
    for (platform, value) in raw.packages {
        // A platform this wai does not know may be one a newer wai does.
        if !PLATFORMS.contains(&platform.as_str()) {
            continue;
        }
        let package = Package::deserialize(value)
            .map_err(|e| format!("{} {platform}: {}", raw.id, e.message()))?;
        if !https_url(&package.url) {
            return Err(format!("{} {platform}: the URL must be https://", raw.id));
        }
        if !sha256_hex(&package.sha256) {
            return Err(format!(
                "{} {platform}: sha256 must be 64 hex digits",
                raw.id
            ));
        }
        let sha256 = package.sha256.to_ascii_lowercase();
        packages.insert(platform, Package { sha256, ..package });
    }
    if packages.is_empty() {
        return Err(format!("{}: no package for a known platform", raw.id));
    }
    Ok(Listing {
        id,
        name: raw.name,
        version: raw.version,
        description: raw.description,
        repository: raw.repository.filter(|r| https_url(r)),
        packages,
    })
}

/// The listings in an index's text, and a notice per entry it skipped. Only text that is not a
/// TOML table with an `extensions` array of tables fails.
pub fn parse_index(text: &str) -> Result<(Vec<Listing>, Vec<String>), String> {
    let index: RawIndex = toml::from_str(text).map_err(|e| e.message().to_string())?;
    let (mut listings, mut skipped) = (Vec::new(), Vec::new());
    for (n, value) in index.extensions.into_iter().enumerate() {
        match listing(value) {
            Ok(l) => listings.push(l),
            Err(e) => skipped.push(format!("entry {}: {e}", n + 1)),
        }
    }
    Ok((listings, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn entry(id: &str, packages: &str) -> String {
        format!(
            "[[extensions]]\nid = \"{id}\"\nname = \"N\"\nversion = \"1.0.0\"\nfuture = 1\n{packages}"
        )
    }

    fn package(platform: &str, url: &str, sha: &str) -> String {
        format!("[extensions.packages.{platform}]\nurl = \"{url}\"\nsha256 = \"{sha}\"\n")
    }

    #[test]
    fn parses_listings_and_skips_bad_entries() {
        let good = package("darwin-arm64", "https://example.com/a.tar.gz", SHA)
            + &package("riscv-os", "anything", "x");
        let text = [
            entry("good", &good),
            entry("BAD id", &package("any", "https://e.com/a", SHA)),
            entry("http", &package("any", "http://e.com/a", SHA)),
            entry("short-sha", &package("any", "https://e.com/a", "abc")),
            entry("none", &package("riscv-os", "https://e.com/a", SHA)),
            "[[extensions]]\nid = \"typo\"\nname = 3\nversion = \"1\"\n".into(),
        ]
        .concat();
        let (listings, skipped) = parse_index(&text).unwrap();
        assert_eq!(listings.len(), 1, "{skipped:?}");
        assert_eq!(skipped.len(), 5, "{skipped:?}");
        let l = &listings[0];
        assert_eq!((l.id.as_str(), l.version.as_str()), ("good", "1.0.0"));
        assert_eq!(l.packages.keys().collect::<Vec<_>>(), ["darwin-arm64"]);
        assert_eq!(skipped[1], "entry 3: http any: the URL must be https://");
    }

    #[test]
    fn upper_case_hashes_are_lowered() {
        let text = entry("a", &package("any", "https://e.com/a", &SHA.to_uppercase()));
        let (listings, _) = parse_index(&text).unwrap();
        assert_eq!(listings[0].packages["any"].sha256, SHA);
    }

    #[test]
    fn picks_the_platform_then_any() {
        let text = entry(
            "a",
            &(package("darwin-arm64", "https://e.com/mac", SHA)
                + &package("any", "https://e.com/any", SHA)),
        );
        let (listings, _) = parse_index(&text).unwrap();
        let l = &listings[0];
        let url = |p| l.package_for(p).map(|p| p.url.as_str());
        assert_eq!(url(Some("darwin-arm64")), Some("https://e.com/mac"));
        assert_eq!(url(Some("windows-x64")), Some("https://e.com/any"));
        assert_eq!(url(None), Some("https://e.com/any"));
    }

    #[test]
    fn an_empty_or_broken_index() {
        assert_eq!(parse_index("").unwrap().0, vec![]);
        assert!(parse_index("extensions = 3").is_err());
        assert!(parse_index("not toml [").is_err());
    }

    #[test]
    fn https_urls() {
        assert!(https_url(
            "https://github.com/x/y/releases/download/v1/a.tar.gz"
        ));
        assert_eq!(
            https_host("https://e.com:8443/i.toml?x"),
            Some("e.com:8443")
        );
        for bad in [
            "http://e.com/a",
            "https://",
            "https:///a",
            "https://e.com/a b",
            "e.com",
        ] {
            assert!(!https_url(bad), "{bad}");
        }
    }

    #[test]
    fn this_platform_is_known() {
        #[cfg(any(
            all(target_os = "macos", target_arch = "aarch64"),
            all(windows, target_arch = "x86_64")
        ))]
        assert!(platform().is_some());
    }
}
