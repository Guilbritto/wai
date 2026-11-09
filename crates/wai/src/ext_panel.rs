//! An extension's panel, a sidebar tab: the last tree it rendered (`panel/render`), drawn with
//! gpui-component. Clicks go back to it as `panel/action` with the form's values.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Map, Value};
use wai_ext::panel::{ButtonStyle, Item, Node, TooLarge};

use crate::sidebar::ROW_HEIGHT;

/// A list row with a subtitle: two lines.
const TWO_LINE_ROW: f32 = 44.;
/// ponytail: a list shows at most this many rows and scrolls the rest (virtual, any length); a
/// list that fills the panel would need the layout to know which node may grow.
const LIST_ROWS: usize = 12;

/// A list item or button was clicked: `values` holds each form field's value by id.
pub struct PanelAction {
    pub action: String,
    pub values: Value,
}

pub struct ExtPanel {
    /// `None` until the extension renders. Parsed once per render, off the main thread.
    tree: Option<Result<Rc<Node>, TooLarge>>,
    /// What was typed, per input id. Kept across renders while the id stays an input.
    inputs: HashMap<String, Entity<InputState>>,
    /// Each select's choice and each checkbox's state, per id; kept the same way.
    values: HashMap<String, Value>,
}

impl EventEmitter<PanelAction> for ExtPanel {}

impl ExtPanel {
    pub fn new() -> Self {
        Self {
            tree: None,
            inputs: HashMap::new(),
            values: HashMap::new(),
        }
    }

    pub fn set_tree(&mut self, tree: Result<Node, TooLarge>, cx: &mut Context<Self>) {
        // A refused tree leaves the form as it was.
        if let Ok(tree) = &tree {
            let (mut inputs, mut fields) = (HashSet::new(), HashSet::new());
            tree.walk(&mut |node| match node {
                Node::Input { id, .. } => {
                    inputs.insert(id.as_str());
                }
                Node::Select { id, .. } | Node::Checkbox { id, .. } => {
                    fields.insert(id.as_str());
                }
                _ => {}
            });
            self.inputs.retain(|id, _| inputs.contains(id.as_str()));
            self.values.retain(|id, _| fields.contains(id.as_str()));
        }
        self.tree = Some(tree.map(Rc::new));
        cx.notify();
    }

    fn act(&mut self, action: String, cx: &mut Context<Self>) {
        let values = self.form(cx);
        cx.emit(PanelAction { action, values });
    }

    fn set(&mut self, id: String, value: Value, cx: &mut Context<Self>) {
        self.values.insert(id, value);
        cx.notify();
    }

    /// Every form field of the tree shown, by id.
    fn form(&self, cx: &App) -> Value {
        let mut form = Map::new();
        if let Some(Ok(tree)) = &self.tree {
            tree.walk(&mut |node| {
                let (id, value) = match node {
                    Node::Input { id, .. } => {
                        let text = self.inputs.get(id).map(|s| s.read(cx).value());
                        (id, Value::from(text.unwrap_or_default().to_string()))
                    }
                    Node::Select { id, options } => {
                        let chosen = self.chosen(id, options);
                        (id, chosen.map_or(Value::Null, |o| Value::from(&**o)))
                    }
                    Node::Checkbox { id, .. } => (id, Value::Bool(self.checked(id))),
                    _ => return,
                };
                form.insert(id.clone(), value);
            });
        }
        Value::Object(form)
    }

    /// The option chosen, while it is still offered; else the first, as in HTML.
    fn chosen<'a>(&self, id: &str, options: &'a [Arc<str>]) -> Option<&'a Arc<str>> {
        let chosen = self.values.get(id).and_then(Value::as_str);
        let offered = chosen.and_then(|c| options.iter().find(|o| ***o == *c));
        offered.or(options.first())
    }

    fn checked(&self, id: &str) -> bool {
        self.values.get(id) == Some(&Value::Bool(true))
    }

    /// `n` numbers the lists, markdown and progress bars for their element ids. Recursion is
    /// bounded: a tree is at most `MAX_DEPTH` deep.
    fn node(
        &mut self,
        node: &Node,
        n: &mut usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        *n += 1;
        match node {
            Node::Column(children) | Node::Row(children) => {
                let children: Vec<_> = (children.iter())
                    .map(|child| self.node(child, n, window, cx))
                    .collect();
                match node {
                    Node::Row(_) => h_flex().gap_2().flex_wrap().children(children),
                    _ => v_flex().gap_2().children(children),
                }
                .into_any_element()
            }
            Node::Text { text, muted: dim } => div()
                .text_sm()
                .when(*dim, |el| el.text_color(muted))
                .child(SharedString::from(text))
                .into_any_element(),
            Node::Markdown(text) => TextView::markdown(("panel-md", *n), SharedString::from(text))
                .text_sm()
                .into_any_element(),
            Node::List { items, subtitles } => self.list(*n, items, *subtitles, cx),
            Node::Button { id, label, style } => {
                let action = id.clone();
                Button::new(name("panel-button", id))
                    .small()
                    .label(SharedString::from(label))
                    .map(|b| match style {
                        ButtonStyle::Primary => b.primary(),
                        ButtonStyle::Danger => b.danger(),
                        ButtonStyle::Default => b,
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.act(action.clone(), cx)))
                    .into_any_element()
            }
            Node::Input { id, placeholder } => {
                // ponytail: the placeholder is the one the input was created with; update it here
                // if extensions come to change it.
                let state = self.inputs.entry(id.clone()).or_insert_with(|| {
                    let placeholder = SharedString::from(placeholder);
                    cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
                });
                Input::new(state).small().into_any_element()
            }
            Node::Select { id, options } => {
                let chosen = self.chosen(id, options).cloned();
                let label = chosen.as_ref().map_or("—".into(), SharedString::from);
                let (this, field, options) = (cx.weak_entity(), id.clone(), options.clone());
                Button::new(name("panel-select", id))
                    .small()
                    .outline()
                    .dropdown_caret(true)
                    .label(label)
                    .dropdown_menu(move |menu, _, _| {
                        options.iter().fold(menu, |menu, option| {
                            let (this, field) = (this.clone(), field.clone());
                            let value = Value::from(&**option);
                            let item = PopupMenuItem::new(SharedString::from(option))
                                .checked(chosen.as_ref() == Some(option))
                                .on_click(move |_, _, cx| {
                                    let (field, value) = (field.clone(), value.clone());
                                    this.update(cx, |this, cx| this.set(field, value, cx)).ok();
                                });
                            menu.item(item)
                        })
                    })
                    .into_any_element()
            }
            Node::Checkbox { id, label } => {
                let field = id.clone();
                Checkbox::new(name("panel-checkbox", id))
                    .small()
                    .label(SharedString::from(label))
                    .checked(self.checked(id))
                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        this.set(field.clone(), Value::Bool(*checked), cx)
                    }))
                    .into_any_element()
            }
            Node::Badge(text) => h_flex()
                .child(Tag::secondary().small().child(SharedString::from(text)))
                .into_any_element(),
            Node::Progress(value) => Progress::new(("panel-progress", *n))
                .value(value * 100.)
                .into_any_element(),
            Node::Unsupported => unsupported(muted),
        }
    }

    /// Only the visible rows render: a list may hold any number of items.
    fn list(
        &self,
        n: usize,
        items: &Arc<[Option<Item>]>,
        subtitles: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let height = if subtitles { TWO_LINE_ROW } else { ROW_HEIGHT };
        let rows = items.len().min(LIST_ROWS) as f32;
        let (items, this) = (items.clone(), cx.weak_entity());
        uniform_list(("panel-list", n), items.len(), move |range, _, cx| {
            range
                .map(|ix| row(ix, items[ix].as_ref(), height, &this, cx))
                .collect::<Vec<_>>()
        })
        .w_full()
        .h(px(height * rows))
        .into_any_element()
    }
}

fn row(
    ix: usize,
    item: Option<&Item>,
    height: f32,
    panel: &WeakEntity<ExtPanel>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let row = h_flex().w_full().h(px(height)).px_1().gap_2();
    let Some(item) = item else {
        return row
            .child(unsupported(theme.muted_foreground))
            .into_any_element();
    };
    let (action, panel) = (item.id.clone(), panel.clone());
    let subtitle = item.subtitle.as_ref().map(|s| {
        let s = SharedString::from(s);
        div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .truncate()
            .child(s)
    });
    let title = div()
        .text_sm()
        .truncate()
        .child(SharedString::from(&item.title));
    row.id(("panel-item", ix))
        .rounded(theme.radius)
        .cursor_pointer()
        .hover(|el| el.bg(theme.sidebar_accent))
        .child(v_flex().flex_1().min_w_0().child(title).children(subtitle))
        .children(
            item.badge
                .as_ref()
                .map(|badge| Tag::secondary().small().child(SharedString::from(badge))),
        )
        .on_click(move |_, _, cx| {
            panel
                .update(cx, |panel, cx| panel.act(action.clone(), cx))
                .ok();
        })
        .test_support()
        .into_any_element()
}

fn unsupported(muted: Hsla) -> AnyElement {
    div()
        .debug_selector(|| "panel-unsupported".into())
        .text_sm()
        .text_color(muted)
        .child("Unsupported element")
        .into_any_element()
}

/// A button or field's element id. Their ids are unique in a tree, and the kind keeps one kind's
/// element state from meeting another's.
fn name(kind: &str, id: &str) -> ElementId {
    ElementId::Name(format!("{kind}:{id}").into())
}

impl Render for ExtPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let status = |text: &'static str, selector: &'static str| {
            div()
                .debug_selector(move || selector.into())
                .text_sm()
                .text_color(muted)
                .child(text)
                .into_any_element()
        };
        let body = match self.tree.clone() {
            None => status("Waiting for the extension…", "panel-waiting"),
            Some(Err(TooLarge)) => status("Panel too large", "panel-too-large"),
            Some(Ok(tree)) => self.node(&tree, &mut 0, window, cx),
        };
        v_flex()
            .id("ext-panel")
            .size_full()
            .p_2()
            .overflow_y_scroll()
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, ElementId, Entity, SharedString, TestAppContext, VisualTestContext,
    };
    use serde_json::{Value, json};
    use wai_ext::panel::{self, Node};
    use wai_settings::Settings;

    use super::ExtPanel;
    use crate::app;
    use crate::workspace::Workspace;

    /// An `ExtPanel` alone in a window.
    fn panel_window(cx: &mut TestAppContext) -> (Entity<ExtPanel>, &mut VisualTestContext) {
        cx.update(|cx| app::init(&Settings::default(), cx));
        let (window, panel) = cx
            .update(|cx| {
                gpui_kit::open_window(Default::default(), cx, |_, cx| cx.new(|_| ExtPanel::new()))
            })
            .expect("open window");
        let cx = Box::leak(Box::new(VisualTestContext::from_window(window, cx)));
        (panel, cx)
    }

    fn show(panel: &Entity<ExtPanel>, tree: Value, cx: &mut VisualTestContext) {
        panel.update(cx, |p, cx| p.set_tree(panel::parse(&tree), cx));
        cx.update(|window, cx| window.render_frame(cx));
    }

    /// Each kind of bad node is drawn as one muted line; the good ones around it still show.
    #[gpui_kit::test]
    fn unknown_node_renders_placeholder(cx: &mut TestAppContext) {
        let (panel, cx) = panel_window(cx);
        show(
            &panel,
            json!({"type": "column", "children": [
                {"type": "sparkline", "points": [1, 2]},
                {"type": "button", "id": "go", "label": "Go"},
            ]}),
            cx,
        );
        assert!(cx.debug_bounds("panel-unsupported").is_some());
        assert!(cx.update(|window, _| window.try_find(name("panel-button:go")).is_some()));

        // Too deep: the whole panel says so.
        let mut deep = json!({"type": "text", "text": "leaf"});
        for _ in 0..panel::MAX_DEPTH {
            deep = json!({"type": "column", "children": [deep]});
        }
        show(&panel, deep, cx);
        assert!(cx.debug_bounds("panel-too-large").is_some());
        assert!(cx.debug_bounds("panel-unsupported").is_none());
    }

    /// The renderer never panics: broken nodes, cut strings, a huge list, every node kind.
    #[gpui_kit::test]
    fn hostile_trees_render(cx: &mut TestAppContext) {
        let (panel, cx) = panel_window(cx);
        let huge = "x\n".repeat(1 << 16);
        let items: Vec<_> = (0..50_000)
            .map(|n| match n % 3 {
                0 => json!({"id": "same", "title": huge, "subtitle": huge, "badge": huge}),
                1 => json!({"id": n.to_string(), "title": "t"}),
                _ => json!(n),
            })
            .collect();
        let trees = [
            json!(null),
            json!([]),
            json!({"type": "column", "children": [
                {"type": "list", "items": items},
                {"type": "list", "items": []},
                {"type": "text", "text": huge, "muted": true},
                {"type": "markdown", "text": "# Title\n\n```rust\nfn main() {}\n```\n| a |\n|---|"},
                {"type": "markdown", "text": huge},
                {"type": "row", "children": [
                    {"type": "button", "id": "b", "label": huge, "style": "danger"},
                    {"type": "input", "id": "i", "placeholder": huge},
                    {"type": "select", "id": "s", "options": []},
                    {"type": "select", "id": "t", "options": [huge]},
                    {"type": "checkbox", "id": "c", "label": huge},
                    {"type": "badge", "text": huge},
                    {"type": "progress", "value": 2},
                    {"type": "progress", "value": -2},
                ]},
                {"type": "row", "children": [1, "a", null, [], {}]},
                {"type": "column", "children": []},
            ]}),
        ];
        for tree in trees {
            show(&panel, tree, cx);
        }
        cx.update(|window, _| window.try_find(("panel-item", 0usize)).expect("list rows"));
    }

    fn name(id: &str) -> ElementId {
        ElementId::Name(SharedString::from(id.to_string()))
    }

    #[cfg(unix)]
    const MANIFEST: &str = "id = \"cards\"\nname = \"Cards\"\nversion = \"1.0.0\"\n\
        permissions = [\"process\", \"panels\"]\n[process]\ncommand = \"bin/run\"\n\
        [[contributes.panels]]\nid = \"main\"\ntitle = \"Cards\"\n";

    /// The workspace with extension `cards` (`wai-fake-ext --panel`) running and its panel's tab
    /// open, once its first tree has arrived.
    #[cfg(unix)]
    fn open_sample<'a>(
        tmp: &Path,
        cx: &'a mut TestAppContext,
    ) -> (
        Entity<Workspace>,
        Entity<ExtPanel>,
        &'a mut VisualTestContext,
    ) {
        use wai_ext::{ExtStore, Permission, Source};

        use crate::extensions_view::tests::{click, running_sample_with, workspace};
        use crate::workspace::tests::wait_until;

        cx.executor().allow_parking(); // the extension is a real process
        let config = tmp.join("config");
        let src = running_sample_with(&tmp.join("src"), MANIFEST, &tmp.join("pid"), "--panel");
        let mut store = ExtStore::open(&config);
        let staged = store.stage(&Source::Folder(src)).unwrap();
        store
            .commit(staged, vec![Permission::Process, Permission::Panels])
            .unwrap();
        let (workspace, cx) = workspace(&config, cx);
        workspace.update(cx, |w, cx| w.start_extensions(cx));
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        let panel = host.read_with(cx, |h, _| h.panels().first().map(|p| p.view.clone()));
        let panel = panel.expect("a tab for the panel");
        wait_until(cx, "the first tree", |cx| {
            panel.read_with(cx, |p, _| p.tree.is_some())
        });
        click(("tab-panel", 0usize), cx);
        (workspace, panel, cx)
    }

    /// Waits for the fake's echo of action `id`; returns the values it got.
    #[cfg(unix)]
    fn echoed(panel: &Entity<ExtPanel>, id: &str, cx: &mut VisualTestContext) -> Value {
        let prefix = format!("last action: {id} ");
        let mut values = None;
        crate::workspace::tests::wait_until(cx, &prefix, |cx| {
            let tree = panel.read_with(cx, |p, _| p.tree.clone());
            let Some(Ok(tree)) = tree else { return false };
            tree.walk(&mut |node| {
                if let Node::Text { text, .. } = node
                    && let Some(json) = text.strip_prefix(&prefix)
                {
                    values = serde_json::from_str(json).ok();
                }
            });
            values.is_some()
        });
        values.unwrap()
    }

    /// Clicking a list row sends `panel/action` with the row's id and the form's values; the
    /// tab sits after the built-in ones.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn panel_list_click_sends_action(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let (_, panel, cx) = open_sample(tmp.path(), cx);
        let tabs = cx.update(|window, _| {
            ["tab-agents", "tab-panel"].map(|id| match id {
                "tab-panel" => window.find(("tab-panel", 0usize)).bounds(),
                id => window.find(id).bounds(),
            })
        });
        assert!(tabs[0].right() <= tabs[1].left() || tabs[0].bottom() <= tabs[1].top());

        crate::extensions_view::tests::click(("panel-item", 1usize), cx);
        assert_eq!(
            echoed(&panel, "card-2", cx),
            json!({"query": "", "scope": "Mine", "done": false})
        );
    }

    /// A button sends what the form holds; the form keeps it across the re-render that follows.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn panel_button_sends_form_values(cx: &mut TestAppContext) {
        use crate::extensions_view::tests::click;

        let tmp = tempfile::tempdir().unwrap();
        let (_, panel, cx) = open_sample(tmp.path(), cx);
        let input = panel.read_with(cx, |p, _| p.inputs.get("query").cloned());
        let input = input.expect("the query input");
        cx.update(|window, cx| input.update(cx, |s, cx| s.set_value("login", window, cx)));
        click(name("panel-checkbox:done"), cx);
        click(name("panel-select:scope"), cx);
        cx.update(|window, cx| window.within("items").click(1usize, cx));
        cx.run_until_parked();

        click(name("panel-button:refresh"), cx);
        let values = json!({"query": "login", "scope": "Team", "done": true});
        assert_eq!(echoed(&panel, "refresh", cx), values);
        click(name("panel-button:clear"), cx);
        assert_eq!(echoed(&panel, "clear", cx), values);
    }

    /// R44: only an enabled extension granted `panels` has tabs. When its tab goes, the sidebar
    /// shows the sessions.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn panel_tabs_need_the_grant(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt as _;
        use wai_ext::{ExtStore, Permission};

        let tmp = tempfile::tempdir().unwrap();
        let (workspace, _, cx) = open_sample(tmp.path(), cx);
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        let installed = ExtStore::open(&tmp.path().join("config")).list();
        let sync = |list: Vec<wai_ext::Installed>, cx: &mut VisualTestContext| {
            host.update(cx, |h, cx| h.sync(&list, cx));
            cx.update(|window, cx| window.render_frame(cx));
            cx.update(|window, _| window.try_find(("tab-panel", 0usize)).is_some())
        };

        let mut ungranted = installed.clone();
        ungranted[0].granted.retain(|p| *p != Permission::Panels);
        assert!(!sync(ungranted, cx), "a tab without the grant");
        assert!(host.read_with(cx, |h, _| h.panels().is_empty()));
        assert!(cx.update(|window, _| window.try_find("all-projects").is_some()));

        let mut disabled = installed.clone();
        disabled[0].enabled = false;
        assert!(!sync(disabled, cx), "a tab while disabled");
        assert!(sync(installed, cx));
    }
}
