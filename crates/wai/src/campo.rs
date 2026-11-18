//! "Campo": one look for every control, dark and light. Colors gpui-component paints itself come
//! from the theme (aurora.json); the shapes it cannot express live here.
//! Spec: docs/superpowers/specs/2026-11-16-wai-campo-design.md.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::*;
use wai_ext::panel::Tone;

/// Height of every clickable control in a row: field, select, button, menu trigger.
pub(crate) const H: f32 = 28.;
/// Control corners.
pub(crate) const RADIUS: f32 = 8.;

fn pick(cx: &App, dark: u32, light: u32) -> Hsla {
    rgba(if cx.theme().is_dark() { dark } else { light }).into()
}

/// Inputs, selects and logs sit below the surface.
pub(crate) fn recessed(cx: &App) -> Hsla {
    pick(cx, 0x0000_0033, 0x1419_3a0a)
}
/// Cards, list rows, the default button.
pub(crate) fn glass(cx: &App) -> Hsla {
    pick(cx, 0xffff_ff0b, 0xffff_ffff)
}
pub(crate) fn glass_hover(cx: &App) -> Hsla {
    pick(cx, 0xffff_ff17, 0x1419_3a0a)
}
/// A control's border.
pub(crate) fn line(cx: &App) -> Hsla {
    pick(cx, 0xffff_ff1a, 0x1419_3a1f)
}
/// Faint text: table headers, placeholders.
pub(crate) fn dim(cx: &App) -> Hsla {
    pick(cx, 0x626c_98ff, 0x8a91_adff)
}
/// Hovered rows, the active tab, focus.
pub(crate) fn violet(cx: &App) -> Hsla {
    pick(cx, 0x8d72_ffff, 0x7b5c_f0ff)
}

/// The focus halo: 3 px of violet around the control.
pub(crate) fn halo(cx: &App) -> Vec<BoxShadow> {
    let color = pick(cx, 0x8d72_ff33, 0x7b5c_f02e);
    vec![BoxShadow::new(px(0.), px(0.), color).spread_radius(px(3.))]
}

/// A field: 28 px, recessed, a hairline border; violet with the halo when `focused`.
/// gpui-component's `Input` and `Select` draw their own focus from the theme's `ring`, so pass
/// `false` for them.
pub(crate) fn field<E: Styled>(el: E, focused: bool, cx: &App) -> E {
    let el = el.h(px(H)).rounded(px(RADIUS)).border_1().bg(recessed(cx));
    match focused {
        true => el.border_color(cx.theme().ring).shadow(halo(cx)),
        false => el.border_color(line(cx)),
    }
}

/// A tone's (background, text, border): tinted, the tone as text.
pub(crate) fn tint(tone: Tone, cx: &App) -> (Hsla, Hsla, Hsla) {
    let theme = cx.theme();
    let color = match tone {
        Tone::Neutral => return (glass_hover(cx), theme.muted_foreground, line(cx)),
        Tone::Info => theme.info,
        Tone::Success => theme.success,
        Tone::Warning => theme.warning,
        Tone::Danger => theme.danger,
    };
    let fill = if theme.is_dark() { 0.15 } else { 0.12 };
    (color.opacity(fill), color, color.opacity(0.38))
}

/// The default button: glass (the theme's `button.*`), 28 px, 12 px semibold.
pub(crate) fn button(b: Button) -> Button {
    Styled::h(b, px(H))
        .px_3()
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
}

/// The one filled button: cyan.
pub(crate) fn primary(b: Button) -> Button {
    button(b.primary())
}

/// Tinted red (the theme's `button.danger.*`) in a red border.
pub(crate) fn danger(b: Button, cx: &App) -> Button {
    let (_, _, border) = tint(Tone::Danger, cx);
    button(b.danger()).border_1().border_color(border)
}

/// An icon-only button: a 28 px square.
pub(crate) fn icon(b: Button) -> Button {
    b.ghost().size(px(H))
}

/// A small uppercase label: table and section headers, settings groups.
pub(crate) fn label(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_size(px(10.5))
        .font_weight(FontWeight::BOLD)
        .text_color(cx.theme().muted_foreground)
        .child(text.into().to_uppercase())
}

/// A row of a list or table standing on its own: glass, violet under the pointer.
pub(crate) fn loose_row(cx: &App) -> Div {
    let violet = violet(cx);
    gpui_kit::component::h_flex()
        .rounded(px(9.))
        .border_1()
        .border_color(line(cx))
        .bg(glass(cx))
        .hover(move |el| {
            el.bg(violet.opacity(0.16))
                .border_color(violet.opacity(0.45))
        })
}

/// A tinted badge, no border.
pub(crate) fn badge(text: impl Into<SharedString>, tone: Tone, cx: &App) -> Div {
    let (fill, color, _) = tint(tone, cx);
    div()
        .flex_none()
        .px(px(6.))
        .rounded(px(6.))
        .bg(fill)
        .text_size(px(10.5))
        .font_weight(FontWeight::BOLD)
        .text_color(color)
        .child(text.into())
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;
    use wai_ext::panel::Tone;

    use crate::app;
    use crate::aurora::{erro, hex};

    #[gpui_kit::test]
    fn tint_mixes_the_tone(cx: &mut TestAppContext) {
        cx.update(|cx| {
            app::init(&Default::default(), cx);
            let (fill, text, border) = super::tint(Tone::Danger, cx);
            assert_eq!(hex(text), hex(erro(cx)));
            assert!((fill.a - 0.15).abs() < 0.01);
            assert!((border.a - 0.38).abs() < 0.01);
        });
    }
}
