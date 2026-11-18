//! Fonts, colors and themes: turns the settings and the theme files into gpui-kit's `Theme` and
//! wai's own `Fonts`, `Palette` and `ThemeCatalog` globals.

pub(crate) mod catalog;
pub(crate) mod colors;
pub(crate) mod fonts;
pub(crate) mod overrides;

use std::path::{Path, PathBuf};

use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::{App, Global, SharedString, Window, WindowAppearance, px};
use wai_settings::fonts::{FontDefaults, ResolvedFonts};
use wai_settings::{Settings, ThemeChoice};

use self::catalog::{CatalogTheme, ThemeCatalog};
use self::colors::Palette;

const BUNDLED: &str = include_str!("../../assets/theme/aurora.json");
pub(crate) const BUNDLED_DARK: &str = "wai Aurora";
pub(crate) const BUNDLED_LIGHT: &str = "wai Aurora Light";

/// The family and size each area draws in, as the settings resolved them.
#[derive(Debug, Clone)]
pub(crate) struct Fonts(pub ResolvedFonts);

impl Global for Fonts {}

/// The mode the window draws in: the chosen one, or the OS's for `System`.
pub(crate) fn effective_mode(choice: ThemeChoice, appearance: WindowAppearance) -> ThemeMode {
    match choice {
        ThemeChoice::Dark => ThemeMode::Dark,
        ThemeChoice::Light => ThemeMode::Light,
        ThemeChoice::System => match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
            WindowAppearance::Light | WindowAppearance::VibrantLight => ThemeMode::Light,
        },
    }
}

/// The theme named `name` for `mode`, else the bundled one (with a notice when a name was set).
fn pick(
    catalog: &ThemeCatalog,
    name: Option<&str>,
    mode: ThemeMode,
    bundled: &str,
    notices: &mut Vec<String>,
) -> CatalogTheme {
    if let Some(found) = name.and_then(|n| catalog.find(n, mode)) {
        return found.clone();
    }
    if let Some(name) = name {
        notices.push(format!(
            "Theme \"{name}\" is not installed; using {bundled}."
        ));
    }
    catalog
        .find(bundled, mode)
        .expect("the bundled themes are always loaded")
        .clone()
}

/// Loads the themes, applies the settings' theme, overrides and fonts to the global `Theme` and
/// stores the `Fonts`, `Palette` and `ThemeCatalog` globals. Runs again whenever anything it reads
/// changes: what the settings no longer set returns to the default. The notices say what it
/// could not use.
pub(crate) fn apply(
    settings: &Settings,
    user_dir: Option<&Path>,
    ext_files: &[(String, PathBuf)],
    window: Option<&mut Window>,
    cx: &mut App,
) -> Vec<String> {
    let any_family = [
        &settings.interface.font_family,
        &settings.editor.font_family,
        &settings.terminal.font_family,
        &settings.conversation.font_family,
        &settings.conversation.code_font_family,
    ]
    .into_iter()
    .any(Option::is_some);
    // The platform's font list is costly: only read when a family is set.
    let names = if any_family {
        cx.text_system().all_font_names()
    } else {
        Vec::new()
    };
    let installed = |family: &str| {
        names
            .iter()
            .find(|n| n.eq_ignore_ascii_case(family))
            .cloned()
    };
    let appearance = cx.window_appearance();
    apply_with(
        settings, user_dir, ext_files, window, cx, appearance, &installed,
    )
}

fn apply_with(
    settings: &Settings,
    user_dir: Option<&Path>,
    ext_files: &[(String, PathBuf)],
    window: Option<&mut Window>,
    cx: &mut App,
    appearance: WindowAppearance,
    installed: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    let (catalog, mut notices) = catalog::load(BUNDLED, user_dir, ext_files);
    let theme = &settings.theme;
    let mut chosen = |name: &Option<String>, mode, bundled| {
        let picked = pick(&catalog, name.as_deref(), mode, bundled, &mut notices);
        let (config, wai, found) =
            overrides::apply(&picked, &theme.overrides, &|n| catalog.has_name(n));
        (std::rc::Rc::new(config), wai, found)
    };
    let (light, light_wai, light_notices) = chosen(&theme.light, ThemeMode::Light, BUNDLED_LIGHT);
    let (dark, dark_wai, dark_notices) = chosen(&theme.dark, ThemeMode::Dark, BUNDLED_DARK);
    notices.extend(light_notices);
    notices.extend(dark_notices);

    let mode = effective_mode(theme.mode, appearance);
    let global = Theme::global_mut(cx);
    global.light_theme = light;
    global.dark_theme = dark;
    Theme::change(mode, window, cx);

    let wai = if mode.is_dark() { dark_wai } else { light_wai };
    let (palette, found) = Palette::from(&wai, &cx.theme().colors);
    notices.extend(found);

    let defaults = Theme::default();
    let defaults = FontDefaults {
        ui_family: defaults.font_family.to_string(),
        mono_family: defaults.mono_font_family.to_string(),
    };
    let (fonts, found) = wai_settings::fonts::resolve(settings, &defaults, installed);
    notices.extend(found);
    Theme::update(cx, |theme| {
        theme.font_family = SharedString::from(fonts.ui.family.clone());
        theme.font_size = px(fonts.ui.size);
        theme.mono_font_family = SharedString::from(fonts.editor.family.clone());
        theme.mono_font_size = px(fonts.editor.size);
    });

    cx.set_global(Fonts(fonts));
    cx.set_global(palette);
    cx.set_global(catalog);

    let mut seen = std::collections::HashSet::new();
    notices.retain(|n| seen.insert(n.clone()));
    notices
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Hsla, TestAppContext, rgb};
    use wai_settings::{FontArea, ThemeSettings};

    use regex::Regex;

    use super::*;

    fn run(
        cx: &mut TestAppContext,
        settings: &Settings,
        user_dir: Option<&Path>,
        installed: &[&str],
    ) -> Vec<String> {
        cx.update(|cx| {
            if !cx.has_global::<Theme>() {
                gpui_kit::init(cx);
            }
            let installed = |f: &str| {
                (installed.iter())
                    .find(|n| n.eq_ignore_ascii_case(f))
                    .map(|n| n.to_string())
            };
            let appearance = cx.window_appearance();
            apply_with(settings, user_dir, &[], None, cx, appearance, &installed)
        })
    }

    /// Every source file under `crates/wai/src` as (path relative to it, text).
    fn sources() -> Vec<(String, String)> {
        fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, root, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let rel = path
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned();
                    out.push((rel, std::fs::read_to_string(&path).unwrap()));
                }
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut out = Vec::new();
        walk(&root, &root, &mut out);
        out
    }

    /// Lines of `text` (with their file) matching `bad`, outside the files `skip` says to skip.
    fn offenders(skip: impl Fn(&str) -> bool, bad: impl Fn(&str) -> bool) -> Vec<String> {
        let mut found = Vec::new();
        for (file, text) in sources().into_iter().filter(|(f, _)| !skip(f)) {
            for (n, line) in text.lines().enumerate().filter(|(_, l)| bad(l)) {
                found.push(format!("{file}:{}: {}", n + 1, line.trim()));
            }
        }
        found
    }

    /// A hex color or `hsla(..)` that is not pure white or black (a hue the theme should own).
    fn has_hue_literal(line: &str) -> bool {
        let hex = Regex::new(r"0x([0-9a-fA-F_]{6,})\b").unwrap();
        let hsla = Regex::new(r"\bhsla\(\s*([^,]*),\s*([^,]*),\s*([^,]*),").unwrap();
        let hue_hex = !line.contains("CREATE_NO_WINDOW")
            && hex.captures_iter(line).any(|c| {
                let digits = c[1].replace('_', "");
                !matches!(&digits[..6], "ffffff" | "000000")
            });
        let hue_hsla = hsla.captures_iter(line).any(|c| {
            let num = |i: usize| c[i].trim().parse::<f32>().ok();
            !(num(2) == Some(0.) && matches!(num(3), Some(l) if l == 0. || l == 1.))
        });
        hue_hex || hue_hsla
    }

    #[test]
    fn views_paint_from_the_theme() {
        let calls = Regex::new(r"aurora::(noite|noite_2|boreal|lilas|magenta|ceu|brasa|ambar|erro|tinta|tinta_2|nevoa|nevoa_2|ok|vidro|vidro_forte|borda|borda_forte|card_fill)\(\)").unwrap();
        let found = offenders(
            |f| f == "aurora.rs" || f.starts_with("appearance/") || f == "terminal_view.rs",
            |l| calls.is_match(l) || has_hue_literal(l),
        );
        assert!(
            found.is_empty(),
            "fixed colors in views:\n{}",
            found.join("\n")
        );
    }

    #[test]
    fn wai_icons_are_pinned_to_px() {
        let chain =
            Regex::new(r"Icon::new\((?:[^()]|\((?:[^()]|\([^()]*\))*\))*\)\s*\.(?:x)?small\(\)")
                .unwrap();
        let found: Vec<_> = (sources().into_iter())
            .filter(|(f, _)| !f.starts_with("appearance/"))
            .filter(|(_, text)| chain.is_match(text))
            .map(|(f, _)| f)
            .collect();
        assert!(found.is_empty(), "rem-sized icons in {found:?}");
    }

    #[gpui_kit::test]
    fn display_font_sizes_follow_interface(cx: &mut TestAppContext) {
        let mut settings = Settings::default();
        settings.interface.font_size = Some(20.);
        run(cx, &settings, None, &[]);
        cx.update(|cx| {
            assert_eq!(super::fonts::ui(cx, 28.), px(35.));
            assert_eq!(super::fonts::ui(cx, 42.), px(52.5));
        });
        let found = offenders(
            |f| f.starts_with("appearance/") || f == "terminal_view.rs",
            |l| l.contains("text_size(px("),
        );
        assert!(found.is_empty(), "fixed text sizes:\n{}", found.join("\n"));
        let mut wrong = Vec::new();
        for (file, text) in sources()
            .iter()
            .filter(|(f, _)| f != "aurora.rs" && !f.starts_with("appearance/"))
        {
            let lines: Vec<_> = text.lines().collect();
            for (n, _) in (lines.iter().enumerate())
                .filter(|(_, l)| l.contains("DISPLAY_FONT") && !l.trim_start().starts_with("use "))
            {
                let end = (lines.iter().enumerate().skip(n + 1))
                    .find(|(_, l)| l.contains(".child("))
                    .map_or(lines.len(), |(i, _)| i + 1);
                let chain = lines[n..end].join("\n");
                if chain.contains("fonts::chat(")
                    || chain.contains("text_size(px(")
                    || !chain.contains("fonts::ui(")
                {
                    wrong.push(format!("{file}:{}: not sized by fonts::ui", n + 1));
                }
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[gpui_kit::test]
    fn chat_text_follows_chat_size(cx: &mut TestAppContext) {
        let mut settings = Settings::default();
        settings.conversation.font_size = Some(26.);
        run(cx, &settings, None, &[]);
        cx.update(|cx| {
            assert_eq!(super::fonts::chat(cx, 13.), px(26.));
            assert_eq!(super::fonts::chat(cx, 12.), px(24.));
            assert_eq!(
                super::fonts::ui(cx, 13.),
                px(13.),
                "the interface keeps its size"
            );
        });
        let fixed = Regex::new(r"text_size\(px\(|\.text_(xs|sm|base)\(\)").unwrap();
        let found = offenders(|f| !f.starts_with("chat/"), |l| fixed.is_match(l));
        assert!(
            found.is_empty(),
            "fixed chat text sizes:\n{}",
            found.join("\n")
        );
    }

    #[gpui_kit::test]
    fn mono_text_reads_the_area_fonts(cx: &mut TestAppContext) {
        let mut settings = Settings::default();
        settings.editor.font_family = Some("Menlo".into());
        settings.editor.font_size = Some(26.);
        settings.conversation.code_font_family = Some("Courier".into());
        settings.conversation.code_font_size = Some(20.);
        settings.terminal.font_family = Some("Monaco".into());
        settings.terminal.font_size = Some(15.);
        run(cx, &settings, None, &["Menlo", "Courier", "Monaco"]);
        cx.update(|cx| {
            assert_eq!(super::fonts::editor_family(cx).as_ref(), "Menlo");
            assert_eq!(super::fonts::editor(cx, 11.), px(22.));
            assert_eq!(super::fonts::chat_code_family(cx).as_ref(), "Courier");
            assert_eq!(super::fonts::chat_code(cx, 13.), px(20.));
            let (family, size) = super::fonts::terminal(cx);
            assert_eq!((family.as_ref(), size), ("Monaco", px(15.)));
        });
        let found = offenders(
            |f| f.starts_with("appearance/"),
            |l| l.contains("mono_font_family") || l.contains("mono_font_size"),
        );
        assert!(
            found.is_empty(),
            "views reading the theme's mono font:\n{}",
            found.join("\n")
        );
    }

    fn fonts(cx: &mut TestAppContext) -> ResolvedFonts {
        cx.update(|cx| cx.global::<Fonts>().0.clone())
    }

    fn face(family: &str, size: f32) -> FontArea {
        FontArea {
            font_family: Some(family.into()),
            font_size: Some(size),
        }
    }

    fn overriding(text: &str) -> Settings {
        Settings {
            theme: ThemeSettings {
                overrides: toml::from_str(text).unwrap(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[gpui_kit::test]
    fn interface_family_sets_theme_font(cx: &mut TestAppContext) {
        let settings = Settings {
            interface: face("Inter", 15.),
            ..Default::default()
        };
        let notices = run(cx, &settings, None, &["Inter"]);
        assert!(notices.is_empty(), "{notices:?}");
        cx.update(|cx| {
            assert_eq!(cx.theme().font_family, "Inter");
            assert_eq!(cx.theme().font_size, px(15.));
        });
        assert_eq!(fonts(cx).ui.family, "Inter");
    }

    #[gpui_kit::test]
    fn editor_family_sets_mono_font(cx: &mut TestAppContext) {
        let settings = Settings {
            editor: face("Fira Code", 15.),
            ..Default::default()
        };
        run(cx, &settings, None, &["Fira Code"]);
        cx.update(|cx| {
            assert_eq!(cx.theme().mono_font_family, "Fira Code");
            assert_eq!(cx.theme().mono_font_size, px(15.));
        });
        // The terminal and the chat code follow the editor.
        let fonts = fonts(cx);
        assert_eq!(fonts.terminal.family, "Fira Code");
        assert_eq!(fonts.chat_code.size, 15.);
    }

    #[gpui_kit::test]
    fn terminal_font_overrides_editor(cx: &mut TestAppContext) {
        let mut settings = Settings {
            editor: face("Fira Code", 15.),
            ..Default::default()
        };
        settings.terminal.font_family = Some("Hack".into());
        settings.terminal.font_size = Some(11.);
        run(cx, &settings, None, &["Fira Code", "Hack"]);
        let fonts = fonts(cx);
        assert_eq!(
            (fonts.terminal.family.as_str(), fonts.terminal.size),
            ("Hack", 11.)
        );
        assert_eq!(
            (fonts.editor.family.as_str(), fonts.editor.size),
            ("Fira Code", 15.)
        );
        cx.update(|cx| assert_eq!(cx.theme().mono_font_family, "Fira Code"));
    }

    #[gpui_kit::test]
    fn chat_family_is_set(cx: &mut TestAppContext) {
        let mut settings = Settings::default();
        settings.conversation.font_family = Some("Georgia".into());
        settings.conversation.font_size = Some(16.);
        run(cx, &settings, None, &["Georgia"]);
        let fonts = fonts(cx);
        assert_eq!(
            (fonts.chat.family.as_str(), fonts.chat.size),
            ("Georgia", 16.)
        );
        cx.update(|cx| assert_ne!(cx.theme().font_family, "Georgia"));
    }

    #[gpui_kit::test]
    fn chat_code_font_is_set(cx: &mut TestAppContext) {
        let mut settings = Settings::default();
        settings.conversation.code_font_family = Some("JetBrains Mono".into());
        settings.conversation.code_font_size = Some(12.);
        run(cx, &settings, None, &["JetBrains Mono"]);
        let fonts = fonts(cx);
        assert_eq!(
            (fonts.chat_code.family.as_str(), fonts.chat_code.size),
            ("JetBrains Mono", 12.)
        );
        assert_ne!(fonts.editor.family, "JetBrains Mono");
    }

    #[gpui_kit::test]
    fn global_override_recolors_active_theme(cx: &mut TestAppContext) {
        let settings = overriding(r##""primary.background" = "#ff8800""##);
        let notices = run(cx, &settings, None, &[]);
        assert!(notices.is_empty(), "{notices:?}");
        cx.update(|cx| assert_eq!(cx.theme().primary, Hsla::from(rgb(0xff8800))));
    }

    #[gpui_kit::test]
    fn removed_override_restores_theme_color(cx: &mut TestAppContext) {
        run(
            cx,
            &overriding(r##""primary.background" = "#ff8800""##),
            None,
            &[],
        );
        run(cx, &Settings::default(), None, &[]);
        cx.update(|cx| assert_eq!(cx.theme().primary, Hsla::from(rgb(0x36e2c0))));
    }

    #[test]
    fn effective_mode_follows_os_only_for_system() {
        use WindowAppearance::{Dark, Light, VibrantDark, VibrantLight};
        for (os, mode) in [
            (Dark, ThemeMode::Dark),
            (VibrantDark, ThemeMode::Dark),
            (Light, ThemeMode::Light),
            (VibrantLight, ThemeMode::Light),
        ] {
            assert_eq!(effective_mode(ThemeChoice::System, os), mode);
        }
        for os in [Dark, Light, VibrantDark, VibrantLight] {
            assert_eq!(effective_mode(ThemeChoice::Dark, os), ThemeMode::Dark);
            assert_eq!(effective_mode(ThemeChoice::Light, os), ThemeMode::Light);
        }
    }

    #[gpui_kit::test]
    fn system_mode_follows_os_appearance(cx: &mut TestAppContext) {
        let settings = Settings {
            theme: ThemeSettings {
                mode: ThemeChoice::System,
                ..Default::default()
            },
            ..Default::default()
        };
        for (os, name, dark) in [
            (WindowAppearance::Dark, "wai Aurora", true),
            (WindowAppearance::Light, "wai Aurora Light", false),
            (WindowAppearance::VibrantDark, "wai Aurora", true),
            (WindowAppearance::Dark, "wai Aurora", true),
        ] {
            cx.update(|cx| {
                if !cx.has_global::<Theme>() {
                    gpui_kit::init(cx);
                }
                apply_with(&settings, None, &[], None, cx, os, &|_| None);
                assert_eq!(cx.theme().theme_name(), name, "{os:?}");
                assert_eq!(cx.theme().mode.is_dark(), dark, "{os:?}");
            });
        }
    }

    #[gpui_kit::test]
    fn fixed_mode_ignores_os_appearance(cx: &mut TestAppContext) {
        for (choice, mode) in [
            (ThemeChoice::Dark, ThemeMode::Dark),
            (ThemeChoice::Light, ThemeMode::Light),
        ] {
            let settings = Settings {
                theme: ThemeSettings {
                    mode: choice,
                    ..Default::default()
                },
                ..Default::default()
            };
            run(cx, &settings, None, &[]);
            cx.update(|cx| assert_eq!(cx.theme().mode, mode));
        }
    }

    #[gpui_kit::test]
    fn missing_theme_falls_back_with_notice(cx: &mut TestAppContext) {
        let settings = Settings {
            theme: ThemeSettings {
                dark: Some("Nope".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let notices = run(cx, &settings, None, &[]);
        assert_eq!(
            notices,
            ["Theme \"Nope\" is not installed; using wai Aurora."]
        );
        cx.update(|cx| assert_eq!(cx.theme().theme_name(), "wai Aurora"));
        assert_eq!(settings.theme.dark.as_deref(), Some("Nope"));
    }

    #[gpui_kit::test]
    fn missing_theme_returns_when_loaded(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("themes")).unwrap();
        let settings = Settings {
            theme: ThemeSettings {
                dark: Some("Nope".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(run(cx, &settings, Some(dir.path()), &[]).len(), 1);
        let theme = r##"{"name":"s","themes":[{"name":"Nope","mode":"dark","colors":{"background":"#102030"}}]}"##;
        std::fs::write(dir.path().join("themes/nope.json"), theme).unwrap();
        let notices = run(cx, &settings, Some(dir.path()), &[]);
        assert!(notices.is_empty(), "{notices:?}");
        cx.update(|cx| {
            assert_eq!(cx.theme().theme_name(), "Nope");
            assert_eq!(cx.theme().background, Hsla::from(rgb(0x102030)));
        });
    }
}
