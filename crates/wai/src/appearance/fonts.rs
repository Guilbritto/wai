//! The sizes and families views draw in, read from the `Fonts` global. wai's own text sizes are
//! written for the default size of their area; these scale them with the user's.

use gpui_kit::component::text::TextViewStyle;
use gpui_kit::{App, Pixels, SharedString, StyleRefinement, Styled as _, px};
use wai_settings::fonts::{CHAT_SIZE, EDITOR_SIZE, Face, UI_SIZE};

use super::Fonts;

/// `base` scaled by `size / default`, to the nearest half pixel: at the default size every
/// base that is a multiple of 0.5 comes back unchanged.
pub(crate) fn scale(base: f32, size: f32, default: f32) -> f32 {
    (base * size / default * 2.0).round() / 2.0
}

fn face(cx: &App, pick: impl Fn(&Fonts) -> &Face) -> Face {
    pick(cx.global::<Fonts>()).clone()
}

/// A text size of the interface (`base` px at the default size).
pub(crate) fn ui(cx: &App, base: f32) -> Pixels {
    px(scale(base, face(cx, |f| &f.0.ui).size, UI_SIZE))
}

/// A text size of the editor and the mono labels.
pub(crate) fn editor(cx: &App, base: f32) -> Pixels {
    px(scale(base, face(cx, |f| &f.0.editor).size, EDITOR_SIZE))
}

/// A text size of the chat transcript and prompt box.
pub(crate) fn chat(cx: &App, base: f32) -> Pixels {
    px(scale(base, face(cx, |f| &f.0.chat).size, CHAT_SIZE))
}

/// A text size of the chat's code blocks and diffs.
pub(crate) fn chat_code(cx: &App, base: f32) -> Pixels {
    px(scale(base, face(cx, |f| &f.0.chat_code).size, EDITOR_SIZE))
}

pub(crate) fn terminal(cx: &App) -> (SharedString, Pixels) {
    let face = face(cx, |f| &f.0.terminal);
    (face.family.into(), px(face.size))
}

pub(crate) fn editor_family(cx: &App) -> SharedString {
    face(cx, |f| &f.0.editor).family.into()
}

pub(crate) fn chat_family(cx: &App) -> SharedString {
    face(cx, |f| &f.0.chat).family.into()
}

pub(crate) fn chat_code_family(cx: &App) -> SharedString {
    face(cx, |f| &f.0.chat_code).family.into()
}

/// The style of a chat markdown view: its code blocks use the chat code family and size.
pub(crate) fn chat_markdown(cx: &App) -> TextViewStyle {
    TextViewStyle::default().code_block(
        StyleRefinement::default()
            .font_family(chat_code_family(cx))
            .text_size(chat_code(cx, 13.)),
    )
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;
    use gpui_kit::component::ActiveTheme as _;
    use wai_settings::Settings;

    use super::*;
    use crate::appearance::apply;

    fn applied(cx: &mut TestAppContext, settings: &Settings) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            apply(settings, None, &[], None, cx);
        });
    }

    /// Every base 10..=18 step 0.5, as a view would write it.
    fn bases() -> impl Iterator<Item = f32> {
        (20..=36).map(|n| n as f32 / 2.)
    }

    #[gpui_kit::test]
    fn ui_sizes_scale_and_round_to_half_pixel(cx: &mut TestAppContext) {
        applied(cx, &Settings::default());
        cx.update(|cx| {
            for base in bases() {
                assert_eq!(ui(cx, base), px(base), "default size keeps {base}");
            }
        });
        let mut settings = Settings::default();
        settings.interface.font_size = Some(20.);
        applied(cx, &settings);
        cx.update(|cx| {
            assert_eq!(ui(cx, 13.), px(16.5));
            assert_eq!(ui(cx, 12.), px(15.));
            assert_eq!(cx.theme().font_size, px(20.));
        });
    }

    #[gpui_kit::test]
    fn interface_size_scales_rem(cx: &mut TestAppContext) {
        let mut settings = Settings::default();
        settings.interface.font_size = Some(20.);
        applied(cx, &settings);
        cx.update(|cx| assert_eq!(cx.theme().font_size, px(20.)));
        applied(cx, &Settings::default());
        cx.update(|cx| assert_eq!(cx.theme().font_size, px(16.)));
    }

    #[gpui_kit::test]
    fn editor_sizes_scale_and_round_to_half_pixel(cx: &mut TestAppContext) {
        applied(cx, &Settings::default());
        cx.update(|cx| {
            for base in bases() {
                assert_eq!(editor(cx, base), px(base));
                assert_eq!(chat_code(cx, base), px(base));
            }
        });
        let mut settings = Settings::default();
        settings.editor.font_size = Some(15.);
        applied(cx, &settings);
        cx.update(|cx| {
            // 12 * 15 / 13 = 13.85
            assert_eq!(editor(cx, 12.), px(14.));
            assert_eq!(chat_code(cx, 12.), px(14.));
            assert_eq!(terminal(cx).1, px(15.));
        });
        settings.conversation.code_font_size = Some(26.);
        applied(cx, &settings);
        cx.update(|cx| {
            assert_eq!(chat_code(cx, 12.), px(24.));
            assert_eq!(editor(cx, 12.), px(14.));
        });
    }

    #[gpui_kit::test]
    fn chat_sizes_scale_and_round_to_half_pixel(cx: &mut TestAppContext) {
        applied(cx, &Settings::default());
        cx.update(|cx| {
            for base in bases() {
                assert_eq!(chat(cx, base), px(base));
            }
        });
        let mut settings = Settings::default();
        settings.conversation.font_size = Some(16.);
        applied(cx, &settings);
        cx.update(|cx| {
            assert_eq!(chat(cx, 13.), px(16.));
            // 14 * 16 / 13 = 17.23
            assert_eq!(chat(cx, 14.), px(17.));
            assert_eq!(ui(cx, 13.), px(13.));
        });
    }

    #[gpui_kit::test]
    fn families_come_from_the_resolved_fonts(cx: &mut TestAppContext) {
        applied(cx, &Settings::default());
        cx.update(|cx| {
            assert_eq!(terminal(cx), (editor_family(cx), px(13.)));
            assert_eq!(chat_family(cx), cx.global::<Fonts>().0.ui.family.as_str());
            assert_eq!(chat_code_family(cx), editor_family(cx));
        });
    }
}
