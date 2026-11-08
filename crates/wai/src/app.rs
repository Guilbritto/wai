use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::input::CursorBlink;
use gpui_kit::component::{Theme, ThemeMode};
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
        OpenFolder
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
    let keymap: [(&str, Box<dyn Action>, Option<&str>); 6] = [
        ("secondary-q", Box::new(Quit), None),
        ("secondary-n", Box::new(NewSession), workspace),
        ("ctrl-`", Box::new(ToggleTerminal), workspace),
        ("secondary-w", Box::new(CloseSession), workspace),
        ("secondary-alt-right", Box::new(FocusNextPane), workspace),
        ("secondary-o", Box::new(OpenFolder), workspace),
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

pub fn run(project: PathBuf, settings: Settings) {
    gpui_kit::application().run(move |cx| {
        init(&settings, cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
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
        let _opened = gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Workspace::new(project, settings, window, cx))
        })
        .expect("open window");
        cx.activate(true);

        #[cfg(feature = "screenshot")]
        if let Ok(path) = std::env::var("WAI_SHOT") {
            screenshot(_opened, path, cx);
        }
    });
}

/// Saves the window, with the terminal panel shown, to `path` and quits. Offscreen, so it
/// also works with the screen locked.
#[cfg(feature = "screenshot")]
fn screenshot(
    (window, workspace): (AnyWindowHandle, Entity<Workspace>),
    path: String,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_secs(2)).await;
        let image = window.update(cx, |_, window, cx| {
            workspace.update(cx, |w, cx| {
                w.terminal_visible = true;
                cx.notify();
            });
            // The resizable groups settle their sizes over a couple of frames.
            for _ in 0..3 {
                window.draw(cx).clear(cx);
            }
            window.render_to_image()
        });
        match image.and_then(|image| Ok(image?.save(&path)?)) {
            Ok(()) => eprintln!("wai: screenshot saved to {path}"),
            Err(e) => eprintln!("wai: screenshot failed: {e}"),
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}
