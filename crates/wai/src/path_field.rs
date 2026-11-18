//! A file or folder path: typed or pasted (`editable`), picked with the system picker or dropped
//! from Finder. It shows the home folder as `~`; `value` gives the path with `~` expanded.

use std::path::{MAIN_SEPARATOR, Path};
use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_ext::panel::Pick;

use crate::appearance::fonts;
use crate::campo;

/// The picker segment on the right.
const SEGMENT: f32 = 30.;

/// `path` with the home folder shown as `~`; anything outside it stays as it is.
pub(crate) fn shorten(path: &str, home: Option<&Path>) -> String {
    let home = home
        .and_then(Path::to_str)
        .filter(|h| !h.is_empty() && *h != "/");
    match home.and_then(|home| path.strip_prefix(home)) {
        Some("") => "~".into(),
        Some(rest) if rest.starts_with(['/', MAIN_SEPARATOR]) => format!("~{rest}"),
        _ => path.into(),
    }
}

/// `text` with a leading `~` (alone or before a separator) as the home folder.
pub(crate) fn expand(text: &str, home: Option<&Path>) -> String {
    let Some(home) = home.and_then(Path::to_str) else {
        return text.into();
    };
    match text.strip_prefix('~') {
        Some("") => home.into(),
        Some(rest) if rest.starts_with(['/', MAIN_SEPARATOR]) => format!("{home}{rest}"),
        _ => text.into(),
    }
}

/// The path in `input`, trimmed, `~` expanded; `""` when empty.
pub(crate) fn value(input: &Entity<InputState>, cx: &App) -> String {
    expand(input.read(cx).value().trim(), dirs::home_dir().as_deref())
}

/// Shows `path` in `input`, the home folder as `~`.
pub(crate) fn set(input: &Entity<InputState>, path: &str, window: &mut Window, cx: &mut App) {
    let shown = shorten(path, dirs::home_dir().as_deref());
    input.update(cx, |s, cx| s.set_value(shown, window, cx));
}

type OnChange = Rc<dyn Fn(&mut Window, &mut App)>;

/// The field over an `InputState` its owner keeps. Form A (`editable`) types into it; form B is a
/// button that opens the picker.
#[derive(IntoElement)]
pub(crate) struct PathField {
    id: ElementId,
    input: Entity<InputState>,
    pick: Pick,
    editable: bool,
    disabled: bool,
    placeholder: SharedString,
    error: Option<SharedString>,
    /// After a pick, a drop or a clear (typing goes through the input's own events).
    on_change: Option<OnChange>,
}

impl PathField {
    pub(crate) fn new(id: impl Into<ElementId>, input: &Entity<InputState>, pick: Pick) -> Self {
        let placeholder = match pick {
            Pick::File => "Choose a file…",
            Pick::Folder => "Choose a folder…",
        };
        Self {
            id: id.into(),
            input: input.clone(),
            pick,
            editable: true,
            disabled: false,
            placeholder: placeholder.into(),
            error: None,
            on_change: None,
        }
    }

    pub(crate) fn editable(mut self, editable: bool) -> Self {
        self.editable = editable;
        self
    }

    pub(crate) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Form B's text while empty (form A shows the input's own placeholder).
    pub(crate) fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub(crate) fn error(mut self, error: Option<SharedString>) -> Self {
        self.error = error;
        self
    }

    pub(crate) fn on_change(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

/// Puts `path` in the field and says so.
fn put(
    input: &Entity<InputState>,
    path: &str,
    on_change: Option<&OnChange>,
    window: &mut Window,
    cx: &mut App,
) {
    set(input, path, window, cx);
    if let Some(f) = on_change {
        f(window, cx);
    }
}

/// Opens the system picker; a chosen path goes in the field.
fn choose(
    input: Entity<InputState>,
    pick: Pick,
    on_change: Option<OnChange>,
    window: &mut Window,
    cx: &mut App,
) {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: pick == Pick::File,
        directories: pick == Pick::Folder,
        multiple: false,
        prompt: None,
    });
    window
        .spawn(cx, async move |cx| {
            let Ok(Ok(Some(mut paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.pop() else {
                return;
            };
            let path = path.to_string_lossy().into_owned();
            cx.update(|window, cx| put(&input, &path, on_change.as_ref(), window, cx))
                .ok();
        })
        .detach();
}

impl RenderOnce for PathField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (danger, ring, muted) = (theme.danger, theme.ring, theme.muted_foreground);
        let mono = theme.mono_font_family.clone();
        let (line, violet, dim) = (campo::line(cx), campo::violet(cx), campo::dim(cx));
        let text = self.input.read(cx).value();
        let focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        let (pick, editable, disabled) = (self.pick, self.editable, self.disabled);
        let icon = match pick {
            Pick::File => Lucide::File,
            Pick::Folder => Lucide::Folder,
        };
        let opener = {
            let (input, on_change) = (self.input.clone(), self.on_change.clone());
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                choose(input.clone(), pick, on_change.clone(), window, cx)
            }
        };
        let body = match editable {
            true => Input::new(&self.input)
                .appearance(false)
                .small()
                .disabled(disabled)
                .flex_1()
                .min_w_0()
                .font_family(mono)
                .text_size(fonts::ui(cx, 11.5))
                .into_any_element(),
            false => div()
                .id((self.id.clone(), "choose"))
                .flex_1()
                .min_w_0()
                .px_1()
                .truncate()
                .rounded(px(6.))
                .border_1()
                .border_color(transparent_black())
                .when(text.is_empty(), |el| el.text_color(dim))
                .when(!text.is_empty(), |el| {
                    el.font_family(mono).text_size(fonts::ui(cx, 11.5))
                })
                .child(if text.is_empty() {
                    self.placeholder.clone()
                } else {
                    text.clone()
                })
                .when(!disabled, |el| {
                    el.cursor_pointer()
                        .tab_index(0)
                        .role(Role::Button)
                        .aria_label(self.placeholder.clone())
                        .focus_visible(move |el| el.border_color(ring))
                        .on_click(opener.clone())
                })
                .into_any_element(),
        };
        let clear = (!text.is_empty() && !disabled).then(|| {
            let (input, on_change) = (self.input.clone(), self.on_change.clone());
            Button::new((self.id.clone(), "clear"))
                .xsmall()
                .ghost()
                .icon(Lucide::X)
                .tooltip("Clear")
                .accessibility_label("Clear")
                .on_click(move |_, window, cx| put(&input, "", on_change.as_ref(), window, cx))
        });
        let segment = Button::new((self.id.clone(), "pick"))
            .ghost()
            .h_full()
            .rounded(px(0.))
            .border_l_1()
            .border_color(line)
            .disabled(disabled)
            .map(|b| match editable {
                true => {
                    let what = match pick {
                        Pick::File => "Choose a file",
                        Pick::Folder => "Choose a folder",
                    };
                    b.w(px(SEGMENT))
                        .icon(Lucide::FolderOpen)
                        .tooltip(what)
                        .accessibility_label(what)
                }
                false => b
                    .px_2()
                    .text_size(fonts::ui(cx, 12.))
                    .label(if text.is_empty() {
                        "Choose…"
                    } else {
                        "Change…"
                    }),
            })
            .on_click(opener);
        let (input, on_change) = (self.input.clone(), self.on_change.clone());
        let field = campo::field(h_flex(), focused, cx)
            .id(self.id.clone())
            .overflow_hidden()
            .when(self.error.is_some() && !focused, |el| {
                el.border_color(danger)
            })
            .when(disabled, |el| el.opacity(0.42))
            .child(
                div()
                    .pl_2()
                    .pr_1()
                    .child(Icon::new(icon).with_size(px(14.)).text_color(muted)),
            )
            .child(body)
            .children(clear)
            .child(segment)
            .when(!disabled, |el| {
                el.drag_over::<ExternalPaths>(move |style, _, _, _| {
                    style
                        .border_dashed()
                        .border_color(violet)
                        .bg(violet.opacity(0.12))
                })
                .on_drop(move |paths: &ExternalPaths, window, cx| {
                    if let Some(path) = paths.paths().first() {
                        let path = path.to_string_lossy().into_owned();
                        put(&input, &path, on_change.as_ref(), window, cx);
                    }
                })
            });
        v_flex().w_full().gap_1().child(field).children(
            self.error
                .map(|error| div().text_xs().text_color(danger).child(error)),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{expand, shorten};

    #[test]
    fn shorten_and_expand_round_trip() {
        let home = Some(Path::new("/Users/x"));
        assert_eq!(shorten("/Users/x/src", home), "~/src");
        assert_eq!(expand("~/src", home), "/Users/x/src");
        assert_eq!(shorten("/Users/x", home), "~");
        assert_eq!(expand("~", home), "/Users/x");
        assert_eq!(shorten("/opt/a", home), "/opt/a");
        assert_eq!(expand("/opt/a", home), "/opt/a");
        // Another user's home, and a sibling that only shares the prefix.
        assert_eq!(expand("~other/a", home), "~other/a");
        assert_eq!(shorten("/Users/xy/a", home), "/Users/xy/a");
        assert_eq!(shorten("/Users/x/src", None), "/Users/x/src");
    }
}
