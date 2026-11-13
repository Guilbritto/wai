//! What the Extensions page offers to install: the official index, then the user's own
//! (`extensions.indexes`), merged. Packages download with the updater's GET.

use std::path::Path;

use wai_ext::index::{Listing, https_host, parse_index};

/// The index in this repository, read by every wai.
pub const OFFICIAL_INDEX: &str =
    "https://raw.githubusercontent.com/Guilbritto/wai/main/extensions/index.toml";

/// One listing and, when a user's index lists it, that index's host.
#[derive(Debug, Clone)]
pub struct Entry {
    pub listing: Listing,
    pub from: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Listings {
    pub entries: Vec<Entry>,
    /// One line per index that did not load.
    pub errors: Vec<String>,
}

/// Reads the official index, then `user`'s. An id an earlier index already listed is skipped: a
/// user's index cannot replace an official entry. Blocks: run it off the main thread.
pub fn fetch_all(user: &[String]) -> Listings {
    let mut out = Listings::default();
    let urls = std::iter::once(OFFICIAL_INDEX).chain(user.iter().map(String::as_str));
    for (n, url) in urls.enumerate() {
        let parsed = get_index(url).and_then(|text| parse_index(&text).map_err(anyhow::Error::msg));
        let (listings, skipped) = match parsed {
            Ok(parsed) => parsed,
            Err(e) => {
                out.errors.push(format!("Could not load {url}: {e:#}"));
                continue;
            }
        };
        for notice in skipped {
            eprintln!("wai: index {url}: skipped {notice}");
        }
        let from = (n > 0).then(|| https_host(url).unwrap_or(url).to_string());
        for listing in listings {
            if !out.entries.iter().any(|e| e.listing.id == listing.id) {
                let from = from.clone();
                out.entries.push(Entry { listing, from });
            }
        }
    }
    out
}

#[cfg(not(test))]
mod net {
    use std::time::Duration;

    use super::*;
    use crate::update::download::{Limits, download_with, get_into};

    fn https(url: &str) -> bool {
        wai_ext::index::https_url(url)
    }

    pub(super) fn get_index(url: &str) -> anyhow::Result<String> {
        anyhow::ensure!(https(url), "not an https:// URL");
        let mut body = Vec::new();
        let limits = Limits {
            allowed: https,
            max: 1024 * 1024,
            timeout: Duration::from_secs(10),
        };
        get_into(url, &mut body, limits)?;
        Ok(String::from_utf8(body)?)
    }

    pub(super) fn get_package(url: &str, to: &Path) -> anyhow::Result<()> {
        let limits = Limits {
            allowed: https,
            max: 100 * 1024 * 1024,
            timeout: Duration::from_secs(600),
        };
        download_with(url, to, limits)
    }
}

// ponytail: tests never reach the network; `https://test.invalid/<path>` reads the file at
// `/<path>`, and any other URL fails.
#[cfg(test)]
mod net {
    use super::*;

    fn file(url: &str) -> anyhow::Result<&Path> {
        let path = url.strip_prefix("https://test.invalid");
        path.map(Path::new)
            .ok_or_else(|| anyhow::anyhow!("offline in tests"))
    }

    pub(super) fn get_index(url: &str) -> anyhow::Result<String> {
        Ok(std::fs::read_to_string(file(url)?)?)
    }

    pub(super) fn get_package(url: &str, to: &Path) -> anyhow::Result<()> {
        std::fs::copy(file(url)?, to)?;
        Ok(())
    }
}

use net::get_index;

/// Downloads a package for `ExtStore::stage_package`. Blocks.
pub fn get_package(url: &str, to: &Path) -> Result<(), String> {
    net::get_package(url, to).map_err(|e| format!("{e:#}"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    /// An index at `dir/<name>`, as the URL tests read it from.
    pub(crate) fn index(dir: &Path, name: &str, text: &str) -> String {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        format!("https://test.invalid{}", path.display())
    }

    pub(crate) fn entry(id: &str, version: &str, url: &str, sha256: &str) -> String {
        format!(
            "[[extensions]]\nid = \"{id}\"\nname = \"{id}\"\nversion = \"{version}\"\n\
             [extensions.packages.any]\nurl = '{url}'\nsha256 = \"{sha256}\"\n"
        )
    }

    /// Every index is read; the first to list an id keeps it; a failed index is one line.
    #[test]
    fn merges_indexes_first_wins() {
        let tmp = tempfile::tempdir().unwrap();
        let a = index(
            tmp.path(),
            "a.toml",
            &(entry("one", "1", "https://e.com/1", SHA)
                + &entry("two", "1", "https://e.com/2", SHA)),
        );
        let b = index(
            tmp.path(),
            "b.toml",
            &(entry("two", "9", "https://evil/2", SHA)
                + &entry("three", "1", "https://e.com/3", SHA)),
        );
        let missing = format!("https://test.invalid{}", tmp.path().join("none").display());
        let got = fetch_all(&[a, b, missing.clone()]);
        let ids: Vec<_> = (got.entries.iter())
            .map(|e| (e.listing.id.as_str(), e.listing.version.as_str()))
            .collect();
        assert_eq!(ids, [("one", "1"), ("two", "1"), ("three", "1")]);
        assert_eq!(got.entries[0].from.as_deref(), Some("test.invalid"));
        // The official index, unreachable in tests, then the missing one.
        assert_eq!(got.errors.len(), 2, "{:?}", got.errors);
        assert!(got.errors[0].starts_with(&format!("Could not load {OFFICIAL_INDEX}: ")));
        assert!(got.errors[1].starts_with(&format!("Could not load {missing}: ")));
    }
}
