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
    let (sublabel, fields) = match (kind, bracket) {
        (Kind::Er, Some(list)) => (None, field_list(&list)),
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

fn field_list(list: &str) -> Vec<String> {
    list.split([',', ';']).map(str::trim).filter(|field| !field.is_empty()).map(str::to_string).collect()
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

fn name_and_sublabel(label: &str) -> (String, Option<String>) {
    let trimmed = label.trim();
    if let (Some(open), true) = (trimmed.rfind(" ("), trimmed.ends_with(')')) {
        let name = trimmed[..open].trim();
        let sub = trimmed[open + 2..trimmed.len() - 1].trim();
        if !name.is_empty() && !sub.is_empty() {
            return (name.to_string(), Some(sub.to_string()));
        }
    }
    (trimmed.to_string(), None)
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
