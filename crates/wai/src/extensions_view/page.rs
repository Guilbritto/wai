//! The Extensions page: the list on the left, the selected extension's page on the right.

use wai_ext::index::Listing;
use wai_ext::panel::clamp;
use wai_ext::{ExtId, Installed, Manifest};

use super::NAME_CHARS;
use crate::ext_host::ExtStatus;
use crate::marketplace::Entry;

/// What a row says under an installed extension's name.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum RowStatus {
    Running,
    Starting,
    Enabled,
    Crashed(String),
    Failed(String),
    Disabled,
}

impl RowStatus {
    pub(super) fn label(&self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Starting => "Starting",
            Self::Enabled => "Enabled",
            Self::Crashed(_) => "Crashed",
            Self::Failed(_) => "Failed",
            Self::Disabled => "Disabled",
        }
    }
}

pub(super) fn row_status(enabled: bool, status: Option<&ExtStatus>) -> RowStatus {
    match status {
        _ if !enabled => RowStatus::Disabled,
        None => RowStatus::Enabled,
        Some(ExtStatus::Ready) => RowStatus::Running,
        Some(ExtStatus::Starting) => RowStatus::Starting,
        Some(ExtStatus::Crashed(tail)) => RowStatus::Crashed(tail.clone()),
        Some(ExtStatus::Failed(error)) => RowStatus::Failed(error.clone()),
    }
}

pub(super) fn listing_for<'a>(id: &ExtId, entries: &'a [Entry]) -> Option<&'a Entry> {
    entries.iter().find(|e| e.listing.id == *id)
}

/// The listing to update `ext` to: same id, a package for this platform, another version.
pub(super) fn listed_update<'a>(ext: &Installed, entries: &'a [Entry]) -> Option<&'a Listing> {
    let listing = &listing_for(&ext.manifest.id, entries)?.listing;
    (listing.package().is_some() && listing.version != ext.manifest.version).then_some(listing)
}

/// The manifest's contributions by kind, only the kinds it has.
pub(super) fn features(m: &Manifest) -> Vec<(&'static str, Vec<String>)> {
    fn names<T>(list: &[T], name: impl Fn(&T) -> &str) -> Vec<String> {
        list.iter().map(|t| clamp(name(t), NAME_CHARS)).collect()
    }
    let c = &m.contributes;
    [
        ("Panels", names(&c.panels, |p| &p.title)),
        ("Bar items", names(&c.items, |i| &i.title)),
        ("Commands", names(&c.commands, |c| &c.title)),
        ("Agents", names(&c.agents, |a| &a.name)),
        ("MCP servers", names(&c.mcp_servers, |s| &s.name)),
        ("Templates", names(&c.templates, |t| &t.name)),
        ("Themes", names(&c.themes, |t| t)),
    ]
    .into_iter()
    .filter(|(_, names)| !names.is_empty())
    .collect()
}

/// A GitHub repository's releases page.
pub(super) fn releases_url(repository: &str) -> Option<String> {
    let repo = repository.trim_end_matches('/');
    let path = repo.strip_prefix("https://github.com/")?;
    let (owner, name) = path.split_once('/')?;
    (!owner.is_empty() && !name.is_empty() && !name.contains('/'))
        .then(|| format!("{repo}/releases"))
}

/// The monogram's color, picked from the id: one of six.
pub(super) fn tint(id: &str) -> usize {
    id.bytes().map(usize::from).sum::<usize>() % 6
}

/// The search: any field contains the query, ignoring case.
pub(super) fn matches(query: &str, fields: &[&str]) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty() || fields.iter().any(|f| f.to_lowercase().contains(&query))
}

pub(super) fn platform_label(key: &str) -> String {
    let (os, arch) = key.split_once('-').unwrap_or((key, ""));
    let os = match os {
        "darwin" => "macOS",
        "linux" => "Linux",
        "windows" => "Windows",
        "any" => "Any",
        other => other,
    };
    format!("{os} {arch}").trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use wai_ext::index::Package;
    use wai_ext::parse_manifest;

    use super::*;

    fn manifest(text: &str) -> Manifest {
        parse_manifest(text, Path::new("/x")).unwrap()
    }

    fn installed(version: &str) -> Installed {
        let text = format!("id = \"s\"\nname = \"S\"\nversion = \"{version}\"\n");
        Installed {
            manifest: manifest(&text),
            dir: "/x/s".into(),
            data: "/x/data/s".into(),
            enabled: true,
            granted: vec![],
        }
    }

    fn listed(id: &str, version: &str, platform: Option<&str>) -> Entry {
        let package = Package {
            url: "https://example.com/s.tar.gz".into(),
            sha256: "0".repeat(64),
        };
        let packages = platform.map(|p| (p.to_string(), package)).into_iter();
        Entry {
            listing: Listing {
                id: ExtId::new(id).unwrap(),
                name: id.into(),
                version: version.into(),
                description: None,
                repository: None,
                packages: packages.collect::<BTreeMap<_, _>>(),
            },
            from: None,
        }
    }

    #[test]
    fn status_labels() {
        use ExtStatus::*;
        assert_eq!(row_status(false, Some(&Ready)).label(), "Disabled");
        assert_eq!(row_status(true, Some(&Ready)).label(), "Running");
        assert_eq!(row_status(true, Some(&Starting)).label(), "Starting");
        assert_eq!(row_status(true, None).label(), "Enabled");
        let crashed = row_status(true, Some(&Crashed("boom".into())));
        assert_eq!(crashed, RowStatus::Crashed("boom".into()));
        assert_eq!(crashed.label(), "Crashed");
        let failed = row_status(true, Some(&Failed("nope".into())));
        assert_eq!(failed, RowStatus::Failed("nope".into()));
        assert_eq!(failed.label(), "Failed");
    }

    #[test]
    fn links_and_labels() {
        assert_eq!(
            releases_url("https://github.com/o/r/").as_deref(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(releases_url("https://github.com/o"), None);
        assert_eq!(releases_url("https://gitlab.com/o/r"), None);
        assert_eq!(platform_label("darwin-arm64"), "macOS arm64");
        assert_eq!(platform_label("darwin-x64"), "macOS x64");
        assert_eq!(platform_label("linux-x64"), "Linux x64");
        assert_eq!(platform_label("windows-x64"), "Windows x64");
        assert_eq!(platform_label("any"), "Any");
        assert!(matches("", &["x"]));
        assert!(matches(" USa ", &["wai-usage", "Usage"]));
        assert!(!matches("zzz", &["Usage", "wai-usage"]));
        assert_eq!(tint("wai-usage"), tint("wai-usage"));
        assert!((0..50).all(|n| tint(&format!("ext-{n}")) < 6));
        assert_ne!(tint("a"), tint("b"));
    }

    #[test]
    fn features_group_by_kind() {
        let m = manifest(
            "id = \"s\"\nname = \"S\"\nversion = \"1\"\npermissions = [\"panels\", \"process\"]\n\
             [process]\ncommand = \"bin/run\"\n\
             [[contributes.commands]]\nid = \"go\"\ntitle = \"Go now\"\nprompt = \"go\"\n\
             [[contributes.panels]]\nid = \"p\"\ntitle = \"Pet\"\n\
             [[contributes.mcp_servers]]\nname = \"playwright\"\ncommand = \"npx\"\n",
        );
        let kinds = features(&m);
        assert_eq!(
            kinds,
            vec![
                ("Panels", vec!["Pet".to_string()]),
                ("Commands", vec!["Go now".to_string()]),
                ("MCP servers", vec!["playwright".to_string()]),
            ]
        );
        assert!(features(&installed("1").manifest).is_empty());
    }

    #[test]
    fn update_needs_another_listed_version() {
        let ext = installed("1.0.0");
        let same = [listed("s", "1.0.0", Some("any"))];
        assert!(listed_update(&ext, &same).is_none());
        let other_id = [listed("t", "2.0.0", Some("any"))];
        assert!(listed_update(&ext, &other_id).is_none());
        let no_package = [listed("s", "2.0.0", None)];
        assert!(listed_update(&ext, &no_package).is_none());
        let newer = [
            listed("t", "1.0.0", Some("any")),
            listed("s", "1.1.0", Some("any")),
        ];
        assert_eq!(
            listed_update(&ext, &newer).map(|l| l.version.as_str()),
            Some("1.1.0")
        );
        assert_eq!(
            listing_for(&ext.manifest.id, &newer).map(|e| e.listing.version.as_str()),
            Some("1.1.0")
        );
    }
}
