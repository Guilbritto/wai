//! `[theme.overrides]`: the user's colors laid over the chosen theme. Keys are gpui-kit color
//! keys (`primary.background`), `wai.<palette key>` and the `syntax` table; a table named after a
//! theme applies to that theme only.

use gpui_kit::component::{ThemeConfig, ThemeConfigColors};
use serde_json::{Map, Value, json};

use super::catalog::CatalogTheme;
use super::colors;

/// The serde names of a struct's fields, from its serialized default.
fn names(value: Value) -> Vec<String> {
    value
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

fn flatten(prefix: &str, value: &toml::Value, out: &mut Vec<(String, toml::Value)>) {
    match value {
        toml::Value::Table(table) => {
            for (key, value) in table {
                flatten(&format!("{prefix}.{key}"), value, out);
            }
        }
        _ => out.push((prefix.to_string(), value.clone())),
    }
}

/// `#rgb`, `#rrggbb` or `#rrggbbaa` as `#rrggbb[aa]`, lowercase.
fn color(value: &toml::Value) -> Option<String> {
    let hex = value.as_str()?.strip_prefix('#')?;
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        3 => Some(format!(
            "#{}",
            hex.chars()
                .flat_map(|c| [c, c])
                .collect::<String>()
                .to_lowercase()
        )),
        6 | 8 => Some(format!("#{}", hex.to_lowercase())),
        _ => None,
    }
}

/// The theme's config and `"wai"` object with `overrides` applied: the global keys first, then
/// the table named after the theme. A key or value that is not valid, or a table that is neither
/// a color key group nor a theme (`is_theme`), is skipped with a notice.
pub(crate) fn apply(
    theme: &CatalogTheme,
    overrides: &toml::Table,
    is_theme: &dyn Fn(&str) -> bool,
) -> (ThemeConfig, Map<String, Value>, Vec<String>) {
    let color_keys = names(serde_json::to_value(ThemeConfigColors::default()).unwrap_or_default());
    let syntax_keys = names(
        serde_json::to_value(ThemeConfig::default().highlight.unwrap_or_default())
            .ok()
            .and_then(|v| v.get("syntax").cloned())
            .unwrap_or_default(),
    );

    // (key, value) in application order.
    let (mut global, mut scoped) = (Vec::new(), Vec::new());
    let mut notices = Vec::new();
    for (key, value) in overrides {
        match value {
            toml::Value::Table(table) => {
                let is_key_prefix = |k: &String| k.starts_with(&format!("{key}."));
                if key == "syntax" || key == "wai" || color_keys.iter().any(is_key_prefix) {
                    flatten(key, value, &mut global);
                } else if theme.config.name.as_ref() == key.as_str() {
                    for (k, v) in table {
                        flatten(k, v, &mut scoped);
                    }
                } else if !is_theme(key) {
                    notices.push(format!("Ignored color override \"{key}\": unknown table."));
                }
            }
            _ => global.push((key.clone(), value.clone())),
        }
    }

    let mut config = (*theme.config).clone();
    let mut wai = theme.wai.clone();
    let mut colors_json = serde_json::to_value(&config.colors).unwrap_or_default();
    let mut highlight_json =
        serde_json::to_value(config.highlight.clone().unwrap_or_default()).unwrap_or_default();
    for (key, value) in global.into_iter().chain(scoped) {
        let mut skip = |reason: &str| {
            notices.push(format!("Ignored color override \"{key}\": {reason}."));
        };
        let known = color_keys.contains(&key)
            || key
                .strip_prefix("wai.")
                .is_some_and(|k| colors::KEYS.contains(&k))
            || key
                .strip_prefix("syntax.")
                .is_some_and(|k| syntax_keys.iter().any(|s| s == k));
        if !known {
            skip("unknown key");
            continue;
        }
        let Some(hex) = color(&value) else {
            skip("expected #rgb, #rrggbb or #rrggbbaa");
            continue;
        };
        if let Some(k) = key.strip_prefix("wai.") {
            wai.insert(k.to_string(), json!(hex));
        } else if let Some(k) = key.strip_prefix("syntax.") {
            let style = &mut highlight_json["syntax"][k];
            if !style.is_object() {
                *style = json!({});
            }
            style["color"] = json!(hex);
        } else {
            colors_json[&key] = json!(hex);
        }
    }
    if let Ok(colors) = serde_json::from_value(colors_json) {
        config.colors = colors;
    }
    if let Ok(highlight) = serde_json::from_value(highlight_json) {
        config.highlight = Some(highlight);
    }
    (config, wai, notices)
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::ThemeMode;
    use gpui_kit::{Hsla, rgb};

    use super::*;
    use crate::appearance::catalog;

    const BUNDLED: &str = include_str!("../../assets/theme/aurora.json");

    fn table(text: &str) -> toml::Table {
        toml::from_str(text).unwrap()
    }

    fn known(name: &str) -> bool {
        name.starts_with("wai Aurora")
    }

    fn bundled(name: &str, mode: ThemeMode) -> CatalogTheme {
        let (catalog, _) = catalog::load(BUNDLED, None, &[]);
        catalog.find(name, mode).unwrap().clone()
    }

    #[test]
    fn theme_scoped_override_wins_only_for_that_theme() {
        let overrides = table(
            r##"
            "primary.background" = "#111111"
            ring = "#abc"
            [secondary]
            background = "#333333"
            ["wai Aurora"]
            primary.background = "#222222"
            "##,
        );
        let dark = bundled("wai Aurora", ThemeMode::Dark);
        let (config, _, notices) = apply(&dark, &overrides, &known);
        assert!(notices.is_empty(), "{notices:?}");
        assert_eq!(config.colors.primary.as_deref(), Some("#222222"));
        assert_eq!(config.colors.ring.as_deref(), Some("#aabbcc"));
        assert_eq!(config.colors.secondary.as_deref(), Some("#333333"));

        let light = bundled("wai Aurora Light", ThemeMode::Light);
        let (config, _, notices) = apply(&light, &overrides, &known);
        assert!(notices.is_empty(), "{notices:?}");
        assert_eq!(config.colors.primary.as_deref(), Some("#111111"));
        assert_eq!(config.colors.secondary.as_deref(), Some("#333333"));
    }

    #[test]
    fn syntax_override_sets_highlight() {
        let dark = bundled("wai Aurora", ThemeMode::Dark);
        let before = dark
            .config
            .highlight
            .as_ref()
            .unwrap()
            .syntax
            .style("string");
        let overrides = table(
            r##"
            [syntax]
            keyword = "#ff8800"
            [wai]
            boreal = "#0f0"
            "backdrop.glow_1" = "#112233"
            "##,
        );
        let (config, wai, notices) = apply(&dark, &overrides, &known);
        assert!(notices.is_empty(), "{notices:?}");
        let keyword = config.highlight.as_ref().unwrap().syntax.style("keyword");
        assert_eq!(keyword.unwrap().color, Some(Hsla::from(rgb(0xff8800))));
        // The other highlight colors are untouched.
        let string = config.highlight.as_ref().unwrap().syntax.style("string");
        assert_eq!(string.map(|s| s.color), before.map(|s| s.color));
        assert_eq!(wai["boreal"], "#00ff00");
        assert_eq!(wai["backdrop.glow_1"], "#112233");
        assert_eq!(wai["noite"], "#070a18");
    }

    #[test]
    fn bad_key_or_value_is_skipped_with_notice() {
        let dark = bundled("wai Aurora", ThemeMode::Dark);
        let overrides = table(
            r##"
            "primary.background" = "#00ff00"
            nope = "#ffffff"
            ring = "red"
            link = "#12345"
            overlay = 5
            [wai]
            nothing = "#ffffff"
            [syntax]
            keyword = "#ff8800"
            nothing = "#ffffff"
            [typo]
            background = "#ffffff"
            "##,
        );
        let (config, _, notices) = apply(&dark, &overrides, &known);
        assert_eq!(config.colors.primary.as_deref(), Some("#00ff00"));
        let keyword = config.highlight.as_ref().unwrap().syntax.style("keyword");
        assert_eq!(keyword.unwrap().color, Some(Hsla::from(rgb(0xff8800))));
        // The bad ones leave the theme's own colors.
        assert_eq!(config.colors.ring, dark.config.colors.ring);
        let mut notices = notices;
        notices.sort();
        let hint = "expected #rgb, #rrggbb or #rrggbbaa";
        assert_eq!(
            notices,
            [
                format!("Ignored color override \"link\": {hint}."),
                "Ignored color override \"nope\": unknown key.".to_string(),
                format!("Ignored color override \"overlay\": {hint}."),
                format!("Ignored color override \"ring\": {hint}."),
                "Ignored color override \"syntax.nothing\": unknown key.".to_string(),
                "Ignored color override \"typo\": unknown table.".to_string(),
                "Ignored color override \"wai.nothing\": unknown key.".to_string(),
            ]
        );
    }
}
