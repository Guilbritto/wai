//! wai's own colors (the `"wai"` object of a theme): the Aurora palette the views paint with.
//! A key a theme leaves out is derived from the theme's base colors.

use gpui_kit::component::{ThemeColor, try_parse_color};
use gpui_kit::{App, Global, Hsla};
use serde_json::{Map, Value};

/// The keys of a theme's `"wai"` object, glows last.
pub(crate) const KEYS: &[&str] = &[
    "noite",
    "noite_2",
    "boreal",
    "lilas",
    "magenta",
    "ceu",
    "brasa",
    "ambar",
    "erro",
    "tinta",
    "tinta_2",
    "nevoa",
    "nevoa_2",
    "ok",
    "vidro",
    "vidro_forte",
    "borda",
    "borda_forte",
    "card_fill",
    "backdrop.glow_1",
    "backdrop.glow_2",
    "backdrop.glow_3",
];

const GLOWS: [&str; 3] = ["backdrop.glow_1", "backdrop.glow_2", "backdrop.glow_3"];

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Palette {
    pub noite: Hsla,
    pub noite_2: Hsla,
    pub boreal: Hsla,
    pub lilas: Hsla,
    pub magenta: Hsla,
    pub ceu: Hsla,
    pub brasa: Hsla,
    pub ambar: Hsla,
    pub erro: Hsla,
    pub tinta: Hsla,
    pub tinta_2: Hsla,
    pub nevoa: Hsla,
    pub nevoa_2: Hsla,
    pub ok: Hsla,
    pub vidro: Hsla,
    pub vidro_forte: Hsla,
    pub borda: Hsla,
    pub borda_forte: Hsla,
    pub card_fill: Hsla,
    /// The three backdrop blobs' base colors; `None` where the theme sets none.
    pub glows: [Option<Hsla>; 3],
}

impl Global for Palette {}

impl Palette {
    pub fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    /// Each key from `wai`, else derived from `colors`; a value that does not parse is reported
    /// and derived too.
    pub fn from(wai: &Map<String, Value>, colors: &ThemeColor) -> (Self, Vec<String>) {
        let mut notices = Vec::new();
        let mut get = |key: &str, derived: Hsla| -> Hsla {
            match wai.get(key) {
                None => derived,
                Some(value) => parse(value)
                    .inspect_err(|reason| {
                        notices.push(format!("Ignored wai color \"{key}\": {reason}."))
                    })
                    .unwrap_or(derived),
            }
        };
        let c = colors;
        let palette = Palette {
            noite: get("noite", c.background),
            noite_2: get("noite_2", c.popover),
            boreal: get("boreal", c.primary),
            lilas: get("lilas", c.primary),
            magenta: get("magenta", c.danger),
            ceu: get("ceu", c.info),
            brasa: get("brasa", c.warning),
            ambar: get("ambar", c.warning),
            erro: get("erro", c.danger),
            tinta: get("tinta", c.foreground),
            tinta_2: get("tinta_2", c.foreground.opacity(0.8)),
            nevoa: get("nevoa", c.muted_foreground),
            nevoa_2: get("nevoa_2", c.muted_foreground.opacity(0.7)),
            ok: get("ok", c.success),
            vidro: get("vidro", c.foreground.opacity(0.055)),
            vidro_forte: get("vidro_forte", c.foreground.opacity(0.09)),
            borda: get("borda", c.border),
            borda_forte: get("borda_forte", c.border),
            card_fill: get("card_fill", c.background.opacity(0.84)),
            glows: GLOWS.map(|key| {
                let value = wai.get(key)?;
                parse(value)
                    .inspect_err(|reason| {
                        notices.push(format!("Ignored wai color \"{key}\": {reason}."))
                    })
                    .ok()
            }),
        };
        (palette, notices)
    }
}

/// A gpui-kit color string (`#rrggbb`, `#rrggbbaa`, `white/5.5`) or `#rrggbb/<opacity percent>`.
pub(crate) fn parse(value: &Value) -> Result<Hsla, String> {
    let text = value.as_str().ok_or("not a string")?;
    match text.split_once('/').filter(|_| text.starts_with('#')) {
        Some((color, percent)) => {
            let percent: f32 = percent.parse().map_err(|_| "bad opacity".to_string())?;
            if !(0. ..=100.).contains(&percent) {
                return Err("opacity is not between 0 and 100".into());
            }
            let color = try_parse_color(color).map_err(|e| e.to_string())?;
            Ok(color.opacity(percent / 100.))
        }
        None => try_parse_color(text).map_err(|e| e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{hsla, rgb};
    use serde_json::json;

    use super::*;
    use crate::appearance::catalog;

    const BUNDLED: &str = include_str!("../../assets/theme/aurora.json");

    fn white(alpha: f32) -> Hsla {
        hsla(0., 0., 1., alpha)
    }

    /// The values `aurora.rs` drew before the palette moved into the theme file.
    fn old(key: &str) -> Hsla {
        match key {
            "noite" => rgb(0x070a18).into(),
            "noite_2" => rgb(0x0d1230).into(),
            "boreal" => rgb(0x36e2c0).into(),
            "lilas" => rgb(0x8d72ff).into(),
            "magenta" => rgb(0xff6ec7).into(),
            "ceu" => rgb(0x5bb8ff).into(),
            "brasa" => rgb(0xff8f4f).into(),
            "ambar" => rgb(0xffc15a).into(),
            "erro" => rgb(0xff6b8b).into(),
            "tinta" => rgb(0xeceffe).into(),
            "tinta_2" => rgb(0xc4cbeb).into(),
            "nevoa" => rgb(0x8f99c2).into(),
            "nevoa_2" => rgb(0x626c98).into(),
            "ok" => rgb(0x46e39a).into(),
            "vidro" => white(0.055),
            "vidro_forte" => white(0.09),
            "borda" => white(0.1),
            "borda_forte" => white(0.16),
            "card_fill" => Hsla::from(rgb(0x080b1c)).alpha(0.84),
            _ => unreachable!("{key}"),
        }
    }

    fn fields(p: &Palette) -> Vec<(&'static str, Hsla)> {
        vec![
            ("noite", p.noite),
            ("noite_2", p.noite_2),
            ("boreal", p.boreal),
            ("lilas", p.lilas),
            ("magenta", p.magenta),
            ("ceu", p.ceu),
            ("brasa", p.brasa),
            ("ambar", p.ambar),
            ("erro", p.erro),
            ("tinta", p.tinta),
            ("tinta_2", p.tinta_2),
            ("nevoa", p.nevoa),
            ("nevoa_2", p.nevoa_2),
            ("ok", p.ok),
            ("vidro", p.vidro),
            ("vidro_forte", p.vidro_forte),
            ("borda", p.borda),
            ("borda_forte", p.borda_forte),
            ("card_fill", p.card_fill),
        ]
    }

    fn bundled(name: &str, mode: gpui_kit::component::ThemeMode) -> Map<String, Value> {
        let (catalog, notices) = catalog::load(BUNDLED, None, &[]);
        assert!(notices.is_empty(), "{notices:?}");
        catalog.find(name, mode).unwrap().wai.clone()
    }

    #[test]
    fn aurora_keys_match_old_constants() {
        use gpui_kit::component::ThemeMode::{Dark, Light};
        for (name, mode) in [("wai Aurora", Dark), ("wai Aurora Light", Light)] {
            let wai = bundled(name, mode);
            let (palette, notices) = Palette::from(&wai, &ThemeColor::default());
            assert!(notices.is_empty(), "{notices:?}");
            for (key, color) in fields(&palette) {
                assert_eq!(color, old(key), "{name} {key}");
            }
        }
    }

    #[test]
    fn glows_only_where_theme_sets_them() {
        use gpui_kit::component::ThemeMode::{Dark, Light};
        let colors = ThemeColor::default();
        let (dark, _) = Palette::from(&bundled("wai Aurora", Dark), &colors);
        assert_eq!(
            dark.glows,
            [
                Some(rgb(0x36e2c0).into()),
                Some(rgb(0x8d72ff).into()),
                Some(rgb(0xff6ec7).into())
            ]
        );
        let (light, _) = Palette::from(&bundled("wai Aurora Light", Light), &colors);
        assert_eq!(light.glows, [None, None, None]);
        let one = json!({"backdrop.glow_2": "#112233"});
        let (partial, _) = Palette::from(one.as_object().unwrap(), &colors);
        assert_eq!(partial.glows, [None, Some(rgb(0x112233).into()), None]);
    }

    #[test]
    fn missing_wai_keys_derive_from_base_colors() {
        let colors = ThemeColor::dark();
        let (p, notices) = Palette::from(&Map::new(), &colors);
        assert!(notices.is_empty());
        let expected = [
            ("noite", colors.background),
            ("noite_2", colors.popover),
            ("boreal", colors.primary),
            ("lilas", colors.primary),
            ("magenta", colors.danger),
            ("ceu", colors.info),
            ("brasa", colors.warning),
            ("ambar", colors.warning),
            ("erro", colors.danger),
            ("ok", colors.success),
            ("tinta", colors.foreground),
            ("tinta_2", colors.foreground.opacity(0.8)),
            ("nevoa", colors.muted_foreground),
            ("nevoa_2", colors.muted_foreground.opacity(0.7)),
            ("vidro", colors.foreground.opacity(0.055)),
            ("vidro_forte", colors.foreground.opacity(0.09)),
            ("borda", colors.border),
            ("borda_forte", colors.border),
            ("card_fill", colors.background.opacity(0.84)),
        ];
        let got = fields(&p);
        for (key, want) in expected {
            let (_, have) = got.iter().find(|(k, _)| *k == key).unwrap();
            assert_eq!(*have, want, "{key}");
        }
        assert_eq!(p.glows, [None, None, None]);
    }

    #[test]
    fn bad_value_notifies_and_derives() {
        let colors = ThemeColor::dark();
        let wai = json!({"boreal": "nonsense", "ok": "#00ff00/150", "ceu": 5});
        let (p, notices) = Palette::from(wai.as_object().unwrap(), &colors);
        assert_eq!(p.boreal, colors.primary);
        assert_eq!(p.ok, colors.success);
        assert_eq!(p.ceu, colors.info);
        assert_eq!(notices.len(), 3, "{notices:?}");
        assert!(notices[0].starts_with("Ignored wai color \"boreal\": "));
    }
}
