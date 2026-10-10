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
}

#[derive(Clone, Debug)]
pub struct NodeContent {
    pub lines: Vec<String>,
    pub sublabel: Option<String>,
    pub tag: Option<&'static str>,
    pub treatment: Treatment,
    pub width: f64,
    pub height: f64,
}

impl NodeContent {
    pub fn top_room(&self) -> f64 {
        if self.tag.is_some() {
            TAG_ROW
        } else {
            0.0
        }
    }

    pub fn block_height(&self) -> f64 {
        self.lines.len() as f64 * NAME_LINE + if self.sublabel.is_some() { SUB_LINE } else { 0.0 }
    }
}

pub fn tag_width(tag: &str) -> f64 {
    mono_width(tag, TAG_SIZE, TAG_TRACKING) + 10.0
}

pub fn content(spec: &NodeSpec, kind: Kind) -> NodeContent {
    let (name, sublabel) = name_and_sublabel(&spec.label);
    let tag = tag_for(spec.shape, kind);
    let lines = name_lines(&name);
    let widest_line = lines.iter().map(|line| sans_width(line, NAME_SIZE)).fold(0.0, f64::max);
    let sub_width = sublabel.as_deref().map_or(0.0, |sub| mono_width(sub, SUB_SIZE, 0.0));
    let tag_room = tag.map_or(0.0, |tag| tag_width(tag) + PADDING);
    let needed = widest_line.max(sub_width).max(tag_room) + 2.0 * PADDING;
    let width = WIDTHS.iter().copied().find(|&width| width >= needed).unwrap_or_else(|| snap_up(needed));
    let mut node = NodeContent { lines, sublabel, tag, treatment: treatment(spec), width, height: 0.0 };
    node.height = MIN_HEIGHT.max(snap_up(node.top_room() + node.block_height() + VERTICAL_ROOM));
    node
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

fn treatment(spec: &NodeSpec) -> Treatment {
    if spec.focal {
        return Treatment::Focal;
    }
    match spec.shape {
        Shape::Database | Shape::Queue | Shape::Cache => Treatment::Store,
        Shape::External => Treatment::External,
        Shape::Actor | Shape::Client => Treatment::Input,
        Shape::Note => Treatment::Note,
        Shape::State => Treatment::State,
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
        let node = content(&spec("Users DB (Postgres)", Shape::Database), Kind::Flow);
        assert_eq!(node.lines, vec!["Users DB".to_string()]);
        assert_eq!(node.sublabel.as_deref(), Some("Postgres"));
        assert_eq!(node.tag, Some("DB"));
    }

    #[test]
    fn sizes_stay_on_the_grid() {
        let node = content(&spec("A rather long node name that needs two lines", Shape::Service), Kind::Tree);
        assert_eq!(node.lines.len(), 2);
        assert_eq!(node.width % 4.0, 0.0);
        assert_eq!(node.height % 4.0, 0.0);
    }
}
