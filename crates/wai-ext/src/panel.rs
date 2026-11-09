//! The declarative panel schema, v1: the tree an extension sends in `panel/render`, checked into
//! typed nodes. The tree is untrusted: parsing never panics and never recurses past `MAX_DEPTH`.
//! An unknown or broken node becomes `Unsupported`; only a tree beyond the size limits is refused
//! whole (`TooLarge`, shown as "Panel too large").
//!
//! - Every field without `?` in the schema is required. An optional field of the wrong type counts
//!   as absent; fields the schema does not name are ignored.
//! - `text` and `markdown` keep at most `TEXT_CHARS` characters. Every other string is one line of
//!   at most `LINE_CHARS` characters.
//! - Ids are 1 to `MAX_ID` bytes. Buttons and form fields share one set of ids: a repeated id makes
//!   that node `Unsupported`. List item ids may repeat.
//! - Nodes are the tree's root and each element of a `children` array, whatever it holds. List
//!   items and select options are not nodes; options past `MAX_OPTIONS` are dropped.
//! - `value` (input, select) and `checked` (checkbox) are initial values: wai applies one when the
//!   field first appears or when the extension sends a different one; otherwise what the user
//!   did stays. `fill` makes a list take the panel's remaining height.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{Map, Value};

pub const MAX_DEPTH: usize = 32;
pub const MAX_NODES: usize = 2_000;
pub const TEXT_CHARS: usize = 10_000;
pub const LINE_CHARS: usize = 500;
pub const MAX_ID: usize = 256;
/// A select's menu draws every option.
pub const MAX_OPTIONS: usize = 100;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Column(Vec<Node>),
    Row(Vec<Node>),
    Text {
        text: Arc<str>,
        muted: bool,
    },
    Markdown(Arc<str>),
    /// A broken item is `None`. `subtitles`: some item has one, so rows take two lines.
    List {
        items: Arc<[Option<Item>]>,
        subtitles: bool,
        fill: bool,
    },
    Button {
        id: String,
        label: Arc<str>,
        style: ButtonStyle,
    },
    Input {
        id: String,
        placeholder: Arc<str>,
        value: Option<Arc<str>>,
    },
    Select {
        id: String,
        options: Vec<Arc<str>>,
        value: Option<Arc<str>>,
    },
    Checkbox {
        id: String,
        label: Arc<str>,
        checked: Option<bool>,
    },
    Badge(Arc<str>),
    /// From 0 to 1.
    Progress(f32),
    /// An unknown type, or a known one that is broken.
    Unsupported,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    pub title: Arc<str>,
    pub subtitle: Option<Arc<str>>,
    pub badge: Option<Arc<str>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ButtonStyle {
    #[default]
    Default,
    Primary,
    Danger,
}

/// Deeper than `MAX_DEPTH` or more than `MAX_NODES` nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooLarge;

pub fn parse(tree: &Value) -> Result<Node, TooLarge> {
    Parser::default().node(tree, 1)
}

impl Node {
    /// Each node, parents first.
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Node)) {
        f(self);
        if let Node::Column(children) | Node::Row(children) = self {
            for child in children {
                child.walk(f);
            }
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

#[derive(Default)]
struct Parser {
    nodes: usize,
    /// Taken by buttons and form fields.
    ids: HashSet<String>,
}

impl Parser {
    /// Recurses at most `MAX_DEPTH` deep, and stops at the first node past a limit.
    fn node(&mut self, value: &Value, depth: usize) -> Result<Node, TooLarge> {
        self.nodes += 1;
        if depth > MAX_DEPTH || self.nodes > MAX_NODES {
            return Err(TooLarge);
        }
        let Some(o) = value.as_object() else {
            return Ok(Node::Unsupported);
        };
        let node = match str_field(o, "type") {
            Some(kind @ ("column" | "row")) => match o.get("children").and_then(Value::as_array) {
                Some(children) => {
                    let children = (children.iter())
                        .map(|child| self.node(child, depth + 1))
                        .collect::<Result<_, _>>()?;
                    Some(match kind {
                        "column" => Node::Column(children),
                        _ => Node::Row(children),
                    })
                }
                None => None,
            },
            Some(kind) => self.leaf(kind, o),
            None => None,
        };
        Ok(node.unwrap_or(Node::Unsupported))
    }

    fn leaf(&mut self, kind: &str, o: &Map<String, Value>) -> Option<Node> {
        let text = || str_field(o, "text").map(|t| cut(t, TEXT_CHARS).into());
        let flag = |key| o.get(key).and_then(Value::as_bool);
        let node = match kind {
            "text" => Node::Text {
                text: text()?,
                muted: flag("muted").unwrap_or(false),
            },
            "markdown" => Node::Markdown(text()?),
            "badge" => Node::Badge(line(o, "text")?),
            "progress" => Node::Progress(o.get("value")?.as_f64()?.clamp(0., 1.) as f32),
            "list" => {
                let items = o.get("items")?.as_array()?;
                let items: Arc<[Option<Item>]> = items.iter().map(item).collect();
                let subtitles = items.iter().flatten().any(|i| i.subtitle.is_some());
                Node::List {
                    items,
                    subtitles,
                    fill: flag("fill").unwrap_or(false),
                }
            }
            "button" => Node::Button {
                id: id(o)?,
                label: line(o, "label")?,
                style: match str_field(o, "style") {
                    Some("primary") => ButtonStyle::Primary,
                    Some("danger") => ButtonStyle::Danger,
                    _ => ButtonStyle::Default,
                },
            },
            "input" => Node::Input {
                id: id(o)?,
                placeholder: line(o, "placeholder")?,
                value: line(o, "value"),
            },
            "select" => {
                let options = o.get("options")?.as_array()?.iter().take(MAX_OPTIONS);
                let options = options.map(|v| Some(clamp(v.as_str()?, LINE_CHARS).into()));
                Node::Select {
                    id: id(o)?,
                    options: options.collect::<Option<_>>()?,
                    value: line(o, "value"),
                }
            }
            "checkbox" => Node::Checkbox {
                id: id(o)?,
                label: line(o, "label")?,
                checked: flag("checked"),
            },
            _ => return None,
        };
        // Only a node that is otherwise whole takes its id.
        match &node {
            Node::Button { id, .. }
            | Node::Input { id, .. }
            | Node::Select { id, .. }
            | Node::Checkbox { id, .. } => self.ids.insert(id.clone()).then_some(node),
            _ => Some(node),
        }
    }
}

fn item(value: &Value) -> Option<Item> {
    let o = value.as_object()?;
    Some(Item {
        id: id(o)?,
        title: line(o, "title")?,
        subtitle: line(o, "subtitle"),
        badge: line(o, "badge"),
    })
}

fn str_field<'a>(o: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    o.get(key)?.as_str()
}

fn line(o: &Map<String, Value>, key: &str) -> Option<Arc<str>> {
    str_field(o, key).map(|s| clamp(s, LINE_CHARS).into())
}

fn id(o: &Map<String, Value>) -> Option<String> {
    let id = str_field(o, "id").filter(|id| (1..=MAX_ID).contains(&id.len()));
    id.map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value, json};

    use super::*;

    fn text(s: &str) -> Node {
        Node::Text {
            text: s.into(),
            muted: false,
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
            Ok(Node::Column(children)) => children,
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
        let expected = Node::Column(vec![
            Node::Row(vec![
                Node::Text {
                    text: "Hello".into(),
                    muted: true,
                },
                Node::Badge("3".into()),
            ]),
            Node::Markdown("# Title\n\n- a".into()),
            Node::List {
                items: vec![
                    Some(Item {
                        id: "a".into(),
                        title: "First".into(),
                        subtitle: Some("sub".into()),
                        badge: Some("new".into()),
                    }),
                    Some(Item {
                        id: "b".into(),
                        title: "Second".into(),
                        subtitle: None,
                        badge: None,
                    }),
                ]
                .into(),
                subtitles: true,
                fill: true,
            },
            Node::Button {
                id: "go".into(),
                label: "Go".into(),
                style: ButtonStyle::Primary,
            },
            Node::Button {
                id: "rm".into(),
                label: "Remove".into(),
                style: ButtonStyle::Danger,
            },
            Node::Button {
                id: "plain".into(),
                label: "Plain".into(),
                style: ButtonStyle::Default,
            },
            Node::Input {
                id: "q".into(),
                placeholder: "Search".into(),
                value: Some("rust".into()),
            },
            Node::Select {
                id: "scope".into(),
                options: vec!["Mine".into(), "All".into()],
                value: Some("All".into()),
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
            },
            Node::Select {
                id: "s2".into(),
                options: vec!["Mine".into()],
                value: None,
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
            matches!(list, Ok(Node::List { items, subtitles: false, fill: false }) if items.len() == 10 * MAX_NODES)
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
        assert_eq!(parse(&tree), Ok(Node::Column(vec![Node::Unsupported])));
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
        };
        let item = Item {
            id: "a".into(),
            title: "A".into(),
            subtitle: None,
            badge: None,
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
                },
                Node::Progress(1.),
                Node::Progress(0.),
                text("ids of other nodes are ignored"),
                Node::Input {
                    id: "i".into(),
                    placeholder: "P".into(),
                    value: None,
                },
                Node::Select {
                    id: "s".into(),
                    options: vec!["a".into()],
                    value: None,
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
                    style: ButtonStyle::Default
                },
                Node::Unsupported,
                Node::Unsupported,
                Node::Unsupported,
                Node::Select {
                    id: "y".into(),
                    options: vec![],
                    value: None,
                },
                Node::Row(vec![Node::Unsupported]),
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
                Node::Badge(text) => line(text),
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

    #[test]
    fn walk_visits_every_node() {
        let tree = parse(&column(vec![
            json!({"type": "row", "children": [{"type": "badge", "text": "a"}]}),
            json!({"type": "badge", "text": "b"}),
        ]))
        .unwrap();
        let mut seen = 0;
        tree.walk(&mut |_| seen += 1);
        assert_eq!(seen, 4);
    }
}
