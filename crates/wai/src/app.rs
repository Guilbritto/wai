use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::input::CursorBlink;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{Theme, ThemeMode, WindowExt as _};
use gpui_kit::*;
use wai_settings::{Settings, Template, ThemeChoice};

use crate::workspace::Workspace;

actions!(
    wai,
    [
        Quit,
        NewSession,
        ToggleTerminal,
        CloseSession,
        FocusNextPane,
        OpenFolder,
        CancelTurn,
        Paste,
        CopySelection,
        Save,
        OpenPalette
    ]
);

/// Everything a window needs before it opens: components, theme, caret blink, keymap.
pub fn init(settings: &Settings, cx: &mut App) {
    gpui_kit::init(cx);
    let mode = match settings.theme {
        ThemeChoice::Dark => ThemeMode::Dark,
        ThemeChoice::Light => ThemeMode::Light,
    };
    Theme::change(mode, None, cx);
    // A focused editor otherwise blinks its caret forever (~0.9% CPU while idle).
    Theme::update(cx, |theme| {
        theme.cursor_blink = CursorBlink::default().with_idle_timeout(Duration::from_secs(5))
    });
    bind_keys(cx);
}

fn bind_keys(cx: &mut App) {
    let workspace = Some("Workspace");
    // ctrl-v (ctrl-c) reaches the shell as a literal ^V (^C), so other platforms paste (copy)
    // with ctrl-shift-v (ctrl-shift-c).
    let (paste, copy) = if cfg!(target_os = "macos") {
        ("cmd-v", "cmd-c")
    } else {
        ("ctrl-shift-v", "ctrl-shift-c")
    };
    let mut keymap: Vec<(&str, Box<dyn Action>, Option<&str>)> = vec![
        ("secondary-q", Box::new(Quit), None),
        ("secondary-n", Box::new(NewSession), workspace),
        ("ctrl-`", Box::new(ToggleTerminal), workspace),
        // R20: in an editor pane it closes the active tab.
        ("secondary-w", Box::new(CloseSession), workspace),
        ("secondary-s", Box::new(Save), workspace),
        ("secondary-alt-right", Box::new(FocusNextPane), workspace),
        ("secondary-o", Box::new(OpenFolder), workspace),
        ("secondary-shift-p", Box::new(OpenPalette), workspace),
        // The prompt box lets Escape through (it has nothing to clear).
        ("escape", Box::new(CancelTurn), Some("ChatSession")),
        (paste, Box::new(Paste), Some("Terminal")),
        (copy, Box::new(CopySelection), Some("Terminal")),
    ];
    if cfg!(windows) {
        let terminal = windows_terminal_keymap().into_iter();
        keymap.extend(terminal.map(|(keys, action)| (keys, action, Some("Terminal"))));
    }
    // `KeyBinding::new` rejects `secondary-`; `load` resolves it for the host OS.
    let bindings: Vec<_> = keymap
        .into_iter()
        .map(|(keys, action, context)| {
            let context = context.map(|c| Rc::new(KeyBindingContextPredicate::parse(c).unwrap()));
            KeyBinding::load(keys, action, context, false, None, cx).unwrap()
        })
        .collect();
    cx.bind_keys(bindings);
}

/// Windows, in the Terminal context: `NoAction` hides the app's ctrl-c/v/n/w/o/q (`secondary-`
/// is ctrl there), so they reach the shell, and the app's actions move to ctrl-shift.
fn windows_terminal_keymap() -> [(&'static str, Box<dyn Action>); 10] {
    [
        ("ctrl-c", Box::new(NoAction)),
        ("ctrl-v", Box::new(NoAction)),
        ("ctrl-n", Box::new(NoAction)),
        ("ctrl-w", Box::new(NoAction)),
        ("ctrl-o", Box::new(NoAction)),
        ("ctrl-q", Box::new(NoAction)),
        ("ctrl-shift-n", Box::new(NewSession)),
        ("ctrl-shift-w", Box::new(CloseSession)),
        ("ctrl-shift-o", Box::new(OpenFolder)),
        ("ctrl-shift-q", Box::new(Quit)),
    ]
}

/// `notices`: what the settings left out, shown once in the window.
/// `state_path`: where the open sessions, the project and the terminal's visibility are kept.
pub fn run(
    project: PathBuf,
    settings: Settings,
    templates: Vec<Template>,
    notices: Vec<String>,
    state_path: PathBuf,
) {
    // Without the asset source every component icon (status, chevrons) renders blank.
    let app = gpui_kit::application().with_assets(gpui_kit::assets::Assets);
    app.run(move |cx| {
        init(&settings, cx);
        // Quit is the workspace's: it asks about unsaved edits first.
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1400.), px(900.)),
                cx,
            ))),
            ..Default::default()
        };
        let opened = gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Workspace::new(project, settings, Some(state_path), true, window, cx))
        })
        .expect("open window");
        let (window, workspace) = opened.clone();
        window
            .update(cx, |_, window, cx| {
                workspace.update(cx, |w, cx| {
                    w.templates = templates;
                    w.start_agent(cx);
                    w.watch_project(window, cx);
                    // Before `restore`: a saved session may be an extension agent's.
                    w.start_extensions(cx);
                    w.restore(window, cx);
                    w.suggest_extensions(window, cx);
                });
                for notice in notices {
                    window.push_notification(Notification::warning(notice).autohide(false), cx);
                }
            })
            .expect("window just opened");
        cx.activate(true);

        #[cfg(feature = "demo")]
        if let Ok(prompt) = std::env::var("WAI_SHOT_PROMPT") {
            send_prompt(opened.clone(), prompt, cx);
        }
        #[cfg(feature = "demo")]
        if let Ok(command) = std::env::var("WAI_SHOT_TERMINAL") {
            let window = opened.0;
            cx.spawn(async move |cx| {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                run_in_terminal(window, &command, cx).await;
            })
            .detach();
        }
        #[cfg(feature = "demo")]
        if let Ok(files) = std::env::var("WAI_SHOT_OPEN") {
            open_files(opened.clone(), files, cx);
        }
        #[cfg(feature = "screenshot")]
        if let Ok(path) = std::env::var("WAI_SHOT") {
            screenshot(opened, path, cx);
        }
    });
}

/// Scripted run (screen locked, no keyboard): after 2 s, opens a session and types `prompt`.
#[cfg(feature = "demo")]
fn send_prompt(
    (window, workspace): (AnyWindowHandle, Entity<Workspace>),
    prompt: String,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        let executor = cx.background_executor().clone();
        executor.timer(Duration::from_secs(2)).await;
        window
            .update(cx, |_, window, cx| {
                window.dispatch_action(Box::new(NewSession), cx)
            })
            .ok();
        // ponytail: a script-only wait; the product never polls.
        while workspace.read_with(cx, |w, _| w.panes[0].session().is_none()) {
            executor.timer(Duration::from_millis(50)).await;
        }
        window
            .update(cx, |_, window, cx| type_text(&prompt, window, cx))
            .ok();
    })
    .detach();
}

/// Scripted run (`WAI_SHOT_OPEN=<files>`): after 1 s, opens `files` (comma-separated, relative
/// to the project) in the editor, the one read last focused; each logs its main-thread time.
#[cfg(feature = "demo")]
fn open_files(
    (window, workspace): (AnyWindowHandle, Entity<Workspace>),
    files: String,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_secs(1)).await;
        window
            .update(cx, |_, window, cx| {
                workspace.update(cx, |w, cx| {
                    for file in files.split(',') {
                        w.open_file(w.project.join(file), None, window, cx);
                    }
                })
            })
            .ok();
    })
    .detach();
}

/// Scripted run: shows the terminal and types `command` and Enter into it
/// (`WAI_SHOT_TERMINAL=<command>`, after 2 s).
#[cfg(feature = "demo")]
async fn run_in_terminal(window: AnyWindowHandle, command: &str, cx: &mut AsyncApp) {
    let executor = cx.background_executor().clone();
    window
        .update(cx, |_, window, cx| {
            window.dispatch_action(Box::new(ToggleTerminal), cx)
        })
        .ok();
    executor.timer(Duration::from_millis(500)).await;
    window
        .update(cx, |_, window, cx| type_text(command, window, cx))
        .ok();
    executor.timer(Duration::from_millis(500)).await;
}

/// Types `text` and Enter into the focused element, as the keyboard would.
#[cfg(feature = "demo")]
fn type_text(text: &str, window: &mut Window, cx: &mut App) {
    let os = cx.platform_info().os;
    for ch in text.chars() {
        let key = if ch == ' ' {
            "space".into()
        } else {
            ch.to_string()
        };
        let mut keystroke = Keystroke::parse_with_os(&key, os).expect("a typed character");
        keystroke.key_char = Some(ch.to_string());
        window.dispatch_keystroke(keystroke, cx);
    }
    let enter = Keystroke::parse_with_os("enter", os).expect("enter");
    window.dispatch_keystroke(enter, cx);
}

/// Saves the window to `path` and quits, offscreen so it also works with the screen locked.
/// With `WAI_SHOT_PROMPT`, once that turn needs the user (a permission or question card) or ends
/// with an Edits block (fase-2-edits.png); otherwise after 2 s,
/// with the terminal panel shown and `echo wai` run in it.
/// `WAI_SHOT=docs/screens/fase-1-sidebar.png WAI_SHOT_PROMPT=… WAI_SHOT_SIDE=1` with a fake
/// agent run with `--sessions 8 --ask-permission --reply-file …`.
/// `WAI_SHOT_AGENTS=1` (with `WAI_SHOT_PROMPT`): the Agents tab (fase-2-agents.png).
/// `WAI_SHOT_FILES=<dir>`: the Files tab with `<dir>` (relative to the project) expanded, and
/// no terminal (fase-2-files.png).
/// `WAI_SHOT_TEMPLATES=1`: the open template menu (fase-2-templates.png).
/// `WAI_SHOT_SCROLLBACK=<command>`: the terminal scrolled back with a selection
/// (fase-2-terminal.png).
/// `WAI_SHOT_CONSENT=<absolute extension folder>`: the Extensions view with the consent dialog
/// for that folder (fase-3-consent.png).
/// `WAI_SHOT_PANEL=1`: the first extension panel's tab, an enabled extension having rendered it
/// (fase-3-panel.png).
/// `WAI_SHOT_PALETTE=1`: the command palette open (fase-3-palette.png).
#[cfg(feature = "screenshot")]
fn screenshot(
    (window, workspace): (AnyWindowHandle, Entity<Workspace>),
    path: String,
    cx: &mut App,
) {
    let chat = std::env::var("WAI_SHOT_PROMPT").is_ok();
    cx.spawn(async move |cx| {
        let executor = cx.background_executor().clone();
        executor.timer(Duration::from_secs(2)).await;
        if chat {
            let needs_you = |w: &Workspace, cx: &App| {
                w.panes[0].session().is_some_and(|s| {
                    let model = &s.read(cx).model;
                    model.state == crate::chat::TurnState::NeedsYou
                        || (model.state == crate::chat::TurnState::Idle
                            && (model.items.iter())
                                .any(|i| matches!(i, crate::chat::ChatItem::Edits { .. })))
                })
            };
            while !workspace.read_with(cx, needs_you) {
                executor.timer(Duration::from_millis(50)).await;
            }
            // `WAI_SHOT_SIDE`: the first history row opens to the side (fase-1-sidebar.png).
            if std::env::var("WAI_SHOT_SIDE").is_ok() {
                window
                    .update(cx, |_, window, cx| {
                        workspace.update(cx, |w, cx| {
                            if let Some(row) = w.history_rows().first().map(|&row| row.clone()) {
                                w.resume(&row, true, window, cx);
                            }
                        })
                    })
                    .ok();
                executor.timer(Duration::from_millis(500)).await;
            }
            // `WAI_SHOT_AGENTS`: the Agents tab (fase-2-agents.png).
            if std::env::var("WAI_SHOT_AGENTS").is_ok() {
                workspace.update(cx, |w, cx| {
                    w.sidebar_tab = crate::sidebar::SidebarTab::Agents;
                    cx.notify();
                });
                executor.timer(Duration::from_millis(500)).await;
            }
        } else if std::env::var("WAI_SHOT_TEMPLATES").is_ok() {
            // `WAI_SHOT_TEMPLATES=1`: the "New from template" menu open (fase-2-templates.png).
            window
                .update(cx, |_, window, cx| {
                    window.draw(cx).clear(cx);
                    gpui_kit::test::TestWindowExt::click(window, "sidebar-templates", cx)
                })
                .ok();
            executor.timer(Duration::from_millis(500)).await;
        } else if let Ok(dir) = std::env::var("WAI_SHOT_CONSENT") {
            window
                .update(cx, |_, window, cx| {
                    workspace.update(cx, |w, cx| {
                        let view = w.show_extensions(window, cx);
                        let source = wai_ext::Source::Folder(dir.into());
                        view.update(cx, |v, cx| v.review(source, None, window, cx));
                    })
                })
                .ok();
            executor.timer(Duration::from_secs(1)).await;
        } else if std::env::var("WAI_SHOT_PALETTE").is_ok() {
            window
                .update(cx, |_, window, cx| {
                    window.dispatch_action(Box::new(OpenPalette), cx)
                })
                .ok();
            executor.timer(Duration::from_millis(500)).await;
        } else if std::env::var("WAI_SHOT_PANEL").is_ok() {
            workspace.update(cx, |w, cx| {
                let first = w.ext_host.read(cx).panels().first();
                if let Some(panel) = first.map(|p| p.view.entity_id()) {
                    w.sidebar_tab = crate::sidebar::SidebarTab::Panel(panel);
                    cx.notify();
                }
            });
            executor.timer(Duration::from_millis(500)).await;
        } else if let Ok(dir) = std::env::var("WAI_SHOT_FILES") {
            workspace.update(cx, |w, cx| {
                w.sidebar_tab = crate::sidebar::SidebarTab::Files;
                let dir = w.project.join(dir);
                w.files.update(cx, |files, cx| files.toggle(dir, cx));
            });
            executor.timer(Duration::from_millis(500)).await;
        } else if let Ok(command) = std::env::var("WAI_SHOT_SCROLLBACK") {
            // `WAI_SHOT_SCROLLBACK=<command>`: its output scrolled back 20 lines, one row
            // selected (fase-2-terminal.png).
            run_in_terminal(window, &command, cx).await;
            executor.timer(Duration::from_millis(500)).await;
            workspace.update(cx, |w, cx| {
                if let Some(terminal) = &w.terminal {
                    terminal.update(cx, |t, cx| t.show_history(20, 3, cx));
                }
            });
        } else {
            run_in_terminal(window, "echo wai", cx).await;
        }
        // A resizable group settles its sizes in a notify deferred past the draw, so each
        // draw gets its own update.
        for _ in 0..3 {
            window
                .update(cx, |_, window, cx| window.draw(cx).clear(cx))
                .ok();
            executor.timer(Duration::from_millis(100)).await;
        }
        let image = window.update(cx, |_, window, _| window.render_to_image());
        match image.and_then(|image| Ok(image?.save(&path)?)) {
            Ok(()) => eprintln!("wai: screenshot saved to {path}"),
            Err(e) => eprintln!("wai: screenshot failed: {e}"),
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}

#[cfg(test)]
mod tests {
    /// Windows, terminal focused: the shell gets ctrl-c/v/n/w/o/q, the app's actions move to
    /// ctrl-shift (copy and paste are ctrl-shift-c/v off macOS already).
    #[test]
    fn windows_terminal_keymap_table() {
        let table: Vec<(&str, &str)> = super::windows_terminal_keymap()
            .iter()
            .map(|(keys, action)| (*keys, action.name()))
            .collect();
        let shell = "zed::NoAction";
        let expected = [
            ("ctrl-c", shell),
            ("ctrl-v", shell),
            ("ctrl-n", shell),
            ("ctrl-w", shell),
            ("ctrl-o", shell),
            ("ctrl-q", shell),
            ("ctrl-shift-n", "wai::NewSession"),
            ("ctrl-shift-w", "wai::CloseSession"),
            ("ctrl-shift-o", "wai::OpenFolder"),
            ("ctrl-shift-q", "wai::Quit"),
        ];
        assert_eq!(table, expected);
    }
}
