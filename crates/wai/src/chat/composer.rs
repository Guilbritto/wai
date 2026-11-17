//! What the draft screen and a running conversation share on their prompt box: the strip of
//! chips on its top edge, the chips of its bottom bar, and the extension panels drawn in and
//! beside it.

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::aurora;
use crate::ext_panel::ExtPanel;

/// An extension's prompt panel (API 1.6) and its width.
#[derive(Clone, PartialEq)]
pub struct PromptSlot {
    pub view: Entity<ExtPanel>,
    pub width: Pixels,
}

/// The prompt panels of `owner`, the focused pane's session or draft: `inside` its prompt box,
/// `side` beside it. The workspace sets it when one of them changes.
#[derive(Clone, Default, PartialEq)]
pub struct PromptSlots {
    pub owner: Option<EntityId>,
    pub inside: Option<PromptSlot>,
    pub side: Option<PromptSlot>,
}

impl Global for PromptSlots {}

/// The narrowest the prompt box gets for a slot: in a narrower pane the slots hide.
pub(super) const MIN_FIELD: Pixels = px(360.);

/// The prompt box's row: `strip` on `field`, which holds `content`, plus the slots of `owner`
/// that fit `width`, the row's width last frame. `measured` gets this frame's.
pub(super) fn prompt_row(
    owner: EntityId,
    strip: impl IntoElement,
    field: Stateful<Div>,
    content: Div,
    width: Pixels,
    measured: impl Fn(Pixels, &mut App) + 'static,
    cx: &App,
) -> Div {
    let slots = cx
        .try_global::<PromptSlots>()
        .filter(|s| s.owner == Some(owner));
    let mut room = width - MIN_FIELD;
    let mut fit = |slot: Option<&PromptSlot>| {
        let slot = slot.filter(|s| s.width <= room)?.clone();
        room -= slot.width;
        Some(slot)
    };
    let inside = fit(slots.and_then(|s| s.inside.as_ref()));
    let side = fit(slots.and_then(|s| s.side.as_ref()));
    let theme = cx.theme();
    let dark = theme.is_dark();
    // A click in a slot is the extension's: it does not put the caret in the textarea.
    let slot = |id: &'static str, slot: PromptSlot| {
        div()
            .id(id)
            .flex_none()
            .w(slot.width)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(slot.view)
    };
    let field = match inside {
        Some(inside) => field
            .flex_row()
            .child(content.flex_1().min_w_0().p_3())
            .child(
                slot("prompt-slot", inside)
                    .border_l_1()
                    .border_color(match dark {
                        true => aurora::borda(),
                        false => theme.border,
                    })
                    // Registers the id so tests can find it.
                    .test_support(),
            ),
        None => field.p_3().child(content),
    }
    // Registers the id so tests can find it.
    .test_support();
    let side = side.map(|side| {
        let el = slot("prompt-side", side)
            .rounded(px(20.))
            .border_1()
            .overflow_hidden();
        match dark {
            true => el
                .bg(aurora::noite_2().alpha(0.78))
                .border_color(aurora::borda_forte()),
            false => el.bg(theme.background).border_color(theme.input),
        }
        // Registers the id so tests can find it.
        .test_support()
    });
    let measure = canvas(
        move |bounds, _, cx| measured(bounds.size.width, cx),
        |_, _, _, _| {},
    )
    .absolute()
    .size_full();
    h_flex()
        .relative()
        .w_full()
        .items_end()
        .gap_2()
        .child(measure)
        .child(v_flex().flex_1().min_w_0().child(strip).child(field))
        .children(side)
}

/// `.strip`: one tab sitting on the composer's top edge, 18 px in from its left. Add the chips
/// as children, then put it in `strip_row`.
pub(super) fn strip(id: &'static str, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let tab = h_flex()
        .id(id)
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
