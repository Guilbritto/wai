//! Every theme wai can draw: the bundled one, `<user dir>/themes/*.json` and the themes
//! extensions contribute. Each file is a gpui-kit theme set; a theme may carry a `"wai"` object
//! with wai's own colors (`colors.rs`).

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::SharedString;
use gpui_kit::component::{ThemeConfig, ThemeMode, ThemeSet};
use gpui_kit::{App, Global};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Source {
    Bundled,
    User(PathBuf),
    Extension(PathBuf),
}

#[derive(Debug, Clone)]
pub(crate) struct CatalogTheme {
    pub config: Rc<ThemeConfig>,
    /// The theme's `"wai"` object (empty when it has none).
    pub wai: Map<String, Value>,
    #[allow(dead_code)]
    pub source: Source,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ThemeCatalog {
    pub themes: Vec<CatalogTheme>,
}

impl Global for ThemeCatalog {}

impl ThemeCatalog {
    #[allow(dead_code)]
    pub fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    /// The names of the themes of `mode`, in load order (for the pickers).
    #[allow(dead_code)]
    pub fn names(&self, mode: ThemeMode) -> Vec<SharedString> {
        self.themes
            .iter()
            .filter(|t| t.config.mode.is_dark() == mode.is_dark())
            .map(|t| t.config.name.clone())
            .collect()
    }

    pub fn find(&self, name: &str, mode: ThemeMode) -> Option<&CatalogTheme> {
        self.themes
            .iter()
            .find(|t| t.config.name == name && t.config.mode.is_dark() == mode.is_dark())
    }

    fn add_file(&mut self, text: &str, path: &str, source: &Source, notices: &mut Vec<String>) {
        let skipped = |reason: String| format!("Skipped theme file {path}: {reason}.");
        let value: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => return notices.push(skipped(e.to_string())),
        };
        let Some(raw) = value.get("themes").and_then(Value::as_array) else {
            return notices.push(skipped("not a theme set".into()));
        };
        let set: ThemeSet = match serde_json::from_value(value.clone()) {
            Ok(s) => s,
            Err(e) => return notices.push(skipped(e.to_string())),
        };
        for (mut config, raw) in set.themes.into_iter().zip(raw) {
            if self.themes.iter().any(|t| t.config.name == config.name) {
                notices.push(format!(
                    "Skipped theme \"{}\" from {path}: a theme with that name is already loaded.",
                    config.name
                ));
                continue;
            }
            // Fonts are the user's (settings), never a theme's.
            config.font_family = None;
            config.font_size = None;
            config.mono_font_family = None;
            config.mono_font_size = None;
            let wai = raw
                .get("wai")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            self.themes.push(CatalogTheme {
                config: Rc::new(config),
                wai,
                source: source.clone(),
            });
        }
    }

    fn add_path(&mut self, path: &Path, source: Source, notices: &mut Vec<String>) {
        let shown = path.display().to_string();
        match wai_settings::read_config(path) {
            Ok(Some(text)) => self.add_file(&text, &shown, &source, notices),
            Ok(None) => notices.push(format!("Skipped theme file {shown}: not found.")),
            Err(e) => notices.push(format!("Skipped theme file {shown}: {e}.")),
        }
    }
}

/// `extension_dir` is the folder of the extension that contributes `file`.
fn inside(extension_dir: &str, file: &Path) -> bool {
    let Ok(dir) = Path::new(extension_dir).canonicalize() else {
        return false;
    };
    match file.canonicalize() {
        Ok(file) => file.starts_with(dir),
        // A file that is not there is reported by `add_path`; a `..` can still escape.
        Err(_) => {
            !file
                .components()
                .any(|c| c == std::path::Component::ParentDir)
                && file.starts_with(extension_dir)
        }
    }
}

/// The bundled theme set, then `user_dir/themes/*.json` by file name, then the extensions' files
/// (`(extension folder, theme file)`) in order. A theme whose name is loaded is refused.
pub(crate) fn load(
    bundled: &str,
    user_dir: Option<&Path>,
    ext_files: &[(String, PathBuf)],
) -> (ThemeCatalog, Vec<String>) {
    let mut catalog = ThemeCatalog::default();
    let mut notices = Vec::new();
    catalog.add_file(bundled, "bundled", &Source::Bundled, &mut notices);
    if let Some(dir) = user_dir.map(|d| d.join("themes")) {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("json"))
            })
            .collect();
        files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
        for path in files {
            catalog.add_path(&path, Source::User(path.clone()), &mut notices);
        }
    }
    for (dir, file) in ext_files {
        if inside(dir, file) {
            catalog.add_path(file, Source::Extension(file.clone()), &mut notices);
        } else {
            notices.push(format!(
                "Skipped theme file {}: outside the extension folder.",
                file.display()
            ));
        }
    }
    (catalog, notices)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUNDLED: &str = include_str!("../../assets/theme/aurora.json");

    fn theme(name: &str, mode: &str) -> String {
        format!(
            r##"{{"name":"set","themes":[{{"name":"{name}","mode":"{mode}","font.family":"Comic Sans","mono_font.family":"Courier","colors":{{"background":"#101010"}},"wai":{{"boreal":"#00ff00"}}}}]}}"##
        )
    }

    fn user_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("themes")).unwrap();
        dir
    }

    #[test]
    fn bad_theme_file_is_skipped_with_notice() {
        let dir = user_dir();
        let themes = dir.path().join("themes");
        std::fs::write(themes.join("a-invalid.json"), "{ not json").unwrap();
        std::fs::write(themes.join("b-not-a-set.json"), r#"{"colors": {}}"#).unwrap();
        std::fs::write(themes.join("c-big.json"), vec![b' '; 1024 * 1024 + 1]).unwrap();
        std::fs::write(themes.join("d-good.json"), theme("Good", "dark")).unwrap();
        std::fs::write(themes.join("notes.txt"), "ignored").unwrap();
        let (catalog, notices) = load(BUNDLED, Some(dir.path()), &[]);
        let path = |f: &str| themes.join(f).display().to_string();
        assert_eq!(notices.len(), 3, "{notices:?}");
        assert!(
            notices[0].starts_with(&format!("Skipped theme file {}: ", path("a-invalid.json")))
        );
        assert!(notices[0].ends_with('.'));
        assert_eq!(
            notices[1],
            format!(
                "Skipped theme file {}: not a theme set.",
                path("b-not-a-set.json")
            )
        );
        assert_eq!(
            notices[2],
            format!(
                "Skipped theme file {}: larger than 1 MB.",
                path("c-big.json")
            )
        );
        let names: Vec<_> = catalog
            .themes
            .iter()
            .map(|t| t.config.name.to_string())
            .collect();
        assert_eq!(names, ["wai Aurora", "wai Aurora Light", "Good"]);
    }

    #[test]
    fn duplicate_theme_name_is_refused() {
        let dir = user_dir();
        let themes = dir.path().join("themes");
        std::fs::write(themes.join("a.json"), theme("wai Aurora", "dark")).unwrap();
        std::fs::write(themes.join("b.json"), theme("Mine", "light")).unwrap();
        std::fs::write(themes.join("c.json"), theme("Mine", "dark")).unwrap();
        let (catalog, notices) = load(BUNDLED, Some(dir.path()), &[]);
        let path = |f: &str| themes.join(f).display().to_string();
        assert_eq!(
            notices,
            [
                format!(
                    "Skipped theme \"wai Aurora\" from {}: a theme with that name is already loaded.",
                    path("a.json")
                ),
                format!(
                    "Skipped theme \"Mine\" from {}: a theme with that name is already loaded.",
                    path("c.json")
                ),
            ]
        );
        assert_eq!(catalog.themes.len(), 3);
        assert_eq!(
            catalog.names(ThemeMode::Light),
            ["wai Aurora Light", "Mine"]
        );
        assert_eq!(catalog.names(ThemeMode::Dark), ["wai Aurora"]);
    }

    #[test]
    fn user_themes_keep_wai_object_and_drop_fonts() {
        let dir = user_dir();
        std::fs::write(dir.path().join("themes/a.json"), theme("Mine", "dark")).unwrap();
        let (catalog, notices) = load(BUNDLED, Some(dir.path()), &[]);
        assert!(notices.is_empty(), "{notices:?}");
        let mine = catalog.find("Mine", ThemeMode::Dark).unwrap();
        assert_eq!(mine.wai["boreal"], "#00ff00");
        let c = &mine.config;
        assert!(c.font_family.is_none() && c.mono_font_family.is_none());
        assert!(c.font_size.is_none() && c.mono_font_size.is_none());
    }

    #[test]
    fn missing_folder_is_silent_and_extension_files_load_in_order() {
        let (_, notices) = load(BUNDLED, Some(Path::new("/nonexistent-wai")), &[]);
        assert!(notices.is_empty());
        let ext = tempfile::tempdir().unwrap();
        std::fs::write(ext.path().join("t.json"), theme("Ext", "dark")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("o.json"), theme("Out", "dark")).unwrap();
        let dir = ext.path().display().to_string();
        let (catalog, notices) = load(
            BUNDLED,
            None,
            &[
                (dir.clone(), ext.path().join("t.json")),
                (dir.clone(), ext.path().join("../escape.json")),
                (dir, outside.path().join("o.json")),
            ],
        );
        assert!(catalog.find("Ext", ThemeMode::Dark).is_some());
        assert!(catalog.find("Out", ThemeMode::Dark).is_none());
        assert_eq!(notices.len(), 2, "{notices:?}");
        assert!(notices[1].ends_with("o.json: outside the extension folder."));
    }
}
