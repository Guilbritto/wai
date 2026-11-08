# GPUI notes (gpui-kit 0.9.0 = gpui-pre 0.3.11 + gpui-base/gpui-component 0.9.0)

Verified in `spikes/s2-ui` (see `docs/spikes/S2-ui.md`) on macOS arm64, Rust 1.99.1, Xcode Command Line Tools only.

Legend: **[V]** compiled and run in `spikes/s2-ui` · **[D]** copied from the crates' own docs/tests, not compiled
here · **[U]** unverified (read in the source, never compiled).

This GPUI is newer than most training data and most blog posts. When in doubt, read the source in
`~/.cargo/registry/src/index.crates.io-*/` (`gpui-pre-0.3.11/src`, `gpui-base-0.9.0/src`,
`gpui-component-0.9.0/src`) and the `#[gpui::test]` tests next to each component.

## 1. Dependency

```toml
gpui-kit = "=0.9.0"   # default features: component + assets. runtime_shaders is always on: no `metal` binary needed
```

- [V] Everything is reached through the facade: `use gpui_kit::*;` is GPUI itself (`div`, `px`, `Window`, `App`,
  `Context`, all element traits). Layers: `gpui_kit::component::*` (styled components), `gpui_kit::base::*`
  (unstyled engine), `gpui_kit::platform` (`application()`), `gpui_kit::assets`.
- [U] Syntax highlighting (editor and markdown code blocks) needs `tree-sitter-<lang>` features
  (e.g. `gpui-kit = { version = "=0.9.0", features = ["tree-sitter-rust"] }`) or `tree-sitter-languages` (all of them).
  Without them `.language("rust")` and fenced ```` ```rust ```` blocks render as plain text.
- Release build of the spike crate from a warm `target/`: ~2–4 s. The GPUI stack itself takes minutes cold.
  `target/` reached ~6 GB with two feature sets (plain + test-support).

## 2. App bootstrap

[V]

```rust
use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::*;

actions!(s2, [Quit]);                              // gpui_kit's macro (GPUI's own spells `gpui::Action`)

fn main() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);                        // once, before anything else (theme, Root plugins, input, dock…)
        Theme::change(ThemeMode::Dark, None, cx);  // optional; default is Light
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_window_closed(|cx, _| if cx.windows().is_empty() { cx.quit() }).detach();

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1400.), px(900.)), cx))),
            titlebar: Some(TitlebarOptions { title: Some("wai".into()), ..Default::default() }),
            ..Default::default()
        };
        // Wraps the view in gpui_base::Root (dialogs, notifications, tooltips): do NOT add another Root.
        gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| Workspace::new(window, cx)))
            .expect("open window");               // -> (AnyWindowHandle, Entity<Workspace>)
        cx.activate(true);
    });
}
```

- Quitting is the app's job: without `on_window_closed` the process keeps running after the last window closes.

## 3. Entities, Context, Render

[V]

```rust
struct Workspace { editor: Entity<EditorState>, terminal: Entity<Terminal>, _tasks: Vec<Task<()>> }

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().size_full().bg(cx.theme().background).child(self.terminal.clone()) // an Entity<V: Render> is an element
    }
}

let terminal = cx.new(|cx| Terminal::new(cx).expect("pty"));     // cx: &mut Context<_> or &mut App
terminal.update(cx, |t, cx| { t.cols = 80; cx.notify(); });      // Entity::update -> R
let focus = terminal.read(cx).focus.clone();                     // Entity::read -> &T
let rows = editor.read_with(cx, |s, _| s.visible_row_range());   // in tests / async contexts
```

- `cx.notify()` marks the entity dirty; every window showing it redraws on the next frame. There is no other
  invalidation API to learn.
- Every frame re-runs `render` of every view in the window (only `AnyView::cached` opts out). Keep `render` cheap and
  do work in event handlers.
- Components (`Editor`, `TextView`, `Button`) are `RenderOnce` values built every frame from a long-lived
  `Entity<…State>` you own. Create the state once (`cx.new`) and keep it in your view.
- `WeakEntity::update(cx, …)` returns `Result<R>` (the entity may be gone); `Entity::update` returns `R`.

## 4. Async: tasks, background work, channel → entity

[V] (`spikes/s2-ui/src/main.rs`, `Workspace::new` and `Terminal::new`)

```rust
// Foreground task bound to this entity. The closure gets a WeakEntity and &mut AsyncApp.
let task: Task<()> = cx.spawn(async move |this, cx| {
    while let Ok(mut text) = rx.recv().await {                     // smol::channel::Receiver
        while let Ok(more) = rx.try_recv() { text.push_str(&more) } // coalesce bursts: one notify per wake-up
        if this.update(cx, |w, cx| w.markdown.update(cx, |md, cx| md.push_str(&text, cx))).is_err() {
            break;                                                 // entity dropped
        }
    }
});

// Same, but with the window: the closure gets &mut AsyncWindowContext.
cx.spawn_in(window, async move |this, cx| {
    let read = cx.background_executor().spawn(async move { std::fs::read_to_string(&path) }).await;
    this.update_in(cx, |w, window, cx| { /* needs Window, e.g. EditorState::set_value */ }).ok();
    cx.update(|window, cx| { /* window + App, no entity leased */ }).ok();
});

// Background (thread pool) future, must be Send. Timers come from the executor.
let exec = cx.background_executor().clone();
let producer = cx.background_spawn(async move {
    exec.timer(Duration::from_millis(25)).await;
    tx.send(token).await.ok();
});
```

- A `Task` cancels when dropped: store it in the view (`_tasks: Vec<Task<()>>`) or call `.detach()`.
- Long-lived blocking reads (PTY, child stdout) belong on a `std::thread` that sends through `smol::channel` with
  `send_blocking`; the foreground task wakes only when data arrives. Short one-off blocking work (reading a file)
  is fine on the background executor; parking a pool thread forever is not.
- No polling: with this pattern the spike sat at 0.0 % CPU and zero new idle wake-ups once the stream ended.

## 5. Events between entities

```rust
// [V] InputEvent from an editor/input state: Change, PressEnter { secondary, shift }, Focus, Blur.
let sub: Subscription = cx.subscribe(&editor, |this, _editor, event: &InputEvent, cx| {
    if let InputEvent::Change = event { /* … */ }
});
// [U] signatures read in gpui-pre src/app/context.rs:
cx.observe(&entity, |this, entity, cx| { /* entity called cx.notify() */ });
cx.subscribe_in(&entity, window, |this, emitter, event: &E, window, cx| { /* … */ });
cx.observe_in(&entity, window, |this, entity, window, cx| { /* … */ });
// Emitting: impl EventEmitter<MyEvent> for MyView {} and cx.emit(MyEvent) inside an update.
```

- `Subscription` unsubscribes when dropped: keep it (`_subscriptions: Vec<Subscription>`) or `.detach()`.

## 6. Actions and keybindings

[V] (app + spike test `scoped_binding_dispatches_action`)

```rust
actions!(s2, [Quit, ClearMarkdown]);

cx.bind_keys([
    KeyBinding::new("cmd-q", Quit, None),                          // global
    KeyBinding::new("cmd-k", ClearMarkdown, Some("Workspace")),    // only inside key_context("Workspace")
]);

// in render:
v_flex()
    .key_context("Workspace")
    .on_action(cx.listener(Self::clear_markdown))  // fn clear_markdown(&mut self, _: &ClearMarkdown, &mut Window, &mut Context<Self>)
```

- A context binding fires only when the focused element is inside that `key_context`. Something must have focus.
- `"secondary-k"` means cmd on macOS and ctrl elsewhere: use it for cross-platform bindings. [V] (wai
  `app.rs`) **`KeyBinding::new` panics on it** (`InvalidKeystrokeError`: it parses without an OS). Use
  `KeyBinding::load(keys, Box::new(action), context.map(|c| Rc::new(KeyBindingContextPredicate::parse(c)?)),
  false, None, cx)`. `simulate_keystrokes("secondary-n")` in tests resolves it fine.

## 7. Focus and raw key input

[V] (`Terminal`)

```rust
let focus = cx.focus_handle();                     // in the entity constructor
window.focus(&focus, cx);                          // NOTE: takes cx in this version
focus.is_focused(window);                          // in render

div()
    .id("terminal")
    .track_focus(&self.focus)
    .on_key_down(cx.listener(Self::on_key_down))   // fn(&mut self, &KeyDownEvent, &mut Window, &mut Context<Self>)
    .on_mouse_down(MouseButton::Left, cx.listener(|t, _, window, cx| window.focus(&t.focus, cx)))

fn on_key_down(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
    let ks = &ev.keystroke;          // key: "enter" | "backspace" | "up" | "a" …, key_char: Some("A") for typed text
    if ks.modifiers.platform { return; }           // leave cmd-* to keybindings
    /* … */
    cx.stop_propagation();
}
```

- `key` is the physical key name; `key_char` is the produced text (`None` for cmd/ctrl combos and named keys,
  `Some(" ")` for space). Use `key_char` for printable input, `key` for named keys, `modifiers.control` for ctrl-x.
- An element with `on_key_down` but no `InputHandler` gets plain key events: fine for a terminal, not for IME
  text. Text fields should use `Input`/`Editor`.
- `EditorState::focus(&self, window, cx)` focuses an editor. A focused editor blinks its caret (see §16).

## 8. Layout helpers

[V]

```rust
use gpui_kit::component::{h_flex, v_flex};     // flex row / flex column Divs
v_flex().size_full().p_2().gap_2().bg(theme.background).text_color(theme.foreground)
div().absolute().size(px(1.)).whitespace_nowrap().overflow_hidden().font_family(family).text_size(px(13.))
```

- `Pixels` has a private field. Get a ratio with `Pixels / Pixels -> f32`; build with `px(f32)`.
- Styling is Tailwind-like method chains on any `Styled` element (`w_full`, `h(px(…))`, `flex_1`, `border_1`,
  `rounded_md`, `hover(|s| …)`).

## 9. Resizable panels

[V]

```rust
use gpui_kit::component::{h_resizable, v_resizable, resizable_panel};
v_resizable("main")
    .child(resizable_panel().child(
        h_resizable("top")
            .child(resizable_panel().size(px(820.)).child(Editor::new(&self.editor).size_full()))
            .child(resizable_panel().child(TextView::new(&self.markdown).scrollable(true).size_full())),
    ))
    .child(resizable_panel().size(px(280.)).child(self.terminal.clone()))
```

- [V] `.size(px)` is the initial size; panels without it share the rest. Nesting groups works.
- [U] `.size_range(a..b)`, `.fixed()` (keeps its size when the window resizes), `.visible(bool)`; owned group
  state with `.with_state(&Entity<ResizableState>)`, read back with `ResizableState::sizes()` for persistence.
- Do not call `.flex_basis`, `.absolute` or `.overflow_hidden` on a `resizable_panel()`: they break the group. [D]

## 10. Dock (tabs, drag-to-split, persisted layout)

[U] Lives in `gpui_kit::component::dock` over `gpui_base::dock`. From the module docs:

```rust
let area = cx.new(|cx| DockArea::new("main", Some(1), window, cx).with_renderer(DockSkin::new(cx)));
// area.update(cx, |a, cx| a.set_center(DockLayout, window, cx)); panels implement the Panel + BasePanel traits.
```

Resizable panels were enough for the spike; evaluate the dock when Phase 2 needs tabs and drag-to-split.

## 11. Code editor

[V]

```rust
use gpui_kit::component::input::{Editor, EditorState, InputEvent};

let editor = cx.new(|cx| EditorState::new(window, cx)  // line numbers, tab 2, indent guides, search on
    .soft_wrap(false)                                   // ALWAYS for code/large files; default is true (see below)
    /* .language("rust") — needs a tree-sitter feature, see §1 */);
editor.update(cx, |s, cx| s.set_value(text, window, cx));        // replaces all, clears undo, no Change event
let rope: &Rope = editor.read(cx).text();                         // borrowed, free; .value() copies into a String
editor.update(cx, |s, cx| s.set_scroll_offset(point(px(0.), px(-y)), cx)); // applied on next layout, clamped
editor.read(cx).visible_row_range();                              // Option<Range<usize>>, None before layout

Editor::new(&editor).size_full()          // in render; .readonly(true), .disabled(true), .bordered(false)
```

- Other state APIs: `default_value(..)` (builder), `insert`, `replace_all` (keeps undo), `set_readonly(bool, cx)`,
  `set_soft_wrap(bool, window, cx)`, `set_line_number`, `set_highlighter("json", cx)`, `cursor_position()`,
  `set_cursor_position`, `diagnostics_mut()`, `lsp_mut()` (completion/hover/definition providers). [U]
- **Large text.** The doc comment says the editor targets "large text up to about 50K lines". Measured with a
  112 MB, 1.25 M-line file and `soft_wrap(false)`: `set_value` blocks the main thread **~340–380 ms** (rope build +
  one display-map item per line, per `sample`), the first frame after it draws in ~3 ms, scrolling draws in
  0.6–2.6 ms per frame even when jumping across the file. Memory ≈ 3.5× the file size (footprint 545 MB vs
  153 MB without the file). `set_value` cannot be moved off the main thread through the public API. A 4.4 MB /
  50 K-line file costs 13 ms.
- **Soft wrap is the default and does not scale**: it lays out every line inside `set_value`. 50 K lines: 1.8 s
  freeze. 1.25 M lines: **53 s freeze and 7.1 GB RSS**. Turn it off for code and for anything big; if wrapping is
  wanted (prose, chat input), only for small texts.
- [U] Rows are 20 px at the default 13 px mono font, and `Pixels` is an `f32`: positions are whole-pixel exact
  only up to 2^24 px (~840 K rows), then round to 2 px. Jumping to row 1.196 M worked
  (`visible_row_range` = 1196000..1196030); nobody looked at the pixels there (locked screen).

## 12. Markdown (streaming)

[V]

```rust
use gpui_kit::component::text::{TextView, TextViewState};

let md = cx.new(|cx| TextViewState::markdown("# Agent\n\n", cx));
md.read(cx).list_state().set_follow_mode(FollowMode::Tail);   // stick to the bottom while streaming
md.update(cx, |s, cx| s.push_str(token, cx));                 // append; parses incrementally on a background task
md.update(cx, |s, cx| s.set_text(full, cx));                  // a new text that extends the old one is also an append

TextView::new(&md).scrollable(true).size_full().p_3()         // scrollable needs a parent with a definite height
```

- `push_str` sends the delta to a background parser; the parsed blocks come back and the state notifies itself.
  You never call `cx.notify()` for it.
- Streamed text fades in by default (`TextViewMotion`, a timer that stops by itself). Measured: no CPU after the
  stream ends.
- Inline `TextView::markdown(id, text)` exists for small static text; it re-hands the whole string every frame.
  For chat use the state version.
- Code blocks show a language label; highlighting needs a tree-sitter feature (§1).

## 13. Lists for chat messages

[V] wai's chat (`crates/wai/src/chat`) uses `MessageScroller` as described below: rows come from
`cx.processor(Self::render_item)` (a `Fn(usize, &mut Window, &mut App)` that updates the view), the
state grows with `append(n, cx)`, and visible rows are remeasured on every layout, so a growing
message follows the tail without `remeasure_items`. Its scrollbar shows on scroll and fades out
2 s later with a 500 ms animation at the display rate (~60 frames on a 120 Hz screen).

Three options, read in source:

- `gpui_kit::component::message_scroller::{MessageScroller, MessageScrollerState}`: the chat component.
  `MessageScrollerState::new(item_count, cx)` (follows the tail), `.append(n, cx)`, `.splice(..)`,
  `.remeasure_items(range, cx)`, `.scroll_to_end(cx)`. Render with
  `MessageScroller::new("chat", state, |ix, window, cx| row(ix))`, plus a "Jump to latest" button.
  Rows have variable height. **Start here for the chat.**
- GPUI `list(list_state, render_item)` with `ListState::new(count, ListAlignment::Top, overdraw_px)`: variable heights,
  what `TextView` and `MessageScroller` are built on.
- `v_virtual_list(view, id, Rc<Vec<Size<Pixels>>>, |view, range, window, cx| -> Vec<_>)`: needs every item's size up
  front; good for uniform rows (file tree, tables).

## 14. Theme, fonts and icons

- [V] **Icons need the asset source**: `gpui_kit::application().with_assets(gpui_kit::assets::Assets)`.
  Without it every `Icon` (button icons, the Select chevron, status icons) renders blank, with no error.
- [V] `Select`'s own view is `size_full()`; `.w(..)` on `Select` styles only its inner trigger. Put it
  in a sized box: `div().w(px(140.)).child(Select::new(&state))`.

[V]

```rust
use gpui_kit::component::ActiveTheme as _;
let theme = cx.theme();                         // on App, Context<T> (deref) — not on Window
theme.background; theme.foreground; theme.border; theme.muted; theme.selection; // Hsla, via Deref<Target=ThemeColor>
theme.mono_font_family.clone();                 // Menlo on macOS, Consolas on Windows (with fallbacks)
theme.mono_font_size;                           // 13 px by default
theme.is_dark();

let ts = window.text_system();                  // cell width for a monospace grid
let cell_w = ts.advance(ts.resolve_font(&font(family)), font_size, 'm').map(|s| s.width)?;
```

## 15. Terminal model (alacritty_terminal 0.26 + portable-pty 0.9)

[V] (not GPUI, but every implementer needs it; written from the crates' docs, no Zed code)

```rust
let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
let mut cmd = CommandBuilder::new(std::env::var("SHELL").unwrap_or("/bin/sh".into()));
cmd.env("TERM", "xterm-256color");
let child = pair.slave.spawn_command(cmd)?;      // keep `child` alive
drop(pair.slave);
let reader = pair.master.try_clone_reader()?;    // blocking Read -> std::thread -> channel
let writer = pair.master.take_writer()?;         // keys go here

let term = Term::new(TermConfig::default(), &TermSize::new(cols, rows), listener); // term::test::TermSize is pub
let mut parser: Processor = Processor::new();    // alacritty_terminal::vte::ansi::Processor
parser.advance(&mut term, &bytes);               // then cx.notify()
term.grid()[Line(row)][Column(col)].c;           // cell char; skip Flags::WIDE_CHAR_SPACER
term.grid().cursor.point;  term.mode().contains(TermMode::APP_CURSOR);   // also TermMode::SHOW_CURSOR
term.resize(TermSize::new(cols, rows)); pair.master.resize(PtySize { .. })?;
```

- The listener (`impl EventListener`) must write `Event::PtyWrite(text)` back to the PTY (cursor-position and
  device-attribute replies), or some programs hang.
- Keys: enter `\r`, backspace `0x7f`, tab `\t`, escape `0x1b`, arrows `ESC [ A–D` (`ESC O A–D` when `APP_CURSOR`),
  ctrl+letter `letter & 0x1f`, otherwise `key_char` bytes.
- The spike paints rows as plain strings (no colors, no selection, no scrollback UI). The real view should paint
  cell runs with `fg/bg/flags`.

## 16. Frames, idle and measuring

- [V] GPUI's macOS window pauses its `CAMetalDisplayLink` after any frame in which nothing drew, presented or asked
  for another frame. With no timers and no notifies the app sits at 0.0 % CPU.
- [V] A focused `Editor` blinks its caret every 500 ms forever by default: **~0.9 % CPU** while it has focus (over
  the 0.5 % idle budget). Fix, verified (back to 0.0 % once the timeout passes):

  ```rust
  use gpui_kit::component::input::CursorBlink;
  Theme::update(cx, |t| t.cursor_blink = CursorBlink::default().with_idle_timeout(Duration::from_secs(5)));
  // or CursorBlink::steady()
  ```
- [V] A token stream redraws the whole window per update. 40 tokens/s → 40 frames/s at ~3 ms CPU each (p95
  4.4–5.5 ms) → **~14–17 % of one core while streaming**. Things to try in Phase 1: coalesce updates to a lower
  rate, `AnyView::cached(..)` for panes that did not change, profile the `TextView` cost as the document grows.
- [V] Timing a frame without the display: `let t = Instant::now(); window.draw(cx).clear(cx); t.elapsed()` inside
  `cx.update(|window, cx| …)` from an `AsyncWindowContext`. It measures render + layout + prepaint + paint. Do not
  ship it: a drawn but never presented frame keeps the display link running.
- [D] `profiler` feature on gpui-kit enables GPUI frame-event instrumentation.
- [V] Chat streaming (release, 943 chunks at 25 ms, ~40 tokens/s): one `push_str` per token gave one
  redraw per token (~4 ms each) and **12–18 %** of a core. Pushing the buffered delta once per
  100 ms (a timer that exists only while text is pending) gave **~5 %**. Skip `cx.notify()` for the
  append itself: the `TextViewState` notifies once its background parse lands.

## 17. Tests (`#[gpui_kit::test]`)

[V] (`spikes/s2-ui/src/main.rs`, `mod tests`, run with `cargo test -p s2-ui --release --features test-support`)

```rust
#[cfg(test)]
mod tests {
    // NOT `use super::*` / `use gpui_kit::*`: the glob brings GPUI's `test` macro, which shadows Rust's `#[test]`
    // and makes `#[gpui_kit::test]` fail with "recursion limit reached while expanding `#[test]`".
    use gpui_kit::prelude::*;                  // traits only
    use gpui_kit::TestAppContext;

    gpui_kit::actions!(t, [Bump]);
    struct Counter { n: usize, focus: gpui_kit::FocusHandle }
    impl Render for Counter {
        fn render(&mut self, _: &mut gpui_kit::Window, cx: &mut Context<Self>) -> impl IntoElement {
            gpui_kit::div().key_context("Counter").track_focus(&self.focus)
                .on_action(cx.listener(|this: &mut Self, _: &Bump, _, cx| { this.n += 1; cx.notify(); }))
                .child(self.n.to_string())
        }
    }

    #[gpui_kit::test]
    fn scoped_binding_dispatches_action(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.bind_keys([gpui_kit::KeyBinding::new("cmd-k", Bump, Some("Counter"))]);
        });
        let (counter, cx) = cx.add_window_view(|window, cx| {   // cx is now &mut VisualTestContext
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            Counter { n: 0, focus }
        });
        cx.simulate_keystrokes("cmd-k cmd-k");                  // space-separated; runs until parked
        assert_eq!(counter.read_with(cx, |c, _| c.n), 2);
    }
}
```

- `cx.simulate_input("echo hi")` types text (each char becomes a key with `key_char`); `simulate_keystrokes("enter
  ctrl-c up")` sends named keys. Both run the scheduler until parked.
- Real I/O (a PTY, a child process) runs outside the deterministic scheduler: call `cx.executor().allow_parking()`
  first, then loop `std::thread::sleep(..)` + `cx.run_until_parked()` until the state you expect shows up
  (see `keys_reach_the_shell`).
- [V] Timers (`background_executor().timer`) run on the test scheduler's fake clock and never fire on
  their own: `cx.executor().advance_clock(d)` moves it (wai's `wait_until` advances it with each sleep).
- [V] `gpui_kit::test::TestWindowExt`: `cx.update(|window, cx| window.click("perm-allow_once", cx))` clicks a
  `Button` by its element id with real hit testing; `window.try_find(id)` after `window.render_frame(cx)`.
- Richer queries (`window.find("id")`, `click`, `input`, `ElementSnapshot`) come from `gpui_kit::test::*`
  (`TestWindowExt`, …) behind `gpui-kit/test-support`. [D] (`gpui-kit-0.9.0/TESTING.md`, `tests/ui.rs`). See the
  gotcha below about enabling it.
- Offscreen screenshot of a real window (works with the screen locked): `window.render_to_image()` →
  `image::RgbaImage` (`.save("x.png")`), test-support only. [V] (`S2_SHOT` in the spike)
  **Not inside `#[gpui_kit::test]`:** its `TestPlatform` has no headless renderer ("render_to_image not
  available") and a no-op text system. `VisualTestAppContext` (real Mac platform) needs the process main thread,
  and libtest runs tests on spawned threads even with `--test-threads=1`. wai captures from the real app
  instead, behind its `screenshot` feature (`WAI_SHOT=<png> cargo run -p wai --features screenshot -- <dir>`).

## 18. Gotchas hit in the spike

1. **`gpui-kit/test-support` does not resolve** in a workspace whose lock picked current crates: it pulls
   `gpui-fast` into resolution, which pins `taffy = "=0.15.0"` and `unicode-properties = "=0.1.3"` while
   `gpui-pre` had locked 0.15.1 and 0.1.5. Workaround used: an optional direct dependency
   `gpui-pre-platform = { version = "=0.3.11", features = ["test-support"] }` behind a crate feature (enables
   `TestAppContext`, `VisualTestContext`, `render_to_image`, but not `gpui_kit::test::*`). The other fix is
   `cargo update -p <crate> --precise <ver>` for each pinned crate to match `gpui-kit-0.9.0/Cargo.lock`.
2. **`use super::*` in a test module** that sits under `use gpui_kit::*;` → "recursion limit reached while
   expanding `#[test]`" on `#[gpui_kit::test]`. Import explicitly (§17).
3. **Soft wrap on by default**: 53 s freeze and 7.1 GB on a 112 MB file. `soft_wrap(false)` (§11).
4. **Caret blink never stops by default**: ~0.9 % CPU while an editor has focus. `with_idle_timeout` (§16).
5. **Screen locked (unattended machine)**: `screencapture` fails ("could not create image from display") and
   frame delivery was inconsistent (one run painted 3 frames in 20 s, later runs ~one per update). A frame-time
   probe that waits for natural frames collected nothing and the stats code panicked on an empty vector. Use
   forced `window.draw()` timing for benchmarks and `window.render_to_image()` for screenshots.
6. **Forcing `window.draw()` without a present** keeps the display link alive: ~0.3 % CPU afterwards. Measure
   idle in a separate run.
7. **Windows cross-check from macOS** (`cargo check --target x86_64-pc-windows-msvc`) stops in the build script of
   `gpui-pre`: `embed_resource` needs `llvm-rc` (`NotAttempted("llvm-rc")`), turned on by `gpui-pre-platform`'s
   `windows-manifest` feature. The other ~340 crates had checked by then. Use a Windows runner (MSVC `rc.exe`) or
   LLVM's `llvm-rc` on the Mac.

Differences from older GPUI examples found online (no error hit because the source was read first):
`window.focus(&handle, cx)` takes `cx`; `cx.spawn(async move |this, cx| …)` takes an async closure with a
`WeakEntity` (not `|this, mut cx| async move`); `WeakEntity::update` returns `Result`; `Pixels.0` is private
(`a / b` → `f32`); `EditorState::soft_wrap(bool)` is `#[doc(hidden)]` (runtime setter: `set_soft_wrap(bool, window,
cx)`).
