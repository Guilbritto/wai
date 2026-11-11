//! What the draft screen and a running conversation share on their prompt box: the strip of
//! chips on its top edge and the chips of its bottom bar.

use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::aurora;

/// `.strip`: one tab sitting on the composer's top edge, 18 px in from its left. Add the chips
/// as children, then put it in `strip_row`.
pub(super) fn strip(cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let tab = h_flex()
        .id("composer-strip")
        .min_w_0()
        .gap(px(2.))
        .px(px(6.))
        .py(px(5.))
        .rounded_t(px(14.))
        .border_1()
        .border_b_0();
    match theme.is_dark() {
        true => tab
            .bg(hsla(0., 0., 1., 0.035))
            .border_color(aurora::borda()),
        false => tab.border_color(theme.border),
    }
}

/// The tab starts 18 px in from the composer's edge and never runs past the other one.
pub(super) fn strip_row(tab: impl IntoElement) -> Div {
    h_flex().w_full().pl(px(18.)).child(tab)
}

/// `.chip`: no box at rest, a vidro fill on hover. `bar`: the composer bar's smaller, fainter
/// chips (`.cbar .chip`). A chip with `shrink > 0` gives up its width (and truncates) before
/// the strip leaves the composer.
pub(super) fn chip(
    id: impl Into<ElementId>,
    content: impl IntoElement,
    bar: bool,
    shrink: f32,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let dark = theme.is_dark();
    let (text, hover) = match (dark, bar) {
        (true, true) => (aurora::nevoa(), aurora::nevoa()),
        (true, false) => (aurora::tinta_2(), aurora::tinta()),
        (false, _) => (theme.muted_foreground, theme.foreground),
    };
    let fill = theme.muted;
    h_flex()
        .id(id)
        .when(shrink == 0., |el| el.flex_none())
        .when(shrink > 0., |el| {
            el.flex_shrink(shrink).min_w(px(48.)).overflow_hidden()
        })
        .h(px(if bar { 28. } else { 30. }))
        .px(px(4.))
        .rounded_full()
        .text_size(px(if bar { 13. } else { 13.5 }))
        .text_color(text)
        .hover(move |el| {
            let fill = match dark {
                true => aurora::vidro(),
                false => fill,
            };
            el.bg(fill).text_color(hover)
        })
        .child(content)
}

/// A chip of the bottom bar.
pub(super) fn bar_chip(
    id: impl Into<ElementId>,
    content: impl IntoElement,
    cx: &App,
) -> Stateful<Div> {
    chip(id, content, true, 0., cx)
}

/// A strip chip that only tells: the session's folder, branch and agent are fixed once it
/// runs. No hover, no chevron.
pub(super) fn strip_label(
    id: &'static str,
    mark: Option<AnyElement>,
    text: String,
    cx: &App,
) -> impl IntoElement + use<> {
    let theme = cx.theme();
    let color = match theme.is_dark() {
        true => aurora::tinta_2(),
        false => theme.muted_foreground,
    };
    h_flex()
        .id(id)
        .flex_shrink(1.)
        .min_w(px(48.))
        .overflow_hidden()
        .h(px(30.))
        .px(px(10.))
        .gap(px(6.))
        .items_center()
        .text_size(px(13.5))
        .text_color(color)
        .children(mark)
        .child(div().min_w_0().truncate().child(text.clone()))
        .aria_label(text)
        // Registers the id so tests can find it.
        .test_support()
}
