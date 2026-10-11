//! UML class diagram: classes with a name, an attribute and an operation compartment, laid out in layers
//! with parents above children, and the six UML relationships drawn with their own line ends.

use std::collections::HashMap;

use crate::cardinality::{self, Ends};
use crate::figure::{Anchor, Figure, LegendKey, Marker, Paint, Swatch, Text, Tone};
use crate::geom::{snap_up, Point, Rect, Segment};
use crate::label::{self, LabelRequest};
use crate::node::{NodeContent, Treatment, NAME_SIZE};
use crate::params::{Direction, Params};
use crate::scene::{layered_with, mask_width, unique_nodes, LabelBox, Scene, SceneEdge};
use crate::spec::{EdgeSpec, View};
use crate::text::{mono_width, sans_width};

const NAME_BAND: f64 = 40.0;
const NAME_BASELINE: f64 = 24.0;
const STEREOTYPE_BASELINE: f64 = 16.0;
const STEREOTYPED_NAME_BASELINE: f64 = 32.0;
const STEREOTYPE_SIZE: f64 = 8.0;
const STEREOTYPE_TRACKING: f64 = 0.08;
const STEREOTYPE: &str = "«interface»";
const MEMBER_SIZE: f64 = 9.0;
const MEMBER_LINE: f64 = 20.0;
const INSET: f64 = 16.0;
const MIN_WIDTH: f64 = 160.0;
const MAX_MEMBERS: usize = 5;
const MORE_MEMBERS: &str = "…";
const LINE_WIDTH: f64 = 1.0;
const FOCAL_LINE_WIDTH: f64 = 1.4;

/// What a class is besides a plain class, from the prefix of its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stereotype {
    Plain,
    Interface,
    Abstract,
}

const STEREOTYPE_PREFIXES: [(&str, Stereotype); 4] = [
    ("«interface» ", Stereotype::Interface),
    ("<<interface>> ", Stereotype::Interface),
    ("interface ", Stereotype::Interface),
    ("abstract ", Stereotype::Abstract),
];

/// A class box's content: its name, stereotype and members, operations being the items with a `(`.
#[derive(Clone, Debug, PartialEq)]
pub struct Class {
    pub name: String,
    pub stereotype: Stereotype,
    pub attributes: Vec<String>,
    pub operations: Vec<String>,
}

impl Class {
    /// The class a node's name and bracket items describe.
    pub fn of(content: &NodeContent) -> Class {
        let (stereotype, name) = stereotype_of(&content.lines.join(" "));
        let (operations, attributes) = content.fields.iter().cloned().partition(|item| item.contains('('));
        Class { name, stereotype, attributes, operations }
    }

    /// The member compartments drawn: attributes, then operations, each without an empty one and cut to
    /// five members and a `…` line.
    pub fn compartments(&self) -> Vec<Vec<String>> {
        [&self.attributes, &self.operations].into_iter().filter(|members| !members.is_empty()).map(|members| shown(members)).collect()
    }

    fn width(&self) -> f64 {
        let name = sans_width(&self.name, NAME_SIZE);
        let stereotype = if self.stereotype == Stereotype::Interface { mono_width(STEREOTYPE, STEREOTYPE_SIZE, STEREOTYPE_TRACKING) } else { 0.0 };
        let members = self.compartments().iter().flatten().map(|member| mono_width(member, MEMBER_SIZE, 0.0)).fold(0.0, f64::max);
        snap_up(name.max(stereotype).max(members) + 2.0 * INSET).max(MIN_WIDTH)
    }

    fn height(&self) -> f64 {
        NAME_BAND + self.compartments().iter().map(|members| compartment_height(members.len())).sum::<f64>()
    }
}

fn stereotype_of(name: &str) -> (Stereotype, String) {
    for (prefix, stereotype) in STEREOTYPE_PREFIXES {
        let leads = name.get(..prefix.len()).is_some_and(|start| start.eq_ignore_ascii_case(prefix));
        if leads && name.len() > prefix.len() {
            return (stereotype, name[prefix.len()..].trim().to_string());
        }
    }
    (Stereotype::Plain, name.to_string())
}

fn shown(members: &[String]) -> Vec<String> {
    let mut lines: Vec<String> = members.iter().take(MAX_MEMBERS).cloned().collect();
    if members.len() > MAX_MEMBERS {
        lines.push(MORE_MEMBERS.to_string());
    }
    lines
}

fn compartment_height(lines: usize) -> f64 {
    (lines + 1) as f64 * MEMBER_LINE
}

/// The six UML relationships, named by the leading words of an edge label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relationship {
    Inheritance,
    Realization,
    Composition,
    Aggregation,
    Association,
    Dependency,
}

const KEYWORDS: [(&str, Relationship); 16] = [
    ("extends", Relationship::Inheritance),
    ("inherits from", Relationship::Inheritance),
    ("inherits", Relationship::Inheritance),
    ("is a", Relationship::Inheritance),
    ("is an", Relationship::Inheritance),
    ("implements", Relationship::Realization),
    ("realizes", Relationship::Realization),
    ("realises", Relationship::Realization),
    ("owns", Relationship::Composition),
    ("composed of", Relationship::Composition),
    ("contains", Relationship::Composition),
    ("has", Relationship::Aggregation),
    ("aggregates", Relationship::Aggregation),
    ("uses", Relationship::Dependency),
    ("depends on", Relationship::Dependency),
    ("calls", Relationship::Dependency),
];

impl Relationship {
    const ALL: [Relationship; 6] = [
        Relationship::Inheritance,
        Relationship::Realization,
        Relationship::Composition,
        Relationship::Aggregation,
        Relationship::Association,
        Relationship::Dependency,
    ];

    /// The relationship an edge label names with its leading words, and the rest of the label.
    pub fn of(label: &str) -> (Relationship, String) {
        let words: Vec<&str> = label.split_whitespace().collect();
        for (phrase, relationship) in KEYWORDS {
            let keyword: Vec<&str> = phrase.split(' ').collect();
            let leads = words.len() >= keyword.len() && words.iter().zip(&keyword).all(|(word, key)| word.eq_ignore_ascii_case(key));
            if leads {
                return (relationship, words[keyword.len()..].join(" "));
            }
        }
        (Relationship::Association, words.join(" "))
    }

    /// True for the relationships that point from a child up to its parent.
    pub fn generalizes(self) -> bool {
        matches!(self, Relationship::Inheritance | Relationship::Realization)
    }

    /// The line ends at the source and at the target.
    pub fn markers(self) -> (Marker, Marker) {
        match self {
            Relationship::Inheritance | Relationship::Realization => (Marker::None, Marker::Triangle),
            Relationship::Composition => (Marker::Diamond, Marker::None),
            Relationship::Aggregation => (Marker::HollowDiamond, Marker::None),
            Relationship::Association | Relationship::Dependency => (Marker::None, Marker::Open),
        }
    }

    pub fn dash(self) -> Option<&'static str> {
        match self {
            Relationship::Realization => Some("5,4"),
            Relationship::Dependency => Some("4,3"),
            _ => None,
        }
    }

    fn paint(self, focal: bool) -> Paint {
        let paint = if focal { Paint::line(Tone::Accent, FOCAL_LINE_WIDTH) } else { Paint::line(Tone::Muted, LINE_WIDTH) };
        Paint { dash: self.dash(), ..paint }
    }

    fn name(self) -> &'static str {
        match self {
            Relationship::Inheritance => "INHERITANCE",
            Relationship::Realization => "REALIZATION",
            Relationship::Composition => "COMPOSITION",
            Relationship::Aggregation => "AGGREGATION",
            Relationship::Association => "ASSOCIATION",
            Relationship::Dependency => "DEPENDENCY",
        }
    }

    fn legend_key(self) -> LegendKey {
        let (start, end) = self.markers();
        let marker = if end == Marker::None { start } else { end };
        LegendKey::new(Swatch::Line(self.paint(false), marker), self.name())
    }
}

/// Lays out `view` as this kind.
pub fn build(view: &View, params: &Params) -> Scene {
    let (_, contents) = unique_nodes(view);
    let classes: Vec<Class> = contents.iter().map(Class::of).collect();
    let focal = contents.iter().position(|content| content.treatment == Treatment::Focal);
    let sized: Vec<NodeContent> =
        contents.into_iter().zip(&classes).enumerate().map(|(index, (content, class))| sized(content, class, Some(index) == focal)).collect();
    let (layout_view, relationships) = layout_view(view);
    let layout_params = Params { direction: Some(Direction::Down), uniform_rank_size: false, ..*params };
    let mut scene = layered_with(&layout_view, &layout_params, sized);
    for edge in &mut scene.edges {
        dress(edge, relationships.get(&edge.id).copied().unwrap_or(Relationship::Association), focal);
    }
    let trims: Vec<(f64, f64)> = scene.edges.iter().map(marker_trims).collect();
    let texts: Vec<String> = scene.edges.iter().map(|edge| edge.label.as_ref().map(|label| label.text.clone()).unwrap_or_default()).collect();
    relabel(&mut scene, texts, &trims, &[]);
    for (node, class) in scene.nodes.iter_mut().zip(&classes) {
        node.figures = class_figures(class, node.rect, node.content.treatment == Treatment::Focal);
    }
    scene.legend = Some(Relationship::ALL.iter().map(|relationship| relationship.legend_key()).collect());
    scene.measured()
}

fn sized(content: NodeContent, class: &Class, focal: bool) -> NodeContent {
    let treatment = if focal { Treatment::Focal } else { Treatment::Service };
    NodeContent { lines: vec![class.name.clone()], width: class.width(), height: class.height(), treatment, ..content }
}

/// The view the layered layout places: relationship keywords taken out of the labels, and every
/// inheritance and realization turned around so the parent ranks above its children.
fn layout_view(view: &View) -> (View, HashMap<String, Relationship>) {
    let mut relationships = HashMap::new();
    let mut edges = Vec::with_capacity(view.edges.len());
    for edge in &view.edges {
        let (relationship, rest) = Relationship::of(&edge.label);
        let id = if edge.id.is_empty() { format!("{}->{}", edge.source, edge.target) } else { edge.id.clone() };
        relationships.insert(id.clone(), relationship);
        edges.push(if relationship.generalizes() {
            let (name, _) = cardinality::split(&rest.to_uppercase());
            EdgeSpec { id, source: edge.target.clone(), target: edge.source.clone(), label: name, ..edge.clone() }
        } else {
            EdgeSpec { id, label: rest, ..edge.clone() }
        });
    }
    (View { edges, ..view.clone() }, relationships)
}

fn dress(edge: &mut SceneEdge, relationship: Relationship, focal: Option<usize>) {
    if relationship.generalizes() {
        edge.points.reverse();
        std::mem::swap(&mut edge.source, &mut edge.target);
    }
    edge.focal = relationship.generalizes() && focal == Some(edge.target);
    edge.markers = Some(relationship.markers());
    edge.paint = Some(relationship.paint(edge.focal));
}

fn marker_length(marker: Marker) -> f64 {
    match marker {
        Marker::None => 0.0,
        Marker::Arrow => 8.0,
        Marker::Open => 11.0,
        Marker::Triangle => 13.0,
        Marker::Diamond | Marker::HollowDiamond => 17.0,
    }
}

fn marker_trims(edge: &SceneEdge) -> (f64, f64) {
    let (start, end) = edge.markers.unwrap_or_default();
    (marker_length(start), marker_length(end))
}

/// Places the cardinality marks and labels of `scene`'s edges again along their final paths, the way
/// the layered layout does: `texts[edge]` is an edge's label (empty for none), `trims[edge]` how much
/// of its line the start and end markers cover, which the marks keep clear of, and `taken` further boxes
/// labels stay off.
pub fn relabel(scene: &mut Scene, texts: Vec<String>, trims: &[(f64, f64)], taken: &[Rect]) {
    let rects: Vec<Rect> = scene.nodes.iter().map(|node| node.rect).chain(taken.iter().copied()).collect();
    let paths: Vec<Vec<Point>> = scene.edges.iter().map(|edge| edge.points.clone()).collect();
    let ends: Vec<Option<Ends>> = scene.edges.iter().map(ends_of).collect();
    let clear: Vec<Vec<Point>> = paths.iter().zip(trims).map(|(path, &trim)| trimmed(path, trim)).collect();
    let mut marks = cardinality::place(&ends, &clear, &rects);
    let obstacles: Vec<Rect> = rects.iter().copied().chain(marks.iter().flatten().map(|mark| mark.rect)).collect();
    let requests: Vec<LabelRequest> = texts
        .iter()
        .enumerate()
        .filter(|(_, text)| !text.is_empty())
        .map(|(edge, text)| LabelRequest { edge, mask_width: mask_width(text), preferred: None, overhang: false })
        .collect();
    let mut masks: HashMap<usize, Rect> = requests.iter().map(|request| request.edge).zip(label::place(&requests, &paths, &obstacles)).collect();
    for (index, (edge, text)) in scene.edges.iter_mut().zip(texts).enumerate() {
        edge.label = masks.remove(&index).map(|rect| LabelBox { rect, text });
        edge.marks = std::mem::take(&mut marks[index]);
    }
}

fn ends_of(edge: &SceneEdge) -> Option<Ends> {
    match edge.marks.as_slice() {
        [source, target] => Some(Ends { source: source.text.clone(), target: target.text.clone() }),
        _ => None,
    }
}

fn trimmed(path: &[Point], (start, end): (f64, f64)) -> Vec<Point> {
    let mut points = path.to_vec();
    let last = points.len().saturating_sub(1);
    if last > 0 {
        points[0] = toward(path[0], path[1], start);
        points[last] = toward(path[last], path[last - 1], end);
    }
    points
}

fn toward(from: Point, to: Point, by: f64) -> Point {
    let length = Segment::new(from, to).length();
    if length <= 0.0 {
        return from;
    }
    let share = by.min(length / 2.0) / length;
    Point::new(from.x + (to.x - from.x) * share, from.y + (to.y - from.y) * share)
}

fn class_figures(class: &Class, rect: Rect, focal: bool) -> Vec<Figure> {
    let (fill, stroke, rule) = if focal { (Tone::AccentTint, Tone::Accent, Tone::AccentWash(0.4)) } else { (Tone::Fill, Tone::Ink, Tone::Wash(0.22)) };
    let mut figures = vec![Figure::rect(rect, 6.0, Paint::shape(fill, stroke, 1.0))];
    figures.extend(name_figures(class, rect, focal));
    let mut top = rect.y + NAME_BAND;
    for members in class.compartments() {
        figures.push(Figure::line(Point::new(rect.x, top), Point::new(rect.right(), top), Paint::line(rule, 1.0)));
        for (index, member) in members.iter().enumerate() {
            let at = Point::new(rect.x + INSET, top + (index + 1) as f64 * MEMBER_LINE);
            figures.push(Figure::Text(Text::mono(at, member.as_str(), MEMBER_SIZE).anchor(Anchor::Start).tone(Tone::Ink)));
        }
        top += compartment_height(members.len());
    }
    figures
}

fn name_figures(class: &Class, rect: Rect, focal: bool) -> Vec<Figure> {
    let mut figures = Vec::new();
    let mut baseline = NAME_BASELINE;
    if class.stereotype == Stereotype::Interface {
        let tone = if focal { Tone::Accent } else { Tone::Muted };
        let at = Point::new(rect.cx(), rect.y + STEREOTYPE_BASELINE);
        figures.push(Figure::Text(Text::mono(at, STEREOTYPE, STEREOTYPE_SIZE).tracking(STEREOTYPE_TRACKING).tone(tone)));
        baseline = STEREOTYPED_NAME_BASELINE;
    }
    let name = Text::sans(Point::new(rect.cx(), rect.y + baseline), class.name.as_str(), NAME_SIZE).weight(600);
    figures.push(Figure::Text(if class.stereotype == Stereotype::Abstract { name.italic() } else { name }));
    figures
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::check;
    use crate::spec::{Kind, NodeSpec};

    fn class(label: &str) -> Class {
        let spec = NodeSpec { id: "c".into(), label: label.into(), shape: Default::default(), focal: false };
        Class::of(&crate::node::content(&spec, Kind::UmlClass, Default::default()))
    }

    fn edge(source: &str, target: &str, label: &str) -> EdgeSpec {
        EdgeSpec { id: format!("{source}->{target}"), source: source.into(), target: target.into(), label: label.into(), style: Default::default(), focal: false }
    }

    fn node(id: &str, label: &str, focal: bool) -> NodeSpec {
        NodeSpec { id: id.into(), label: label.into(), shape: Default::default(), focal }
    }

    fn payments() -> View {
        View {
            kind: Kind::UmlClass,
            nodes: vec![
                node("method", "interface PaymentMethod (+ authorize(amount: Money): Auth)", true),
                node("card", "Card (- number: String)", false),
                node("bank", "BankTransfer (- iban: String)", false),
                node("order", "Order (- id: UUID, + place(): Receipt)", false),
                node("line", "OrderLine (- qty: Int)", false),
            ],
            edges: vec![
                edge("card", "method", "implements"),
                edge("bank", "method", "realizes"),
                edge("order", "line", "owns 1:1..*"),
                edge("order", "method", "uses"),
            ],
            ..View::default()
        }
    }

    fn scene_edge<'a>(scene: &'a Scene, id: &str) -> &'a SceneEdge {
        scene.edges.iter().find(|edge| edge.id == id).expect("a known edge")
    }

    #[test]
    fn items_with_a_bracket_are_operations_and_the_rest_attributes() {
        let order = class("Order (+ id: UUID, - total: Money, + place(order, now): Receipt, + total(): Money)");
        assert_eq!(order.name, "Order");
        assert_eq!(order.attributes, vec!["+ id: UUID", "- total: Money"]);
        assert_eq!(order.operations, vec!["+ place(order, now): Receipt", "+ total(): Money"]);
    }

    #[test]
    fn an_interface_prefix_becomes_a_stereotype_and_abstract_an_italic_name() {
        assert_eq!(class("interface Shape (+ area(): Float)").stereotype, Stereotype::Interface);
        assert_eq!(class("«interface» Shape").name, "Shape");
        let base = class("abstract Animal (- name: String)");
        assert_eq!((base.stereotype, base.name.as_str()), (Stereotype::Abstract, "Animal"));
        let figures = class_figures(&base, Rect::new(0.0, 0.0, 160.0, 80.0), false);
        assert!(figures.iter().any(|figure| matches!(figure, Figure::Text(text) if text.text == "Animal" && text.italic)));
        let shape = class_figures(&class("interface Shape"), Rect::new(0.0, 0.0, 160.0, 40.0), false);
        assert!(shape.iter().any(|figure| matches!(figure, Figure::Text(text) if text.text == STEREOTYPE && text.size == 8.0)));
    }

    #[test]
    fn a_compartment_shows_five_members_and_then_an_ellipsis() {
        let crowded = class("Big (a: A, b: B, c: C, d: D, e: E, f: F, g: G)");
        assert_eq!(crowded.compartments(), vec![vec!["a: A", "b: B", "c: C", "d: D", "e: E", "…"]]);
    }

    #[test]
    fn an_empty_compartment_is_left_out_and_heights_follow_content() {
        let interface = class("interface Shape (+ area(): Float)");
        assert_eq!(interface.compartments(), vec![vec!["+ area(): Float"]]);
        assert_eq!(interface.height(), NAME_BAND + 2.0 * MEMBER_LINE);
        assert_eq!(class("Card (- number: String, - expiry: Date, + pay(): Receipt)").height(), NAME_BAND + 3.0 * MEMBER_LINE + 2.0 * MEMBER_LINE);
    }

    #[test]
    fn every_relationship_keyword_picks_its_line_end_and_dash() {
        let cases = [
            ("extends", (Marker::None, Marker::Triangle), None),
            ("Implements", (Marker::None, Marker::Triangle), Some("5,4")),
            ("owns", (Marker::Diamond, Marker::None), None),
            ("has", (Marker::HollowDiamond, Marker::None), None),
            ("depends on", (Marker::None, Marker::Open), Some("4,3")),
            ("places", (Marker::None, Marker::Open), None),
        ];
        for (label, markers, dash) in cases {
            let (relationship, _) = Relationship::of(label);
            assert_eq!((relationship.markers(), relationship.dash()), (markers, dash), "{label}");
        }
        assert_eq!(Relationship::of("is a").0, Relationship::Inheritance);
        assert_eq!(Relationship::of("composed of").0, Relationship::Composition);
        assert_eq!(Relationship::of("aggregates").0, Relationship::Aggregation);
        assert_eq!(Relationship::of("calls").0, Relationship::Dependency);
        assert_eq!(Relationship::of("hash of").0, Relationship::Association);
    }

    #[test]
    fn the_relationship_keyword_is_not_repeated_in_the_label() {
        assert_eq!(Relationship::of("owns the lines 1:N"), (Relationship::Composition, "the lines 1:N".to_string()));
        let scene = build(&payments(), &Params::default());
        assert!(scene_edge(&scene, "order->method").label.is_none());
        assert!(scene_edge(&scene, "card->method").label.is_none());
    }

    #[test]
    fn a_drawn_edge_carries_its_relationship_markers_and_dash() {
        let scene = build(&payments(), &Params::default());
        let realization = scene_edge(&scene, "bank->method");
        assert_eq!(realization.markers, Some((Marker::None, Marker::Triangle)));
        assert_eq!(realization.paint.and_then(|paint| paint.dash), Some("5,4"));
        let dependency = scene_edge(&scene, "order->method");
        assert_eq!(dependency.paint.and_then(|paint| paint.dash), Some("4,3"));
    }

    #[test]
    fn a_composition_diamond_sits_at_the_owner_and_its_multiplicities_clear_it() {
        let scene = build(&payments(), &Params::default());
        let composition = scene_edge(&scene, "order->line");
        let owner = scene.nodes.iter().find(|node| node.id == "order").expect("order").rect;
        let (start, next) = (composition.points[0], composition.points[1]);
        assert_eq!(start.y, owner.bottom());
        let marks: Vec<&str> = composition.marks.iter().map(|mark| mark.text.as_str()).collect();
        assert_eq!(marks, vec!["1", "1..*"]);
        let mark = composition.marks[0].rect;
        let gap = if start.x == next.x { (mark.y - start.y).max(start.y - mark.bottom()) } else { (mark.x - start.x).max(start.x - mark.right()) };
        assert!(gap >= marker_length(Marker::Diamond), "the mark covers the diamond: {gap}");
    }

    #[test]
    fn parents_rank_above_their_children_and_the_arrow_ends_at_the_parent() {
        let scene = build(&payments(), &Params::default());
        let rect = |id: &str| scene.nodes.iter().find(|node| node.id == id).expect("a class").rect;
        for child in ["card", "bank"] {
            assert!(rect("method").bottom() < rect(child).y, "{child}");
            let edge = scene_edge(&scene, &format!("{child}->method"));
            assert_eq!(edge.points.last().map(|point| point.y), Some(rect("method").bottom()));
            assert_eq!(scene.nodes[edge.target].id, "method");
        }
        assert!(check(&scene).0.is_empty(), "{:#?}", check(&scene).0);
    }

    #[test]
    fn the_focal_class_and_its_inbound_realizations_are_the_only_accent() {
        let scene = build(&payments(), &Params::default());
        let accent: Vec<&str> = scene.edges.iter().filter(|edge| edge.paint.is_some_and(|paint| paint.stroke == Tone::Accent)).map(|edge| edge.id.as_str()).collect();
        assert_eq!(accent, vec!["card->method", "bank->method"]);
        let method = scene.nodes.iter().find(|node| node.id == "method").expect("method");
        assert!(matches!(method.figures[0], Figure::Rect { paint, .. } if paint.stroke == Tone::Accent && paint.fill == Tone::AccentTint));
        let card = scene.nodes.iter().find(|node| node.id == "card").expect("card");
        assert!(matches!(card.figures[0], Figure::Rect { paint, .. } if paint.stroke == Tone::Ink));
    }

    #[test]
    fn the_legend_lists_all_six_relationships() {
        let scene = build(&payments(), &Params::default());
        let names: Vec<String> = scene.legend.expect("own legend").into_iter().map(|key| key.text).collect();
        assert_eq!(names, vec!["INHERITANCE", "REALIZATION", "COMPOSITION", "AGGREGATION", "ASSOCIATION", "DEPENDENCY"]);
    }
}
