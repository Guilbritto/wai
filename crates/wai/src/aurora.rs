//! The "wai Aurora" look, after the Central de IA: its palette, its gpui-component theme (the dark
//! one; light mode keeps Default Light) and the bundled display font.
//! Palette: docs/superpowers/specs/2026-11-09-wai-visual-aurora-design.md.
#![expect(
    dead_code,
    reason = "the views take the rest of the palette in the next tasks"
)]

use std::borrow::Cow;

use gpui_kit::component::{Theme, ThemeRegistry};
use gpui_kit::{App, Hsla, hsla, rgb};

/// Nunito Bold (OFL 1.1, `assets/fonts/OFL.txt`), for the brand and titles.
pub(crate) const DISPLAY_FONT: &str = "Nunito";
const FONT: &[u8] = include_bytes!("../assets/fonts/Nunito-Bold.ttf");
const THEME: &str = include_str!("../assets/theme/aurora.json");

/// Adds the display font and makes "wai Aurora" the dark theme. Between `gpui_kit::init` (the
/// global theme exists) and `Theme::change` (which applies it).
pub(crate) fn register(cx: &mut App) {
    if let Err(err) = cx.text_system().add_fonts(vec![Cow::Borrowed(FONT)]) {
        eprintln!("wai: the display font did not load: {err:#}");
    }
    let registry = ThemeRegistry::global_mut(cx);
    registry
        .load_themes_from_str(THEME)
        .expect("aurora.json is a theme set");
    let aurora = registry.themes()["wai Aurora"].clone();
    Theme::global_mut(cx).dark_theme = aurora;
}

fn white(alpha: f32) -> Hsla {
    hsla(0., 0., 1., alpha)
}

/// Window background.
pub(crate) fn noite() -> Hsla {
    rgb(0x070a18).into()
}
/// Popovers, dialogs, menus.
pub(crate) fn noite_2() -> Hsla {
    rgb(0x0d1230).into()
}
/// Secondary surfaces.
pub(crate) fn noite_3() -> Hsla {
    rgb(0x141a3d).into()
}
/// Primary, focus ring, "working", send.
pub(crate) fn boreal() -> Hsla {
    rgb(0x36e2c0).into()
}
/// Active tab, selection, focused card, user bubble.
pub(crate) fn lilas() -> Hsla {
    rgb(0x8d72ff).into()
}
pub(crate) fn magenta() -> Hsla {
    rgb(0xff6ec7).into()
}
/// Info, links.
pub(crate) fn ceu() -> Hsla {
    rgb(0x5bb8ff).into()
}
/// Agent header star, eyebrows, numbered badges.
pub(crate) fn brasa() -> Hsla {
    rgb(0xff8f4f).into()
}
pub(crate) fn ok() -> Hsla {
    rgb(0x46e39a).into()
}
/// Warning, "needs you".
pub(crate) fn ambar() -> Hsla {
    rgb(0xffc15a).into()
}
pub(crate) fn erro() -> Hsla {
    rgb(0xff6b8b).into()
}
/// Text.
pub(crate) fn tinta() -> Hsla {
    rgb(0xeceffe).into()
}
/// Secondary text.
pub(crate) fn tinta_2() -> Hsla {
    rgb(0xc4cbeb).into()
}
/// Muted text.
pub(crate) fn nevoa() -> Hsla {
    rgb(0x8f99c2).into()
}
/// Faint text.
pub(crate) fn nevoa_2() -> Hsla {
    rgb(0x626c98).into()
}
/// Hover, chips, inputs.
pub(crate) fn vidro() -> Hsla {
    white(0.055)
}
pub(crate) fn vidro_forte() -> Hsla {
    white(0.09)
}
pub(crate) fn borda() -> Hsla {
    white(0.1)
}
pub(crate) fn borda_forte() -> Hsla {
    white(0.16)
}
/// A card's fill (the Central's `.painel-cartao`).
pub(crate) fn card_fill() -> Hsla {
    Hsla::from(rgb(0x080b1c)).alpha(0.84)
}

/// `c` as 0xRRGGBBAA, each channel rounded to a byte: a color parsed from the theme file and the
/// same one built here may differ in the last float bits.
#[cfg(test)]
pub(crate) fn hex(c: Hsla) -> u32 {
    let c = gpui_kit::Rgba::from(c);
    [c.r, c.g, c.b, c.a]
        .into_iter()
        .fold(0, |acc, v| acc << 8 | (v * 255.).round() as u32)
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;
    use gpui_kit::component::ActiveTheme as _;
    use wai_settings::{Settings, ThemeChoice};

    use super::*;
    use crate::app;

    #[gpui_kit::test]
    fn aurora_is_the_default_dark_theme(cx: &mut TestAppContext) {
        cx.update(|cx| {
            app::init(&Settings::default(), cx);
            let theme = cx.theme();
            assert_eq!(theme.theme_name(), "wai Aurora");
            assert_eq!(hex(theme.background), hex(noite()));
            assert_eq!(hex(theme.primary), hex(boreal()));
            // Keys the theme file misspells are skipped silently (Default Dark's own
            // `link.foreground` and `drag_border` are).
            assert_eq!(hex(theme.link), hex(ceu()));
            assert_eq!(hex(theme.drag_border), hex(lilas()));
        });
    }

    #[gpui_kit::test]
    fn light_theme_stays_default(cx: &mut TestAppContext) {
        let settings = Settings {
            theme: ThemeChoice::Light,
            ..Default::default()
        };
        cx.update(|cx| {
            app::init(&settings, cx);
            assert_eq!(cx.theme().theme_name(), "Default Light");
        });
    }

    /// GPUI's test platform has no fonts (`add_fonts` keeps nothing, `all_font_names` is empty),
    /// so this checks what a real one reads: `register` runs, and the bundled file is a TrueType
    /// font whose family (name id 1) is `DISPLAY_FONT`. The running app lists it (R77).
    #[gpui_kit::test]
    fn display_font_is_registered(cx: &mut TestAppContext) {
        cx.update(|cx| app::init(&Settings::default(), cx));
        assert_eq!(FONT[..4], [0, 1, 0, 0], "a TrueType file");
        assert_eq!(family(FONT), DISPLAY_FONT);
    }

    /// The family name in `ttf`'s `name` table, from its Windows (UTF-16BE) record.
    fn family(ttf: &[u8]) -> String {
        let u16_at = |i: usize| u16::from_be_bytes([ttf[i], ttf[i + 1]]) as usize;
        let u32_at = |i: usize| u32::from_be_bytes(ttf[i..i + 4].try_into().unwrap()) as usize;
        let mut tables = (0..u16_at(4)).map(|n| 12 + 16 * n);
        let name = tables
            .find(|&t| &ttf[t..t + 4] == b"name")
            .expect("a name table");
        let table = u32_at(name + 8);
        let mut records = (0..u16_at(table + 2)).map(|n| table + 6 + 12 * n);
        let record = (records.find(|&r| u16_at(r) == 3 && u16_at(r + 6) == 1))
            .expect("a Windows family name");
        let start = table + u16_at(table + 4) + u16_at(record + 10);
        let utf16 = ttf[start..start + u16_at(record + 8)].chunks(2);
        char::decode_utf16(utf16.map(|c| u16::from_be_bytes([c[0], c[1]])))
            .collect::<Result<_, _>>()
            .unwrap()
    }
}
