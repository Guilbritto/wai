//! An extension's panel, a sidebar tab or a main-area pane: the last tree it rendered
//! (`panel/render`), drawn with gpui-component. Clicks go back to it as `panel/action` with the
//! form's values.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::alert::{Alert, AlertVariant};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::link::Link;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::text::TextView;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Map, Value};
use wai_ext::panel::{ButtonStyle, Column, Confirm, Item, Node, TableRow, Tone, TooLarge};

use crate::aurora;
use crate::sidebar::ROW_HEIGHT;

/// A list row with a subtitle: two lines.
const TWO_LINE_ROW: f32 = 44.;
/// A list, table or log without `fill` shows at most this many rows (lines) and scrolls the rest.
const LIST_ROWS: usize = 12;
/// R57: a list is never narrower, so a wide sibling in a row wraps below it instead.
const LIST_MIN_WIDTH: f32 = 160.;
const LOG_LINE: f32 = 18.;
/// A `kv` key's width.
const KV_KEY: f32 = 120.;

/// A list item or button was clicked: `values` holds each form field's value by id.
pub struct PanelAction {
    pub action: String,
    pub values: Value,
}

/// An input's state: one line, or a text area of `rows` lines.
#[derive(Clone)]
pub(crate) enum Field {
    Line(Entity<InputState>),
    Area(Entity<TextareaState>, u8),
}

impl Field {
    fn new(rows: Option<u8>, window: &mut Window, cx: &mut App) -> Self {
        match rows {
            None => Self::Line(cx.new(|cx| InputState::new(window, cx))),
            Some(rows) => {
                let state = cx.new(|cx| TextareaState::new(window, cx).rows(rows.into()));
                Self::Area(state, rows)
            }
        }
    }

    fn rows(&self) -> Option<u8> {
        match self {
            Self::Line(_) => None,
            Self::Area(_, rows) => Some(*rows),
        }
    }

    pub(crate) fn value(&self, cx: &App) -> SharedString {
        match self {
            Self::Line(state) => state.read(cx).value(),
            Self::Area(state, _) => state.read(cx).value(),
        }
    }

    pub(crate) fn set_value(&self, text: &str, window: &mut Window, cx: &mut App) {
        let text = text.to_string();
        match self {
            Self::Line(s) => s.update(cx, |s, cx| s.set_value(text, window, cx)),
            Self::Area(s, _) => s.update(cx, |s, cx| s.set_value(text, window, cx)),
        }
    }

    fn set_placeholder(&self, text: SharedString, window: &mut Window, cx: &mut App) {
        match self {
            Self::Line(s) => s.update(cx, |s, cx| s.set_placeholder(text, window, cx)),
            Self::Area(s, _) => s.update(cx, |s, cx| s.set_placeholder(text, window, cx)),
        }
    }
}

/// Where a node sits. `column`: every parent is a column (or the panel), so a `fill` may grow;
/// `row`: the parent is a row; `click`: inside a clickable card or table row.
#[derive(Clone, Copy)]
struct At {
    column: bool,
    row: bool,
    click: bool,
}

pub struct ExtPanel {
    /// The extension's name, shown in its confirmations.
    ext_name: SharedString,
    /// `None` until the extension renders. Parsed once per render, off the main thread.
    pub(crate) tree: Option<Result<Rc<Node>, TooLarge>>,
    /// What was typed and the placeholder shown, per input id. Kept across renders while the id
    /// stays an input.
    pub(crate) inputs: HashMap<String, (Field, Arc<str>)>,
    /// Each select's choice, each checkbox's state and each section's openness, per id; kept the
    /// same way.
    values: HashMap<String, Value>,
    /// The initial value the extension last sent, per field id. Only a different one replaces
    /// what the user did.
    initials: HashMap<String, Value>,
    /// Initial input values not shown yet: setting one needs the window, so it waits for render.
    pending: HashMap<String, Arc<str>>,
    /// Each log's scroll and the line count it last showed, by node number.
    logs: HashMap<usize, (UniformListScrollHandle, usize)>,
}

impl EventEmitter<PanelAction> for ExtPanel {}

impl ExtPanel {
    pub fn new(ext_name: SharedString) -> Self {
        Self {
            ext_name,
            tree: None,
            inputs: HashMap::new(),
            values: HashMap::new(),
            initials: HashMap::new(),
            pending: HashMap::new(),
            logs: HashMap::new(),
        }
    }

    pub fn set_tree(&mut self, tree: Result<Node, TooLarge>, cx: &mut Context<Self>) {
        // A refused tree leaves the form as it was.
        if let Ok(tree) = &tree {
            let (mut inputs, mut fields) = (HashSet::new(), HashSet::new());
            tree.walk(&mut |node| {
                let (id, initial) = match node {
                    Node::Input { id, value, .. } => {
                        inputs.insert(id.as_str());
                        (id, value.as_deref().map(Value::from))
                    }
                    Node::Select { id, value, .. } => (id, value.as_deref().map(Value::from)),
                    Node::Checkbox { id, checked, .. } => (id, checked.map(Value::Bool)),
                    Node::Section { id, open, .. } => (id, open.map(Value::Bool)),
                    _ => return,
                };
                fields.insert(id.as_str());
                let Some(initial) = initial else {
                    self.initials.remove(id);
                    return;
                };
                if self.initials.get(id) == Some(&initial) {
                    return;
                }
                match (node, &initial) {
                    (Node::Input { .. }, Value::String(text)) => {
                        self.pending.insert(id.clone(), text.as_str().into());
                    }
                    _ => {
                        self.values.insert(id.clone(), initial.clone());
                    }
                }
                self.initials.insert(id.clone(), initial);
            });
            self.inputs.retain(|id, _| inputs.contains(id.as_str()));
            self.pending.retain(|id, _| inputs.contains(id.as_str()));
            self.values.retain(|id, _| fields.contains(id.as_str()));
            self.initials.retain(|id, _| fields.contains(id.as_str()));
        }
        self.tree = Some(tree.map(Rc::new));
        cx.notify();
    }

    fn act(&mut self, action: String, cx: &mut Context<Self>) {
        let values = self.form(cx);
        cx.emit(PanelAction { action, values });
    }

    /// A button or menu item: sends `action`, after asking when it has a `confirm`. The action is
    /// the one clicked; the values are the form's at Confirm. Nothing goes if the panel is gone.
    fn press(
        &mut self,
        action: String,
        confirm: Option<&Confirm>,
        danger: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(confirm) = confirm else {
            return self.act(action, cx);
        };
        let this = cx.weak_entity();
        let ext = self.ext_name.clone();
        let title = SharedString::from(&confirm.title);
        let text = confirm.text.as_ref().map(SharedString::from);
        let label = (confirm.label.as_ref()).map_or("Confirm".into(), SharedString::from);
        // A dialog fading out still takes clicks.
        let sent = Rc::new(Cell::new(false));
        window.open_dialog(cx, move |dialog, _, cx| {
            let (this, action, sent) = (this.clone(), action.clone(), sent.clone());
            // The extension's name, so its question cannot pass for wai's own.
            let heading = v_flex()
                .child(
                    div()
                        .debug_selector(|| "confirm-extension".into())
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(ext.clone()),
                )
                .child(title.clone());
            let dialog = dialog.title(heading);
            let dialog = match &text {
                Some(text) => dialog.child(text.clone()),
                None => dialog,
            };
            dialog.footer(
                DialogFooter::new()
                    .child(
                        Button::new("cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("confirm")
                            .label(label.clone())
                            .map(|b| if danger { b.danger() } else { b.primary() })
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                if !sent.replace(true) {
                                    this.update(cx, |this, cx| this.act(action.clone(), cx))
                                        .ok();
                                }
                            }),
                    ),
            )
        });
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
                        let typed = || (self.inputs.get(id)).map(|(s, _)| s.value(cx).to_string());
                        let text = self.pending.get(id).map(|t| t.to_string()).or_else(typed);
                        (id, Value::from(text.unwrap_or_default()))
                    }
                    Node::Select { id, options, .. } => {
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

    /// A section is open unless closed.
    fn open(&self, id: &str) -> bool {
        self.values.get(id) != Some(&Value::Bool(false))
    }

    /// The node and whether it grows to fill the panel's height: a `fill` list, table or log
    /// whose parents are all columns, and those columns. `n` numbers the nodes for the element
    /// ids of those without an id of their own. Recursion is bounded: a tree is at most
    /// `MAX_DEPTH` deep.
    fn node(
        &mut self,
        node: &Node,
        at: At,
        n: &mut usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, bool) {
        let theme = cx.theme();
        let (muted, ring) = (theme.muted_foreground, theme.ring);
        *n += 1;
        let k = *n;
        let element = match node {
            Node::Column(children) | Node::Row(children) => {
                let column = matches!(node, Node::Column(_));
                // R56: below a row nothing fills, not even in a column: a row never grows.
                let filling = at.column && column;
                let inner = At {
                    column: filling,
                    row: !column,
                    click: at.click,
                };
                let mut fills = false;
                let children: Vec<_> = (children.iter())
                    .map(|child| {
                        let (element, grows) = self.node(child, inner, n, window, cx);
                        fills |= grows;
                        element
                    })
                    .collect();
                let element = match column {
                    // In a row, a column takes the width left: a list has none of its own.
                    true => (v_flex().gap_2())
                        .when(fills, |el| el.flex_1().min_h_0())
                        .when(at.row, |el| el.flex_1()),
                    false => h_flex().gap_2().flex_wrap(),
                };
                return (element.children(children).into_any_element(), fills);
            }
            Node::List {
                items,
                subtitles,
                fill,
            } => {
                let fill = *fill && at.column;
                return (self.list(k, items, *subtitles, fill, cx), fill);
            }
            Node::Table {
                columns,
                rows,
                fill,
            } => {
                let fill = *fill && at.column;
                return (self.table(k, columns, rows, fill, cx), fill);
            }
            Node::Log { text, follow, fill } => {
                let fill = *fill && at.column;
                return (self.log(k, text, *follow, fill, cx), fill);
            }
            Node::Text {
                text,
                muted: dim,
                mono,
            } => div()
                .text_sm()
                .when(*dim, |el| el.text_color(muted))
                .when(*mono, |el| {
                    el.font_family(cx.theme().mono_font_family.clone())
                })
                .child(SharedString::from(text))
                .into_any_element(),
            Node::Markdown(text) => TextView::markdown(("panel-md", k), SharedString::from(text))
                .text_sm()
                .into_any_element(),
            Node::Button {
                id,
                label,
                style,
                icon,
                disabled,
                confirm,
            } => {
                let (action, confirm) = (id.clone(), confirm.clone());
                let danger = *style == ButtonStyle::Danger;
                let button = Button::new(name("panel-button", id))
                    .small()
                    .label(SharedString::from(label))
                    .map(|b| match style {
                        ButtonStyle::Primary => b.primary(),
                        ButtonStyle::Danger => b.danger(),
                        ButtonStyle::Default => b,
                    })
                    .when_some(icon.as_deref().and_then(lucide), |b, icon| b.icon(icon))
                    .disabled(*disabled)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.press(action.clone(), confirm.as_ref(), danger, window, cx)
                    }));
                own(button, at)
            }
            Node::Input {
                id,
                placeholder,
                rows,
                ..
            } => {
                let (field, shown) = match self.inputs.get(id) {
                    Some((field, shown)) if field.rows() == *rows => (field.clone(), shown.clone()),
                    // New, or it changed between one line and a text area: what was typed stays.
                    old => {
                        let typed = old.map(|(field, _)| field.value(cx));
                        let field = Field::new(*rows, window, cx);
                        if let Some(typed) = typed.filter(|t| !t.is_empty()) {
                            field.set_value(&typed, window, cx);
                        }
                        (field, "".into())
                    }
                };
                if *shown != **placeholder {
                    field.set_placeholder(SharedString::from(placeholder), window, cx);
                }
                if let Some(text) = self.pending.remove(id) {
                    field.set_value(&text, window, cx);
                }
                self.inputs
                    .insert(id.clone(), (field.clone(), placeholder.clone()));
                let element = match &field {
                    Field::Line(state) => Input::new(state).small().into_any_element(),
                    Field::Area(state, _) => Textarea::new(state).into_any_element(),
                };
                own(element, at)
            }
            Node::Select { id, options, .. } => {
                let chosen = self.chosen(id, options).cloned();
                let label = chosen.as_ref().map_or("—".into(), SharedString::from);
                let (this, field, options) = (cx.weak_entity(), id.clone(), options.clone());
                let select = Button::new(name("panel-select", id))
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
                    });
                own(select, at)
            }
            Node::Checkbox { id, label, .. } => {
                let field = id.clone();
                let checkbox = Checkbox::new(name("panel-checkbox", id))
                    .small()
                    .label(SharedString::from(label))
                    .checked(self.checked(id))
                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        this.set(field.clone(), Value::Bool(*checked), cx)
                    }));
                own(checkbox, at)
            }
            Node::Badge { text, tone } => h_flex()
                .child(tag(*tone).small().child(SharedString::from(text)))
                .into_any_element(),
            Node::Progress(value) => Progress::new(("panel-progress", k))
                .value(value * 100.)
                .into_any_element(),
            Node::Card {
                id,
                title,
                subtitle,
                badge,
                icon,
                children,
            } => {
                let inner = At {
                    column: false,
                    row: false,
                    click: at.click || id.is_some(),
                };
                let icon = icon.as_deref().and_then(lucide);
                let header =
                    (title.is_some() || subtitle.is_some() || badge.is_some()) || icon.is_some();
                let header = header.then(|| {
                    let line = |text: &Arc<str>| div().truncate().child(SharedString::from(text));
                    h_flex()
                        .gap_2()
                        .children(icon.map(|icon| Icon::new(icon).small().text_color(muted)))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .children(
                                    title.as_ref().map(|t| {
                                        line(t).text_sm().font_weight(FontWeight::SEMIBOLD)
                                    }),
                                )
                                .children(
                                    (subtitle.as_ref())
                                        .map(|s| line(s).text_xs().text_color(muted)),
                                ),
                        )
                        .children(
                            (badge.as_ref())
                                .map(|b| Tag::secondary().small().child(SharedString::from(b))),
                        )
                });
                let children: Vec<_> = (children.iter())
                    .map(|child| self.node(child, inner, n, window, cx).0)
                    .collect();
                let card = aurora::tile(cx).children(header).children(children);
                match id {
                    Some(action) => {
                        let action = action.clone();
                        card.id(("panel-card", k))
                            .cursor_pointer()
                            .hover(move |el| el.border_color(ring))
                            .role(Role::Button)
                            .aria_label(title.as_ref().map_or("Card".into(), SharedString::from))
                            .focus_visible(move |el| el.border_color(ring))
                            // Focusable, and Enter or Space clicks it.
                            .tab_index(0)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.act(action.clone(), cx)),
                            )
                            .test_support()
                            .into_any_element()
                    }
                    None => card.into_any_element(),
                }
            }
            Node::Section {
                id,
                title,
                badge,
                children,
                ..
            } => {
                let open = self.open(id);
                let field = id.clone();
                let chevron = match open {
                    true => Lucide::ChevronDown,
                    false => Lucide::ChevronRight,
                };
                let header = h_flex()
                    .id(name("panel-section", id))
                    .gap_1()
                    .px_1()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(transparent_black())
                    .cursor_pointer()
                    .role(Role::Button)
                    .aria_label(SharedString::from(title))
                    .aria_expanded(open)
                    .focus_visible(move |el| el.border_color(ring))
                    .tab_index(0)
                    .child(Icon::new(chevron).xsmall().text_color(muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(SharedString::from(title)),
                    )
                    .children(
                        (badge.as_ref())
                            .map(|b| Tag::secondary().small().child(SharedString::from(b))),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let open = this.open(&field);
                        this.set(field.clone(), Value::Bool(!open), cx)
                    }))
                    .test_support();
                let inner = At {
                    column: false,
                    row: false,
                    click: at.click,
                };
                let body = match open {
                    true => {
                        let children: Vec<_> = (children.iter())
                            .map(|child| self.node(child, inner, n, window, cx).0)
                            .collect();
                        Some(v_flex().gap_2().pl_4().children(children))
                    }
                    // What follows keeps its numbers, so its element state.
                    false => {
                        *n += children.iter().map(size).sum::<usize>();
                        None
                    }
                };
                v_flex()
                    .gap_2()
                    .child(own(header, at))
                    .children(body)
                    .into_any_element()
            }
            Node::Alert { tone, title, text } => {
                let variant = match tone {
                    Tone::Info => AlertVariant::Info,
                    Tone::Success => AlertVariant::Success,
                    Tone::Warning => AlertVariant::Warning,
                    Tone::Danger | Tone::Neutral => AlertVariant::Error,
                };
                Alert::new(("panel-alert", k), SharedString::from(text))
                    .with_variant(variant)
                    .when_some(title.as_ref(), |a, t| a.title(SharedString::from(t)))
                    .into_any_element()
            }
            Node::Link { text, url } => {
                let url = SharedString::from(url);
                let tip = url.clone();
                // The link takes its own clicks, even in a clickable card or row.
                div()
                    .id(("panel-link", k))
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .child(
                        Link::new(("panel-link-text", k))
                            .href(url)
                            .text_sm()
                            .truncate()
                            .child(SharedString::from(text)),
                    )
                    .test_support()
                    .into_any_element()
            }
            Node::Icon { name, tone } => match lucide(name) {
                Some(icon) => Icon::new(icon)
                    .small()
                    .text_color(tone_color(*tone, cx))
                    .into_any_element(),
                None => unsupported(muted),
            },
            Node::Spinner(label) => h_flex()
                .gap_2()
                .child(Spinner::new().small())
                .children(label.as_ref().map(|l| {
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(SharedString::from(l))
                }))
                .into_any_element(),
            Node::Divider => div()
                .w_full()
                .h(px(1.))
                .bg(cx.theme().border)
                .into_any_element(),
            Node::Spacer => div().flex_1().into_any_element(),
            Node::Kv(items) => v_flex()
                .gap_1()
                .children(items.iter().map(|(key, value)| {
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .w(px(KV_KEY))
                                .flex_none()
                                .truncate()
                                .text_color(muted)
                                .child(SharedString::from(key)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(SharedString::from(value)),
                        )
                }))
                .into_any_element(),
            Node::Menu { label, icon, items } => {
                let icon = icon.as_deref().and_then(lucide);
                let icon = icon.or(label.is_none().then_some(Lucide::Ellipsis));
                let (this, items) = (cx.weak_entity(), items.clone());
                let menu = Button::new(("panel-menu", k))
                    .small()
                    .ghost()
                    .when_some(icon, |b, icon| b.icon(icon))
                    .when_some(label.as_ref(), |b, l| b.label(SharedString::from(l)))
                    .dropdown_menu(move |menu, _, _| {
                        items.iter().fold(menu, |menu, item| {
                            let (this, action) = (this.clone(), item.id.clone());
                            let (confirm, danger) = (item.confirm.clone(), item.danger);
                            menu.item(
                                PopupMenuItem::new(SharedString::from(&item.label)).on_click(
                                    move |_, window, cx| {
                                        let action = action.clone();
                                        this.update(cx, |this, cx| {
                                            this.press(action, confirm.as_ref(), danger, window, cx)
                                        })
                                        .ok();
                                    },
                                ),
                            )
                        })
                    });
                own(menu, at)
            }
            Node::Unsupported => unsupported(muted),
        };
        (element, false)
    }

    /// Only the visible rows render: a list may hold any number of items.
    fn list(
        &self,
        k: usize,
        items: &Arc<[Option<Item>]>,
        subtitles: bool,
        fill: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let height = if subtitles { TWO_LINE_ROW } else { ROW_HEIGHT };
        let rows = items.len().min(LIST_ROWS) as f32;
        let (items, this) = (items.clone(), cx.weak_entity());
        uniform_list(("panel-list", k), items.len(), move |range, _, cx| {
            range
                .map(|ix| row(ix, items[ix].as_ref(), height, &this, cx))
                .collect::<Vec<_>>()
        })
        .w_full()
        .min_w(px(LIST_MIN_WIDTH))
        .map(|list| match fill {
            true => list.flex_1().min_h_0(),
            // A list scrolls, so in a filling column it would shrink to nothing.
            false => list.h(px(height * rows)).flex_none(),
        })
        .into_any_element()
    }

    /// A header, then only the visible rows: their cells are nodes, drawn when shown.
    fn table(
        &self,
        k: usize,
        columns: &[Column],
        rows: &Arc<[Option<TableRow>]>,
        fill: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let header = h_flex()
            .h(px(ROW_HEIGHT))
            .px_1()
            .gap_2()
            .flex_none()
            .border_b_1()
            .border_color(theme.border)
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.muted_foreground)
            .children((columns.iter()).map(|c| cell(c.width).child(SharedString::from(&c.title))));
        let widths: Rc<[Option<f32>]> = columns.iter().map(|c| c.width).collect();
        let (rows, count) = (rows.clone(), rows.len());
        let body = uniform_list(
            ("panel-table", k),
            count,
            cx.processor(move |this, range: Range<usize>, window, cx| {
                (range.map(|ix| this.table_row(ix, rows[ix].as_ref(), &widths, window, cx)))
                    .collect::<Vec<_>>()
            }),
        )
        .w_full()
        .map(|list| match fill {
            true => list.flex_1().min_h_0(),
            false => list
                .h(px(ROW_HEIGHT * count.min(LIST_ROWS) as f32))
                .flex_none(),
        });
        v_flex()
            .w_full()
            .min_w(px(LIST_MIN_WIDTH))
            .when(fill, |el| el.flex_1().min_h_0())
            .child(header)
            .child(body)
            .into_any_element()
    }

    fn table_row(
        &mut self,
        ix: usize,
        row: Option<&TableRow>,
        widths: &[Option<f32>],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (foreground, muted) = (cx.theme().foreground, cx.theme().muted_foreground);
        let Some(row) = row else {
            return (h_flex().id(("panel-row", ix)).h(px(ROW_HEIGHT)).px_1())
                .child(unsupported(muted))
                .into_any_element();
        };
        let at = At {
            column: false,
            row: true,
            click: row.id.is_some(),
        };
        // Cells number their own nodes: the row's element id scopes theirs.
        let mut n = 0;
        let cells: Vec<_> = (widths.iter().enumerate())
            .map(|(c, width)| {
                let content =
                    (row.cells.get(c)).map(|node| self.node(node, at, &mut n, window, cx).0);
                cell(*width).children(content)
            })
            .collect();
        let line = |el: Div| el.h(px(ROW_HEIGHT)).px_1().gap_2().text_color(foreground);
        match &row.id {
            Some(action) => {
                let action = action.clone();
                line(crate::sidebar::row(false, cx))
                    .id(("panel-row", ix))
                    .children(cells)
                    .on_click(cx.listener(move |this, _, _, cx| this.act(action.clone(), cx)))
                    .test_support()
                    .into_any_element()
            }
            None => line(h_flex())
                .id(("panel-row", ix))
                .children(cells)
                .test_support()
                .into_any_element(),
        }
    }

    /// Monospace lines, only the visible ones drawn. With `follow` it stays at the end unless the
    /// user scrolled up.
    fn log(
        &mut self,
        k: usize,
        text: &Arc<str>,
        follow: bool,
        fill: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut lines: Vec<Range<usize>> = Vec::new();
        let mut start = 0;
        for (i, _) in text.match_indices('\n') {
            lines.push(start..i);
            start = i + 1;
        }
        if start < text.len() {
            lines.push(start..text.len());
        }
        let count = lines.len();
        let (scroll, seen) =
            (self.logs.entry(k)).or_insert_with(|| (UniformListScrollHandle::new(), 0));
        // Before this layout: whether the user left it at the end (`None`: nothing to scroll).
        if follow && count != *seen && scroll.is_scrolled_to_end() != Some(false) {
            scroll.scroll_to_bottom();
        }
        *seen = count;
        let scroll = scroll.clone();
        let theme = cx.theme();
        let (lines, body) = (Rc::new(lines), text.clone());
        let list = uniform_list(("panel-log", k), count, move |range, _, _| {
            range
                .map(|ix| {
                    let line = body[lines[ix].clone()].replace('\t', "    ");
                    div()
                        .id(("panel-log-line", ix))
                        .h(px(LOG_LINE))
                        .px_1()
                        .whitespace_nowrap()
                        .child(SharedString::from(line))
                        .test_support()
                })
                .collect::<Vec<_>>()
        })
        .track_scroll(&scroll)
        .size_full();
        let copied = text.clone();
        let copy = Button::new(("panel-log-copy", k))
            .xsmall()
            .ghost()
            .icon(Lucide::Copy)
            .tooltip("Copy")
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copied.to_string()))
            });
        div()
            .relative()
            .w_full()
            .min_w(px(LIST_MIN_WIDTH))
            .rounded(px(6.))
            .bg(theme.muted)
            .font_family(theme.mono_font_family.clone())
            .text_xs()
            .map(|el| match fill {
                true => el.flex_1().min_h_0(),
                false => el
                    .h(px(LOG_LINE * count.clamp(1, LIST_ROWS) as f32))
                    .flex_none(),
            })
            .child(list)
            .child(div().absolute().top_0().right_0().child(copy))
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
    // The sidebar's row (radius, hover), at this list's height; the title keeps the text color.
    crate::sidebar::row(false, cx)
        .h(px(height))
        .px_1()
        .gap_2()
        .text_color(theme.foreground)
        .id(("panel-item", ix))
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

/// How many numbers `node` takes in `ExtPanel::node`: itself and what it holds, as if every
/// section were open. A table's cells number their own.
fn size(node: &Node) -> usize {
    1 + match node {
        Node::Column(children)
        | Node::Row(children)
        | Node::Card { children, .. }
        | Node::Section { children, .. } => children.iter().map(size).sum(),
        _ => 0,
    }
}

/// A table cell: `width` pixels, or a share of what the fixed ones leave.
fn cell(width: Option<f32>) -> Div {
    match width {
        Some(width) => div().w(px(width)).flex_none(),
        None => div().flex_1().min_w_0(),
    }
    .truncate()
}

/// Inside a clickable card or table row, a control takes its own clicks: the card or row does
/// not get them too.
fn own(control: impl IntoElement, at: At) -> AnyElement {
    match at.click {
        true => div()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(control)
            .into_any_element(),
        false => control.into_any_element(),
    }
}

fn tag(tone: Tone) -> Tag {
    match tone {
        Tone::Neutral => Tag::secondary(),
        Tone::Info => Tag::info(),
        Tone::Success => Tag::success(),
        Tone::Warning => Tag::warning(),
        Tone::Danger => Tag::danger(),
    }
}

fn tone_color(tone: Tone, cx: &App) -> Hsla {
    let theme = cx.theme();
    match tone {
        Tone::Neutral => theme.muted_foreground,
        Tone::Info => theme.info,
        Tone::Success => theme.success,
        Tone::Warning => theme.warning,
        Tone::Danger => theme.danger,
    }
}

/// The bundled Lucide icon named `name` in kebab-case (`git-pull-request`), as the panel `icon`
/// and the `icon` nodes name them.
pub(crate) fn lucide(name: &str) -> Option<Lucide> {
    static ICONS: OnceLock<HashMap<String, Lucide>> = OnceLock::new();
    let icons = ICONS.get_or_init(|| {
        (Lucide::ALL.iter())
            .filter_map(|&icon| {
                let path = icon.path();
                let name = path.strip_prefix("icons/")?.strip_suffix(".svg")?;
                Some((name.to_owned(), icon))
            })
            .collect()
    });
    icons.get(name).copied()
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
        let at = At {
            column: true,
            row: false,
            click: false,
        };
        let (body, fills) = match self.tree.clone() {
            None => (status("Waiting for the extension…", "panel-waiting"), false),
            Some(Err(TooLarge)) => (status("Panel too large", "panel-too-large"), false),
            Some(Ok(tree)) => self.node(&tree, at, &mut 0, window, cx),
        };
        // A filling list scrolls itself; anything else scrolls with the panel.
        v_flex()
            .id("ext-panel")
            .size_full()
            .p_2()
            .when(!fills, |el| el.overflow_y_scroll())
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    #[cfg(unix)]
    use std::path::Path;
    use std::rc::Rc;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, ElementId, Entity, ScrollDelta, ScrollEvent, SharedString, TestAppContext,
        VisualTestContext, point, px,
    };
    use serde_json::{Value, json};
    use wai_ext::panel::{self, Node};
    use wai_settings::Settings;

    use super::{ExtPanel, PanelAction, lucide};
    use crate::app;
    #[cfg(unix)]
    use crate::sidebar::SidebarTab;
    #[cfg(unix)]
    use crate::workspace::Workspace;

    /// An `ExtPanel` alone in a window.
    fn panel_window(cx: &mut TestAppContext) -> (Entity<ExtPanel>, &mut VisualTestContext) {
        cx.update(|cx| app::init(&Settings::default(), cx));
        let (window, panel) = cx
            .update(|cx| {
                gpui_kit::open_window(Default::default(), cx, |_, cx| {
                    cx.new(|_| ExtPanel::new("Cards".into()))
                })
            })
            .expect("open window");
        let cx = Box::leak(Box::new(VisualTestContext::from_window(window, cx)));
        (panel, cx)
    }

    fn show(panel: &Entity<ExtPanel>, tree: Value, cx: &mut VisualTestContext) {
        panel.update(cx, |p, cx| p.set_tree(panel::parse(&tree), cx));
        cx.update(|window, cx| window.render_frame(cx));
    }

    fn shown(id: impl Into<ElementId>, cx: &mut VisualTestContext) -> bool {
        let id = id.into();
        cx.update(|window, _| window.try_find(id).is_some_and(|e| e.visible()))
    }

    fn name(id: &str) -> ElementId {
        ElementId::Name(SharedString::from(id.to_string()))
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
        assert!(shown(name("panel-button:go"), cx));

        // Too deep: the whole panel says so.
        let mut deep = json!({"type": "text", "text": "leaf"});
        for _ in 0..panel::MAX_DEPTH {
            deep = json!({"type": "column", "children": [deep]});
        }
        show(&panel, deep, cx);
        assert!(cx.debug_bounds("panel-too-large").is_some());
        assert!(cx.debug_bounds("panel-unsupported").is_none());
    }

    /// The renderer never panics on what fits in one message (4 MB): broken nodes, 128 KB
    /// strings in every field, 50,000 list items, every node kind; and a tree with too many nodes
    /// says so.
    #[gpui_kit::test]
    fn hostile_trees_render(cx: &mut TestAppContext) {
        let (panel, cx) = panel_window(cx);
        let huge = "x\n".repeat(1 << 16);
        let mut items: Vec<_> = (0..50_000)
            .map(|n| match n % 3 {
                0 => json!({"id": "same", "title": "t", "subtitle": "s", "badge": "b"}),
                1 => json!({"id": n.to_string(), "title": "t"}),
                _ => json!(n),
            })
            .collect();
        let big = json!({"id": "big", "title": huge, "subtitle": huge, "badge": huge});
        items.splice(0..0, [big.clone(), big]);
        let tree = json!({"type": "column", "children": [
            {"type": "list", "items": items},
            {"type": "list", "items": [], "fill": true},
            {"type": "text", "text": huge, "muted": true},
            {"type": "markdown", "text": "# Title\n\n```rust\nfn main() {}\n```\n| a |\n|---|"},
            {"type": "markdown", "text": huge},
            {"type": "row", "children": [
                {"type": "button", "id": "b", "label": huge, "style": "danger"},
                {"type": "input", "id": "i", "placeholder": huge, "value": huge},
                {"type": "select", "id": "s", "options": []},
                {"type": "select", "id": "t", "options": [huge], "value": huge},
                {"type": "checkbox", "id": "c", "label": huge, "checked": 1},
                {"type": "list", "items": [{"id": "x", "title": "t"}], "fill": true},
                {"type": "progress", "value": 2},
                {"type": "progress", "value": -2},
            ]},
            {"type": "row", "children": [1, "a", null, [], {}]},
            {"type": "column", "children": []},
        ]});
        let wire = serde_json::to_vec(&tree).unwrap().len();
        assert!(
            wire <= wai_ext::MAX_MESSAGE,
            "{wire} bytes: more than a message"
        );
        for tree in [json!(null), json!([]), tree] {
            show(&panel, tree, cx);
        }
        // The first list is node 2, after the root column.
        let row = cx.update(|window, _| {
            let rows = window
                .within(("panel-list", 2usize))
                .find_all(("panel-item", 0usize));
            rows.first().is_some_and(|row| row.visible())
        });
        assert!(row, "list rows");
        let mut cut = 0;
        let tree = panel.read_with(cx, |p, _| p.tree.clone());
        tree.unwrap().unwrap().walk(&mut |node| {
            if let Node::Text { text, .. } | Node::Markdown(text) = node {
                assert!(text.chars().count() <= panel::TEXT_CHARS + 1);
                cut += usize::from(text.ends_with('…'));
            }
        });
        assert_eq!(cut, 2, "the huge text and markdown are cut");

        let nodes = vec![json!({"type": "badge", "text": "b"}); panel::MAX_NODES];
        show(&panel, json!({"type": "column", "children": nodes}), cx);
        assert!(cx.debug_bounds("panel-too-large").is_some());
    }

    /// A `fill` list takes the panel's remaining height, through columns only; an input's
    /// placeholder follows each render.
    #[gpui_kit::test]
    fn fill_and_placeholder_follow_the_tree(cx: &mut TestAppContext) {
        let (panel, cx) = panel_window(cx);
        let items: Vec<_> = (0..200)
            .map(|n| json!({"id": n.to_string(), "title": "t"}))
            .collect();
        let list = |fill: bool| json!({"type": "list", "fill": fill, "items": items});
        let tree = |list: Value, placeholder: &str| {
            json!({"type": "column", "children": [
                {"type": "input", "id": "q", "placeholder": placeholder},
                {"type": "column", "children": [list]},
                {"type": "button", "id": "after", "label": "After"},
            ]})
        };
        let placeholder = |cx: &mut VisualTestContext| {
            cx.update(|window, _| window.find("input-state").placeholder().map(str::to_owned))
        };
        let below = |cx: &mut VisualTestContext| {
            cx.update(|window, _| {
                let after = window.find(name("panel-button:after")).bounds();
                after.bottom() <= window.viewport_size().height
            })
        };

        show(&panel, tree(list(false), "First"), cx);
        assert!(shown(("panel-item", 11usize), cx) && !shown(("panel-item", 30usize), cx));
        assert_eq!(placeholder(cx).as_deref(), Some("First"));

        show(&panel, tree(list(true), "Second"), cx);
        assert!(
            shown(("panel-item", 30usize), cx),
            "the list fills the panel"
        );
        assert!(below(cx), "what follows the list stays in view");
        assert_eq!(placeholder(cx).as_deref(), Some("Second"));

        // In a row, `fill` has no height to take.
        let row = json!({"type": "row", "children": [list(true)]});
        show(&panel, tree(row, "Second"), cx);
        assert!(!shown(("panel-item", 30usize), cx));
    }

    /// R57: a list in a column in a row keeps 160 px beside a sibling as wide as the row is
    /// (a long line of text, all but ~40 px of this window), instead of being squeezed to that.
    #[gpui_kit::test]
    fn row_column_list_keeps_min_width(cx: &mut TestAppContext) {
        let (panel, cx) = panel_window(cx);
        let list = json!({"type": "list", "items": [{"id": "0", "title": "t"}]});
        show(
            &panel,
            json!({"type": "row", "children": [
                {"type": "column", "children": [list]},
                {"type": "text", "text": "x".repeat(175)},
            ]}),
            cx,
        );

        let width = cx.update(|window, _| window.find(("panel-item", 0usize)).bounds().size.width);
        assert!(width >= px(160.), "{width:?}");
    }

    /// R56: nor below a row, even in a column: such a list keeps its own height (it does not
    /// collapse) and the panel still scrolls.
    #[gpui_kit::test]
    fn fill_below_a_row_keeps_the_panel_scrolling(cx: &mut TestAppContext) {
        let (panel, cx) = panel_window(cx);
        let items: Vec<_> = (0..200)
            .map(|n| json!({"id": n.to_string(), "title": "t"}))
            .collect();
        let lines: Vec<_> = (0..80)
            .map(|n| json!({"type": "text", "text": format!("line {n}")}))
            .collect();
        let list = json!({"type": "list", "fill": true, "items": items});
        let column = json!({"type": "column", "children": [list]});
        show(
            &panel,
            json!({"type": "column", "children": [
                {"type": "row", "children": [column]},
                {"type": "column", "children": lines},
                {"type": "button", "id": "after", "label": "After"},
            ]}),
            cx,
        );
        assert!(shown(("panel-item", 0usize), cx), "the list's rows show");
        assert!(!shown(("panel-item", 30usize), cx), "at its own height");

        let top = |cx: &mut VisualTestContext| {
            cx.update(|window, _| window.find(name("panel-button:after")).bounds().top())
        };
        let before = top(cx);
        // Over the lines below the list: the list scrolls itself.
        let position =
            cx.update(|window, _| point(px(20.), window.viewport_size().height - px(20.)));
        cx.simulate_event(ScrollEvent {
            position,
            delta: ScrollDelta::Lines(point(0., -20.)),
            ..Default::default()
        });
        cx.update(|window, cx| window.render_frame(cx));
        assert!(top(cx) < before, "the panel scrolls");
    }

    /// Each action the panel sends, with the form's values.
    fn actions(
        panel: &Entity<ExtPanel>,
        cx: &mut VisualTestContext,
    ) -> Rc<RefCell<Vec<(String, Value)>>> {
        let sent = Rc::new(RefCell::new(Vec::new()));
        let log = sent.clone();
        cx.update(|_, cx| {
            cx.subscribe(panel, move |_, a: &PanelAction, _| {
                log.borrow_mut().push((a.action.clone(), a.values.clone()))
            })
            .detach()
        });
        sent
    }

    fn sent(actions: &Rc<RefCell<Vec<(String, Value)>>>) -> Vec<String> {
        actions.borrow().iter().map(|(a, _)| a.clone()).collect()
    }

    fn frame(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.render_frame(cx));
    }

    /// One of every v2 node: none is drawn as unsupported.
    #[gpui_kit::test]
    fn every_node_renders(cx: &mut TestAppContext) {
        let (panel, cx) = panel_window(cx);
        let menu = json!({"type": "menu", "items": [{"id": "m", "label": "M"}]});
        show(
            &panel,
            json!({"type": "column", "children": [
                {"type": "card", "id": "c", "title": "T", "subtitle": "S", "badge": "B",
                 "icon": "git-pull-request", "children": [{"type": "text", "text": "x", "mono": true}]},
                {"type": "card", "children": []},
                {"type": "section", "id": "s", "title": "S", "badge": "2", "children": [
                    {"type": "kv", "items": [{"key": "k", "value": "v"}]},
                ]},
                {"type": "table", "columns": [{"title": "A"}, {"title": "B", "width": 60}], "rows": [
                    {"id": "r", "cells": ["x", {"type": "badge", "text": "ok", "tone": "success"}]},
                    {"cells": [{"type": "link", "text": "l", "url": "https://example.com"}]},
                ]},
                {"type": "log", "text": "a\n\tb"},
                {"type": "alert", "tone": "warning", "title": "W", "text": "w"},
                {"type": "link", "text": "l", "url": "https://example.com/a"},
                {"type": "icon", "name": "check", "tone": "success"},
                {"type": "spinner", "label": "busy"},
                {"type": "spinner"},
                {"type": "divider"},
                {"type": "row", "children": [{"type": "text", "text": "a"}, {"type": "spacer"}, menu]},
                {"type": "menu", "label": "More", "icon": "ellipsis", "items": []},
                {"type": "input", "id": "i", "placeholder": "p", "multiline": true, "rows": 3},
                {"type": "button", "id": "b", "label": "B", "icon": "copy", "disabled": true},
                {"type": "badge", "text": "d", "tone": "danger"},
            ]}),
            cx,
        );
        assert!(cx.debug_bounds("panel-unsupported").is_none());
        assert!(shown(("panel-card", 2usize), cx));
        assert!(shown(name("panel-section:s"), cx));
        assert!(shown(("panel-row", 1usize), cx));
        assert!(shown(("panel-log-line", 1usize), cx));
        assert!(lucide("git-pull-request").is_some() && lucide("no-such-icon").is_none());
    }

    /// A well-formed icon name that is not in the catalog shows as unsupported; an unknown
    /// optional icon (a button's) is left out.
    #[gpui_kit::test]
    fn unknown_icon_is_unsupported(cx: &mut TestAppContext) {
        let (panel, cx) = panel_window(cx);
        show(
            &panel,
            json!({"type": "column", "children": [
                {"type": "button", "id": "b", "label": "B", "icon": "no-such-icon"},
            ]}),
            cx,
        );
        assert!(cx.debug_bounds("panel-unsupported").is_none());
        show(&panel, json!({"type": "icon", "name": "no-such-icon"}), cx);
        assert!(cx.debug_bounds("panel-unsupported").is_some());
    }

    /// A clickable card, table row and menu item send their ids; a button inside the card or
    /// the row sends only its own.
    #[gpui_kit::test]
    fn card_row_and_menu_send_actions(cx: &mut TestAppContext) {
        use crate::extensions_view::tests::click;

        let (panel, cx) = panel_window(cx);
        let actions = actions(&panel, cx);
        // Nodes: column 1, card 2, its button 3, table 4, menu 5.
        show(
            &panel,
            json!({"type": "column", "children": [
                {"type": "card", "id": "c1", "title": "PR", "children": [
                    {"type": "button", "id": "b", "label": "B"},
                ]},
                {"type": "table", "columns": [{"title": "A"}, {"title": "B", "width": 60}],
                 "rows": [{"id": "r1", "cells": ["x", {"type": "button", "id": "rb", "label": "Go"}]}]},
                {"type": "menu", "items": [{"id": "m1", "label": "One"}, {"id": "m2", "label": "Two"}]},
            ]}),
            cx,
        );
        // Its padding: not on the button, which may be as wide as the card.
        cx.update(|window, cx| window.click_at(("panel-card", 2usize), point(px(3.), px(3.)), cx));
        cx.run_until_parked();
        click(name("panel-button:b"), cx);
        click(("panel-row", 0usize), cx);
        click(name("panel-button:rb"), cx);
        click(("panel-menu", 5usize), cx);
        cx.update(|window, cx| window.within("items").click(1usize, cx));
        cx.run_until_parked();
        assert_eq!(sent(&actions), ["c1", "b", "r1", "rb", "m2"]);
    }

    /// A row's controls take their own clicks too: a checkbox in a clickable card toggles
    /// without sending the card's action.
    #[gpui_kit::test]
    fn button_in_a_row_sends_only_its_own(cx: &mut TestAppContext) {
        use crate::extensions_view::tests::click;

        let (panel, cx) = panel_window(cx);
        let actions = actions(&panel, cx);
        show(
            &panel,
            json!({"type": "card", "id": "card", "children": [
                {"type": "row", "children": [
                    {"type": "checkbox", "id": "done", "label": "Done"},
                    {"type": "button", "id": "go", "label": "Go"},
                ]},
            ]}),
            cx,
        );
        click(name("panel-checkbox:done"), cx);
        click(name("panel-button:go"), cx);
        assert_eq!(
            *actions.borrow(),
            [("go".to_string(), json!({"done": true}))]
        );
    }

    /// Cancel sends nothing; Confirm sends the action clicked once, with the form's values at
    /// Confirm; the dialog names the extension.
    #[gpui_kit::test]
    fn confirm_cancel_sends_nothing_confirm_sends_once(cx: &mut TestAppContext) {
        use crate::extensions_view::tests::click;

        let (panel, cx) = panel_window(cx);
        let actions = actions(&panel, cx);
        show(
            &panel,
            json!({"type": "column", "children": [
                {"type": "input", "id": "q", "placeholder": "q"},
                {"type": "button", "id": "drop", "label": "Drop", "style": "danger",
                 "confirm": {"title": "Drop it?", "text": "It cannot be undone.", "confirm_label": "Drop"}},
            ]}),
            cx,
        );
        click(name("panel-button:drop"), cx);
        frame(cx);
        assert!(cx.debug_bounds("confirm-extension").is_some());
        click("cancel", cx);
        frame(cx);
        assert!(
            cx.debug_bounds("confirm-extension").is_none(),
            "the dialog closes"
        );
        assert!(actions.borrow().is_empty());

        click(name("panel-button:drop"), cx);
        let input = panel.read_with(cx, |p, _| p.inputs.get("q").map(|(f, _)| f.clone()));
        cx.update(|window, cx| input.unwrap().set_value("typed", window, cx));
        click("confirm", cx);
        frame(cx);
        if cx.update(|window, _| window.try_find("confirm").is_some_and(|e| e.visible())) {
            click("confirm", cx);
        }
        assert_eq!(
            *actions.borrow(),
            [("drop".to_string(), json!({"q": "typed"}))]
        );
    }

    /// A section opens and closes on a click and stays so across renders with the same `open`;
    /// a different `open` replaces it. Its state is not a form value.
    #[gpui_kit::test]
    fn section_state_survives_rerender(cx: &mut TestAppContext) {
        use crate::extensions_view::tests::click;

        let (panel, cx) = panel_window(cx);
        let actions = actions(&panel, cx);
        let tree = |open: bool| {
            json!({"type": "column", "children": [
                {"type": "section", "id": "s", "title": "More", "open": open, "children": [
                    {"type": "button", "id": "inner", "label": "In"},
                ]},
                {"type": "button", "id": "after", "label": "After"},
            ]})
        };
        let inner = |cx: &mut VisualTestContext| {
            frame(cx);
            shown(name("panel-button:inner"), cx)
        };
        show(&panel, tree(true), cx);
        assert!(inner(cx));
        click(name("panel-section:s"), cx);
        assert!(!inner(cx), "a click closes it");
        show(&panel, tree(true), cx);
        assert!(!inner(cx), "the same `open` keeps it closed");
        show(&panel, tree(false), cx);
        assert!(!inner(cx));
        show(&panel, tree(true), cx);
        assert!(inner(cx), "a different `open` opens it");
        click(name("panel-button:after"), cx);
        assert_eq!(*actions.borrow(), [("after".to_string(), json!({}))]);
    }

    /// A following log shows its last line as it grows, unless the user scrolled up.
    #[gpui_kit::test]
    fn log_follows_only_at_the_end(cx: &mut TestAppContext) {
        let (panel, cx) = panel_window(cx);
        let log = |n: usize| {
            let lines: Vec<_> = (0..n).map(|i| format!("line {i}")).collect();
            json!({"type": "column", "children": [{"type": "log", "text": lines.join("\n")}]})
        };
        let last = |n: usize, cx: &mut VisualTestContext| {
            frame(cx);
            shown(("panel-log-line", n - 1), cx)
        };
        show(&panel, log(100), cx);
        assert!(last(100, cx), "at the end");
        show(&panel, log(150), cx);
        assert!(last(150, cx), "it follows");

        let position =
            cx.update(|window, _| window.find(("panel-log-line", 149usize)).bounds().center());
        cx.simulate_event(ScrollEvent {
            position,
            delta: ScrollDelta::Lines(point(0., 30.)),
            ..Default::default()
        });
        frame(cx);
        show(&panel, log(200), cx);
        assert!(!last(200, cx), "scrolled up, it stays");
        assert!(!shown(("panel-log-line", 149usize), cx));

        let position =
            cx.update(|window, _| window.find(("panel-log-line", 110usize)).bounds().center());
        cx.simulate_event(ScrollEvent {
            position,
            delta: ScrollDelta::Lines(point(0., -500.)),
            ..Default::default()
        });
        frame(cx);
        show(&panel, log(250), cx);
        assert!(last(250, cx), "back at the end, it follows again");
    }

    /// The docs' sample trees parse whole and render.
    #[gpui_kit::test]
    fn docs_samples_render(cx: &mut TestAppContext) {
        let (panel, cx) = panel_window(cx);
        for sample in [
            include_str!("../../../docs/samples/meus-prs.json"),
            include_str!("../../../docs/samples/restore.json"),
        ] {
            let tree: Value = serde_json::from_str(sample).unwrap();
            let mut whole = true;
            panel::parse(&tree).unwrap().walk(&mut |node| {
                whole &= !matches!(node, Node::Unsupported);
            });
            assert!(whole, "{sample}");
            show(&panel, tree, cx);
            assert!(cx.debug_bounds("panel-unsupported").is_none());
        }
    }

    #[cfg(unix)]
    const MANIFEST: &str = "id = \"cards\"\nname = \"Cards\"\nversion = \"1.0.0\"\n\
        permissions = [\"process\", \"panels\"]\n[process]\ncommand = \"bin/run\"\n\
        [[contributes.panels]]\nid = \"main\"\ntitle = \"Cards\"\n";

    /// The workspace with extension `cards` (`wai-fake-ext <flags>`) installed and started.
    #[cfg(unix)]
    fn start_sample<'a>(
        tmp: &Path,
        flags: &str,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Workspace>, &'a mut VisualTestContext) {
        use wai_ext::{ExtStore, Permission, Source};

        use crate::extensions_view::tests::{running_sample_with, workspace};

        cx.executor().allow_parking(); // the extension is a real process
        let config = tmp.join("config");
        let src = running_sample_with(&tmp.join("src"), MANIFEST, &tmp.join("pid"), flags);
        let mut store = ExtStore::open(&config);
        let staged = store.stage(&Source::Folder(src)).unwrap();
        store
            .commit(staged, vec![Permission::Process, Permission::Panels])
            .unwrap();
        let (workspace, cx) = workspace(&config, cx);
        workspace.update(cx, |w, cx| w.start_extensions(cx));
        (workspace, cx)
    }

    /// `start_sample` with `--panel`, its panel's tab open once its first tree has arrived.
    #[cfg(unix)]
    fn open_sample<'a>(
        tmp: &Path,
        cx: &'a mut TestAppContext,
    ) -> (
        Entity<Workspace>,
        Entity<ExtPanel>,
        &'a mut VisualTestContext,
    ) {
        use crate::extensions_view::tests::click;
        use crate::workspace::tests::wait_until;

        let (workspace, cx) = start_sample(tmp, "--panel", cx);
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

    /// Types `text` into the panel's `query` input.
    #[cfg(unix)]
    fn type_query(panel: &Entity<ExtPanel>, text: &str, cx: &mut VisualTestContext) {
        let input = panel.read_with(cx, |p, _| p.inputs.get("query").map(|(s, _)| s.clone()));
        let input = input.expect("the query input");
        cx.update(|window, cx| input.set_value(text, window, cx));
    }

    /// Clicking a list row sends `panel/action` with the row's id and the form's values, the
    /// initial ones at first; the tab sits after the built-in ones.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn panel_list_click_sends_action(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let (_, panel, cx) = open_sample(tmp.path(), cx);
        let tabs = cx.update(|window, _| {
            let agents = window.find("tab-agents").bounds();
            (agents, window.find(("tab-panel", 0usize)).bounds())
        });
        assert!(tabs.0.right() <= tabs.1.left() || tabs.0.bottom() <= tabs.1.top());
        let input = cx.update(|window, _| window.find("input-state").value().map(str::to_owned));
        assert_eq!(input.as_deref(), Some("is:open"), "the initial value shows");

        crate::extensions_view::tests::click(("panel-item", 1usize), cx);
        assert_eq!(
            echoed(&panel, "card-2", cx),
            json!({"query": "is:open", "scope": "Mine", "done": false})
        );
    }

    /// A button sends what the form holds; a re-render with the same initial values keeps it.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn panel_button_sends_form_values(cx: &mut TestAppContext) {
        use crate::extensions_view::tests::click;

        let tmp = tempfile::tempdir().unwrap();
        let (_, panel, cx) = open_sample(tmp.path(), cx);
        type_query(&panel, "login", cx);
        click(name("panel-checkbox:done"), cx);
        click(name("panel-select:scope"), cx);
        cx.update(|window, cx| window.within("items").click(1usize, cx));
        cx.run_until_parked();

        click(name("panel-button:refresh"), cx);
        let values = json!({"query": "login", "scope": "Team", "done": true});
        assert_eq!(echoed(&panel, "refresh", cx), values);
        click(("panel-item", 0usize), cx);
        assert_eq!(echoed(&panel, "card-1", cx), values);
    }

    /// R53: a different initial value replaces what the user did; the same one again does not.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn new_initial_values_replace_edits(cx: &mut TestAppContext) {
        use crate::extensions_view::tests::click;

        let tmp = tempfile::tempdir().unwrap();
        let (_, panel, cx) = open_sample(tmp.path(), cx);
        type_query(&panel, "login", cx);
        click(name("panel-button:reset"), cx);
        let edited = json!({"query": "login", "scope": "Mine", "done": false});
        assert_eq!(echoed(&panel, "reset", cx), edited);
        let checked = |cx: &mut VisualTestContext| {
            let id = name("panel-checkbox:done");
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.find(id).checked()
            })
        };
        assert_eq!(checked(cx), Some(true), "the new initial value shows");
        click(("panel-item", 0usize), cx);
        let reset = json!({"query": "", "scope": "Team", "done": true});
        assert_eq!(echoed(&panel, "card-1", cx), reset);

        type_query(&panel, "again", cx);
        click(name("panel-checkbox:done"), cx);
        click(name("panel-button:reset"), cx);
        echoed(&panel, "reset", cx);
        click(("panel-item", 1usize), cx);
        let kept = json!({"query": "again", "scope": "Team", "done": false});
        assert_eq!(echoed(&panel, "card-2", cx), kept);
    }

    /// R44: only an enabled extension granted `panels` has tabs. When the shown tab goes, the
    /// sidebar shows the sessions.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn panel_tabs_need_the_grant(cx: &mut TestAppContext) {
        use wai_ext::{ExtStore, Permission};

        let tmp = tempfile::tempdir().unwrap();
        let (workspace, _, cx) = open_sample(tmp.path(), cx);
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        let installed = ExtStore::open(&tmp.path().join("config")).list();
        let sync = |list: Vec<wai_ext::Installed>, cx: &mut VisualTestContext| {
            host.update(cx, |h, cx| h.sync(&list, cx));
            cx.update(|window, cx| window.render_frame(cx));
            shown(("tab-panel", 0usize), cx)
        };
        let tab = |cx: &mut VisualTestContext| workspace.read_with(cx, |w, _| w.sidebar_tab);
        assert!(matches!(tab(cx), SidebarTab::Panel(_)));

        let mut ungranted = installed.clone();
        ungranted[0].granted.retain(|p| *p != Permission::Panels);
        assert!(!sync(ungranted, cx), "a tab without the grant");
        assert!(host.read_with(cx, |h, _| h.panels().is_empty()));
        assert_eq!(tab(cx), SidebarTab::Sessions);

        let mut disabled = installed.clone();
        disabled[0].enabled = false;
        assert!(!sync(disabled, cx), "a tab while disabled");
        assert!(sync(installed, cx));
    }

    /// A crashed extension's tab goes, and the sidebar shows the sessions; a later sync does not
    /// bring it back.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn crashed_extension_loses_its_tab(cx: &mut TestAppContext) {
        use wai_ext::ExtStore;

        use crate::ext_host::ExtStatus;
        use crate::workspace::tests::wait_until;

        let tmp = tempfile::tempdir().unwrap();
        let (workspace, cx) = start_sample(tmp.path(), "--crash", cx);
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        let panel = host.read_with(cx, |h, _| h.panels()[0].view.entity_id());
        workspace.update(cx, |w, cx| {
            w.sidebar_tab = SidebarTab::Panel(panel);
            cx.notify();
        });
        let id = wai_ext::ExtId::new("cards").unwrap();
        wait_until(cx, "Crashed", |cx| {
            host.read_with(cx, |h, _| {
                matches!(h.status(&id), Some(ExtStatus::Crashed(_)))
            })
        });
        assert!(host.read_with(cx, |h, _| h.panels().is_empty()));
        assert_eq!(
            workspace.read_with(cx, |w, _| w.sidebar_tab),
            SidebarTab::Sessions
        );

        let list = ExtStore::open(&tmp.path().join("config")).list();
        host.update(cx, |h, cx| h.sync(&list, cx));
        assert!(host.read_with(cx, |h, _| h.panels().is_empty()));
    }
}
