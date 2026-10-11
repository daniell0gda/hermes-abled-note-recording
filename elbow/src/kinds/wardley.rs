//! Wardley map: components placed by visibility (their depth in the dependency chain, most visible on
//! top) and evolution (four stage bands from genesis to commodity), joined by thin straight dependency
//! links. The stage is the first stage word in the label's bracket, `Name (stage, qualifier)`; the
//! focal component is the evolving one and carries a dashed accent arrow toward the next band.

use std::collections::BTreeMap;

use crate::figure::{Anchor, Figure, LegendKey, Marker, Paint, Swatch, Text, Tone};
use crate::geom::{snap, snap_up, Point, Rect};
use crate::layout::back_edges;
use crate::node::{field_list, name_and_sublabel, NodeContent, Treatment};
use crate::params::Params;
use crate::scene::{known_edges, unique_nodes, Bend, EdgeInput, Scene, SceneEdge, SceneNode};
use crate::spec::View;
use crate::text::{mono_width, sans_width};

const DOT: f64 = 6.0;
const NAME_SIZE: f64 = 12.0;
const NAME_RISE: f64 = 12.0;
const SUB_SIZE: f64 = 9.0;
const SUB_DROP: f64 = 18.0;
const SIDE_GAP: f64 = 12.0;
const SIDE_SUB_DROP: f64 = 16.0;
const TEXT_PADDING: f64 = 2.0;
const MASK_RADIUS: f64 = 2.0;
const ROW: f64 = 72.0;
const PLACEMENT_SWEEPS: usize = 3;
const SLOT_GAP: f64 = 24.0;
const BAND_PADDING: f64 = 16.0;
const BAND_SLACK: f64 = 96.0;
const MIN_BAND: f64 = 200.0;
const ARROW_START: f64 = 9.0;
const ARROW_END: f64 = 72.0;
const ARROW_ROOM: f64 = 80.0;
const MAP_TOP: f64 = 48.0;
const MAP_BOTTOM: f64 = 40.0;
const AXIS_LABEL_GAP: f64 = 8.0;
const AXIS_SIZE: f64 = 9.0;
const AXIS_LINE: f64 = 12.0;
const AXIS_TRACKING: f64 = 0.14;
const BAND_LABEL_SIZE: f64 = 8.0;
const BAND_LABEL_DROP: f64 = 16.0;
const BAND_TRACKING: f64 = 0.18;
const OFFSET_STEPS: usize = 4;
const DOT_COST: f64 = 8.0;
const LABEL_COST: f64 = 4.0;
const SPAN_COST: f64 = 0.001;
const LINK_COST: f64 = 4.0;
const OVERLAP_COST: f64 = 8.0;
const RULE_COST: f64 = 2.0;
const BANDS: usize = 4;

/// The evolution stage of a component: the four bands of the x axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Genesis,
    Custom,
    Product,
    Commodity,
}

const STAGES: [Stage; BANDS] = [Stage::Genesis, Stage::Custom, Stage::Product, Stage::Commodity];

impl Stage {
    /// The stage a bracket word names, with its common synonyms.
    pub fn parse(word: &str) -> Option<Stage> {
        match word.trim().to_lowercase().as_str() {
            "genesis" => Some(Stage::Genesis),
            "custom" | "custom-built" | "custom built" | "bespoke" => Some(Stage::Custom),
            "product" | "rental" | "off-the-shelf" | "off the shelf" => Some(Stage::Product),
            "commodity" | "utility" | "cloud" => Some(Stage::Commodity),
            _ => None,
        }
    }

    fn band(self) -> usize {
        self as usize
    }

    fn label(self) -> &'static str {
        match self {
            Stage::Genesis => "GENESIS",
            Stage::Custom => "CUSTOM-BUILT",
            Stage::Product => "PRODUCT",
            Stage::Commodity => "COMMODITY",
        }
    }
}

/// What a component's label says: its name, its stage (product when no bracket word names one) and
/// the other bracket items as a sublabel.
#[derive(Debug, PartialEq)]
pub struct Component {
    pub name: String,
    pub stage: Stage,
    pub sublabel: Option<String>,
}

/// Reads `Name (stage, qualifier)`.
pub fn component(label: &str) -> Component {
    let (name, bracket) = name_and_sublabel(label);
    let items = bracket.as_deref().map(field_list).unwrap_or_default();
    let stage_at = items.iter().position(|item| Stage::parse(item).is_some());
    let stage = stage_at.and_then(|at| Stage::parse(&items[at])).unwrap_or(Stage::Product);
    let rest: Vec<&str> = items.iter().enumerate().filter(|(at, _)| Some(*at) != stage_at).map(|(_, item)| item.as_str()).collect();
    Component { name, stage, sublabel: (!rest.is_empty()).then(|| rest.join(", ")) }
}

/// The depth of every node below the components nobody depends on, by the longest dependency path.
/// Edges that close a cycle do not count.
pub fn depths(count: usize, pairs: &[(usize, usize)]) -> Vec<usize> {
    let between: Vec<(usize, usize)> = pairs.iter().copied().filter(|(a, b)| a != b).collect();
    let reversed = back_edges(count, &between, None);
    let forward: Vec<(usize, usize)> = between.into_iter().zip(reversed).filter(|(_, back)| !back).map(|(pair, _)| pair).collect();
    let mut depth = vec![0; count];
    for _ in 0..count {
        let mut changed = false;
        for &(a, b) in &forward {
            if depth[b] < depth[a] + 1 {
                depth[b] = depth[a] + 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    depth
}

/// Lays out `view` as a Wardley map.
pub fn build(view: &View, _params: &Params) -> Scene {
    let (ids, contents) = unique_nodes(view);
    let edges = known_edges(view, &ids);
    let map = Map::plan(view, &ids, &contents, &edges);
    let labels = map.labels();
    let links = map.links(&map.centres);
    let scene_edges = edges.iter().map(|edge| link(edge, &map.centres)).collect();
    let nodes = ids
        .into_iter()
        .zip(contents)
        .enumerate()
        .map(|(node, (id, content))| map.node(node, &labels[node], &links, id, content))
        .collect();
    let overlay = map.arrow().into_iter().collect();
    let legend = legend(map.marks.iter().any(|mark| mark.evolving));
    Scene { nodes, edges: scene_edges, decor: map.axes(), overlay, legend: Some(legend), ..Scene::empty(view.kind) }.measured()
}

fn link(edge: &EdgeInput, centres: &[Point]) -> SceneEdge {
    SceneEdge {
        bend: Bend::Free,
        markers: Some((Marker::None, Marker::None)),
        paint: Some(Paint::line(Tone::Muted, 0.8)),
        ..SceneEdge::new(edge, vec![centres[edge.source], centres[edge.target]])
    }
}

struct Mark {
    component: Component,
    depth: usize,
    evolving: bool,
}

impl Mark {
    fn has_arrow(&self) -> bool {
        self.evolving && self.component.stage != Stage::Commodity
    }

    fn half_width(&self) -> f64 {
        let name = sans_width(&self.component.name, NAME_SIZE);
        let sub = self.component.sublabel.as_deref().map_or(0.0, |sub| mono_width(sub, SUB_SIZE, 0.0));
        name.max(sub).max(2.0 * DOT) / 2.0 + TEXT_PADDING
    }

    fn right_reach(&self) -> f64 {
        if self.has_arrow() {
            self.half_width().max(ARROW_ROOM)
        } else {
            self.half_width()
        }
    }

    fn slot(&self) -> f64 {
        self.half_width() + self.right_reach()
    }

    fn texts(&self, centre: Point, side: Side) -> Vec<Text> {
        let (name_at, sub_at, anchor) = match side {
            Side::Above => (Point::new(centre.x, centre.y - NAME_RISE), Point::new(centre.x, centre.y + SUB_DROP), Anchor::Middle),
            Side::Right => {
                let x = centre.x + SIDE_GAP;
                (Point::new(x, centre.y + NAME_SIZE / 3.0), Point::new(x, centre.y + NAME_SIZE / 3.0 + SIDE_SUB_DROP), Anchor::Start)
            }
            Side::Left => {
                let x = centre.x - SIDE_GAP;
                (Point::new(x, centre.y + NAME_SIZE / 3.0), Point::new(x, centre.y + NAME_SIZE / 3.0 + SIDE_SUB_DROP), Anchor::End)
            }
            Side::Below => {
                let name_y = centre.y + DOT + SIDE_GAP + NAME_SIZE * 2.0 / 3.0;
                (Point::new(centre.x, name_y), Point::new(centre.x, name_y + SIDE_SUB_DROP), Anchor::Middle)
            }
        };
        let name = Text::sans(name_at, self.component.name.clone(), NAME_SIZE).weight(600).anchor(anchor);
        let sub = self.component.sublabel.as_ref().map(|sub| Text::mono(sub_at, sub.clone(), SUB_SIZE).anchor(anchor));
        std::iter::once(name).chain(sub).collect()
    }
}

/// Where a component's name sits: above its dot (the default), or beside or below it when a link or
/// another component is in the way.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Side {
    Above,
    Right,
    Left,
    Below,
}

const SIDES: [Side; 4] = [Side::Above, Side::Right, Side::Left, Side::Below];

/// A straight line between two dots (a dependency, or the evolving arrow) and the nodes it ends at.
struct Link {
    a: Point,
    b: Point,
    ends: (usize, usize),
}

impl Link {
    fn touches(&self, node: usize) -> bool {
        self.ends.0 == node || self.ends.1 == node
    }
}

/// The placed map: every component's dot centre, the band width and the depth rows.
struct Map {
    marks: Vec<Mark>,
    pairs: Vec<(usize, usize)>,
    centres: Vec<Point>,
    band_width: f64,
}

impl Map {
    fn plan(view: &View, ids: &[String], contents: &[NodeContent], edges: &[EdgeInput]) -> Map {
        let pairs: Vec<(usize, usize)> = edges.iter().map(|edge| (edge.source, edge.target)).collect();
        let depth = depths(contents.len(), &pairs);
        let focal = contents.iter().position(|content| content.treatment == Treatment::Focal);
        let marks: Vec<Mark> = ids
            .iter()
            .enumerate()
            .map(|(node, id)| {
                let label = view.nodes.iter().find(|spec| &spec.id == id).map_or("", |spec| spec.label.as_str());
                Mark { component: component(label), depth: depth[node], evolving: Some(node) == focal }
            })
            .collect();
        let groups = groups(&marks);
        let widest = groups.values().map(|members| group_width(&marks, members)).fold(0.0, f64::max);
        let band_width = snap_up(MIN_BAND.max(widest + 2.0 * BAND_PADDING + BAND_SLACK));
        let mut map = Map { centres: vec![Point::new(f64::NAN, f64::NAN); marks.len()], marks, pairs, band_width };
        for _ in 0..PLACEMENT_SWEEPS {
            for ((depth, band), members) in &groups {
                map.place_group(*depth, *band, members);
            }
        }
        map
    }

    fn place_group(&mut self, depth: usize, band: usize, members: &[usize]) {
        let total = group_width(&self.marks, members);
        let free = (self.band_width - 2.0 * BAND_PADDING - total).max(0.0);
        let centre = (band as f64 + 0.5) * self.band_width;
        let y = depth as f64 * ROW;
        let cost = |offset: f64| {
            let mut tried = self.centres.clone();
            self.lay_out(&mut tried, members, centre + offset - total / 2.0, y);
            self.placement_cost(&tried)
        };
        let best = offsets(free).map(|offset| (offset, cost(offset))).min_by(|a, b| a.1.total_cmp(&b.1)).map_or(0.0, |(offset, _)| offset);
        let mut centres = std::mem::take(&mut self.centres);
        self.lay_out(&mut centres, members, centre + best - total / 2.0, y);
        self.centres = centres;
    }

    fn lay_out(&self, centres: &mut [Point], members: &[usize], left: f64, y: f64) {
        let mut x = left;
        for &node in members {
            let mark = &self.marks[node];
            centres[node] = Point::new(snap(x + mark.half_width()), y);
            x += mark.slot() + SLOT_GAP;
        }
    }

    fn links(&self, centres: &[Point]) -> Vec<Link> {
        let placed = |node: usize| !centres[node].x.is_nan();
        let dependencies = self
            .pairs
            .iter()
            .filter(|(a, b)| placed(*a) && placed(*b))
            .map(|&(a, b)| Link { a: centres[a], b: centres[b], ends: (a, b) });
        let arrow = self.arrow_node().filter(|&node| placed(node)).map(|node| {
            let (start, end) = arrow_ends(centres[node]);
            Link { a: start, b: end, ends: (node, node) }
        });
        dependencies.chain(arrow).collect()
    }

    fn arrow_node(&self) -> Option<usize> {
        self.marks.iter().position(Mark::has_arrow)
    }

    /// How many links cross the dots of components they do not end at, plus, for every component, the
    /// links crossing its label on its clearest side; shorter links break ties.
    fn placement_cost(&self, centres: &[Point]) -> f64 {
        let links = self.links(centres);
        let mut cost = SPAN_COST * links.iter().map(|link| (link.b.x - link.a.x).abs()).sum::<f64>();
        for (node, centre) in centres.iter().enumerate().filter(|(_, centre)| !centre.x.is_nan()) {
            let dot = dot_box(*centre);
            let on_dot = links.iter().filter(|link| !link.touches(node) && crosses(link, &dot)).count();
            let on_label = SIDES.iter().map(|&side| crossings(&self.marks[node].texts(*centre, side), &links)).min().unwrap_or(0);
            cost += DOT_COST * on_dot as f64 + LABEL_COST * on_label as f64;
        }
        cost
    }

    /// Picks a side for every component's label, in order, against the links, the other dots, the
    /// components already labelled, the band rules and the map's edges.
    fn labels(&self) -> Vec<Vec<Text>> {
        let links = self.links(&self.centres);
        let dots: Vec<Rect> = self.centres.iter().map(|centre| dot_box(*centre)).collect();
        let mut taken: Vec<Rect> = Vec::new();
        let mut chosen = Vec::with_capacity(self.marks.len());
        for (node, mark) in self.marks.iter().enumerate() {
            let score = |side: Side| self.label_cost(node, &mark.texts(self.centres[node], side), &links, &dots, &taken);
            let side = SIDES.into_iter().map(|side| (side, score(side))).min_by(|a, b| a.1.total_cmp(&b.1)).map_or(Side::Above, |(side, _)| side);
            let texts = mark.texts(self.centres[node], side);
            taken.push(texts.iter().map(text_box).fold(dots[node], union));
            chosen.push(texts);
        }
        chosen
    }

    fn label_cost(&self, node: usize, texts: &[Text], links: &[Link], dots: &[Rect], taken: &[Rect]) -> f64 {
        let boxes: Vec<Rect> = texts.iter().map(text_box).collect();
        let whole = boxes.iter().copied().fold(dots[node], union);
        let on_dots = dots.iter().enumerate().filter(|(other, dot)| *other != node && whole.overlaps(dot)).count();
        let on_components = taken.iter().filter(|other| whole.overlaps(other)).count();
        let mut cost = OVERLAP_COST * (on_dots + on_components) as f64 + LINK_COST * crossings(texts, links) as f64;
        for rect in &boxes {
            let rules = (1..BANDS).filter(|band| rect.x < self.band_x(*band) && rect.right() > self.band_x(*band)).count();
            let outside = rect.x < 0.0 || rect.right() > self.band_x(BANDS);
            cost += OVERLAP_COST * f64::from(u8::from(outside)) + RULE_COST * rules as f64;
        }
        cost
    }

    fn band_x(&self, band: usize) -> f64 {
        band as f64 * self.band_width
    }

    fn bottom(&self) -> f64 {
        self.marks.iter().map(|mark| mark.depth).max().unwrap_or(0) as f64 * ROW + MAP_BOTTOM
    }

    /// The component's hit area, dot and label; a label some link still crosses gets a paper mask, as
    /// a connector label would.
    fn node(&self, node: usize, texts: &[Text], links: &[Link], id: String, content: NodeContent) -> SceneNode {
        let centre = self.centres[node];
        let rect = texts.iter().map(text_box).fold(dot_box(centre), union);
        let paint = if self.marks[node].evolving {
            Paint::shape(Tone::AccentTint, Tone::Accent, 1.5)
        } else {
            Paint::shape(Tone::Paper, Tone::Ink, 1.0)
        };
        let mut figures = vec![Figure::hit_area(rect), Figure::circle(centre, DOT, paint)];
        for text in texts {
            let mask = text_box(text);
            if links.iter().any(|link| crosses(link, &mask)) {
                figures.push(Figure::rect(mask, MASK_RADIUS, Paint::filled(Tone::Paper)));
            }
            figures.push(Figure::Text(text.clone()));
        }
        SceneNode { figures, ..SceneNode::new(id, rect, content) }
    }

    fn arrow(&self) -> Option<Figure> {
        let (start, end) = arrow_ends(self.centres[self.arrow_node()?]);
        Some(Figure::Line { points: vec![start, end], paint: Paint::line(Tone::Accent, 1.5).dashed("5,4"), end: Marker::Arrow })
    }

    /// The value-chain axis with its stacked labels, the evolution axis, the dashed band rules and
    /// the band names along the bottom.
    fn axes(&self) -> Vec<Figure> {
        let (top, bottom, right) = (-MAP_TOP, self.bottom(), self.band_x(BANDS));
        let axis = Paint::line(Tone::Wash(0.20), 1.0);
        let mut figures = vec![Figure::line(Point::new(0.0, top), Point::new(0.0, bottom), axis), Figure::line(Point::new(0.0, bottom), Point::new(right, bottom), axis)];
        figures.extend((1..BANDS).map(|band| {
            Figure::line(Point::new(self.band_x(band), top), Point::new(self.band_x(band), bottom), Paint::line(Tone::Wash(0.10), 0.8).dashed("4,4"))
        }));
        let axis_label = |y: f64, text: &str| {
            Figure::Text(Text::mono(Point::new(-AXIS_LABEL_GAP, y), text, AXIS_SIZE).weight(500).tracking(AXIS_TRACKING).tone(Tone::Ink).anchor(Anchor::End))
        };
        figures.push(axis_label(top - AXIS_LINE - AXIS_SIZE / 3.0, "VISIBLE TO"));
        figures.push(axis_label(top - AXIS_SIZE / 3.0, "THE USER"));
        figures.push(axis_label(bottom + AXIS_LINE, "INVISIBLE"));
        figures.extend(STAGES.iter().map(|stage| {
            let at = Point::new((stage.band() as f64 + 0.5) * self.band_width, bottom + BAND_LABEL_DROP);
            Figure::Text(Text::mono(at, stage.label(), BAND_LABEL_SIZE).tracking(BAND_TRACKING))
        }));
        figures
    }
}

/// The components that share a depth row and a band, in view order with the evolving one last, so its
/// arrow runs into free space toward the next band.
fn groups(marks: &[Mark]) -> BTreeMap<(usize, usize), Vec<usize>> {
    let mut groups: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
    for (node, mark) in marks.iter().enumerate() {
        groups.entry((mark.depth, mark.component.stage.band())).or_default().push(node);
    }
    groups.values_mut().for_each(|members| members.sort_by_key(|&node| marks[node].evolving));
    groups
}

/// The shifts a group may take from its band's centre, nearest first, spread over the free room.
fn offsets(free: f64) -> impl Iterator<Item = f64> {
    std::iter::once(0.0).chain((1..=OFFSET_STEPS).flat_map(move |step| {
        let reach = step as f64 / OFFSET_STEPS as f64 * free / 2.0;
        [reach, -reach]
    }))
}

fn group_width(marks: &[Mark], members: &[usize]) -> f64 {
    members.iter().map(|&node| marks[node].slot()).sum::<f64>() + SLOT_GAP * members.len().saturating_sub(1) as f64
}

fn arrow_ends(centre: Point) -> (Point, Point) {
    (Point::new(centre.x + ARROW_START, centre.y), Point::new(centre.x + ARROW_END, centre.y))
}

fn dot_box(centre: Point) -> Rect {
    Rect::new(centre.x - DOT - TEXT_PADDING, centre.y - DOT - TEXT_PADDING, 2.0 * (DOT + TEXT_PADDING), 2.0 * (DOT + TEXT_PADDING))
}

fn text_box(text: &Text) -> Rect {
    text.bounds().inflate(TEXT_PADDING)
}

fn crossings(texts: &[Text], links: &[Link]) -> usize {
    texts.iter().map(text_box).map(|rect| links.iter().filter(|link| crosses(link, &rect)).count()).sum()
}

fn union(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect::new(x, y, a.right().max(b.right()) - x, a.bottom().max(b.bottom()) - y)
}

/// True when the straight link runs through the rectangle (Liang–Barsky clipping).
fn crosses(link: &Link, rect: &Rect) -> bool {
    let (dx, dy) = (link.b.x - link.a.x, link.b.y - link.a.y);
    let (mut enter, mut leave) = (0.0_f64, 1.0_f64);
    let bounds = [(-dx, link.a.x - rect.x), (dx, rect.right() - link.a.x), (-dy, link.a.y - rect.y), (dy, rect.bottom() - link.a.y)];
    for (direction, room) in bounds {
        if direction.abs() < 1e-9 {
            if room < 0.0 {
                return false;
            }
            continue;
        }
        let t = room / direction;
        if direction < 0.0 {
            enter = enter.max(t);
        } else {
            leave = leave.min(t);
        }
        if enter > leave {
            return false;
        }
    }
    true
}

fn legend(has_evolving: bool) -> Vec<LegendKey> {
    let mut keys = vec![
        LegendKey::new(Swatch::Dot(Paint::shape(Tone::Paper, Tone::Ink, 1.0)), "COMPONENT"),
        LegendKey::new(Swatch::Line(Paint::line(Tone::Muted, 0.8), Marker::None), "DEPENDS ON"),
    ];
    if has_evolving {
        keys.push(LegendKey::new(Swatch::Dot(Paint::shape(Tone::AccentTint, Tone::Accent, 1.5)), "EVOLVING"));
        keys.push(LegendKey::new(Swatch::Line(Paint::line(Tone::Accent, 1.5).dashed("5,4"), Marker::Arrow), "NEXT STAGE"));
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::build as build_scene;

    fn map_view(focal: &str, focal_label: &str) -> View {
        serde_json::from_value(serde_json::json!({
            "title": "Grocery delivery",
            "kind": "wardley",
            "nodes": [
                {"id": "app", "label": "Shopping app (custom)", "focal": focal == "app"},
                {"id": "checkout", "label": "Checkout (product)", "focal": focal == "checkout"},
                {"id": "routing", "label": focal_label, "focal": focal == "routing"},
                {"id": "payments", "label": "Payments (product, Stripe)"},
                {"id": "compute", "label": "Cloud compute (utility)"}
            ],
            "edges": [
                {"source": "app", "target": "checkout"},
                {"source": "app", "target": "routing"},
                {"source": "checkout", "target": "payments"},
                {"source": "routing", "target": "compute"},
                {"source": "payments", "target": "compute"}
            ]
        }))
        .expect("a view")
    }

    fn plan(view: &View) -> Map {
        let (ids, contents) = unique_nodes(view);
        Map::plan(view, &ids, &contents, &known_edges(view, &ids))
    }

    #[test]
    fn the_bracket_names_the_stage_and_the_rest_is_a_sublabel() {
        let read = |label: &str| {
            let parsed = component(label);
            (parsed.name, parsed.stage, parsed.sublabel)
        };
        assert_eq!(read("Chat UI (product)"), ("Chat UI".into(), Stage::Product, None));
        assert_eq!(read("Eval harness (custom built, Python)"), ("Eval harness".into(), Stage::Custom, Some("Python".into())));
        assert_eq!(read("Billing (Bespoke)"), ("Billing".into(), Stage::Custom, None));
        assert_eq!(read("GPU compute (utility)"), ("GPU compute".into(), Stage::Commodity, None));
        assert_eq!(read("CRM (off-the-shelf)").1, Stage::Product);
        assert_eq!(read("Storage (cloud)").1, Stage::Commodity);
        assert_eq!(read("Agent (genesis, LLM, tools)"), ("Agent".into(), Stage::Genesis, Some("LLM, tools".into())));
    }

    #[test]
    fn an_unknown_or_missing_stage_is_product() {
        assert_eq!(component("Search").stage, Stage::Product);
        assert_eq!(component("Search (Elastic)"), Component { name: "Search".into(), stage: Stage::Product, sublabel: Some("Elastic".into()) });
    }

    #[test]
    fn a_dependency_sits_below_everything_that_depends_on_it() {
        assert_eq!(depths(4, &[(0, 1), (1, 2), (0, 2), (3, 2)]), vec![0, 1, 2, 0]);
        assert_eq!(depths(2, &[(0, 1), (1, 0)]).iter().filter(|&&depth| depth == 0).count(), 1, "a cycle still gets two rows");
        let map = plan(&map_view("routing", "Delivery routing (custom)"));
        let y = |node: usize| map.centres[node].y;
        assert!(y(0) < y(1) && y(1) < y(3) && y(3) < y(4));
        assert_eq!(y(1), y(2), "checkout and routing are both one step from the user");
    }

    #[test]
    fn every_component_sits_inside_its_stage_band() {
        let map = plan(&map_view("routing", "Delivery routing (custom)"));
        for (mark, centre) in map.marks.iter().zip(&map.centres) {
            let band = mark.component.stage.band();
            assert!(centre.x - mark.half_width() >= map.band_x(band) && centre.x + mark.right_reach() <= map.band_x(band + 1), "{}", mark.component.name);
        }
    }

    #[test]
    fn components_sharing_a_band_and_a_row_do_not_collide() {
        let view: View = serde_json::from_value(serde_json::json!({
            "kind": "wardley",
            "nodes": [
                {"id": "a", "label": "User need (custom)"},
                {"id": "b", "label": "Search index (product)"},
                {"id": "c", "label": "Recommendations (product)"},
                {"id": "d", "label": "Catalogue (product, Postgres)"}
            ],
            "edges": [{"source": "a", "target": "b"}, {"source": "a", "target": "c"}, {"source": "a", "target": "d"}]
        }))
        .expect("a view");
        let scene = build_scene(&view, &Params::default());
        for (index, node) in scene.nodes.iter().enumerate() {
            assert!(scene.nodes[index + 1..].iter().all(|other| !node.rect.overlaps(&other.rect)), "{}", node.id);
        }
    }

    #[test]
    fn the_evolving_component_points_right_toward_the_next_band() {
        let view = map_view("routing", "Delivery routing (custom)");
        let scene = build_scene(&view, &Params::default());
        let Some(Figure::Line { points, paint, end }) = scene.overlay.first() else { panic!("an arrow") };
        assert!(points[1].x > points[0].x && points[0].y == points[1].y);
        assert_eq!((paint.stroke, paint.dash, *end), (Tone::Accent, Some("5,4"), Marker::Arrow));
        let routing = scene.nodes.iter().find(|node| node.id == "routing").expect("routing");
        let Figure::Circle { centre, paint, .. } = &routing.figures[1] else { panic!("a dot") };
        assert_eq!(points[0].y, centre.y);
        assert_eq!((paint.fill, paint.stroke), (Tone::AccentTint, Tone::Accent));
    }

    #[test]
    fn an_evolving_commodity_has_no_arrow() {
        let scene = build_scene(&map_view("routing", "Delivery routing (cloud)"), &Params::default());
        assert!(scene.overlay.is_empty());
    }

    #[test]
    fn every_dependency_is_a_thin_straight_muted_line_without_arrowheads() {
        let view = map_view("routing", "Delivery routing (custom)");
        let scene = build_scene(&view, &Params::default());
        assert_eq!(scene.edges.len(), view.edges.len());
        for edge in &scene.edges {
            assert_eq!(edge.bend, Bend::Free);
            assert_eq!(edge.markers, Some((Marker::None, Marker::None)));
            assert_eq!(edge.paint, Some(Paint::line(Tone::Muted, 0.8)));
            assert!(edge.points[0].y < edge.points[1].y, "from the dependent down to the dependency");
        }
    }

    #[test]
    fn a_label_a_link_still_crosses_is_masked() {
        let view: View = serde_json::from_str(include_str!("../../tests/wardley.json")).expect("the fixture");
        let map = plan(&view);
        let links = map.links(&map.centres);
        let scene = build_scene(&view, &Params::default());
        let shift = |rect: Rect, node: &SceneNode| {
            let Figure::Circle { centre, .. } = &node.figures[1] else { panic!("a dot") };
            let (planned, placed) = (map.centres[scene.nodes.iter().position(|other| other.id == node.id).expect("known")], *centre);
            Rect::new(rect.x - placed.x + planned.x, rect.y - placed.y + planned.y, rect.w, rect.h)
        };
        for node in &scene.nodes {
            for (index, figure) in node.figures.iter().enumerate().skip(2) {
                let Figure::Text(text) = figure else { continue };
                let crossed = links.iter().any(|link| crosses(link, &shift(text_box(text), node)));
                let masked = matches!(node.figures[index - 1], Figure::Rect { paint, .. } if paint.fill == Tone::Paper);
                assert_eq!(crossed, masked, "{}: {}", node.id, text.text);
            }
        }
    }

    #[test]
    fn no_label_sits_on_a_link_or_another_label() {
        let view = map_view("routing", "Delivery routing (custom)");
        let map = plan(&view);
        let labels = map.labels();
        let links = map.links(&map.centres);
        let boxes: Vec<Rect> = labels.iter().flatten().map(text_box).collect();
        for (index, rect) in boxes.iter().enumerate() {
            assert!(links.iter().all(|link| !crosses(link, rect)), "{rect:?}");
            assert!(boxes[index + 1..].iter().all(|other| !rect.overlaps(other)));
        }
    }
}
