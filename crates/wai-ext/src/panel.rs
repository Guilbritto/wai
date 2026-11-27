//! The declarative panel schema (API 1.2): the tree an extension sends in `panel/render` (and, as
//! a display-only `card`, in `item/update`), checked
//! into typed nodes. The tree is untrusted: parsing never panics and never recurses past
//! `MAX_DEPTH`. An unknown or broken node becomes `Unsupported`; only a tree beyond the size limits
//! is refused whole (`TooLarge`, shown as "Panel too large").
//!
//! - Every field without `?` in the schema is required. An optional field of the wrong type counts
//!   as absent (but a `confirm` that is there and broken breaks its node); fields the schema does
//!   not name are ignored.
//! - `text` (and `markdown`, `alert`, a multiline input's `value`) keep at most `TEXT_CHARS`
//!   characters; a `log` keeps its last `LOG_CHARS`. Every other string is one line of at most
//!   `LINE_CHARS` characters.
//! - Ids are 1 to `MAX_ID` bytes. Buttons, form fields, sections, tabs, bubbles and pixels share one set of ids: a
//!   repeated id makes that node `Unsupported`. List item, card, table row and menu item ids may
//!   repeat.
//! - Nodes are the tree's root, each element of a `children` array and each table cell that is
//!   not a string. List items, select options, table columns and rows, `kv` items and menu items
//!   are not nodes; each has its own cap, past which the rest are dropped.
//! - `value` (input, select, tabs), `checked` (checkbox) and `open` (section) are initial values: wai
//!   applies one when the node first appears or when the extension sends a different one;
//!   otherwise what the user did stays. `fill` makes a list, table or log take the panel's
//!   remaining height.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use serde_json::{Map, Value};

pub const MAX_DEPTH: usize = 32;
pub const MAX_NODES: usize = 2_000;
/// The limits of an item's hover card.
pub const CARD_DEPTH: usize = 8;
pub const CARD_NODES: usize = 200;
pub const TEXT_CHARS: usize = 10_000;
pub const LINE_CHARS: usize = 500;
pub const MAX_ID: usize = 256;
pub const MAX_TABS: usize = 10;
/// A select's menu draws every option.
pub const MAX_OPTIONS: usize = 100;
pub const MAX_COLUMNS: usize = 20;
pub const MAX_ROWS: usize = 1_000;
/// A log keeps its last this many characters.
pub const LOG_CHARS: usize = 100_000;
pub const MAX_KV: usize = 200;
pub const MAX_MENU: usize = 50;
/// A multiline input's height, in lines.
pub const DEFAULT_INPUT_ROWS: u8 = 4;
pub const MAX_INPUT_ROWS: u8 = 20;
pub const MAX_URL: usize = 2_048;
/// Lucide names are kebab-case: `git-pull-request`.
pub const MAX_ICON: usize = 64;
/// A column's `width`, in px.
pub const MIN_WIDTH: f64 = 24.;
pub const MAX_WIDTH: f64 = 2_000.;
/// The most a `pixels` node keeps, in cells.
pub const PIXEL_ROWS: usize = 64;
pub const PIXEL_COLS: usize = 256;
/// A scene's limits (API 1.10).
pub const SCENE_PALETTE: usize = 64;
pub const SCENE_PATTERNS: usize = 64;
pub const SCENE_PATTERN_SIDE: usize = 32;
pub const SCENE_MAP_SIDE: usize = 128;
pub const SCENE_SPRITES: usize = 100;
pub const SCENE_LABEL: usize = 32;
pub const SCENE_BADGE: usize = 24;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// `width`: fixed, in px; else columns in a row share the width.
    Column {
        children: Vec<Node>,
        width: Option<f32>,
        /// Already true when `title` or `collapsed` is given.
        framed: bool,
        title: Option<Arc<str>>,
        badge: Option<Arc<str>>,
        tone: Tone,
        collapsed: bool,
    },
    /// `fill`: below columns only, it takes the height left and each child scrolls on its own.
    Row {
        children: Vec<Node>,
        fill: bool,
    },
    Text {
        text: Arc<str>,
        muted: bool,
        mono: bool,
        style: TextStyle,
    },
    Markdown(Arc<str>),
    /// A broken item is `None`. `subtitles`: some item has one, so rows take two lines.
    List {
        items: Arc<[Option<Item>]>,
        subtitles: bool,
        fill: bool,
        plain: bool,
    },
    /// A speech bubble (API 1.8): wai animates it and sends `panel/action` with its id when it ends.
    Bubble {
        id: String,
        text: Arc<str>,
        tone: Tone,
        sticky: bool,
    },
    Button {
        id: String,
        label: Arc<str>,
        style: ButtonStyle,
        icon: Option<Arc<str>>,
        disabled: bool,
        confirm: Option<Confirm>,
        /// False without `icon`.
        icon_only: bool,
    },
    /// `rows`: a multiline input's height in lines; `None` for one line.
    Input {
        id: String,
        placeholder: Arc<str>,
        value: Option<Arc<str>>,
        rows: Option<u8>,
        icon: Option<Arc<str>>,
        /// False when `rows` is `Some`.
        submit: bool,
        clear: bool,
    },
    Select {
        id: String,
        options: Vec<Arc<str>>,
        value: Option<Arc<str>>,
        segmented: bool,
    },
    Checkbox {
        id: String,
        label: Arc<str>,
        checked: Option<bool>,
    },
    /// A file or folder path (API 1.6). `editable`: typed or picked; else picked only.
    Path {
        id: String,
        pick: Pick,
        placeholder: Option<Arc<str>>,
        value: Option<Arc<str>>,
        editable: bool,
    },
    Badge {
        text: Arc<str>,
        tone: Tone,
    },
    /// From 0 to 1.
    Progress(f32),
    /// With `id`, the whole card is clickable.
    Card {
        id: Option<String>,
        title: Option<Arc<str>>,
        subtitle: Option<Arc<str>>,
        badge: Option<Arc<str>>,
        icon: Option<Arc<str>>,
        selected: bool,
        children: Vec<Node>,
    },
    /// `open`: the initial state; open when never given.
    Section {
        id: String,
        title: Arc<str>,
        open: Option<bool>,
        badge: Option<Arc<str>>,
        children: Vec<Node>,
    },
    /// `value`: the id of the tab selected at first; the first tab when absent or unknown.
    Tabs {
        id: String,
        value: Option<String>,
        tabs: Vec<Tab>,
        segmented: bool,
    },
    /// A broken row is `None`.
    Table {
        columns: Vec<Column>,
        rows: Arc<[Option<TableRow>]>,
        fill: bool,
    },
    /// ANSI escapes and control characters (but tabs and newlines) removed.
    Log {
        text: Arc<str>,
        follow: bool,
        fill: bool,
    },
    /// Never `Tone::Neutral`.
    Alert {
        tone: Tone,
        title: Option<Arc<str>>,
        text: Arc<str>,
    },
    /// `url` is `http://` or `https://`.
    Link {
        text: Arc<str>,
        url: Arc<str>,
        icon: Option<Arc<str>>,
        icon_only: bool,
    },
    Icon {
        name: Arc<str>,
        tone: Tone,
    },
    Spinner(Option<Arc<str>>),
    Divider,
    Spacer,
    Kv(Arc<[(Arc<str>, Arc<str>)]>),
    /// Without `label` or `icon`, shown as the `ellipsis` icon.
    Menu {
        label: Option<Arc<str>>,
        icon: Option<Arc<str>>,
        items: Vec<MenuItem>,
    },
    /// A grid of cells (API 1.9): 0 empty, 1 dim (`+`), 2 full (`#`); every row is `width` long.
    Pixels {
        id: Option<String>,
        rows: Arc<[Box<[u8]>]>,
        width: usize,
        scale: u8,
    },
    /// A grid of coloured tiles with sprites on it (API 1.10).
    Scene(Box<Scene>),
    /// An unknown type, or a known one that is broken.
    Unsupported,
}

/// A scene's pattern: `rows` of palette keys, a space being transparent; every row is `width`
/// long.
#[derive(Debug, Clone, PartialEq, Hash)]
pub struct Pattern {
    pub width: usize,
    pub rows: Vec<Vec<char>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sprite {
    pub id: String,
    pub x: usize,
    pub y: usize,
    pub pattern: String,
    pub label: Option<Arc<str>>,
    pub badge: Option<Arc<str>>,
    pub tone: Tone,
}

/// The map is `rows` of characters, padded to `width`; `legend` names each character's pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub id: String,
    pub cell: usize,
    pub scale: u8,
    pub palette: BTreeMap<char, [u8; 3]>,
    pub patterns: BTreeMap<String, Pattern>,
    pub legend: BTreeMap<char, String>,
    pub map: Vec<Vec<char>>,
    pub width: usize,
    pub sprites: Vec<Sprite>,
    pub follow: Option<String>,
    pub margin: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    pub title: Arc<str>,
    pub subtitle: Option<Arc<str>>,
    pub badge: Option<Arc<str>>,
    pub icon: Option<Arc<str>>,
    pub tone: Tone,
    pub selected: bool,
    pub lead: Option<Arc<str>>,
    pub note: Option<Arc<str>>,
    pub indent: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextStyle {
    #[default]
    Plain,
    Strong,
    Heading,
    Label,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ButtonStyle {
    #[default]
    Default,
    Primary,
    Danger,
}

/// What a `path` node's system picker chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    File,
    Folder,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Neutral,
    Info,
    Success,
    Warning,
    Danger,
}

/// Asked before a button or menu item sends its action.
#[derive(Debug, Clone, PartialEq)]
pub struct Confirm {
    pub title: Arc<str>,
    pub text: Option<Arc<str>>,
    /// The confirm button's label; `Confirm` when `None`.
    pub label: Option<Arc<str>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    pub id: String,
    pub title: Arc<str>,
    pub badge: Option<Arc<str>>,
    pub tone: Tone,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub title: Arc<str>,
    /// In pixels; columns without one share the rest.
    pub width: Option<f32>,
}

/// `cells` holds at most one per column; missing ones are blank. A string cell is a `Text`.
#[derive(Debug, Clone, PartialEq)]
pub struct TableRow {
    pub id: Option<String>,
    pub cells: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MenuItem {
    pub id: String,
    pub label: Arc<str>,
    pub danger: bool,
    pub confirm: Option<Confirm>,
}

/// Deeper than `MAX_DEPTH` or more than `MAX_NODES` nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooLarge;

pub fn parse(tree: &Value) -> Result<Node, TooLarge> {
    Parser::new(MAX_DEPTH, MAX_NODES).node(tree, 1)
}

/// An item's hover card: the display nodes only; any other node becomes `Unsupported`. `None`
/// when past the card's limits.
fn card(tree: &Value) -> Option<Node> {
    fn display(node: Node) -> Node {
        match node {
            Node::Column {
                children,
                width,
                framed,
                title,
                badge,
                tone,
                collapsed,
            } => Node::Column {
                children: children.into_iter().map(display).collect(),
                width,
                framed,
                title,
                badge,
                tone,
                collapsed,
            },
            Node::Row { children, fill } => Node::Row {
                children: children.into_iter().map(display).collect(),
                fill,
            },
            Node::Text { .. }
            | Node::Badge { .. }
            | Node::Icon { .. }
            | Node::Spacer
            | Node::Progress(_)
            | Node::Divider
            | Node::Kv(_) => node,
            _ => Node::Unsupported,
        }
    }
    Parser::new(CARD_DEPTH, CARD_NODES)
        .node(tree, 1)
        .ok()
        .map(display)
}

impl Node {
    /// Each node, parents first; a table's cells after the table.
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Node)) {
        f(self);
        match self {
            Node::Column { children, .. }
            | Node::Row { children, .. }
            | Node::Card { children, .. }
            | Node::Section { children, .. } => {
                for child in children {
                    child.walk(f);
                }
            }
            Node::Tabs { tabs, .. } => {
                for child in tabs.iter().flat_map(|tab| &tab.children) {
                    child.walk(f);
                }
            }
            Node::Table { rows, .. } => {
                for cell in rows.iter().flatten().flat_map(|row| &row.cells) {
                    cell.walk(f);
                }
            }
            _ => {}
        }
    }
}

/// `text` on one line (whitespace and control characters collapsed), at most `max` characters
/// and `…`.
pub fn clamp(text: &str, max: usize) -> String {
    let words = text.split(|c: char| c.is_whitespace() || c.is_control());
    let words = words.filter(|w| !w.is_empty()).enumerate();
    // Lazily, so a 4 MB string costs `max` characters, not a copy.
    let mut chars = words.flat_map(|(ix, w)| (ix > 0).then_some(' ').into_iter().chain(w.chars()));
    let mut line: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        line.push('…');
    }
    line
}

/// `text`, at most `max` characters and `…`.
pub fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

/// `…` and the last `max` characters of `text`, or all of it if it is not longer.
pub fn tail(text: &str, max: usize) -> String {
    match text.char_indices().rev().nth(max) {
        Some((start, c)) => format!("…{}", &text[start + c.len_utf8()..]),
        None => text.to_owned(),
    }
}

/// `text` without ANSI escape sequences (CSI, OSC and two-character ones) or control characters
/// other than tabs and newlines.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.next() {
                // Parameters and intermediates, up to one final byte.
                Some('[') => _ = chars.find(|c| ('@'..='~').contains(c)),
                // Up to BEL or ST (`ESC \`).
                Some(']') => {
                    if chars.find(|c| matches!(c, '\u{7}' | '\u{1b}' | '\u{9c}')) == Some('\u{1b}')
                        && chars.clone().next() == Some('\\')
                    {
                        chars.next();
                    }
                }
                // Intermediates, then one final byte: `ESC ( B`.
                Some(' '..='/') => _ = chars.find(|c| !(' '..='/').contains(c)),
                _ => {}
            },
            '\n' | '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// A Lucide name's shape: `[a-z0-9-]`, 1 to `MAX_ICON` bytes. Whether wai has it is the
/// renderer's business.
pub fn icon_name(name: &str) -> bool {
    (1..=MAX_ICON).contains(&name.len())
        && (name.bytes()).all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A bubble's text, in characters.
pub const BUBBLE_CHARS: usize = 140;

/// An extension item's text, in characters.
pub const ITEM_TEXT_CHARS: usize = 32;

/// What `item/update` last said about a bar item.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemState {
    pub text: Option<Arc<str>>,
    pub icon: Option<Arc<str>>,
    pub tooltip: Option<Arc<str>>,
    pub tone: Tone,
    pub card: Option<Arc<Node>>,
}

impl ItemState {
    /// Drawn only with a text or an icon.
    pub fn shown(&self) -> bool {
        self.text.is_some() || self.icon.is_some()
    }
}

/// The state `params` describe; anything that is not an object is the default.
pub fn item_state(params: &Value) -> ItemState {
    let Some(o) = params.as_object() else {
        return ItemState::default();
    };
    let line = |key: &str, max: usize| {
        let line = clamp(str_field(o, key)?, max);
        (!line.is_empty()).then(|| Arc::from(line))
    };
    // `clamp` may add its `…` to `max` characters; the text's limit counts it.
    let text = line("text", ITEM_TEXT_CHARS).map(|t: Arc<str>| match t.chars().count() {
        n if n > ITEM_TEXT_CHARS => Arc::from(cut(&t, ITEM_TEXT_CHARS - 1)),
        _ => t,
    });
    ItemState {
        text,
        icon: icon(o),
        tooltip: line("tooltip", LINE_CHARS),
        tone: tone(o).unwrap_or_default(),
        card: o
            .get("card")
            .filter(|c| !c.is_null())
            .and_then(card)
            .map(Arc::new),
    }
}

/// Node types a table cell may be.
const CELLS: [&str; 8] = [
    "text", "badge", "link", "icon", "button", "menu", "spinner", "progress",
];

struct Parser {
    max_depth: usize,
    max_nodes: usize,
    nodes: usize,
    /// Taken by buttons, form fields and sections.
    ids: HashSet<String>,
}

impl Parser {
    fn new(max_depth: usize, max_nodes: usize) -> Self {
        Parser {
            max_depth,
            max_nodes,
            nodes: 0,
            ids: HashSet::new(),
        }
    }

    /// Counts a node at `depth`.
    fn count(&mut self, depth: usize) -> Result<(), TooLarge> {
        self.nodes += 1;
        match depth > self.max_depth || self.nodes > self.max_nodes {
            true => Err(TooLarge),
            false => Ok(()),
        }
    }

    /// Recurses at most `max_depth` deep, and stops at the first node past a limit.
    fn node(&mut self, value: &Value, depth: usize) -> Result<Node, TooLarge> {
        self.count(depth)?;
        let Some(o) = value.as_object() else {
            return Ok(Node::Unsupported);
        };
        let node = match str_field(o, "type") {
            Some(kind) => self.kind(kind, o, depth)?,
            None => None,
        };
        Ok(node.unwrap_or(Node::Unsupported))
    }

    /// `children`, if it is an array.
    fn children(
        &mut self,
        o: &Map<String, Value>,
        depth: usize,
    ) -> Result<Option<Vec<Node>>, TooLarge> {
        let Some(children) = o.get("children").and_then(Value::as_array) else {
            return Ok(None);
        };
        let children = children.iter().map(|child| self.node(child, depth + 1));
        children.collect::<Result<_, _>>().map(Some)
    }

    fn kind(
        &mut self,
        kind: &str,
        o: &Map<String, Value>,
        depth: usize,
    ) -> Result<Option<Node>, TooLarge> {
        Ok(match kind {
            "column" => self.children(o, depth)?.map(|children| {
                let title = line(o, "title");
                let collapsed = flag(o, "collapsed").unwrap_or(false);
                Node::Column {
                    children,
                    width: (o.get("width").and_then(Value::as_f64))
                        .map(|w| w.clamp(MIN_WIDTH, MAX_WIDTH) as f32),
                    framed: flag(o, "framed").unwrap_or(false) || title.is_some() || collapsed,
                    title,
                    badge: line(o, "badge"),
                    tone: tone(o).unwrap_or_default(),
                    collapsed,
                }
            }),
            "row" => self.children(o, depth)?.map(|children| Node::Row {
                children,
                fill: flag(o, "fill").unwrap_or(false),
            }),
            "card" => {
                let card = |children| Node::Card {
                    id: id(o),
                    title: line(o, "title"),
                    subtitle: line(o, "subtitle"),
                    badge: line(o, "badge"),
                    icon: icon(o),
                    selected: flag(o, "selected").unwrap_or(false),
                    children,
                };
                Some(card(self.children(o, depth)?.unwrap_or_default()))
            }
            "section" => {
                let (Some(id), Some(title)) = (id(o), line(o, "title")) else {
                    return Ok(None);
                };
                // Only a section that is otherwise whole takes its id.
                if !o.get("children").is_some_and(Value::is_array) || !self.ids.insert(id.clone()) {
                    return Ok(None);
                }
                Some(Node::Section {
                    id,
                    title,
                    open: flag(o, "open"),
                    badge: line(o, "badge"),
                    children: self.children(o, depth)?.unwrap_or_default(),
                })
            }
            "tabs" => self.tabs(o, depth)?,
            "table" => self.table(o, depth)?,
            _ => self.leaf(kind, o),
        })
    }

    fn tabs(&mut self, o: &Map<String, Value>, depth: usize) -> Result<Option<Node>, TooLarge> {
        let Some(id) = id(o) else {
            return Ok(None);
        };
        let Some(raw) = o.get("tabs").and_then(Value::as_array) else {
            return Ok(None);
        };
        if raw.is_empty() || raw.len() > MAX_TABS {
            return Ok(None);
        }
        // Each tab whole and its id its own, before the node takes its id or reads a child.
        let mut seen = HashSet::new();
        let mut heads = Vec::new();
        for tab in raw {
            let Some(t) = tab.as_object() else {
                return Ok(None);
            };
            let (Some(tab_id), Some(title)) = (self::id(t), line(t, "title")) else {
                return Ok(None);
            };
            if !t.get("children").is_some_and(Value::is_array) || !seen.insert(tab_id.clone()) {
                return Ok(None);
            }
            heads.push((t, tab_id, title));
        }
        if !self.ids.insert(id.clone()) {
            return Ok(None);
        }
        let mut tabs = Vec::new();
        for (t, tab_id, title) in heads {
            tabs.push(Tab {
                id: tab_id,
                title,
                badge: line(t, "badge"),
                tone: tone(t).unwrap_or_default(),
                children: self.children(t, depth)?.unwrap_or_default(),
            });
        }
        Ok(Some(Node::Tabs {
            id,
            value: str_field(o, "value").map(str::to_owned),
            tabs,
            segmented: str_field(o, "style") == Some("segmented"),
        }))
    }

    fn table(&mut self, o: &Map<String, Value>, depth: usize) -> Result<Option<Node>, TooLarge> {
        let column = |value: &Value| {
            let o = value.as_object()?;
            let width = o.get("width").and_then(Value::as_f64);
            Some(Column {
                title: line(o, "title")?,
                width: width.filter(|w| *w > 0.).map(|w| w.min(4_000.) as f32),
            })
        };
        let (Some(columns), Some(rows)) = (
            o.get("columns").and_then(Value::as_array),
            o.get("rows").and_then(Value::as_array),
        ) else {
            return Ok(None);
        };
        let columns = columns.iter().take(MAX_COLUMNS).map(column);
        let Some(columns) = columns.collect::<Option<Vec<_>>>() else {
            return Ok(None);
        };
        let rows = rows.iter().take(MAX_ROWS).map(|row| {
            let Some(o) = row.as_object() else {
                return Ok(None);
            };
            let Some(cells) = o.get("cells").and_then(Value::as_array) else {
                return Ok(None);
            };
            let cells = cells.iter().take(columns.len());
            let cells = cells.map(|cell| self.cell(cell, depth + 1));
            Ok(Some(TableRow {
                id: id(o),
                cells: cells.collect::<Result<_, _>>()?,
            }))
        });
        Ok(Some(Node::Table {
            rows: rows.collect::<Result<_, _>>()?,
            columns,
            fill: flag(o, "fill").unwrap_or(false),
        }))
    }

    /// A string is one line of text and not a node; anything else is a node, `Unsupported`
    /// unless it is one of `CELLS`.
    fn cell(&mut self, value: &Value, depth: usize) -> Result<Node, TooLarge> {
        if let Some(text) = value.as_str() {
            return Ok(Node::Text {
                text: clamp(text, LINE_CHARS).into(),
                muted: false,
                mono: false,
                style: TextStyle::Plain,
            });
        }
        let kind = value.get("type").and_then(Value::as_str);
        if kind.is_some_and(|kind| CELLS.contains(&kind)) {
            return self.node(value, depth);
        }
        self.count(depth)?;
        Ok(Node::Unsupported)
    }

    fn leaf(&mut self, kind: &str, o: &Map<String, Value>) -> Option<Node> {
        let text = || str_field(o, "text").map(|t| cut(t, TEXT_CHARS).into());
        let node = match kind {
            "text" => Node::Text {
                text: text()?,
                muted: flag(o, "muted").unwrap_or(false),
                mono: flag(o, "mono").unwrap_or(false),
                style: match str_field(o, "style") {
                    Some("strong") => TextStyle::Strong,
                    Some("heading") => TextStyle::Heading,
                    Some("label") => TextStyle::Label,
                    _ => TextStyle::Plain,
                },
            },
            "markdown" => Node::Markdown(text()?),
            "badge" => Node::Badge {
                text: line(o, "text")?,
                tone: tone(o).unwrap_or_default(),
            },
            "progress" => Node::Progress(o.get("value")?.as_f64()?.clamp(0., 1.) as f32),
            "list" => {
                let items = o.get("items")?.as_array()?;
                let items: Arc<[Option<Item>]> = items.iter().map(item).collect();
                let subtitles = items.iter().flatten().any(|i| i.subtitle.is_some());
                Node::List {
                    items,
                    subtitles,
                    fill: flag(o, "fill").unwrap_or(false),
                    plain: flag(o, "plain").unwrap_or(false),
                }
            }
            "bubble" => {
                let text = line(o, "text")?;
                Node::Bubble {
                    id: id(o)?,
                    text: match text.chars().count() {
                        n if n > BUBBLE_CHARS => Arc::from(cut(&text, BUBBLE_CHARS - 1)),
                        _ => text,
                    },
                    tone: tone(o).unwrap_or_default(),
                    sticky: flag(o, "sticky").unwrap_or(false),
                }
            }
            "button" => {
                let icon = icon(o);
                Node::Button {
                    id: id(o)?,
                    label: line(o, "label")?,
                    style: match str_field(o, "style") {
                        Some("primary") => ButtonStyle::Primary,
                        Some("danger") => ButtonStyle::Danger,
                        _ => ButtonStyle::Default,
                    },
                    icon_only: icon.is_some() && flag(o, "icon_only").unwrap_or(false),
                    icon,
                    disabled: flag(o, "disabled").unwrap_or(false),
                    confirm: confirm(o)?,
                }
            }
            "input" => {
                let rows = flag(o, "multiline").unwrap_or(false).then(|| {
                    let rows = o.get("rows").and_then(Value::as_u64);
                    rows.map_or(DEFAULT_INPUT_ROWS, |r| {
                        r.clamp(1, MAX_INPUT_ROWS.into()) as u8
                    })
                });
                Node::Input {
                    id: id(o)?,
                    placeholder: line(o, "placeholder")?,
                    value: match rows {
                        Some(_) => str_field(o, "value").map(|t| cut(t, TEXT_CHARS).into()),
                        None => line(o, "value"),
                    },
                    submit: rows.is_none() && flag(o, "submit").unwrap_or(false),
                    rows,
                    icon: icon(o),
                    clear: flag(o, "clear").unwrap_or(false),
                }
            }
            "select" => {
                let options = o.get("options")?.as_array()?.iter().take(MAX_OPTIONS);
                let options = options.map(|v| Some(clamp(v.as_str()?, LINE_CHARS).into()));
                Node::Select {
                    id: id(o)?,
                    options: options.collect::<Option<_>>()?,
                    value: line(o, "value"),
                    segmented: flag(o, "segmented").unwrap_or(false),
                }
            }
            "path" => Node::Path {
                id: id(o)?,
                pick: match str_field(o, "pick")? {
                    "file" => Pick::File,
                    "folder" => Pick::Folder,
                    _ => return None,
                },
                placeholder: line(o, "placeholder"),
                value: line(o, "value"),
                editable: flag(o, "editable").unwrap_or(true),
            },
            "checkbox" => Node::Checkbox {
                id: id(o)?,
                label: line(o, "label")?,
                checked: flag(o, "checked"),
            },
            "log" => Node::Log {
                text: tail(&strip_ansi(str_field(o, "text")?), LOG_CHARS).into(),
                follow: flag(o, "follow").unwrap_or(true),
                fill: flag(o, "fill").unwrap_or(false),
            },
            "alert" => Node::Alert {
                tone: tone(o).filter(|t| *t != Tone::Neutral)?,
                title: line(o, "title"),
                text: text()?,
            },
            "link" => {
                let icon = icon(o);
                Node::Link {
                    text: line(o, "text")?,
                    url: str_field(o, "url").filter(|url| web_url(url))?.into(),
                    icon_only: icon.is_some() && flag(o, "icon_only").unwrap_or(false),
                    icon,
                }
            }
            "icon" => Node::Icon {
                name: str_field(o, "name").filter(|n| icon_name(n))?.into(),
                tone: tone(o).unwrap_or_default(),
            },
            "spinner" => Node::Spinner(line(o, "label")),
            "divider" => Node::Divider,
            "spacer" => Node::Spacer,
            "kv" => {
                let items = o.get("items")?.as_array()?.iter().take(MAX_KV);
                let items = items.map(|item| {
                    let item = item.as_object()?;
                    Some((line(item, "key")?, line(item, "value")?))
                });
                Node::Kv(items.collect::<Option<_>>()?)
            }
            "menu" => {
                let items = o.get("items")?.as_array()?.iter().take(MAX_MENU);
                let items = items.map(|item| {
                    let item = item.as_object()?;
                    Some(MenuItem {
                        id: id(item)?,
                        label: line(item, "label")?,
                        danger: flag(item, "danger").unwrap_or(false),
                        confirm: confirm(item)?,
                    })
                });
                Node::Menu {
                    label: line(o, "label"),
                    icon: icon(o),
                    items: items.collect::<Option<_>>()?,
                }
            }
            "pixels" => {
                let rows = o.get("rows")?.as_array()?;
                let rows = rows
                    .iter()
                    .take(PIXEL_ROWS)
                    .map(|r| {
                        let cells = r.as_str()?.chars().take(PIXEL_COLS);
                        Some(
                            cells
                                .map(|c| match c {
                                    '#' => 2,
                                    '+' => 1,
                                    _ => 0,
                                })
                                .collect::<Vec<u8>>(),
                        )
                    })
                    .collect::<Option<Vec<_>>>()?;
                let width = rows.iter().map(Vec::len).max()?;
                if width == 0 {
                    return None;
                }
                Node::Pixels {
                    id: match o.contains_key("id") {
                        true => Some(id(o)?),
                        false => None,
                    },
                    rows: rows
                        .into_iter()
                        .map(|mut r| {
                            r.resize(width, 0);
                            r.into_boxed_slice()
                        })
                        .collect(),
                    width,
                    scale: o
                        .get("scale")
                        .and_then(Value::as_f64)
                        .map_or(2, |s| s.clamp(1., 8.) as u8),
                }
            }
            "scene" => Node::Scene(Box::new(scene(o)?)),
            _ => return None,
        };
        // Only a node that is otherwise whole takes its id.
        match &node {
            Node::Bubble { id, .. }
            | Node::Button { id, .. }
            | Node::Input { id, .. }
            | Node::Select { id, .. }
            | Node::Path { id, .. }
            | Node::Checkbox { id, .. } => self.ids.insert(id.clone()).then_some(node),
            Node::Pixels { id: Some(id), .. } => self.ids.insert(id.clone()).then_some(node),
            Node::Scene(scene) => self.ids.insert(scene.id.clone()).then_some(node),
            _ => Some(node),
        }
    }
}

/// `#rgb` or `#rrggbb`.
fn hex(s: &str) -> Option<[u8; 3]> {
    let h = s.strip_prefix('#')?;
    if !h.is_ascii() || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let n = |i: usize, len: usize| u8::from_str_radix(&h[i..i + len], 16).ok();
    match h.len() {
        3 => Some([n(0, 1)? * 17, n(1, 1)? * 17, n(2, 1)? * 17]),
        6 => Some([n(0, 2)?, n(2, 2)?, n(4, 2)?]),
        _ => None,
    }
}

/// One character, the key of a palette or legend entry.
fn key(k: &str) -> Option<char> {
    let mut chars = k.chars();
    chars.next().filter(|_| chars.next().is_none())
}

/// Rows of at most `SCENE_PATTERN_SIDE` characters, padded to the longest.
fn pattern(value: &Value) -> Option<Pattern> {
    let rows = value.as_array()?;
    if rows.is_empty() || rows.len() > SCENE_PATTERN_SIDE {
        return None;
    }
    let mut rows = rows
        .iter()
        .map(|r| Some(r.as_str()?.chars().collect::<Vec<_>>()))
        .collect::<Option<Vec<_>>>()?;
    let width = rows.iter().map(Vec::len).max()?;
    if width == 0 || width > SCENE_PATTERN_SIDE {
        return None;
    }
    rows.iter_mut().for_each(|r| r.resize(width, ' '));
    Some(Pattern { width, rows })
}

fn scene(o: &Map<String, Value>) -> Option<Scene> {
    let scene_id = id(o)?;
    let cell = o.get("cell")?.as_u64()? as usize;
    if !(4..=SCENE_PATTERN_SIDE).contains(&cell) {
        return None;
    }
    let scale = match o.get("scale").and_then(Value::as_u64) {
        Some(s) => u8::try_from(s).ok().filter(|s| (1..=4).contains(s))?,
        None => 2,
    };
    let palette = o.get("palette")?.as_object()?;
    if palette.len() > SCENE_PALETTE {
        return None;
    }
    let palette = (palette.iter())
        .map(|(k, v)| Some((key(k)?, hex(v.as_str()?)?)))
        .collect::<Option<BTreeMap<_, _>>>()?;
    let patterns = o.get("patterns")?.as_object()?;
    if patterns.len() > SCENE_PATTERNS {
        return None;
    }
    let patterns = (patterns.iter())
        .map(|(k, v)| Some((k.clone(), pattern(v)?)))
        .collect::<Option<BTreeMap<_, _>>>()?;
    let legend = (o.get("legend")?.as_object()?.iter())
        .map(|(k, v)| Some((key(k)?, v.as_str()?.to_owned())))
        .collect::<Option<BTreeMap<_, _>>>()?;
    // A tile is exactly `cell` by `cell`.
    let tile = |p: &String| {
        patterns
            .get(p)
            .is_some_and(|p| p.width == cell && p.rows.len() == cell)
    };
    if !legend.values().all(tile) {
        return None;
    }
    let mut map = (o.get("map")?.as_array()?.iter())
        .map(|r| Some(r.as_str()?.chars().collect::<Vec<_>>()))
        .collect::<Option<Vec<_>>>()?;
    let width = map.iter().map(Vec::len).max()?;
    if map.len() > SCENE_MAP_SIDE || width == 0 || width > SCENE_MAP_SIDE {
        return None;
    }
    map.iter_mut().for_each(|r| r.resize(width, ' '));
    let sprites = o.get("sprites")?.as_array()?;
    if sprites.len() > SCENE_SPRITES {
        return None;
    }
    let mut seen = HashSet::new();
    let sprites = sprites
        .iter()
        .map(|s| {
            let s = s.as_object()?;
            let sprite = Sprite {
                id: id(s)?,
                x: s.get("x")?.as_u64()? as usize,
                y: s.get("y")?.as_u64()? as usize,
                pattern: str_field(s, "pattern")?.to_owned(),
                label: sized(s, "label", SCENE_LABEL)?,
                badge: sized(s, "badge", SCENE_BADGE)?,
                tone: tone(s).unwrap_or_default(),
            };
            let inside = sprite.x < width && sprite.y < map.len();
            (inside && patterns.contains_key(&sprite.pattern) && seen.insert(sprite.id.clone()))
                .then_some(sprite)
        })
        .collect::<Option<Vec<_>>>()?;
    let follow = str_field(o, "follow")
        .filter(|f| sprites.iter().any(|s| s.id == *f))
        .map(str::to_owned);
    let margin = (o.get("margin").and_then(Value::as_u64)).map_or(3, |m| m.min(16) as u8);
    Some(Scene {
        id: scene_id,
        cell,
        scale,
        palette,
        patterns,
        legend,
        map,
        width,
        sprites,
        follow,
        margin,
    })
}

/// An optional line of at most `max` characters; `None` when it is there and longer.
fn sized(o: &Map<String, Value>, key: &str, max: usize) -> Option<Option<Arc<str>>> {
    match str_field(o, key) {
        Some(s) if s.chars().count() > max => None,
        s => Some(s.map(Arc::from)),
    }
}

fn item(value: &Value) -> Option<Item> {
    let o = value.as_object()?;
    Some(Item {
        id: id(o)?,
        title: line(o, "title")?,
        subtitle: line(o, "subtitle"),
        badge: line(o, "badge"),
        icon: icon(o),
        tone: tone(o).unwrap_or_default(),
        selected: flag(o, "selected").unwrap_or(false),
        lead: line(o, "lead"),
        note: line(o, "note"),
        indent: flag(o, "indent").unwrap_or(false),
    })
}

fn str_field<'a>(o: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    o.get(key)?.as_str()
}

fn flag(o: &Map<String, Value>, key: &str) -> Option<bool> {
    o.get(key)?.as_bool()
}

fn line(o: &Map<String, Value>, key: &str) -> Option<Arc<str>> {
    str_field(o, key).map(|s| clamp(s, LINE_CHARS).into())
}

fn id(o: &Map<String, Value>) -> Option<String> {
    let id = str_field(o, "id").filter(|id| (1..=MAX_ID).contains(&id.len()));
    id.map(str::to_owned)
}

/// An `http://` or `https://` URL with something after the scheme, on one line, at most `MAX_URL`
/// bytes.
pub fn web_url(url: &str) -> bool {
    let start = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    let scheme = ["https://", "http://"]
        .into_iter()
        .find(|s| start.starts_with(s));
    url.len() <= MAX_URL
        && scheme.is_some_and(|s| url.len() > s.len())
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// An optional `icon`: absent unless it is a name's shape.
fn icon(o: &Map<String, Value>) -> Option<Arc<str>> {
    str_field(o, "icon").filter(|n| icon_name(n)).map(Arc::from)
}

/// `tone`, if it is one.
fn tone(o: &Map<String, Value>) -> Option<Tone> {
    Some(match str_field(o, "tone")? {
        "neutral" => Tone::Neutral,
        "info" => Tone::Info,
        "success" => Tone::Success,
        "warning" => Tone::Warning,
        "danger" => Tone::Danger,
        _ => return None,
    })
}

/// `Some(None)` without a `confirm` (or a null one); `None` when it is there but broken, which
/// breaks its node rather than dropping the question.
fn confirm(o: &Map<String, Value>) -> Option<Option<Confirm>> {
    let confirm = match o.get("confirm") {
        None | Some(Value::Null) => return Some(None),
        Some(confirm) => confirm.as_object()?,
    };
    Some(Some(Confirm {
        title: line(confirm, "title")?,
        text: str_field(confirm, "text").map(|t| cut(t, TEXT_CHARS).into()),
        label: line(confirm, "confirm_label"),
    }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn pixels_parse() {
        let Ok(Node::Pixels {
            id,
            rows,
            width,
            scale,
        }) = parse(&json!({"type": "pixels", "rows": ["#+", "..#", "é#"]}))
        else {
            panic!()
        };
        assert_eq!((id, width, scale), (None, 3, 2));
        assert_eq!(
            &*rows,
            &[
                vec![2, 1, 0].into(),
                vec![0, 0, 2].into(),
                vec![0, 2, 0].into()
            ]
        );
        let scale = |s: Value| match parse(&json!({"type": "pixels", "rows": ["#"], "scale": s})) {
            Ok(Node::Pixels { scale, .. }) => scale,
            n => panic!("{n:?}"),
        };
        assert_eq!(
            (scale(json!(0)), scale(json!(3)), scale(json!(99))),
            (1, 3, 8)
        );
        let long = parse(&json!({"type": "pixels", "rows": vec!["é".repeat(300); 70]}));
        let Ok(Node::Pixels { rows, width, .. }) = long else {
            panic!()
        };
        assert_eq!((rows.len(), width), (PIXEL_ROWS, PIXEL_COLS));
        for bad in [
            json!({"type": "pixels"}),
            json!({"type": "pixels", "rows": []}),
            json!({"type": "pixels", "rows": [""]}),
            json!({"type": "pixels", "rows": [3]}),
        ] {
            assert_eq!(parse(&bad), Ok(Node::Unsupported), "{bad}");
        }
        // The id shares the set of buttons, fields, sections and bubbles.
        let Ok(Node::Column { children, .. }) = parse(&json!({"type": "column", "children": [
            {"type": "button", "id": "pet", "label": "Pet"},
            {"type": "pixels", "id": "pet", "rows": ["#"]},
        ]})) else {
            panic!()
        };
        assert_eq!(children[1], Node::Unsupported);
    }

    fn scene_json() -> Value {
        let tile = vec!["ab".repeat(2); 4];
        json!({
            "type": "scene", "id": "office", "cell": 4, "scale": 3,
            "palette": {"a": "#f00", "b": "#00ff80"},
            "patterns": {"floor": tile, "ana": ["  a", "bbb"]},
            "legend": {".": "floor"},
            "map": ["...", ".."],
            "sprites": [
                {"id": "u1", "x": 2, "y": 1, "pattern": "ana", "label": "Ana", "badge": "Focused", "tone": "info"},
                {"id": "u2", "x": 0, "y": 0, "pattern": "ana"},
            ],
            "follow": "u1", "margin": 5,
        })
    }

    fn scene_with(edit: impl FnOnce(&mut Value)) -> Result<Node, TooLarge> {
        let mut v = scene_json();
        edit(&mut v);
        parse(&v)
    }

    #[test]
    fn scene_parse() {
        let Ok(Node::Scene(s)) = parse(&scene_json()) else {
            panic!()
        };
        assert_eq!(
            (s.id.as_str(), s.cell, s.scale, s.margin),
            ("office", 4, 3, 5)
        );
        assert_eq!(s.palette[&'a'], [255, 0, 0]);
        assert_eq!(s.palette[&'b'], [0, 255, 128]);
        assert_eq!((s.width, s.map.len()), (3, 2));
        assert_eq!(s.map[1], vec!['.', '.', ' ']);
        assert_eq!(s.patterns["ana"].rows[0], vec![' ', ' ', 'a']);
        assert_eq!(s.legend[&'.'], "floor");
        assert_eq!(s.follow.as_deref(), Some("u1"));
        assert_eq!(s.sprites.len(), 2);
        let ana = &s.sprites[0];
        assert_eq!((ana.x, ana.y, ana.tone), (2, 1, Tone::Info));
        assert_eq!(ana.label.as_deref(), Some("Ana"));
        assert_eq!(ana.badge.as_deref(), Some("Focused"));
        // Defaults.
        let Ok(Node::Scene(s)) = scene_with(|v| {
            let o = v.as_object_mut().unwrap();
            o.remove("scale");
            o.remove("margin");
            o.remove("follow");
        }) else {
            panic!()
        };
        assert_eq!((s.scale, s.margin, s.follow), (2, 3, None));
        // A scene shares the panel's id set.
        let Ok(Node::Column { children, .. }) = parse(&json!({"type": "column", "children": [
            {"type": "button", "id": "office", "label": "x"}, scene_json()]}))
        else {
            panic!()
        };
        assert_eq!(children[1], Node::Unsupported);
    }

    #[test]
    fn scene_rules_unsupported() {
        let sq = |n: usize| vec!["a".repeat(n); n];
        let cases: Vec<(&str, Box<dyn FnOnce(&mut Value)>)> = vec![
            (
                "missing id",
                Box::new(|v| drop(v.as_object_mut().unwrap().remove("id"))),
            ),
            ("cell 3", Box::new(|v| v["cell"] = json!(3))),
            ("cell 33", Box::new(|v| v["cell"] = json!(33))),
            ("scale 5", Box::new(|v| v["scale"] = json!(5))),
            ("bad hex", Box::new(|v| v["palette"]["a"] = json!("red"))),
            (
                "65 palette keys",
                Box::new(|v| {
                    let p = (0..65).map(|i| {
                        (
                            char::from_u32(0x100 + i).unwrap().to_string(),
                            json!("#000"),
                        )
                    });
                    v["palette"] = Value::Object(p.collect());
                }),
            ),
            (
                "33x33 pattern",
                Box::new(move |v| v["patterns"]["big"] = json!(sq(33))),
            ),
            (
                "tile not cell x cell",
                Box::new(|v| v["patterns"]["floor"] = json!(vec!["aaaa"; 3])),
            ),
            ("129 rows", Box::new(|v| v["map"] = json!(vec!["."; 129]))),
            (
                "101 sprites",
                Box::new(|v| {
                    let s = (0..101)
                        .map(|i| json!({"id": format!("s{i}"), "x": 0, "y": 0, "pattern": "ana"}));
                    v["sprites"] = Value::Array(s.collect());
                }),
            ),
            (
                "sprite outside map",
                Box::new(|v| v["sprites"][0]["x"] = json!(3)),
            ),
            (
                "unknown pattern",
                Box::new(|v| v["sprites"][0]["pattern"] = json!("nope")),
            ),
            (
                "unknown legend pattern",
                Box::new(|v| v["legend"]["."] = json!("nope")),
            ),
            (
                "repeated sprite id",
                Box::new(|v| v["sprites"][1]["id"] = json!("u1")),
            ),
            (
                "label 33 chars",
                Box::new(|v| v["sprites"][0]["label"] = json!("é".repeat(33))),
            ),
        ];
        for (name, edit) in cases {
            assert_eq!(scene_with(edit), Ok(Node::Unsupported), "{name}");
        }
        // The limits themselves are fine.
        let Ok(Node::Scene(_)) = scene_with(|v| {
            v["sprites"][0]["label"] = json!("é".repeat(32));
            v["map"] = json!(vec!["...".repeat(42); 128]);
        }) else {
            panic!()
        };
    }

    #[test]
    fn scene_not_in_card() {
        assert_eq!(card(&scene_json()), Some(Node::Unsupported));
    }

    fn col(children: Vec<Node>) -> Node {
        Node::Column {
            children,
            width: None,
            framed: false,
            title: None,
            badge: None,
            tone: Tone::Neutral,
            collapsed: false,
        }
    }

    fn row(children: Vec<Node>) -> Node {
        Node::Row {
            children,
            fill: false,
        }
    }

    use serde_json::{Map, Value, json};

    use super::*;

    #[test]
    fn bubble_parses() {
        let tree = parse(
            &json!({"type": "bubble", "id": "say", "text": "Oi", "tone": "success", "sticky": true}),
        );
        assert_eq!(
            tree,
            Ok(Node::Bubble {
                id: "say".into(),
                text: "Oi".into(),
                tone: Tone::Success,
                sticky: true
            })
        );
        let plain = parse(&json!({"type": "bubble", "id": "say", "text": "Oi"}));
        assert_eq!(
            plain,
            Ok(Node::Bubble {
                id: "say".into(),
                text: "Oi".into(),
                tone: Tone::Neutral,
                sticky: false
            })
        );
    }

    #[test]
    fn bubble_needs_id_and_text_and_is_cut() {
        assert_eq!(
            parse(&json!({"type": "bubble", "text": "Oi"})),
            Ok(Node::Unsupported)
        );
        assert_eq!(
            parse(&json!({"type": "bubble", "id": "say"})),
            Ok(Node::Unsupported)
        );
        let Ok(Node::Bubble { text, .. }) =
            parse(&json!({"type": "bubble", "id": "b", "text": "x".repeat(300)}))
        else {
            panic!()
        };
        assert_eq!(text.chars().count(), BUBBLE_CHARS);
        assert!(text.ends_with('…'));
    }

    #[test]
    fn bubble_id_joins_the_id_set() {
        let tree = parse(&json!({"type": "column", "children": [
            {"type": "button", "id": "x", "label": "B"},
            {"type": "bubble", "id": "x", "text": "Oi"},
        ]}));
        let Ok(Node::Column { children, .. }) = tree else {
            panic!()
        };
        assert_eq!(children[1], Node::Unsupported);
    }

    #[test]
    fn item_state_is_clamped() {
        let long = "x".repeat(40);
        let state = item_state(&json!({"text": long}));
        let text = state.text.unwrap();
        assert_eq!(text.chars().count(), ITEM_TEXT_CHARS);
        assert!(text.ends_with('…'));
        assert_eq!(item_state(&json!({"icon": "Bad Name"})).icon, None);
        assert_eq!(item_state(&json!({"tone": "loud"})).tone, Tone::Neutral);
        assert!(!item_state(&json!({"text": ""})).shown());
        assert_eq!(item_state(&json!(5)), ItemState::default());
    }

    fn text(s: &str) -> Node {
        Node::Text {
            text: s.into(),
            muted: false,
            mono: false,
            style: TextStyle::Plain,
        }
    }

    /// `n` columns, each the only child of the last, around a text: depth `n + 1`.
    /// Built by moving, not with `json!`, which deep-copies an interpolated `Value`.
    fn nested(n: usize) -> Value {
        let mut tree = json!({"type": "text", "text": "leaf"});
        for _ in 0..n {
            let mut column = Map::new();
            column.insert("type".into(), "column".into());
            column.insert("children".into(), Value::Array(vec![tree]));
            tree = Value::Object(column);
        }
        tree
    }

    /// Takes a tree apart without recursion: dropping a very deep `Value` overflows the stack.
    fn drop_deep(tree: Value) {
        let mut stack = vec![tree];
        while let Some(value) = stack.pop() {
            match value {
                Value::Array(values) => stack.extend(values),
                Value::Object(map) => stack.extend(map.into_iter().map(|(_, v)| v)),
                _ => {}
            }
        }
    }

    fn column(children: Vec<Value>) -> Value {
        json!({"type": "column", "children": children})
    }

    fn children(tree: Value) -> Vec<Node> {
        match parse(&tree) {
            Ok(Node::Column { children, .. }) => children,
            other => panic!("not a column: {other:?}"),
        }
    }

    #[test]
    fn schema_parses_and_limits() {
        let tree = json!({"type": "column", "children": [
            {"type": "row", "children": [
                {"type": "text", "text": "Hello", "muted": true},
                {"type": "badge", "text": "3"},
            ]},
            {"type": "markdown", "text": "# Title\n\n- a"},
            {"type": "list", "fill": true, "items": [
                {"id": "a", "title": "First", "subtitle": "sub", "badge": "new"},
                {"id": "b", "title": "Second"},
            ]},
            {"type": "button", "id": "go", "label": "Go", "style": "primary"},
            {"type": "button", "id": "rm", "label": "Remove", "style": "danger"},
            {"type": "button", "id": "plain", "label": "Plain"},
            {"type": "input", "id": "q", "placeholder": "Search", "value": "rust"},
            {"type": "select", "id": "scope", "options": ["Mine", "All"], "value": "All"},
            {"type": "checkbox", "id": "done", "label": "Done", "checked": true},
            {"type": "input", "id": "q2", "placeholder": "No value"},
            {"type": "select", "id": "s2", "options": ["Mine"]},
            {"type": "checkbox", "id": "c2", "label": "Unchecked"},
            {"type": "progress", "value": 0.25},
        ]});
        let expected = col(vec![
            row(vec![
                Node::Text {
                    text: "Hello".into(),
                    muted: true,
                    mono: false,
                    style: TextStyle::Plain,
                },
                Node::Badge {
                    text: "3".into(),
                    tone: Tone::Neutral,
                },
            ]),
            Node::Markdown("# Title\n\n- a".into()),
            Node::List {
                items: vec![
                    Some(Item {
                        id: "a".into(),
                        title: "First".into(),
                        subtitle: Some("sub".into()),
                        badge: Some("new".into()),
                        icon: None,
                        tone: Tone::Neutral,
                        selected: false,
                        lead: None,
                        note: None,
                        indent: false,
                    }),
                    Some(Item {
                        id: "b".into(),
                        title: "Second".into(),
                        subtitle: None,
                        badge: None,
                        icon: None,
                        tone: Tone::Neutral,
                        selected: false,
                        lead: None,
                        note: None,
                        indent: false,
                    }),
                ]
                .into(),
                subtitles: true,
                fill: true,
                plain: false,
            },
            Node::Button {
                id: "go".into(),
                label: "Go".into(),
                style: ButtonStyle::Primary,
                icon: None,
                disabled: false,
                confirm: None,
                icon_only: false,
            },
            Node::Button {
                id: "rm".into(),
                label: "Remove".into(),
                style: ButtonStyle::Danger,
                icon: None,
                disabled: false,
                confirm: None,
                icon_only: false,
            },
            Node::Button {
                id: "plain".into(),
                label: "Plain".into(),
                style: ButtonStyle::Default,
                icon: None,
                disabled: false,
                confirm: None,
                icon_only: false,
            },
            Node::Input {
                id: "q".into(),
                placeholder: "Search".into(),
                value: Some("rust".into()),
                rows: None,
                icon: None,
                submit: false,
                clear: false,
            },
            Node::Select {
                id: "scope".into(),
                options: vec!["Mine".into(), "All".into()],
                value: Some("All".into()),
                segmented: false,
            },
            Node::Checkbox {
                id: "done".into(),
                label: "Done".into(),
                checked: Some(true),
            },
            Node::Input {
                id: "q2".into(),
                placeholder: "No value".into(),
                value: None,
                rows: None,
                icon: None,
                submit: false,
                clear: false,
            },
            Node::Select {
                id: "s2".into(),
                options: vec!["Mine".into()],
                value: None,
                segmented: false,
            },
            Node::Checkbox {
                id: "c2".into(),
                label: "Unchecked".into(),
                checked: None,
            },
            Node::Progress(0.25),
        ]);
        assert_eq!(parse(&tree), Ok(expected));

        // Depth: the root is 1.
        assert!(parse(&nested(MAX_DEPTH - 1)).is_ok());
        assert_eq!(parse(&nested(MAX_DEPTH)), Err(TooLarge));
        // An empty container at the deepest level is fine; anything inside it is too deep.
        let mut deepest = json!({"type": "row", "children": []});
        for _ in 1..MAX_DEPTH {
            deepest = column(vec![deepest]);
        }
        assert!(parse(&deepest).is_ok());

        // Nodes: the root and every child, whatever it is.
        let texts = |n: usize| column(vec![json!({"type": "text", "text": "x"}); n]);
        assert!(parse(&texts(MAX_NODES - 1)).is_ok());
        assert_eq!(parse(&texts(MAX_NODES)), Err(TooLarge));
        assert_eq!(parse(&column(vec![json!(1); MAX_NODES])), Err(TooLarge));
        // A list's items are not nodes.
        let items = vec![json!({"id": "i", "title": "t"}); 10 * MAX_NODES];
        let list = parse(&json!({"type": "list", "items": items}));
        assert!(
            matches!(list, Ok(Node::List { items, subtitles: false, fill: false, .. }) if items.len() == 10 * MAX_NODES)
        );
    }

    #[test]
    fn deep_nesting_is_too_large_without_overflow() {
        let tree = nested(100_000);
        assert_eq!(parse(&tree), Err(TooLarge));
        drop_deep(tree);
        // Arrays in arrays are nodes too.
        let mut arrays = json!([]);
        for _ in 0..100_000 {
            arrays = Value::Array(vec![arrays]);
        }
        let mut tree = json!({"type": "column"});
        tree["children"] = arrays;
        assert_eq!(parse(&tree), Ok(col(vec![Node::Unsupported])));
        drop_deep(tree);
    }

    #[test]
    fn unknown_and_non_object_nodes_are_unsupported() {
        let tree = column(vec![
            json!({"type": "chart", "data": [1, 2]}),
            json!({"text": "no type"}),
            json!({"type": 7}),
            json!(null),
            json!(42),
            json!("text"),
            json!([{"type": "text", "text": "in an array"}]),
            json!({"type": "text", "text": "kept"}),
        ]);
        let mut expected = vec![Node::Unsupported; 7];
        expected.push(text("kept"));
        assert_eq!(children(tree), expected);
        assert_eq!(parse(&json!("root")), Ok(Node::Unsupported));
        assert_eq!(parse(&json!([])), Ok(Node::Unsupported));
    }

    #[test]
    fn missing_or_wrong_required_fields_are_unsupported() {
        let broken = [
            json!({"type": "column"}),
            json!({"type": "row", "children": {"a": 1}}),
            json!({"type": "text"}),
            json!({"type": "text", "text": 5}),
            json!({"type": "markdown", "text": ["a"]}),
            json!({"type": "list"}),
            json!({"type": "list", "items": "a"}),
            json!({"type": "button", "label": "No id"}),
            json!({"type": "button", "id": "b", "label": 1}),
            json!({"type": "button", "id": 3, "label": "Numeric id"}),
            json!({"type": "button", "id": "", "label": "Empty id"}),
            json!({"type": "button", "id": "x".repeat(MAX_ID + 1), "label": "Long id"}),
            json!({"type": "input", "id": "i"}),
            json!({"type": "input", "placeholder": "p"}),
            json!({"type": "select", "id": "s"}),
            json!({"type": "select", "id": "s2", "options": ["a", 2]}),
            json!({"type": "checkbox", "id": "c"}),
            json!({"type": "checkbox", "label": "c"}),
            json!({"type": "badge"}),
            json!({"type": "progress"}),
            json!({"type": "progress", "value": "half"}),
        ];
        let n = broken.len();
        assert_eq!(children(column(broken.into())), vec![Node::Unsupported; n]);
    }

    #[test]
    fn wrong_optional_fields_are_ignored() {
        let tree = column(vec![
            json!({"type": "text", "text": "t", "muted": "yes", "extra": {"deep": [1]}}),
            json!({"type": "button", "id": "b", "label": "B", "style": 9}),
            json!({"type": "button", "id": "c", "label": "C", "style": "huge"}),
            json!({"type": "list", "fill": "yes", "items": [{"id": "a", "title": "A", "subtitle": 1, "badge": []}]}),
            json!({"type": "progress", "value": 7}),
            json!({"type": "progress", "value": -1}),
            json!({"type": "text", "id": 12, "text": "ids of other nodes are ignored"}),
            json!({"type": "input", "id": "i", "placeholder": "P", "value": 5}),
            json!({"type": "select", "id": "s", "options": ["a"], "value": ["a"]}),
            json!({"type": "checkbox", "id": "c2", "label": "L", "checked": "true"}),
            json!({"type": "input", "id": "j", "placeholder": "P", "value": "two\nlines"}),
        ]);
        let button = |id: &str, label: &str| Node::Button {
            id: id.into(),
            label: label.into(),
            style: ButtonStyle::Default,
            icon: None,
            disabled: false,
            confirm: None,
            icon_only: false,
        };
        let item = Item {
            id: "a".into(),
            title: "A".into(),
            subtitle: None,
            badge: None,
            icon: None,
            tone: Tone::Neutral,
            selected: false,
            lead: None,
            note: None,
            indent: false,
        };
        assert_eq!(
            children(tree),
            vec![
                text("t"),
                button("b", "B"),
                button("c", "C"),
                Node::List {
                    items: vec![Some(item)].into(),
                    subtitles: false,
                    fill: false,
                    plain: false,
                },
                Node::Progress(1.),
                Node::Progress(0.),
                text("ids of other nodes are ignored"),
                Node::Input {
                    id: "i".into(),
                    placeholder: "P".into(),
                    value: None,
                    rows: None,
                    icon: None,
                    submit: false,
                    clear: false,
                },
                Node::Select {
                    id: "s".into(),
                    options: vec!["a".into()],
                    value: None,
                    segmented: false,
                },
                Node::Checkbox {
                    id: "c2".into(),
                    label: "L".into(),
                    checked: None,
                },
                // An input holds one line.
                Node::Input {
                    id: "j".into(),
                    placeholder: "P".into(),
                    value: Some("two lines".into()),
                    rows: None,
                    icon: None,
                    submit: false,
                    clear: false,
                },
            ]
        );
    }

    #[test]
    fn broken_list_items_are_none() {
        let tree = json!({"type": "list", "items": [
            {"id": "a", "title": "A"},
            {"title": "no id"},
            {"id": "b"},
            {"id": "c", "title": 3},
            "a string",
            {"id": "a", "title": "A again"},
        ]});
        let Ok(Node::List { items, .. }) = parse(&tree) else {
            panic!("not a list")
        };
        let ids: Vec<_> = items.iter().map(|i| i.as_ref().map(|i| &*i.id)).collect();
        assert_eq!(ids, [Some("a"), None, None, None, None, Some("a")]);
    }

    #[test]
    fn repeated_ids_are_unsupported() {
        let tree = column(vec![
            json!({"type": "button", "id": "x", "label": "First"}),
            json!({"type": "input", "id": "x", "placeholder": "Same id"}),
            json!({"type": "checkbox", "id": "x", "label": "Same id"}),
            // A broken node does not take its id.
            json!({"type": "select", "id": "y"}),
            json!({"type": "select", "id": "y", "options": []}),
            json!({"type": "row", "children": [{"type": "button", "id": "y", "label": "Nested"}]}),
        ]);
        assert_eq!(
            children(tree),
            vec![
                Node::Button {
                    id: "x".into(),
                    label: "First".into(),
                    style: ButtonStyle::Default,
                    icon: None,
                    disabled: false,
                    confirm: None,
                    icon_only: false,
                },
                Node::Unsupported,
                Node::Unsupported,
                Node::Unsupported,
                Node::Select {
                    id: "y".into(),
                    options: vec![],
                    value: None,
                    segmented: false,
                },
                row(vec![Node::Unsupported]),
            ]
        );
    }

    #[test]
    fn huge_strings_are_cut() {
        let huge = "é\n".repeat(1 << 20);
        let options: Vec<_> = (0..MAX_OPTIONS + 50).map(|n| n.to_string()).collect();
        let tree = column(vec![
            json!({"type": "text", "text": huge}),
            json!({"type": "markdown", "text": huge}),
            json!({"type": "list", "items": [{"id": "a", "title": huge, "subtitle": huge, "badge": huge}]}),
            json!({"type": "button", "id": "b", "label": huge}),
            json!({"type": "input", "id": "i", "placeholder": huge, "value": huge}),
            json!({"type": "select", "id": "s", "options": options}),
            json!({"type": "checkbox", "id": "c", "label": huge}),
            json!({"type": "badge", "text": huge}),
        ]);
        let chars = |s: &str| s.chars().count();
        let line = |s: &str| {
            assert_eq!(chars(s), LINE_CHARS + 1);
            assert!(s.ends_with('…') && !s.contains('\n'));
        };
        let children = children(tree);
        for node in &children {
            match node {
                Node::Text { text, .. } | Node::Markdown(text) => {
                    assert_eq!(chars(text), TEXT_CHARS + 1);
                    assert!(text.contains('\n'), "text keeps its lines");
                }
                Node::List { items, .. } => {
                    let item = items[0].as_ref().unwrap();
                    line(&item.title);
                    line(item.subtitle.as_deref().unwrap());
                    line(item.badge.as_deref().unwrap());
                }
                Node::Button { label, .. } | Node::Checkbox { label, .. } => line(label),
                Node::Input {
                    placeholder, value, ..
                } => {
                    line(placeholder);
                    line(value.as_deref().unwrap());
                }
                Node::Badge { text, .. } => line(text),
                Node::Select { options, .. } => assert_eq!(options.len(), MAX_OPTIONS),
                other => panic!("unexpected {other:?}"),
            }
        }
        assert_eq!(children.len(), 8);
    }

    #[test]
    fn clamp_collapses_lines_and_cuts() {
        assert_eq!(clamp("  a\n\n b\t\u{7}c  ", 80), "a b c");
        let long = clamp(&"é".repeat(100), 80);
        assert_eq!(long.chars().count(), 81);
        assert!(long.ends_with('…'));
        assert_eq!(clamp("exactly", 7), "exactly");
        assert_eq!(cut("a\nb", 2), "a\n…");
        assert_eq!(cut("ab", 2), "ab");
    }

    fn button(id: &str, label: &str) -> Node {
        Node::Button {
            id: id.into(),
            label: label.into(),
            style: ButtonStyle::Default,
            icon: None,
            disabled: false,
            confirm: None,
            icon_only: false,
        }
    }

    #[test]
    fn new_nodes_parse() {
        let confirm = json!({"title": "Restore?", "text": "It replaces the database.", "confirm_label": "Restore"});
        let tree = column(vec![
            json!({"type": "card", "id": "pr-1", "title": "Fix login", "subtitle": "wai#12", "badge": "open", "icon": "git-pull-request", "children": [
                {"type": "text", "text": "body", "mono": true},
            ]}),
            json!({"type": "card"}),
            json!({"type": "section", "id": "more", "title": "More", "open": false, "badge": "2", "children": [
                {"type": "divider"},
                {"type": "spacer"},
            ]}),
            json!({"type": "table", "fill": true, "columns": [{"title": "Repo", "width": 120}, {"title": "Title"}], "rows": [
                {"id": "r1", "cells": ["wai", {"type": "link", "text": "Fix", "url": "https://example.com/pr/1"}]},
                {"cells": [{"type": "badge", "text": "CI", "tone": "success"}]},
            ]}),
            json!({"type": "log", "text": "a\nb", "follow": false}),
            json!({"type": "alert", "tone": "warning", "title": "Careful", "text": "Two\nlines"}),
            json!({"type": "icon", "name": "database", "tone": "danger"}),
            json!({"type": "spinner", "label": "Loading"}),
            json!({"type": "spinner"}),
            json!({"type": "kv", "items": [{"key": "Branch", "value": "main"}]}),
            json!({"type": "menu", "items": [
                {"id": "open", "label": "Open"},
                {"id": "close", "label": "Close", "danger": true, "confirm": confirm},
            ]}),
            json!({"type": "menu", "label": "More", "icon": "chevron-down", "items": []}),
            json!({"type": "button", "id": "rm", "label": "Remove", "style": "danger", "icon": "trash-2", "disabled": true, "confirm": confirm}),
            json!({"type": "input", "id": "notes", "placeholder": "Notes", "multiline": true, "value": "a\nb"}),
            json!({"type": "input", "id": "big", "placeholder": "Big", "multiline": true, "rows": 99}),
            json!({"type": "badge", "text": "late", "tone": "danger"}),
        ]);
        let confirm = Confirm {
            title: "Restore?".into(),
            text: Some("It replaces the database.".into()),
            label: Some("Restore".into()),
        };
        let expected = vec![
            Node::Card {
                id: Some("pr-1".into()),
                title: Some("Fix login".into()),
                subtitle: Some("wai#12".into()),
                badge: Some("open".into()),
                icon: Some("git-pull-request".into()),
                selected: false,
                children: vec![Node::Text {
                    text: "body".into(),
                    muted: false,
                    mono: true,
                    style: TextStyle::Plain,
                }],
            },
            Node::Card {
                id: None,
                title: None,
                subtitle: None,
                badge: None,
                icon: None,
                selected: false,
                children: vec![],
            },
            Node::Section {
                id: "more".into(),
                title: "More".into(),
                open: Some(false),
                badge: Some("2".into()),
                children: vec![Node::Divider, Node::Spacer],
            },
            Node::Table {
                columns: vec![
                    Column {
                        title: "Repo".into(),
                        width: Some(120.),
                    },
                    Column {
                        title: "Title".into(),
                        width: None,
                    },
                ],
                rows: vec![
                    Some(TableRow {
                        id: Some("r1".into()),
                        cells: vec![
                            text("wai"),
                            Node::Link {
                                text: "Fix".into(),
                                url: "https://example.com/pr/1".into(),
                                icon: None,
                                icon_only: false,
                            },
                        ],
                    }),
                    Some(TableRow {
                        id: None,
                        cells: vec![Node::Badge {
                            text: "CI".into(),
                            tone: Tone::Success,
                        }],
                    }),
                ]
                .into(),
                fill: true,
            },
            Node::Log {
                text: "a\nb".into(),
                follow: false,
                fill: false,
            },
            Node::Alert {
                tone: Tone::Warning,
                title: Some("Careful".into()),
                text: "Two\nlines".into(),
            },
            Node::Icon {
                name: "database".into(),
                tone: Tone::Danger,
            },
            Node::Spinner(Some("Loading".into())),
            Node::Spinner(None),
            Node::Kv(vec![("Branch".into(), "main".into())].into()),
            Node::Menu {
                label: None,
                icon: None,
                items: vec![
                    MenuItem {
                        id: "open".into(),
                        label: "Open".into(),
                        danger: false,
                        confirm: None,
                    },
                    MenuItem {
                        id: "close".into(),
                        label: "Close".into(),
                        danger: true,
                        confirm: Some(confirm.clone()),
                    },
                ],
            },
            Node::Menu {
                label: Some("More".into()),
                icon: Some("chevron-down".into()),
                items: vec![],
            },
            Node::Button {
                id: "rm".into(),
                label: "Remove".into(),
                style: ButtonStyle::Danger,
                icon: Some("trash-2".into()),
                disabled: true,
                confirm: Some(confirm),
                icon_only: false,
            },
            Node::Input {
                id: "notes".into(),
                placeholder: "Notes".into(),
                value: Some("a\nb".into()),
                rows: Some(DEFAULT_INPUT_ROWS),
                icon: None,
                submit: false,
                clear: false,
            },
            Node::Input {
                id: "big".into(),
                placeholder: "Big".into(),
                value: None,
                rows: Some(MAX_INPUT_ROWS),
                icon: None,
                submit: false,
                clear: false,
            },
            Node::Badge {
                text: "late".into(),
                tone: Tone::Danger,
            },
        ];
        assert_eq!(children(tree), expected);
    }

    #[test]
    fn new_nodes_break_on_bad_required_fields() {
        let broken = [
            json!({"type": "section", "title": "No id", "children": []}),
            json!({"type": "section", "id": "s", "children": []}),
            json!({"type": "section", "id": "s2", "title": "No children"}),
            json!({"type": "table", "rows": []}),
            json!({"type": "table", "columns": []}),
            json!({"type": "table", "columns": [{"width": 3}], "rows": []}),
            json!({"type": "table", "columns": ["Repo"], "rows": []}),
            json!({"type": "log"}),
            json!({"type": "log", "text": 3}),
            json!({"type": "alert", "text": "No tone"}),
            json!({"type": "alert", "tone": "neutral", "text": "Neutral"}),
            json!({"type": "alert", "tone": "loud", "text": "Unknown"}),
            json!({"type": "alert", "tone": "info"}),
            json!({"type": "link", "text": "No url"}),
            json!({"type": "link", "url": "https://example.com"}),
            json!({"type": "link", "text": "js", "url": "javascript:alert(1)"}),
            json!({"type": "link", "text": "file", "url": "file:///etc/passwd"}),
            json!({"type": "link", "text": "space", "url": "https://exa mple.com"}),
            json!({"type": "link", "text": "bare", "url": "https://"}),
            json!({"type": "link", "text": "long", "url": format!("https://{}", "a".repeat(MAX_URL))}),
            json!({"type": "icon"}),
            json!({"type": "icon", "name": "Git Pull"}),
            json!({"type": "icon", "name": "x".repeat(MAX_ICON + 1)}),
            json!({"type": "kv"}),
            json!({"type": "kv", "items": [{"key": "k"}]}),
            json!({"type": "kv", "items": [{"key": "k", "value": 3}]}),
            json!({"type": "menu"}),
            json!({"type": "menu", "items": [{"label": "No id"}]}),
            json!({"type": "menu", "items": [{"id": "a", "label": "A", "confirm": {"text": "No title"}}]}),
            json!({"type": "menu", "items": [{"id": "a", "label": "A", "confirm": true}]}),
            // A confirmation that is there but broken is not dropped: the button is.
            json!({"type": "button", "id": "b1", "label": "B", "confirm": "Sure?"}),
            json!({"type": "button", "id": "b2", "label": "B", "confirm": {"title": 1}}),
        ];
        let n = broken.len();
        assert_eq!(children(column(broken.into())), vec![Node::Unsupported; n]);

        // Wrong optional fields count as absent.
        let tree = column(vec![
            json!({"type": "card", "id": 3, "title": 4, "icon": "Not An Icon", "children": "x"}),
            json!({"type": "button", "id": "b", "label": "B", "icon": 1, "disabled": "yes", "confirm": null}),
            json!({"type": "input", "id": "i", "placeholder": "P", "multiline": true, "rows": "x", "value": "a\nb"}),
            json!({"type": "input", "id": "j", "placeholder": "P", "rows": 8, "value": "a\nb"}),
            json!({"type": "badge", "text": "t", "tone": "loud"}),
            json!({"type": "log", "text": "t", "follow": "no", "fill": 1}),
        ]);
        assert_eq!(
            children(tree),
            vec![
                Node::Card {
                    id: None,
                    title: None,
                    subtitle: None,
                    badge: None,
                    icon: None,
                    selected: false,
                    children: vec![],
                },
                button("b", "B"),
                Node::Input {
                    id: "i".into(),
                    placeholder: "P".into(),
                    value: Some("a\nb".into()),
                    rows: Some(DEFAULT_INPUT_ROWS),
                    icon: None,
                    submit: false,
                    clear: false,
                },
                // Without `multiline`, `rows` means nothing and the value is one line.
                Node::Input {
                    id: "j".into(),
                    placeholder: "P".into(),
                    value: Some("a b".into()),
                    rows: None,
                    icon: None,
                    submit: false,
                    clear: false,
                },
                Node::Badge {
                    text: "t".into(),
                    tone: Tone::Neutral,
                },
                Node::Log {
                    text: "t".into(),
                    follow: true,
                    fill: false,
                },
            ]
        );
    }

    #[test]
    fn table_caps_and_cells() {
        let columns: Vec<_> = (0..MAX_COLUMNS + 1)
            .map(|n| json!({"title": n.to_string()}))
            .collect();
        let cells: Vec<_> = (0..MAX_COLUMNS + 5).map(|n| json!(n.to_string())).collect();
        let rows = vec![json!({"cells": cells}); MAX_ROWS + 1];
        let Ok(Node::Table { columns, rows, .. }) =
            parse(&json!({"type": "table", "columns": columns, "rows": rows}))
        else {
            panic!("not a table")
        };
        assert_eq!(columns.len(), MAX_COLUMNS);
        assert_eq!(rows.len(), MAX_ROWS);
        assert_eq!(rows[0].as_ref().unwrap().cells.len(), MAX_COLUMNS);

        // Broken rows are `None`; cells that may not be in a table are `Unsupported` and take no
        // id; missing cells stay missing (blank).
        let tree = column(vec![
            json!({"type": "table", "columns": [{"title": "a"}, {"title": "b"}], "rows": [
                {"id": "r", "cells": [
                    {"type": "input", "id": "x", "placeholder": "p"},
                    {"type": "column", "children": [{"type": "button", "id": "y", "label": "Y"}]},
                ]},
                {"cells": [3]},
                {"id": "no cells"},
                "a string",
                {"cells": [{"type": "button", "id": "z", "label": "Z"}]},
            ]}),
            json!({"type": "button", "id": "x", "label": "X"}),
            json!({"type": "button", "id": "y", "label": "Y"}),
            json!({"type": "button", "id": "z", "label": "Again"}),
        ]);
        let children = children(tree);
        let Node::Table { rows, .. } = &children[0] else {
            panic!("not a table")
        };
        let cells: Vec<_> = rows
            .iter()
            .map(|r| r.as_ref().map(|r| r.cells.clone()))
            .collect();
        assert_eq!(
            cells,
            vec![
                Some(vec![Node::Unsupported, Node::Unsupported]),
                Some(vec![Node::Unsupported]),
                None,
                None,
                Some(vec![button("z", "Z")]),
            ]
        );
        assert_eq!(
            children[1..],
            [button("x", "X"), button("y", "Y"), Node::Unsupported]
        );

        // Node cells count toward `MAX_NODES`; string cells do not.
        let table = |cell: Value, n: usize| {
            let rows = vec![json!({"cells": [cell.clone(), cell]}); n];
            json!({"type": "table", "columns": [{"title": "a"}, {"title": "b"}], "rows": rows})
        };
        let badge = json!({"type": "badge", "text": "b"});
        assert!(parse(&table(badge.clone(), (MAX_NODES - 1) / 2)).is_ok());
        assert_eq!(parse(&table(badge, MAX_NODES / 2)), Err(TooLarge));
        assert!(parse(&table(json!("s"), MAX_ROWS)).is_ok());
        // A cell is one level below its table.
        let mut deep = table(json!({"type": "spinner"}), 1);
        for _ in 2..MAX_DEPTH {
            deep = column(vec![deep]);
        }
        assert!(parse(&deep).is_ok());
        assert_eq!(parse(&column(vec![deep])), Err(TooLarge));
    }

    #[test]
    fn kv_and_menu_caps() {
        let kv = vec![json!({"key": "k", "value": "v"}); MAX_KV + 1];
        let items = vec![json!({"id": "i", "label": "l"}); MAX_MENU + 1];
        let tree = column(vec![
            json!({"type": "kv", "items": kv}),
            json!({"type": "menu", "items": items}),
        ]);
        let children = children(tree);
        assert!(matches!(&children[0], Node::Kv(items) if items.len() == MAX_KV));
        assert!(matches!(&children[1], Node::Menu { items, .. } if items.len() == MAX_MENU));
    }

    #[test]
    fn log_keeps_the_tail_without_ansi() {
        let colored = "\u{1b}[1;31merror\u{1b}[0m: \u{1b}]8;;https://x\u{1b}\\link\u{1b}]8;;\u{7}\r\n\tdone\u{7}\u{1b}(B";
        assert_eq!(strip_ansi(colored), "error: link\n\tdone");
        let Ok(Node::Log { text, .. }) = parse(&json!({"type": "log", "text": colored})) else {
            panic!("not a log")
        };
        assert_eq!(&*text, "error: link\n\tdone");

        let long = format!("{}{}", "\u{1b}[2m.\u{1b}[0m".repeat(LOG_CHARS), "é\nend");
        let Ok(Node::Log { text, .. }) = parse(&json!({"type": "log", "text": long})) else {
            panic!("not a log")
        };
        assert_eq!(text.chars().count(), LOG_CHARS + 1);
        assert!(text.starts_with('…') && text.ends_with(".é\nend"));
        assert_eq!(tail("abc", 3), "abc");
        assert_eq!(tail("abcd", 3), "…bcd");
    }

    #[test]
    fn tabs_parse() {
        let tree = json!({"type": "tabs", "id": "t", "value": "help", "tabs": [
            {"id": "work", "title": "Work", "badge": "3", "children": [{"type": "text", "text": "a"}]},
            {"id": "help", "title": "Help", "children": []},
        ]});
        let Ok(Node::Tabs {
            id, value, tabs, ..
        }) = parse(&tree)
        else {
            panic!("not tabs")
        };
        assert_eq!((id.as_str(), value.as_deref()), ("t", Some("help")));
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0].badge.as_deref(), Some("3"));
        assert_eq!(tabs[0].children.len(), 1);
        assert!(tabs[1].badge.is_none());
    }

    #[test]
    fn broken_tabs_are_unsupported() {
        let tab = |id: &str| json!({"id": id, "title": id, "children": []});
        let many: Vec<_> = (0..=MAX_TABS).map(|n| tab(&n.to_string())).collect();
        let cases = [
            json!({"type": "tabs", "tabs": [tab("a")]}),
            json!({"type": "tabs", "id": "t", "tabs": "a"}),
            json!({"type": "tabs", "id": "t", "tabs": []}),
            json!({"type": "tabs", "id": "t", "tabs": many}),
            json!({"type": "tabs", "id": "t", "tabs": [{"id": "a", "children": []}]}),
            json!({"type": "tabs", "id": "t", "tabs": [{"id": "a", "title": "A"}]}),
            json!({"type": "tabs", "id": "t", "tabs": [tab("a"), tab("a")]}),
        ];
        for case in cases {
            assert_eq!(parse(&case), Ok(Node::Unsupported), "{case}");
        }
        let Ok(Node::Column { children, .. }) = parse(&json!({"type": "column", "children": [
            {"type": "button", "id": "b", "label": "B"},
            {"type": "tabs", "id": "b", "tabs": [tab("a")]},
            {"type": "tabs", "id": "t", "tabs": [tab("a")]},
            {"type": "tabs", "id": "t", "tabs": [tab("a")]},
        ]})) else {
            panic!("not a column")
        };
        assert_eq!(children[1], Node::Unsupported);
        assert!(matches!(&children[2], Node::Tabs { .. }));
        assert_eq!(children[3], Node::Unsupported);
    }

    #[test]
    fn tabs_children_count_toward_limits() {
        let texts = vec![json!({"type": "text", "text": "t"}); MAX_NODES];
        let tree = json!({"type": "tabs", "id": "t", "tabs": [
            {"id": "a", "title": "A", "children": []},
            {"id": "b", "title": "B", "children": texts},
        ]});
        assert_eq!(parse(&tree), Err(TooLarge));
    }

    #[test]
    fn walk_visits_every_tab() {
        let tree = parse(&json!({"type": "tabs", "id": "t", "tabs": [
            {"id": "a", "title": "A", "children": []},
            {"id": "b", "title": "B", "children": [{"type": "text", "text": "hidden"}]},
        ]}))
        .unwrap();
        let mut seen = Vec::new();
        tree.walk(&mut |n| {
            if let Node::Text { text, .. } = n {
                seen.push(text.to_string())
            }
        });
        assert_eq!(seen, ["hidden"]);
    }

    #[test]
    fn section_ids_are_unique() {
        let tree = column(vec![
            json!({"type": "section", "id": "a", "title": "A", "children": [
                {"type": "button", "id": "a", "label": "Same as its section"},
                {"type": "checkbox", "id": "b", "label": "B"},
            ]}),
            json!({"type": "section", "id": "b", "title": "Same as a checkbox", "children": []}),
            // A broken section takes no id, and its children are not read.
            json!({"type": "section", "id": "c", "title": "C"}),
            json!({"type": "section", "id": "c", "title": "C", "children": []}),
        ]);
        let children = children(tree);
        let Node::Section {
            children: inner, ..
        } = &children[0]
        else {
            panic!("not a section")
        };
        assert_eq!(inner[0], Node::Unsupported);
        assert!(matches!(inner[1], Node::Checkbox { .. }));
        assert_eq!(children[1], Node::Unsupported);
        assert_eq!(children[2], Node::Unsupported);
        assert!(matches!(&children[3], Node::Section { id, .. } if id == "c"));
    }

    #[test]
    fn item_card_keeps_display_nodes_only() {
        let card = |tree| item_state(&json!({"text": "x", "card": tree})).card;
        let ok = json!({"type": "column", "children": [
            {"type": "row", "children": [{"type": "text", "text": "Current session"}, {"type": "spacer"},
                                         {"type": "badge", "text": "16% used"}, {"type": "icon", "name": "gauge"}]},
            {"type": "progress", "value": 0.16}, {"type": "divider"},
            {"type": "kv", "items": [{"key": "a", "value": "b"}]}]});
        let node = card(ok).expect("card");
        let mut unsupported = 0;
        node.walk(&mut |n| unsupported += matches!(n, Node::Unsupported) as usize);
        assert_eq!(unsupported, 0);
        let button =
            json!({"type": "column", "children": [{"type": "button", "id": "b", "label": "Go"}]});
        assert_eq!(*card(button).unwrap(), col(vec![Node::Unsupported]));
        assert_eq!(item_state(&json!({"text": "x"})).card, None);
        assert_eq!(card(Value::Null), None);
        assert_eq!(card(json!("not a tree")), Some(Arc::new(Node::Unsupported)));
    }

    #[test]
    fn item_card_caps() {
        let card = |tree| item_state(&json!({"text": "x", "card": tree})).card;
        let flat = |n| json!({"type": "column", "children": vec![json!({"type": "divider"}); n]});
        assert!(card(flat(CARD_NODES - 1)).is_some()); // the column is a node
        assert!(card(flat(CARD_NODES)).is_none());
        let mut deep = json!({"type": "divider"});
        for _ in 1..CARD_DEPTH {
            deep = json!({"type": "column", "children": [deep]});
        }
        assert!(card(deep.clone()).is_some());
        assert!(card(json!({"type": "column", "children": [deep]})).is_none());
    }

    #[test]
    fn walk_visits_every_node() {
        let tree = parse(&column(vec![
            json!({"type": "row", "children": [{"type": "badge", "text": "a"}]}),
            json!({"type": "badge", "text": "b"}),
            json!({"type": "card", "children": [{"type": "divider"}]}),
            json!({"type": "section", "id": "s", "title": "S", "children": [{"type": "spacer"}]}),
            json!({"type": "table", "columns": [{"title": "a"}], "rows": [{"cells": ["x"]}, 3]}),
        ]))
        .unwrap();
        let mut seen = 0;
        tree.walk(&mut |_| seen += 1);
        assert_eq!(seen, 10);
    }

    #[test]
    fn path_node() {
        let tree = json!({"type": "column", "children": [
            {"type": "path", "id": "dir", "pick": "folder", "value": "~/src", "editable": false},
            {"type": "path", "id": "dump", "pick": "file", "placeholder": "Dump", "editable": "no"},
            {"type": "path", "id": "both", "pick": "both"},
            {"type": "path", "pick": "file"},
            {"type": "path", "id": "dir", "pick": "file"},
        ]});
        let path =
            |id: &str, pick, placeholder: Option<&str>, value: Option<&str>, editable| Node::Path {
                id: id.into(),
                pick,
                placeholder: placeholder.map(Into::into),
                value: value.map(Into::into),
                editable,
            };
        assert_eq!(
            parse(&tree),
            Ok(Node::Column {
                framed: false,
                title: None,
                badge: None,
                tone: Tone::Neutral,
                collapsed: false,
                children: vec![
                    path("dir", Pick::Folder, None, Some("~/src"), false),
                    path("dump", Pick::File, Some("Dump"), None, true),
                    Node::Unsupported,
                    Node::Unsupported,
                    Node::Unsupported,
                ],
                width: None,
            })
        );
    }

    /// A row's `fill`, a column's `width` (clamped) and a card's `selected`; wrong types are
    /// ignored.
    #[test]
    fn split_fields() {
        let tree = json!({"type": "column", "children": [
            {"type": "row", "fill": true, "children": [
                {"type": "column", "width": 380, "children": []},
                {"type": "column", "width": 5, "children": []},
                {"type": "column", "width": 1e9, "children": []},
                {"type": "column", "width": "wide", "children": []},
            ]},
            {"type": "row", "fill": "yes", "children": []},
            {"type": "card", "id": "c", "selected": true},
            {"type": "card", "id": "d"},
        ]});
        let Ok(Node::Column {
            children, width, ..
        }) = parse(&tree)
        else {
            panic!("a column")
        };
        assert_eq!(width, None);
        let Node::Row {
            children: columns,
            fill: true,
        } = &children[0]
        else {
            panic!("a fill row: {:?}", children[0])
        };
        let widths: Vec<_> = (columns.iter())
            .map(|c| match c {
                Node::Column { width, .. } => *width,
                _ => panic!("a column"),
            })
            .collect();
        assert_eq!(widths, [Some(380.), Some(24.), Some(2_000.), None]);
        assert!(matches!(children[1], Node::Row { fill: false, .. }));
        assert!(matches!(children[2], Node::Card { selected: true, .. }));
        assert!(matches!(
            children[3],
            Node::Card {
                selected: false,
                ..
            }
        ));
    }

    #[test]
    fn parses_list_item_extras() {
        let tree = json!({"type": "list", "items": [
            {"id": "a", "title": "A", "icon": "git-branch", "tone": "warning", "selected": true,
             "lead": "EC-1", "note": "2d", "indent": true},
            {"id": "b", "title": "B", "icon": "No Such!"},
        ]});
        let Ok(Node::List { items, .. }) = parse(&tree) else {
            panic!("not a list")
        };
        let a = items[0].as_ref().unwrap();
        assert_eq!(a.icon.as_deref(), Some("git-branch"));
        assert_eq!(a.tone, Tone::Warning);
        assert!(a.selected && a.indent);
        assert_eq!(
            (a.lead.as_deref(), a.note.as_deref()),
            (Some("EC-1"), Some("2d"))
        );
        let b = items[1].as_ref().unwrap();
        assert_eq!(b.icon, None);
        assert_eq!(b.tone, Tone::Neutral);
    }

    #[test]
    fn parses_text_style() {
        let style = |s: &str| match parse(&json!({"type": "text", "text": "a", "style": s})) {
            Ok(Node::Text { style, .. }) => style,
            other => panic!("{other:?}"),
        };
        assert_eq!(style("label"), TextStyle::Label);
        assert_eq!(style("strong"), TextStyle::Strong);
        assert_eq!(style("heading"), TextStyle::Heading);
        assert_eq!(style("huge"), TextStyle::Plain);
    }

    #[test]
    fn title_or_collapsed_implies_framed() {
        let framed = |tree: Value| match parse(&tree) {
            Ok(Node::Column { framed, .. }) => framed,
            other => panic!("{other:?}"),
        };
        assert!(framed(
            json!({"type": "column", "title": "A", "children": []})
        ));
        assert!(framed(
            json!({"type": "column", "collapsed": true, "children": []})
        ));
        assert!(!framed(json!({"type": "column", "children": []})));
    }

    #[test]
    fn parses_segmented_and_input_extras() {
        let tree = json!({"type": "column", "children": [
            {"type": "tabs", "id": "t", "style": "segmented", "tabs": [
                {"id": "a", "title": "A", "tone": "warning", "children": []}]},
            {"type": "select", "id": "s", "options": ["a"], "segmented": true},
            {"type": "input", "id": "i", "placeholder": "p", "icon": "search", "submit": true, "clear": true},
            {"type": "input", "id": "m", "placeholder": "p", "multiline": true, "submit": true},
        ]});
        let Ok(Node::Column { children, .. }) = parse(&tree) else {
            panic!("not a column")
        };
        let Node::Tabs {
            segmented, tabs, ..
        } = &children[0]
        else {
            panic!("not tabs")
        };
        assert!(*segmented);
        assert_eq!(tabs[0].tone, Tone::Warning);
        assert!(matches!(
            &children[1],
            Node::Select {
                segmented: true,
                ..
            }
        ));
        let Node::Input {
            icon,
            submit,
            clear,
            ..
        } = &children[2]
        else {
            panic!("not an input")
        };
        assert_eq!(icon.as_deref(), Some("search"));
        assert!(*submit && *clear);
        assert!(matches!(&children[3], Node::Input { submit: false, .. }));
    }

    #[test]
    fn icon_only_needs_an_icon() {
        let tree = json!({"type": "column", "children": [
            {"type": "button", "id": "b", "label": "B", "icon_only": true},
            {"type": "button", "id": "c", "label": "C", "icon": "x", "icon_only": true},
            {"type": "link", "text": "L", "url": "https://a.b", "icon": "external-link", "icon_only": true},
            {"type": "link", "text": "L", "url": "https://a.b", "icon_only": true},
        ]});
        let Ok(Node::Column { children, .. }) = parse(&tree) else {
            panic!("not a column")
        };
        assert!(matches!(
            &children[0],
            Node::Button {
                icon_only: false,
                ..
            }
        ));
        assert!(matches!(
            &children[1],
            Node::Button {
                icon_only: true,
                ..
            }
        ));
        assert!(matches!(
            &children[2],
            Node::Link {
                icon: Some(_),
                icon_only: true,
                ..
            }
        ));
        assert!(matches!(
            &children[3],
            Node::Link {
                icon: None,
                icon_only: false,
                ..
            }
        ));
    }

    #[test]
    fn old_trees_parse_unchanged() {
        let tree = json!({"type": "column", "children": [
            {"type": "text", "text": "a"},
            {"type": "list", "items": [{"id": "a", "title": "A"}]},
            {"type": "tabs", "id": "t", "tabs": [{"id": "a", "title": "A", "children": []}]},
        ]});
        let expected = col(vec![
            text("a"),
            Node::List {
                items: vec![Some(Item {
                    id: "a".into(),
                    title: "A".into(),
                    subtitle: None,
                    badge: None,
                    icon: None,
                    tone: Tone::Neutral,
                    selected: false,
                    lead: None,
                    note: None,
                    indent: false,
                })]
                .into(),
                subtitles: false,
                fill: false,
                plain: false,
            },
            Node::Tabs {
                id: "t".into(),
                value: None,
                tabs: vec![Tab {
                    id: "a".into(),
                    title: "A".into(),
                    badge: None,
                    tone: Tone::Neutral,
                    children: vec![],
                }],
                segmented: false,
            },
        ]);
        assert_eq!(parse(&tree), Ok(expected));
    }
}
