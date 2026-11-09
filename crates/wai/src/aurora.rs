//! The "wai Aurora" look, after the Central de IA: its palette, its gpui-component theme (the dark
//! one; light mode keeps Default Light) and the bundled display font.
//! Palette: docs/superpowers/specs/2026-11-09-wai-visual-aurora-design.md.
#![expect(
    dead_code,
    reason = "the views take the rest of the palette in the next tasks"
)]

use std::borrow::Cow;
use std::rc::Rc;

use gpui_kit::base::{ResizeHandleRenderer, ResizeHandleState};
use gpui_kit::component::{ActiveTheme as _, Theme, ThemeRegistry, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Axis, Bounds, BoxShadow, Corners, Div, FontWeight, Hsla, InteractiveElement,
    IntoElement, ParentElement, Styled, canvas, div, hsla, linear_color_stop, linear_gradient,
    point, px, rgb, size,
};

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

/// The static aurora behind the cards, dark only: the Central's `.aurora` base gradient and its
/// three blobs, each a circle painted only by a blurred shadow, centred where `.aurora-1..3` put
/// their 64 vmax circles.
pub(crate) fn backdrop(cx: &App) -> Option<AnyElement> {
    if !cx.theme().is_dark() {
        return None;
    }
    let blobs = canvas(
        |_, _, _| {},
        |bounds, _, window, _| {
            let (w, h) = (bounds.size.width, bounds.size.height);
            let vmax = w.max(h) / 100.;
            // Center, colour. A 28 vmax disc blurred by 7 vmax peaks near the colour's alpha and
            // fades out around 23 vmax, like the Central's radial gradients.
            let blobs = [
                (point(vmax * 14., vmax * 2.), boreal().alpha(0.30)),
                (point(w - vmax * 10., vmax * 8.), lilas().alpha(0.34)),
                (point(vmax * 44., h + vmax * 10.), magenta().alpha(0.20)),
            ];
            for (center, color) in blobs {
                let disc =
                    Bounds::centered_at(bounds.origin + center, size(vmax * 28., vmax * 28.));
                let shadow = BoxShadow::new(px(0.), px(0.), color).blur_radius(vmax * 7.);
                window.paint_drop_shadows(disc, Corners::all(vmax * 14.), &[shadow]);
            }
        },
    )
    .size_full();
    let base = linear_gradient(
        180.,
        linear_color_stop(rgb(0x060816), 0.),
        linear_color_stop(rgb(0x0a0e24), 1.),
    );
    let backdrop = div()
        .debug_selector(|| "aurora-backdrop".into())
        .absolute()
        .inset_0()
        .overflow_hidden()
        .bg(base)
        .child(blobs);
    Some(backdrop.into_any_element())
}

/// A floating card (`.painel-cartao`): dark glass with a violet glow; `focused` (the focused
/// pane of a split) gets a violet border and a stronger glow. Light mode: a plain bordered panel.
pub(crate) fn card(focused: bool, cx: &App) -> Div {
    let theme = cx.theme();
    if !theme.is_dark() {
        let border = if focused { theme.ring } else { theme.border };
        return div()
            .bg(theme.background)
            .border_1()
            .border_color(border)
            .rounded(px(12.))
            .overflow_hidden();
    }
    let (border, glow) = if focused {
        (lilas().alpha(0.5), lilas().alpha(0.8))
    } else {
        (borda(), lilas().alpha(0.45))
    };
    div()
        .bg(card_fill())
        .border_1()
        .border_color(border)
        .rounded(px(16.))
        // The Central's `0 40 80 -40 black .95` and `0 0 60 -30 lilas .45`. GPUI's blur radius is
        // the Gaussian's sigma, half the CSS one. CSS stacks the first shadow on top and GPUI
        // paints them in order, so the black one goes last: under the 84% fill it hides the
        // glow, which would tint the whole card violet.
        .shadow(vec![
            BoxShadow::new(px(0.), px(0.), glow)
                .blur_radius(px(30.))
                .spread_radius(px(-30.)),
            BoxShadow::new(px(0.), px(40.), hsla(0., 0., 0., 0.95))
                .blur_radius(px(40.))
                .spread_radius(px(-40.)),
        ])
        .overflow_hidden()
}

/// A card's header strip (`.painel-barra`).
pub(crate) fn card_bar(cx: &App) -> Div {
    let theme = cx.theme();
    let bar = h_flex().flex_shrink_0().border_b_1();
    if !theme.is_dark() {
        return bar.border_color(theme.border);
    }
    bar.bg(linear_gradient(
        180.,
        linear_color_stop(white(0.05), 0.),
        linear_color_stop(white(0.015), 1.),
    ))
    .border_color(white(0.06))
}

/// A chip (`.chip`): fully rounded, glass.
pub(crate) fn pill(cx: &App) -> Div {
    let theme = cx.theme();
    let (fill, border, text) = if theme.is_dark() {
        (vidro(), borda(), tinta_2())
    } else {
        (theme.secondary, theme.border, theme.secondary_foreground)
    };
    h_flex()
        .h(px(26.))
        .px(px(10.))
        .gap(px(6.))
        .rounded_full()
        .bg(fill)
        .border_1()
        .border_color(border)
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(text)
}

/// A selectable chip for tabs and rows (`.janela-aba`): the selected one is lilas-tinted. The
/// caller sets size and radius. Light mode: theme tokens.
pub(crate) fn tab(active: bool, cx: &App) -> Div {
    let theme = cx.theme();
    let base = h_flex()
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .cursor_pointer();
    if !theme.is_dark() {
        return if active {
            base.bg(theme.accent)
                .border_color(theme.border)
                .text_color(theme.accent_foreground)
        } else {
            base.text_color(theme.muted_foreground)
                .hover(|el| el.bg(theme.accent))
        };
    }
    if active {
        base.bg(lilas().alpha(0.16))
            .border_color(lilas().alpha(0.45))
            .text_color(tinta())
    } else {
        base.text_color(nevoa()).hover(|el| el.bg(vidro()))
    }
}

/// Resize handles draw nothing at rest (the gap between cards is the divider) and a
/// `drag_border` line while hovered or dragged. gpui-component's own renderer paints a
/// `theme.border` hairline at rest.
pub(crate) fn handle() -> ResizeHandleRenderer {
    Rc::new(|handle, _, cx| {
        let line = div()
            .flex_none()
            .when(handle.state() != ResizeHandleState::Idle, |el| {
                el.bg(cx.theme().drag_border)
            });
        let line = match handle.axis() {
            Axis::Horizontal => line.w(px(1.)).h_full(),
            Axis::Vertical => line.h(px(1.)).w_full(),
        };
        Some(line.into_any_element())
    })
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
