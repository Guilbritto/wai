//! The top bar: the brand, the project, how many agents work, and the global actions. On macOS
//! it is the title bar, the traffic lights inline.

use std::sync::{Arc, LazyLock};

use gpui_kit::base::Selectable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, TitleBar, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_ext::panel::clamp;

use crate::app::{NewSession, OpenPalette, ToggleAgents, ToggleTerminal};
use crate::aurora::{self, DISPLAY_FONT};
use crate::chat::TurnState;
use crate::settings_view::Section;
use crate::workspace::{Workspace, folder_name};

pub(crate) static BRAND: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let png = include_bytes!("../assets/icon/icon.png");
    Arc::new(Image::from_bytes(ImageFormat::Png, png.to_vec()))
});

/// The status pill's words: how many sessions are working, then how many need the user.
pub(crate) fn status_label(working: usize, needs_you: usize) -> String {
    let working = match working {
        0 => "No agent working".to_string(),
        1 => "1 agent working".to_string(),
        n => format!("{n} agents working"),
    };
    match needs_you {
        0 => working,
        1 => format!("{working} · 1 needs you"),
        n => format!("{working} · {n} need you"),
    }
}

/// The running build: the version, then the commit it was built from (`build.rs`), when known.
pub(crate) fn version() -> String {
    match env!("WAI_BUILD") {
        "" => env!("CARGO_PKG_VERSION").to_string(),
        build => format!("{} · {build}", env!("CARGO_PKG_VERSION")),
    }
}

impl Workspace {
    pub(crate) fn render_top_bar(
        &self,
        window: &Window,
        cx: &mut Context<Workspace>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let dark = theme.is_dark();
        let (working, needs_you) = (self.sessions.iter()).fold((0, 0), |(w, n), session| {
            match session.read(cx).status() {
                TurnState::Working => (w + 1, n),
                TurnState::NeedsYou => (w, n + 1),
                _ => (w, n),
            }
        });
        let starting = self.drafts.iter().filter(|d| d.read(cx).starting.is_some());
        let working = working + starting.count();
        let (on, off, warn) = if dark {
            (aurora::boreal(), aurora::nevoa_2(), aurora::ambar())
        } else {
            // Light's warning (yellow-500) reads ~1.5:1 on the pill; the text colour reads.
            (theme.cyan, theme.muted_foreground, theme.foreground)
        };
        let dot = div()
            .flex_none()
            .size(px(7.))
            .rounded_full()
            .bg(if working > 0 { on } else { off })
            .when(working > 0, |el| {
                el.shadow(vec![BoxShadow::new(px(0.), px(0.), on).blur_radius(px(5.))])
            });
        let label = status_label(working, needs_you);
        let needs = label.find(" · ").map(|ix| {
            let style = HighlightStyle {
                color: Some(warn),
                ..Default::default()
            };
            (ix..label.len(), style)
        });
        let status = aurora::pill(cx)
            .debug_selector(|| "topbar-status".into())
            .flex_none()
            .child(dot)
            .child(StyledText::new(label).with_highlights(needs));

        let brand = h_flex()
            .flex_none()
            .gap_2()
            .child(img(BRAND.clone()).size(px(22.)).rounded(px(6.)))
            .child(
                div()
                    .font_family(DISPLAY_FONT)
                    .text_size(px(15.))
                    .font_weight(FontWeight::BOLD)
                    .child("wai"),
            )
            .child(
                div()
                    .debug_selector(|| "topbar-version".into())
                    .text_xs()
                    .text_color(off)
                    .child(version()),
            );
        let project = aurora::pill(cx)
            .debug_selector(|| "topbar-project".into())
            .min_w_0()
            .child(Icon::new(IconName::Folder).xsmall())
            .child(div().min_w_0().truncate().child(folder_name(&self.project)));
        // Clipped: in a very narrow window the brand and status give way, not the actions.
        let left = h_flex()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .gap_2()
            .child(brand)
            .child(project)
            .child(status);

        let extensions = Button::new("sidebar-extensions")
            .ghost()
            .small()
            .icon(IconName::LayoutDashboard)
            .tooltip("Extensions")
            .accessibility_label("Extensions")
            .on_click(cx.listener(|this, _, window, cx| {
                this.show_extensions(window, cx);
            }));
        let palette = Button::new("topbar-palette")
            .ghost()
            .small()
            .icon(IconName::Menu)
            .tooltip("Run a command…")
            .accessibility_label("Run a command…")
            .on_click(
                cx.listener(|this, _, window, cx| this.open_palette(&OpenPalette, window, cx)),
            );
        let settings =
            Button::new("topbar-settings")
                .ghost()
                .small()
                .icon(IconName::Settings)
                .tooltip("Settings")
                .accessibility_label("Settings")
                .selected(self.settings_view().is_some())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_settings(Section::Ai, None, window, cx)
                }));
        let terminal =
            Button::new("topbar-terminal")
                .ghost()
                .small()
                .icon(IconName::SquareTerminal)
                .tooltip("Toggle terminal")
                .accessibility_label("Toggle terminal")
                .selected(self.terminal_visible)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_terminal(&ToggleTerminal, window, cx)
                }));
        // Only while Agents is on the right: shows or hides its panel, with how many need the user.
        let agents =
            self.agents_right.then(|| {
                let label = match needs_you {
                    0 => "Toggle agents".to_string(),
                    1 => "Toggle agents · 1 needs you".to_string(),
                    n => format!("Toggle agents · {n} need you"),
                };
                Button::new("topbar-agents")
                    .ghost()
                    .small()
                    .icon(IconName::PanelRight)
                    .when(needs_you > 0, |el| el.label(needs_you.to_string()))
                    .tooltip(label.clone())
                    .accessibility_label(label)
                    .selected(!self.agents_collapsed)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_agents(&ToggleAgents, window, cx)
                    }))
            });
        let new = Button::new("sidebar-new")
            .debug_selector(|| "sidebar-new".into())
            .primary()
            .small()
            .icon(IconName::Plus)
            .label("New session")
            .tooltip("New session")
            .on_click(cx.listener(|this, _, window, cx| this.new_session(&NewSession, window, cx)));
        let right = h_flex()
            .debug_selector(|| "topbar-actions".into())
            // The title bar moves the window on a press anywhere in it, then a 1 px move, and
            // zooms on a double click; neither may start on a button.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .flex_none()
            .gap_1()
            .children(self.templates_menu(cx))
            .child(extensions)
            .child(palette)
            .child(settings)
            .child(terminal)
            .children(agents)
            .child(new);

        let bar = h_flex()
            .debug_selector(|| "topbar".into())
            // Width 0, then grown: the title bar's row sizes to its content otherwise, and a long
            // project name would push the actions out of the window.
            .w_0()
            .flex_1()
            .h_full()
            .gap_3()
            .pr_2()
            .child(left)
            .child(right);
        if cfg!(target_os = "macos") {
            // Transparent over the aurora; light mode keeps the title bar's own colours.
            TitleBar::new()
                .when(dark, |el| el.bg(transparent_black()))
                // In full screen the traffic lights leave, but the title bar keeps their 80 pt.
                .when(window.is_fullscreen(), |el| el.pl_0())
                .child(bar)
                .into_any_element()
        } else {
            h_flex().h(px(40.)).pl_3().child(bar).into_any_element()
        }
    }

    /// "New from template", while there are templates (an extension's too).
    fn templates_menu(&self, cx: &Context<Workspace>) -> Option<impl IntoElement + use<>> {
        let templates = self.all_templates(cx);
        if templates.is_empty() {
            return None;
        }
        // One line each.
        let names: Vec<_> = (templates.iter())
            .map(|t| clamp(&t.name, crate::palette::TITLE_CHARS))
            .collect();
        let workspace = cx.entity().downgrade();
        let menu = Button::new("sidebar-templates")
            .ghost()
            .small()
            .icon(IconName::FileText)
            .tooltip("New from template")
            .accessibility_label("New from template")
            .dropdown_menu(move |menu, _, _| {
                names.iter().enumerate().fold(menu, |menu, (ix, name)| {
                    let workspace = workspace.clone();
                    menu.item(
                        PopupMenuItem::new(name.clone()).on_click(move |_, window, cx| {
                            workspace
                                .update(cx, |w, cx| w.start_template(ix, window, cx))
                                .ok();
                        }),
                    )
                })
            });
        Some(
            div()
                .debug_selector(|| "sidebar-templates".into())
                .child(menu),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{status_label, version};

    #[test]
    fn status_label_counts() {
        assert_eq!(status_label(0, 0), "No agent working");
        assert_eq!(status_label(1, 0), "1 agent working");
        assert_eq!(status_label(3, 1), "3 agents working · 1 needs you");
        assert_eq!(status_label(0, 2), "No agent working · 2 need you");
    }

    /// The version, then the commit this test binary was built from (build.rs); only the
    /// version outside a git checkout.
    #[test]
    fn version_names_the_build() {
        let head = std::process::Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned());
        let expected = match head {
            Some(head) => format!("{} · {head}", env!("CARGO_PKG_VERSION")),
            None => env!("CARGO_PKG_VERSION").to_string(),
        };
        assert_eq!(version(), expected);
    }
}
