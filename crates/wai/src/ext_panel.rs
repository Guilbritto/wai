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
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::text::TextView;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Map, Value};
use wai_ext::panel::{
    ButtonStyle, Column, Confirm, Item, Node, Pick, Tab, TableRow, Tone, TooLarge,
};

use crate::campo;
use crate::path_field::{self, PathField};

/// A list row with a subtitle: two lines.
const TWO_LINE_ROW: f32 = 42.;
/// A table row, or a list row of one line.
const ROW_HEIGHT: f32 = 30.;
/// The space between loose rows.
const ROW_GAP: f32 = 4.;
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
    /// Each select's choice, each checkbox's state, each section's openness and each tabs'
    /// selected tab, per id; kept the same way.
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
                    Node::Input { id, value, .. } | Node::Path { id, value, .. } => {
                        inputs.insert(id.as_str());
                        (id, value.as_deref().map(Value::from))
                    }
                    Node::Select { id, value, .. } => (id, value.as_deref().map(Value::from)),
                    Node::Checkbox { id, checked, .. } => (id, checked.map(Value::Bool)),
                    Node::Section { id, open, .. } => (id, open.map(Value::Bool)),
                    Node::Tabs { id, value, .. } => (id, value.as_deref().map(Value::from)),
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
                    (Node::Input { .. } | Node::Path { .. }, Value::String(text)) => {
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
        // Confirm takes the focus once, when the dialog opens.
        let (focus, focused) = (cx.focus_handle(), Cell::new(false));
        window.open_dialog(cx, move |dialog, window, cx| {
            if !focused.replace(true) {
                focus.focus(window, cx);
            }
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
                    .child(campo::button(
                        Button::new("cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ))
                    .child(
                        Button::new("confirm")
                            .label(label.clone())
                            .track_focus(&focus)
                            .map(|b| match danger {
                                true => campo::danger(b, cx),
                                false => campo::primary(b),
                            })
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
                    Node::Path { id, .. } => {
                        let home = dirs::home_dir();
                        let pending = self.pending.get(id);
                        let pending =
                            pending.map(|t| path_field::expand(t.trim(), home.as_deref()));
                        let picked = || match self.inputs.get(id) {
                            Some((Field::Line(state), _)) => Some(path_field::value(state, cx)),
                            _ => None,
                        };
                        (id, Value::from(pending.or_else(picked).unwrap_or_default()))
                    }
                    Node::Select { id, options, .. } => {
                        let chosen = self.chosen(id, options);
                        (id, chosen.map_or(Value::Null, |o| Value::from(&**o)))
                    }
                    Node::Checkbox { id, .. } => (id, Value::Bool(self.checked(id))),
                    Node::Tabs { id, tabs, .. } => (id, Value::from(&*self.selected(id, tabs).id)),
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

    /// The tab chosen, while it is still offered; else the first.
    fn selected<'a>(&self, id: &str, tabs: &'a [Tab]) -> &'a Tab {
        let chosen = self.values.get(id).and_then(Value::as_str);
        let offered = chosen.and_then(|c| tabs.iter().find(|t| t.id == c));
        offered.unwrap_or(&tabs[0])
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
        let (line, violet) = (campo::line(cx), campo::violet(cx));
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
            Node::Text { .. }
            | Node::Badge { .. }
            | Node::Progress(_)
            | Node::Icon { .. }
            | Node::Divider
            | Node::Spacer
            | Node::Kv(_) => leaf(node, k, cx),
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
                        ButtonStyle::Primary => campo::primary(b),
                        ButtonStyle::Danger => campo::danger(b, cx),
                        ButtonStyle::Default => campo::button(b),
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
                    Field::Line(state) => {
                        campo::field(Input::new(state).small(), false, cx).into_any_element()
                    }
                    Field::Area(state, _) => Textarea::new(state)
                        .rounded(px(campo::RADIUS))
                        .bg(campo::recessed(cx))
                        .into_any_element(),
                };
                own(element, at)
            }
            Node::Path {
                id,
                pick,
                placeholder,
                editable,
                ..
            } => {
                let placeholder = placeholder.clone().unwrap_or_else(|| match pick {
                    Pick::File => "Choose a file…".into(),
                    Pick::Folder => "Choose a folder…".into(),
                });
                let (state, shown) = match self.inputs.get(id) {
                    Some((Field::Line(state), shown)) => (state.clone(), shown.clone()),
                    _ => (cx.new(|cx| InputState::new(window, cx)), "".into()),
                };
                if shown != placeholder {
                    let text = SharedString::from(&placeholder);
                    state.update(cx, |s, cx| s.set_placeholder(text, window, cx));
                }
                if let Some(text) = self.pending.remove(id) {
                    path_field::set(&state, &text, window, cx);
                }
                (self.inputs).insert(
                    id.clone(),
                    (Field::Line(state.clone()), placeholder.clone()),
                );
                let this = cx.weak_entity();
                let field = PathField::new(name("panel-path", id), &state, *pick)
                    .editable(*editable)
                    .placeholder(SharedString::from(&placeholder))
                    .on_change(move |_, cx| {
                        this.update(cx, |_, cx| cx.notify()).ok();
                    });
                own(field, at)
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
                    .map(|b| campo::field(b, false, cx))
                    .min_w(px(LIST_MIN_WIDTH))
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
                    .label(SharedString::from(label))
                    .checked(self.checked(id))
                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        this.set(field.clone(), Value::Bool(*checked), cx)
                    }));
                own(checkbox, at)
            }
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
                let recessed = campo::recessed(cx);
                let header = header.then(|| {
                    let line = |text: &Arc<str>| div().truncate().child(SharedString::from(text));
                    h_flex()
                        .gap_2()
                        .children(icon.map(|icon| {
                            h_flex()
                                .size(px(campo::H))
                                .flex_none()
                                .justify_center()
                                .rounded(px(campo::RADIUS))
                                .bg(recessed)
                                .child(Icon::new(icon).small().text_color(muted))
                        }))
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
                                .map(|b| campo::badge(b.to_string(), Tone::Neutral, cx)),
                        )
                        // A clickable card shows where it goes on hover.
                        .when(id.is_some(), |el| {
                            el.child(
                                div()
                                    .ml_auto()
                                    .opacity(0.)
                                    .group_hover("panel-card", |s| s.opacity(1.))
                                    .child(
                                        Icon::new(Lucide::ChevronRight).small().text_color(muted),
                                    ),
                            )
                        })
                });
                let children: Vec<_> = (children.iter())
                    .map(|child| self.node(child, inner, n, window, cx).0)
                    .collect();
                let card = v_flex()
                    .p_2()
                    .gap_2()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(line)
                    .bg(campo::glass(cx))
                    .children(header)
                    .children(children);
                match id {
                    Some(action) => {
                        let action = action.clone();
                        card.id(("panel-card", k))
                            .group("panel-card")
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
                    .h(px(24.))
                    .gap_1p5()
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
                        campo::label(title.to_string(), cx)
                            .flex_none()
                            .max_w_2_3()
                            .truncate(),
                    )
                    .children(
                        (badge.as_ref()).map(|b| campo::badge(b.to_string(), Tone::Neutral, cx)),
                    )
                    // A hairline to the edge.
                    .child(div().flex_1().h(px(1.)).bg(line))
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
                        Some(v_flex().gap_2().children(children))
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
            Node::Tabs { id, tabs, .. } => {
                let current = self.selected(id, tabs).id.clone();
                let hover = campo::glass_hover(cx);
                let bar = h_flex()
                    .gap_1()
                    .flex_wrap()
                    .border_b_1()
                    .border_color(line)
                    .children(tabs.iter().map(|tab| {
                        let on = tab.id == current;
                        let (field, choice) = (id.clone(), tab.id.clone());
                        h_flex()
                            .id(name("panel-tab", &format!("{id}:{}", tab.id)))
                            .h(px(campo::H))
                            .gap_1()
                            .px_2()
                            .rounded_t(px(6.))
                            .border_b_2()
                            .border_color(if on { violet } else { transparent_black() })
                            .cursor_pointer()
                            .text_sm()
                            .when(on, |el| el.font_weight(FontWeight::SEMIBOLD))
                            .when(!on, |el| el.text_color(muted))
                            .hover(move |el| el.bg(hover))
                            .role(Role::Tab)
                            .aria_label(SharedString::from(&tab.title))
                            .aria_selected(on)
                            .focus_visible(move |el| el.bg(hover).border_color(ring))
                            .tab_index(0)
                            .child(SharedString::from(&tab.title))
                            .children(
                                (tab.badge.as_ref())
                                    .map(|b| campo::badge(b.to_string(), Tone::Neutral, cx)),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set(field.clone(), Value::from(choice.as_str()), cx)
                            }))
                            .test_support()
                    }));
                let inner = At {
                    column: at.column,
                    row: false,
                    click: at.click,
                };
                let mut fills = false;
                let mut body = Vec::new();
                for tab in tabs {
                    if tab.id == current {
                        for child in &tab.children {
                            let (element, grows) = self.node(child, inner, n, window, cx);
                            fills |= grows;
                            body.push(element);
                        }
                    } else {
                        // What follows keeps its numbers, so its element state.
                        *n += tab.children.iter().map(size).sum::<usize>();
                    }
                }
                let element = (v_flex().gap_2())
                    .when(fills, |el| el.flex_1().min_h_0())
                    .child(own(bar, at))
                    .child(
                        v_flex()
                            .gap_2()
                            .when(fills, |el| el.flex_1().min_h_0())
                            .children(body),
                    );
                return (element.into_any_element(), fills);
            }
            Node::Alert { tone, title, text } => {
                let variant = match tone {
                    Tone::Info => AlertVariant::Info,
                    Tone::Success => AlertVariant::Success,
                    Tone::Warning => AlertVariant::Warning,
                    Tone::Danger | Tone::Neutral => AlertVariant::Error,
                };
                let (fill, _, border) = campo::tint(*tone, cx);
                Alert::new(("panel-alert", k), SharedString::from(text))
                    .with_variant(variant)
                    .when_some(title.as_ref(), |a, t| a.title(SharedString::from(t)))
                    .rounded(px(10.))
                    .bg(fill)
                    .border_color(border)
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
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(SharedString::from(text))
                                    .child(Icon::new(Lucide::ExternalLink).xsmall()),
                            ),
                    )
                    .test_support()
                    .into_any_element()
            }
            Node::Spinner(label) => h_flex()
                .gap_2()
                .child(Spinner::new().small().color(violet))
                .children(label.as_ref().map(|l| {
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(SharedString::from(l))
                }))
                .into_any_element(),
            Node::Menu { label, icon, items } => {
                let icon = icon.as_deref().and_then(lucide);
                let icon = icon.or(label.is_none().then_some(Lucide::Ellipsis));
                let (this, items) = (cx.weak_entity(), items.clone());
                let danger = cx.theme().danger;
                let menu = Button::new(("panel-menu", k))
                    .small()
                    .ghost()
                    .when_some(icon, |b, icon| b.icon(icon))
                    .map(|b| match label {
                        Some(l) => campo::button(b.label(SharedString::from(l))),
                        None => campo::icon(b),
                    })
                    .dropdown_menu(move |menu, _, _| {
                        items.iter().fold(menu, |menu, item| {
                            let danger_color = danger;
                            let (this, action) = (this.clone(), item.id.clone());
                            let (confirm, danger) = (item.confirm.clone(), item.danger);
                            let text = SharedString::from(&item.label);
                            let entry = match danger {
                                true => PopupMenuItem::element(move |_, _| {
                                    div().text_color(danger_color).child(text.clone())
                                }),
                                false => PopupMenuItem::new(text),
                            };
                            menu.item(entry.on_click(move |_, window, cx| {
                                let action = action.clone();
                                this.update(cx, |this, cx| {
                                    this.press(action, confirm.as_ref(), danger, window, cx)
                                })
                                .ok();
                            }))
                        })
                    });
                own(menu, at)
            }
            Node::Unsupported => unsupported(cx),
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
        let height = height + ROW_GAP;
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
        let header = h_flex()
            .h(px(24.))
            .px_2()
            .gap_2()
            .flex_none()
            .text_size(px(10.5))
            .font_weight(FontWeight::BOLD)
            .text_color(campo::dim(cx))
            .children((columns.iter()).map(|c| cell(c.width).child(c.title.to_uppercase())));
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
                .h(px((ROW_HEIGHT + ROW_GAP) * count.min(LIST_ROWS) as f32))
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
        let foreground = cx.theme().foreground;
        let Some(row) = row else {
            return gap(h_flex().id(("panel-row", ix)).h(px(ROW_HEIGHT)).px_1())
                .child(unsupported(cx))
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
        let line = |el: Div| el.h(px(ROW_HEIGHT)).px_2().gap_2().text_color(foreground);
        let element = match &row.id {
            Some(action) => {
                let action = action.clone();
                line(campo::loose_row(cx))
                    .id(("panel-row", ix))
                    .cursor_pointer()
                    .children(cells)
                    .on_click(cx.listener(move |this, _, _, cx| this.act(action.clone(), cx)))
                    .test_support()
                    .into_any_element()
            }
            // Read-only: the same glass, no hover.
            None => line(h_flex())
                .rounded(px(9.))
                .border_1()
                .border_color(campo::line(cx))
                .bg(campo::glass(cx))
                .id(("panel-row", ix))
                .children(cells)
                .test_support()
                .into_any_element(),
        };
        gap(div()).child(element).into_any_element()
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
            .size(px(22.))
            .icon(Lucide::Copy)
            .tooltip("Copy")
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copied.to_string()))
            });
        div()
            .relative()
            .w_full()
            .min_w(px(LIST_MIN_WIDTH))
            .rounded(px(campo::RADIUS))
            .border_1()
            .border_color(campo::line(cx))
            .bg(campo::recessed(cx))
            .font_family(theme.mono_font_family.clone())
            .text_size(px(11.))
            .map(|el| match fill {
                true => el.flex_1().min_h_0(),
                false => el
                    .h(px(LOG_LINE * count.clamp(1, LIST_ROWS) as f32))
                    .flex_none(),
            })
            .child(list)
            .child(div().absolute().top_1().right_1().child(copy))
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
    let height = height - ROW_GAP;
    let Some(item) = item else {
        return gap(h_flex().w_full().h(px(height)).px_1())
            .child(unsupported(cx))
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
    let item = campo::loose_row(cx)
        .h(px(height))
        .px_2()
        .gap_2()
        .text_color(theme.foreground)
        .id(("panel-item", ix))
        .cursor_pointer()
        .child(v_flex().flex_1().min_w_0().child(title).children(subtitle))
        .children((item.badge.as_ref()).map(|b| campo::badge(b.to_string(), Tone::Neutral, cx)))
        .on_click(move |_, _, cx| {
            panel
                .update(cx, |panel, cx| panel.act(action.clone(), cx))
                .ok();
        })
        .test_support();
    gap(div()).child(item).into_any_element()
}

/// A loose row's slot in a list: the row, then the space before the next.
fn gap<E: Styled>(el: E) -> E {
    el.pb(px(ROW_GAP))
}

/// How many numbers `node` takes in `ExtPanel::node`: itself and what it holds, as if every
/// section were open. A table's cells number their own.
fn size(node: &Node) -> usize {
    1 + match node {
        Node::Column(children)
        | Node::Row(children)
        | Node::Card { children, .. }
        | Node::Section { children, .. } => children.iter().map(size).sum(),
        Node::Tabs { tabs, .. } => tabs.iter().flat_map(|t| &t.children).map(size).sum(),
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

pub(crate) fn tone_color(tone: Tone, cx: &App) -> Hsla {
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

/// The display leaves, drawn the same in a panel and in an item's hover card; `k` numbers the
/// node for its element id. Anything else is unsupported.
fn leaf(node: &Node, k: usize, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    match node {
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
        Node::Badge { text, tone } => h_flex()
            .child(campo::badge(text.to_string(), *tone, cx))
            .into_any_element(),
        // A violet-to-boreal fill on a recessed track.
        Node::Progress(value) => div()
            .id(("panel-progress", k))
            .w_full()
            .h(px(6.))
            .rounded_full()
            .bg(campo::recessed(cx))
            .child(
                div()
                    .h_full()
                    .w(relative(value.clamp(0., 1.)))
                    .rounded_full()
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(campo::violet(cx), 0.),
                        linear_color_stop(theme.primary, 1.),
                    )),
            )
            .into_any_element(),
        Node::Icon { name, tone } => match lucide(name) {
            Some(icon) => Icon::new(icon)
                .small()
                .text_color(tone_color(*tone, cx))
                .into_any_element(),
            None => unsupported(cx),
        },
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
                            .text_xs()
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
        _ => unsupported(cx),
    }
}

/// An item's hover card: the tree, then the extension's name. Display nodes only (the parser
/// makes anything else `Unsupported`), so nothing in it is interactive.
pub(crate) fn hover_card(node: &Node, ext_name: &SharedString, cx: &App) -> AnyElement {
    fn draw(node: &Node, n: &mut usize, cx: &App) -> AnyElement {
        *n += 1;
        match node {
            Node::Column(children) => v_flex()
                .gap_1()
                .children(children.iter().map(|c| draw(c, n, cx)))
                .into_any_element(),
            Node::Row(children) => h_flex()
                .gap_1()
                .children(children.iter().map(|c| draw(c, n, cx)))
                .into_any_element(),
            leaf_node => leaf(leaf_node, *n, cx),
        }
    }
    v_flex()
        .debug_selector(|| "item-card".into())
        .w(px(320.))
        .gap_2()
        .child(draw(node, &mut 0, cx))
        .child(
            div()
                .debug_selector(|| "item-card-footer".into())
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(format!("\u{2014} {ext_name}")),
        )
        .into_any_element()
}

/// A dashed chip: the extension sent something this wai does not draw.
fn unsupported(cx: &App) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    h_flex()
        .debug_selector(|| "panel-unsupported".into())
        .flex_none()
        .gap_1()
        .px_2()
        .py_0p5()
        .rounded(px(6.))
        .border_1()
        .border_dashed()
        .border_color(campo::line(cx))
        .text_xs()
        .text_color(muted)
        .child(Icon::new(Lucide::CircleQuestionMark).xsmall())
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
            shown(("panel-item", 20usize), cx),
            "the list fills the panel"
        );
        assert!(below(cx), "what follows the list stays in view");
        assert_eq!(placeholder(cx).as_deref(), Some("Second"));

        // In a row, `fill` has no height to take.
        let row = json!({"type": "row", "children": [list(true)]});
        show(&panel, tree(row, "Second"), cx);
        assert!(!shown(("panel-item", 20usize), cx));
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
                {"type": "progress", "value": 0.4},
                {"type": "path", "id": "pa", "pick": "file"},
                {"type": "path", "id": "pb", "pick": "folder", "editable": false, "value": "/opt"},
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

    /// A path field sends its path with `~` expanded; one outside home goes as it is.
    #[gpui_kit::test]
    fn path_value_reaches_the_form(cx: &mut TestAppContext) {
        use crate::extensions_view::tests::click;

        let (panel, cx) = panel_window(cx);
        let actions = actions(&panel, cx);
        show(
            &panel,
            json!({"type": "column", "children": [
                {"type": "path", "id": "p", "pick": "folder", "value": "~/x"},
                {"type": "path", "id": "q", "pick": "file", "editable": false, "value": "/opt/a"},
                {"type": "button", "id": "go", "label": "Go"},
            ]}),
            cx,
        );
        click(name("panel-button:go"), cx);
        let home = dirs::home_dir().unwrap().to_string_lossy().into_owned();
        let values = actions.borrow()[0].1.clone();
        assert_eq!(values, json!({"p": format!("{home}/x"), "q": "/opt/a"}));
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

    fn tabs_tree(value: Option<&str>) -> Value {
        json!({"type": "tabs", "id": "t", "value": value, "tabs": [
            {"id": "work", "title": "Work", "badge": "3", "children": [
                {"type": "button", "id": "in-work", "label": "Work button"},
            ]},
            {"id": "help", "title": "Help", "children": [
                {"type": "input", "id": "q", "placeholder": "q"},
                {"type": "button", "id": "in-help", "label": "Help button"},
            ]},
        ]})
    }

    /// Switching tabs is local: it shows the tab's children and sends nothing.
    #[gpui_kit::test]
    fn tabs_switch_locally(cx: &mut TestAppContext) {
        use crate::extensions_view::tests::click;

        let (panel, cx) = panel_window(cx);
        let actions = actions(&panel, cx);
        show(&panel, tabs_tree(None), cx);
        assert!(shown(name("panel-button:in-work"), cx));
        assert!(!shown(name("panel-button:in-help"), cx));
        click(name("panel-tab:t:help"), cx);
        frame(cx);
        assert!(shown(name("panel-button:in-help"), cx));
        assert!(!shown(name("panel-button:in-work"), cx));
        assert!(actions.borrow().is_empty());
    }

    #[gpui_kit::test]
    fn tab_value_reaches_the_form(cx: &mut TestAppContext) {
        use crate::extensions_view::tests::click;

        let (panel, cx) = panel_window(cx);
        let actions = actions(&panel, cx);
        show(&panel, tabs_tree(None), cx);
        click(name("panel-tab:t:help"), cx);
        frame(cx);
        click(name("panel-button:in-help"), cx);
        assert_eq!(
            *actions.borrow(),
            [("in-help".to_string(), json!({"q": "", "t": "help"}))]
        );
    }

    /// A hidden tab keeps what was typed in it, and its fields still go with an action.
    #[gpui_kit::test]
    fn typed_text_survives_a_tab_switch(cx: &mut TestAppContext) {
        use crate::extensions_view::tests::click;

        let (panel, cx) = panel_window(cx);
        let actions = actions(&panel, cx);
        show(&panel, tabs_tree(Some("help")), cx);
        let input = panel.read_with(cx, |p, _| p.inputs.get("q").map(|(f, _)| f.clone()));
        cx.update(|window, cx| input.unwrap().set_value("typed", window, cx));
        click(name("panel-tab:t:work"), cx);
        frame(cx);
        show(&panel, tabs_tree(Some("help")), cx);
        click(name("panel-button:in-work"), cx);
        assert_eq!(
            *actions.borrow(),
            [("in-work".to_string(), json!({"q": "typed", "t": "work"}))]
        );
        click(name("panel-tab:t:help"), cx);
        frame(cx);
        let typed = panel.read_with(cx, |p, _| p.inputs.get("q").map(|(f, _)| f.clone()));
        assert_eq!(cx.update(|_, cx| typed.unwrap().value(cx)), "typed");
    }

    /// `value` selects a tab when it first appears or changes; otherwise the user's choice stays.
    #[gpui_kit::test]
    fn tabs_value_is_an_initial_value(cx: &mut TestAppContext) {
        use crate::extensions_view::tests::click;

        let (panel, cx) = panel_window(cx);
        let on_help = |cx: &mut VisualTestContext| {
            frame(cx);
            shown(name("panel-button:in-help"), cx)
        };
        show(&panel, tabs_tree(Some("help")), cx);
        assert!(on_help(cx));
        click(name("panel-tab:t:work"), cx);
        assert!(!on_help(cx));
        show(&panel, tabs_tree(Some("help")), cx);
        assert!(!on_help(cx), "the same `value` keeps the user's choice");
        show(&panel, tabs_tree(Some("work")), cx);
        assert!(!on_help(cx));
        show(&panel, tabs_tree(Some("help")), cx);
        assert!(on_help(cx), "a different `value` selects it");
    }

    #[gpui_kit::test]
    fn fill_through_tabs(cx: &mut TestAppContext) {
        let (panel, cx) = panel_window(cx);
        let items: Vec<_> = (0..200)
            .map(|n| json!({"id": n.to_string(), "title": "t"}))
            .collect();
        let tree = |fill: bool| {
            json!({"type": "tabs", "id": "t", "tabs": [
                {"id": "a", "title": "A", "children": [
                    {"type": "list", "fill": fill, "items": items},
                ]},
            ]})
        };
        show(&panel, tree(false), cx);
        assert!(shown(("panel-item", 11usize), cx) && !shown(("panel-item", 30usize), cx));
        show(&panel, tree(true), cx);
        assert!(
            shown(("panel-item", 30usize), cx),
            "the list fills the panel"
        );
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
