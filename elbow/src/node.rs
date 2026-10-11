//! What a node box shows (name lines, sublabel, type tag) and how big it must be.

use crate::geom::snap_up;
use crate::spec::{Kind, NodeSpec, Shape};
use crate::text::{mono_width, sans_width, split_in_two};

pub const NAME_SIZE: f64 = 12.0;
pub const NAME_LINE: f64 = 15.0;
pub const SUB_SIZE: f64 = 9.0;
pub const SUB_LINE: f64 = 14.0;
pub const TAG_SIZE: f64 = 7.0;
pub const TAG_TRACKING: f64 = 0.08;
pub const TAG_ROW: f64 = 18.0;
pub const FIELD_SIZE: f64 = 9.0;
pub const FIELD_LINE: f64 = 14.0;
pub const FIELD_INSET: f64 = 12.0;
const FIELD_ROOM: f64 = 20.0;
const HEADER_ROOM: f64 = 14.0;
const BADGE_GAP: f64 = 8.0;
const PADDING: f64 = 16.0;
const MIN_HEIGHT: f64 = 48.0;
const VERTICAL_ROOM: f64 = 30.0;
const ONE_LINE_LIMIT: f64 = 240.0;
const WIDTHS: [f64; 12] = [96.0, 112.0, 120.0, 128.0, 140.0, 144.0, 160.0, 180.0, 200.0, 240.0, 280.0, 320.0];

/// The fill and stroke family of a node, after the diagram-design node table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Treatment {
    Focal,
    Service,
    State,
    Store,
    External,
    Input,
    Note,
    Leaf,
}

/// How many edges end at and start from a node, not counting self loops.
#[derive(Clone, Copy, Debug, Default)]
pub struct Links {
    pub incoming: usize,
    pub outgoing: usize,
}

#[derive(Clone, Debug)]
pub struct NodeContent {
    pub lines: Vec<String>,
    pub sublabel: Option<String>,
    pub fields: Vec<String>,
    pub tag: Option<&'static str>,
    pub badge: Option<String>,
    pub treatment: Treatment,
    pub width: f64,
    pub height: f64,
}

impl NodeContent {
    pub fn top_room(&self) -> f64 {
        if self.tag.is_some() || self.badge.is_some() {
            TAG_ROW
        } else {
            0.0
        }
    }

    pub fn block_height(&self) -> f64 {
        self.lines.len() as f64 * NAME_LINE + if self.sublabel.is_some() { SUB_LINE } else { 0.0 }
    }

    /// The height of the name section above an entity's field list.
    pub fn header_height(&self) -> f64 {
        snap_up(self.top_room() + self.block_height() + HEADER_ROOM)
    }

    fn fitted_height(&self) -> f64 {
        if self.fields.is_empty() {
            return MIN_HEIGHT.max(snap_up(self.top_room() + self.block_height() + VERTICAL_ROOM));
        }
        snap_up(self.header_height() + self.fields.len() as f64 * FIELD_LINE + FIELD_ROOM)
    }
}

pub fn tag_width(tag: &str) -> f64 {
    mono_width(tag, TAG_SIZE, TAG_TRACKING) + 10.0
}

pub fn content(spec: &NodeSpec, kind: Kind, links: Links) -> NodeContent {
    let (name, bracket) = name_and_sublabel(&spec.label);
    let lists_fields = matches!(kind, Kind::Er | Kind::UmlClass | Kind::DbSchema | Kind::Medallion);
    let (sublabel, fields) = match (lists_fields, bracket) {
        (true, Some(list)) => (None, field_list(&list)),
        (_, bracket) => (bracket, Vec::new()),
    };
    let tag = tag_for(spec.shape, kind);
    let badge = (kind == Kind::Dependency).then(|| format!("{} IN", links.incoming));
    let lines = name_lines(&name);
    let widest_line = lines.iter().map(|line| sans_width(line, NAME_SIZE)).fold(0.0, f64::max);
    let sub_width = sublabel.as_deref().map_or(0.0, |sub| mono_width(sub, SUB_SIZE, 0.0));
    let field_box = fields.iter().map(|field| mono_width(field, FIELD_SIZE, 0.0) + 2.0 * FIELD_INSET).fold(0.0, f64::max);
    let chips = tag.map_or(0.0, tag_width) + badge.as_deref().map_or(0.0, |badge| tag_width(badge) + BADGE_GAP);
    let tag_room = if chips > 0.0 { chips + PADDING } else { 0.0 };
    let needed = (widest_line.max(sub_width).max(tag_room) + 2.0 * PADDING).max(field_box);
    let width = WIDTHS.iter().copied().find(|&width| width >= needed).unwrap_or_else(|| snap_up(needed));
    let treatment = treatment(spec, kind, links);
    let mut node = NodeContent { lines, sublabel, fields, tag, badge, treatment, width, height: 0.0 };
    node.height = node.fitted_height();
    node
}

/// The items of a bracket list, split at commas and semicolons outside inner brackets, so
/// `numeric(12,2)` or `total(a, b)` stay one item.
pub fn field_list(list: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for c in list.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' | ';' if depth == 0 => {
                items.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    items.push(current);
    items.into_iter().map(|item| item.trim().to_string()).filter(|item| !item.is_empty()).collect()
}

fn name_lines(name: &str) -> Vec<String> {
    if sans_width(name, NAME_SIZE) + 2.0 * PADDING <= ONE_LINE_LIMIT {
        return vec![name.to_string()];
    }
    match split_in_two(name) {
        Some((first, second)) => vec![first, second],
        None => vec![name.to_string()],
    }
}

/// Splits `Name (bracket)` into the name and the bracket's content. The bracket is the one the final
/// `)` closes, so brackets inside it (`total(): Money`) stay part of it.
pub fn name_and_sublabel(label: &str) -> (String, Option<String>) {
    let trimmed = label.trim();
    if let Some(open) = trimmed.strip_suffix(')').and_then(matching_open) {
        let name = trimmed[..open].trim();
        let sub = trimmed[open + 1..trimmed.len() - 1].trim();
        if !name.is_empty() && !sub.is_empty() && trimmed[..open].ends_with(' ') {
            return (name.to_string(), Some(sub.to_string()));
        }
    }
    (trimmed.to_string(), None)
}

fn matching_open(before_close: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, c) in before_close.char_indices().rev() {
        match c {
            ')' => depth += 1,
            '(' if depth == 0 => return Some(index),
            '(' => depth -= 1,
            _ => {}
        }
    }
    None
}

fn tag_for(shape: Shape, kind: Kind) -> Option<&'static str> {
    let technical = matches!(kind, Kind::Flow | Kind::Dependency);
    match shape {
        Shape::Service if kind == Kind::Er => Some("ENTITY"),
        Shape::Service if technical => Some("SVC"),
        Shape::Service | Shape::State => None,
        Shape::Database => Some("DB"),
        Shape::Queue => Some("QUEUE"),
        Shape::Cache => Some("CACHE"),
        Shape::Actor => Some("USER"),
        Shape::External => Some("EXT"),
        Shape::Client => Some("APP"),
        Shape::Decision => Some("IF"),
        Shape::Note => Some("NOTE"),
    }
}

fn treatment(spec: &NodeSpec, kind: Kind, links: Links) -> Treatment {
    if spec.focal {
        return Treatment::Focal;
    }
    match spec.shape {
        Shape::Database | Shape::Queue | Shape::Cache => Treatment::Store,
        Shape::External => Treatment::External,
        Shape::Actor | Shape::Client => Treatment::Input,
        Shape::Note => Treatment::Note,
        Shape::State => Treatment::State,
        Shape::Service if kind == Kind::Dependency && links.outgoing == 0 => Treatment::Leaf,
        Shape::Service | Shape::Decision => Treatment::Service,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(label: &str, shape: Shape) -> NodeSpec {
        NodeSpec { id: "n".into(), label: label.into(), shape, focal: false }
    }

    #[test]
    fn a_trailing_bracket_becomes_the_sublabel() {
        let node = content(&spec("Users DB (Postgres)", Shape::Database), Kind::Flow, Links::default());
        assert_eq!(node.lines, vec!["Users DB".to_string()]);
        assert_eq!(node.sublabel.as_deref(), Some("Postgres"));
        assert_eq!(node.tag, Some("DB"));
    }

    #[test]
    fn a_bracket_with_inner_brackets_stays_one_sublabel() {
        assert_eq!(
            name_and_sublabel("Order (+ id: UUID, + total(): Money)"),
            ("Order".to_string(), Some("+ id: UUID, + total(): Money".to_string()))
        );
        assert_eq!(name_and_sublabel("total()"), ("total()".to_string(), None));
        assert_eq!(field_list("id uuid PK, amount numeric(12,2); note"), vec!["id uuid PK", "amount numeric(12,2)", "note"]);
    }

    #[test]
    fn sizes_stay_on_the_grid() {
        let node = content(&spec("A rather long node name that needs two lines", Shape::Service), Kind::Tree, Links::default());
        assert_eq!(node.lines.len(), 2);
        assert_eq!(node.width % 4.0, 0.0);
        assert_eq!(node.height % 4.0, 0.0);
    }

    #[test]
    fn an_entity_lists_its_bracket_as_fields() {
        let node = content(&spec("Order (id, customer_id; total)", Shape::Service), Kind::Er, Links::default());
        assert_eq!(node.fields, vec!["id".to_string(), "customer_id".to_string(), "total".to_string()]);
        assert_eq!(node.sublabel, None);
        assert_eq!(node.tag, Some("ENTITY"));
        assert!(node.height >= node.header_height() + 3.0 * FIELD_LINE);
    }

    #[test]
    fn a_dependency_shows_its_fan_in_and_a_leaf_its_own_look() {
        let leaf = content(&spec("Utils", Shape::Service), Kind::Dependency, Links { incoming: 3, outgoing: 0 });
        assert_eq!(leaf.badge.as_deref(), Some("3 IN"));
        assert_eq!(leaf.treatment, Treatment::Leaf);
        let inner = content(&spec("Web app", Shape::Service), Kind::Dependency, Links { incoming: 0, outgoing: 2 });
        assert_eq!(inner.badge.as_deref(), Some("0 IN"));
        assert_eq!(inner.treatment, Treatment::Service);
    }
}
