use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::input::CursorBlink;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{Theme, ThemeMode, WindowExt as _};
use gpui_kit::*;
use wai_settings::{Settings, ThemeChoice};

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
        Save
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
    // ctrl-v reaches the shell as a literal ^V, so other platforms paste with ctrl-shift-v.
    let paste = if cfg!(target_os = "macos") {
        "cmd-v"
    } else {
        "ctrl-shift-v"
    };
    let keymap: [(&str, Box<dyn Action>, Option<&str>); 9] = [
        ("secondary-q", Box::new(Quit), None),
        ("secondary-n", Box::new(NewSession), workspace),
        ("ctrl-`", Box::new(ToggleTerminal), workspace),
        // R20: in an editor pane it closes the active tab.
        ("secondary-w", Box::new(CloseSession), workspace),
        ("secondary-s", Box::new(Save), workspace),
        ("secondary-alt-right", Box::new(FocusNextPane), workspace),
        ("secondary-o", Box::new(OpenFolder), workspace),
        // The prompt box lets Escape through (it has nothing to clear).
        ("escape", Box::new(CancelTurn), Some("ChatSession")),
        (paste, Box::new(Paste), Some("Terminal")),
    ];
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

/// `notices`: what the settings left out, shown once in the window.
/// `state_path`: where the open sessions, the project and the terminal's visibility are kept.
pub fn run(project: PathBuf, settings: Settings, notices: Vec<String>, state_path: PathBuf) {
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
                    w.start_agent(cx);
                    w.watch_project(window, cx);
                    w.restore(window, cx);
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
/// with an Edits block (a fake agent run with `--edit`, fase-2-edits.png); otherwise after 2 s,
/// with the terminal panel shown and `echo wai` run in it.
/// `WAI_SHOT=docs/screens/fase-1-sidebar.png WAI_SHOT_PROMPT=… WAI_SHOT_SIDE=1` with a fake
/// agent run with `--sessions 8 --ask-permission --reply-file …`.
/// `WAI_SHOT_FILES=<dir>`: the Files tab with `<dir>` (relative to the project) expanded, and
/// no terminal (fase-2-files.png).
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
        } else if let Ok(dir) = std::env::var("WAI_SHOT_FILES") {
            workspace.update(cx, |w, cx| {
                w.sidebar_tab = crate::sidebar::SidebarTab::Files;
                let dir = w.project.join(dir);
                w.files.update(cx, |files, cx| files.toggle(dir, cx));
            });
            executor.timer(Duration::from_millis(500)).await;
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
