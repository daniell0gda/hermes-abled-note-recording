//! The placed diagram: node boxes, edge polylines, label masks and group zones in final page coordinates.

use std::collections::HashMap;

use crate::cardinality::{self, Ends};
use crate::cycle::{self, Cycle};
use crate::figure::{Figure, LegendKey, Marker, Paint, Section};
use crate::geom::{segments, snap_up, Point, Rect, Segment};
use crate::kinds;
use crate::label::{place, LabelRequest, LABEL_SIZE, LABEL_TRACKING, MASK_HEIGHT, MASK_PADDING};
use crate::layout::{back_edges, layer, Bands, NodeBox, Pin};
use crate::node::{content, Links, NodeContent};
use crate::params::{Direction, Params};
use crate::rings::{self, Ring, RingPlan};
use crate::route::route;
use crate::sequence;
use crate::spec::{EdgeStyle, Kind, NodeSpec, View};
use crate::text::mono_width;
use crate::tiers::{self, TierPlan, LAYER_PADDING};

pub const MARGIN: f64 = 40.0;
const LOOP_RISE: f64 = 20.0;
const LOOP_WIDTH: f64 = 36.0;
const LOOP_INSET: f64 = 16.0;
const LABEL_RUN_ROOM: f64 = 24.0;
const ZONE_PADDING: f64 = 16.0;
const ZONE_HEADER: f64 = 16.0;
pub const LANE_LABEL_SIZE: f64 = 9.0;
pub const LANE_LABEL_TRACKING: f64 = 0.14;
const LANE_RULE_INSET: f64 = 16.0;
const LANE_ROOM: f64 = 24.0;
const ER_CHANNEL: f64 = 96.0;
const CYCLE_LABEL: &str = "CYCLE";

/// A placed node. Without `figures` it is drawn as the editorial box around `content`; with them it is
/// drawn as those figures, the first being the shape the selection looks change.
pub struct SceneNode {
    pub id: String,
    pub rect: Rect,
    pub content: NodeContent,
    pub figures: Vec<Figure>,
}

impl SceneNode {
    pub fn new(id: String, rect: Rect, content: NodeContent) -> SceneNode {
        SceneNode { id, rect, content, figures: Vec::new() }
    }
}

pub struct LabelBox {
    pub rect: Rect,
    pub text: String,
}

/// How an edge's points are joined: orthogonal runs with rounded elbows (checked against the connector
/// rules), straight runs in any direction, or one circular arc from the first point to the last. Free and
/// arc edges are the data lines some kinds are exempt for (fishbone bones, Wardley links, loop rings).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Bend {
    #[default]
    Elbow,
    Free,
    Arc { radius: f64, clockwise: bool },
}

pub struct SceneEdge {
    pub id: String,
    pub source: usize,
    pub target: usize,
    pub points: Vec<Point>,
    pub style: EdgeStyle,
    pub focal: bool,
    pub cycle: bool,
    pub label: Option<LabelBox>,
    pub marks: Vec<LabelBox>,
    pub bend: Bend,
    pub markers: Option<(Marker, Marker)>,
    pub paint: Option<Paint>,
}

impl SceneEdge {
    /// An edge for `input` along `points`, with the default look of its style.
    pub fn new(input: &EdgeInput, points: Vec<Point>) -> SceneEdge {
        SceneEdge {
            id: input.id.clone(),
            source: input.source,
            target: input.target,
            points,
            style: input.style,
            focal: input.focal,
            cycle: false,
            label: None,
            marks: Vec::new(),
            bend: Bend::Elbow,
            markers: None,
            paint: None,
        }
    }

    /// True when the edge follows the orthogonal connector rules.
    pub fn is_elbow(&self) -> bool {
        self.bend == Bend::Elbow
    }

    /// The edge's label and its cardinality marks.
    pub fn texts(&self) -> impl Iterator<Item = &LabelBox> {
        self.label.iter().chain(&self.marks)
    }
}

pub struct Zone {
    pub id: String,
    pub label: String,
    pub rect: Rect,
}

/// A band across the diagram: a swimlane or a layer. `id` is the group's id, empty for the band of
/// nodes no group names.
pub struct Lane {
    pub id: String,
    pub label: String,
    pub top: f64,
    pub bottom: f64,
}

pub struct Scene {
    pub kind: Kind,
    pub nodes: Vec<SceneNode>,
    pub edges: Vec<SceneEdge>,
    pub zones: Vec<Zone>,
    pub rings: Vec<Ring>,
    pub skipped_zones: usize,
    pub lifelines: Vec<Segment>,
    pub lanes: Vec<Lane>,
    pub lane_gutter: f64,
    pub sections: Vec<Section>,
    pub decor: Vec<Figure>,
    pub overlay: Vec<Figure>,
    pub legend: Option<Vec<LegendKey>>,
    pub width: f64,
    pub height: f64,
}

impl Scene {
    pub fn empty(kind: Kind) -> Scene {
        Scene {
            kind,
            nodes: Vec::new(),
            edges: Vec::new(),
            zones: Vec::new(),
            rings: Vec::new(),
            skipped_zones: 0,
            lifelines: Vec::new(),
            lanes: Vec::new(),
            lane_gutter: 0.0,
            sections: Vec::new(),
            decor: Vec::new(),
            overlay: Vec::new(),
            legend: None,
            width: 0.0,
            height: 0.0,
        }
    }

    /// Every box drawn: nodes, zones, rings, labels, marks and the bounds of every figure.
    pub fn boxes(&self) -> impl Iterator<Item = Rect> + '_ {
        self.nodes
            .iter()
            .map(|node| node.rect)
            .chain(self.zones.iter().map(|zone| zone.rect))
            .chain(self.rings.iter().flat_map(|ring| [ring.rect, ring.label_rect]))
            .chain(self.edges.iter().flat_map(SceneEdge::texts).map(|text| text.rect))
            .chain(self.figures().map(Figure::bounds))
    }

    fn figures(&self) -> impl Iterator<Item = &Figure> + '_ {
        self.nodes
            .iter()
            .flat_map(|node| &node.figures)
            .chain(self.sections.iter().flat_map(|section| &section.figures))
            .chain(&self.decor)
            .chain(&self.overlay)
    }

    fn figures_mut(&mut self) -> impl Iterator<Item = &mut Figure> + '_ {
        self.nodes
            .iter_mut()
            .flat_map(|node| &mut node.figures)
            .chain(self.sections.iter_mut().flat_map(|section| &mut section.figures))
            .chain(&mut self.decor)
            .chain(&mut self.overlay)
    }

    fn points(&self) -> impl Iterator<Item = Point> + '_ {
        let corners = self.boxes().flat_map(|rect| [Point::new(rect.x, rect.y), Point::new(rect.right(), rect.bottom())]);
        let lanes = self.lanes.iter().flat_map(|lane| [Point::new(MARGIN, lane.top), Point::new(MARGIN, lane.bottom)]);
        corners.chain(self.edges.iter().flat_map(|edge| edge.points.iter().copied())).chain(lanes)
    }
}

pub struct EdgeInput {
    pub id: String,
    pub source: usize,
    pub target: usize,
    pub style: EdgeStyle,
    pub focal: bool,
    pub text: String,
    pub ends: Option<Ends>,
}

/// Lays out, routes and labels `view` with the layout its kind calls for.
pub fn build(view: &View, params: &Params) -> Scene {
    match view.kind {
        Kind::Sequence => sequence::build(view),
        Kind::Timeline => kinds::timeline::build(view, params),
        Kind::Loop => kinds::cycle_loop::build(view, params),
        Kind::Pyramid | Kind::Funnel => kinds::pyramid::build(view, params),
        Kind::Venn => kinds::venn::build(view, params),
        Kind::Fishbone => kinds::fishbone::build(view, params),
        Kind::Kanban => kinds::kanban::build(view, params),
        Kind::Journey => kinds::journey::build(view, params),
        Kind::StoryMap => kinds::story_map::build(view, params),
        Kind::Quadrant => kinds::quadrant::build(view, params),
        Kind::Wardley => kinds::wardley::build(view, params),
        Kind::Process => kinds::process::build(view, params),
        Kind::UmlClass => kinds::uml_class::build(view, params),
        Kind::Deployment => kinds::deployment::build(view, params),
        Kind::Medallion => kinds::medallion::build(view, params),
        Kind::OrgChart => kinds::org_chart::build(view, params),
        Kind::DbSchema => kinds::db_schema::build(view, params),
        _ => layered(view, params),
    }
    .fitted_to_canvas()
}

impl Scene {
    fn lowest_corner(&self) -> (f64, f64) {
        self.points().fold((f64::INFINITY, f64::INFINITY), |(x, y), point| (x.min(point.x), y.min(point.y)))
    }

    fn fitted_to_canvas(mut self) -> Scene {
        let (low_x, low_y) = self.lowest_corner();
        let dx = if low_x.is_finite() { snap_up((MARGIN - low_x).max(0.0)) } else { 0.0 };
        let dy = if low_y.is_finite() { snap_up((MARGIN - low_y).max(0.0)) } else { 0.0 };
        if dx == 0.0 && dy == 0.0 {
            return self;
        }
        let shift = |rect: &mut Rect| *rect = Rect::new(rect.x + dx, rect.y + dy, rect.w, rect.h);
        let shift_point = |point: &mut Point| *point = Point::new(point.x + dx, point.y + dy);
        self.nodes.iter_mut().for_each(|node| shift(&mut node.rect));
        self.figures_mut().for_each(|figure| figure.shift(dx, dy));
        self.zones.iter_mut().for_each(|zone| shift(&mut zone.rect));
        for ring in &mut self.rings {
            shift(&mut ring.rect);
            shift(&mut ring.label_rect);
        }
        for edge in &mut self.edges {
            edge.points.iter_mut().for_each(shift_point);
            edge.label.iter_mut().chain(&mut edge.marks).for_each(|text| shift(&mut text.rect));
        }
        for lifeline in &mut self.lifelines {
            shift_point(&mut lifeline.a);
            shift_point(&mut lifeline.b);
        }
        for lane in &mut self.lanes {
            lane.top += dy;
            lane.bottom += dy;
        }
        self.lane_gutter += dx;
        self.width += dx;
        self.height += dy;
        self
    }

    /// Sets the canvas size to everything drawn plus the margin.
    pub fn measured(mut self) -> Scene {
        let (right, bottom) = self.points().fold((0.0, 0.0), |(x, y): (f64, f64), point| (x.max(point.x), y.max(point.y)));
        self.width = right + MARGIN;
        self.height = bottom + MARGIN;
        self
    }
}

/// How the groups of a view shape its layout: lanes across the ranks (swimlane), lanes held by rings
/// (nested), tiers of whole ranks (layers), or nothing but zones drawn around the members.
enum Banding {
    Free,
    Lanes(LanePlan),
    Rings(RingPlan),
    Tiers(TierPlan),
}

impl Banding {
    fn for_view(view: &View, ids: &[String]) -> Banding {
        let planned = match view.kind {
            Kind::Swimlane | Kind::Process => lane_plan(view, ids).map(Banding::Lanes),
            Kind::Nested | Kind::Deployment => rings::plan(view, ids).map(Banding::Rings),
            Kind::Layers => tiers::plan(view, ids).map(Banding::Tiers),
            _ => None,
        };
        planned.unwrap_or(Banding::Free)
    }

    fn lane_of(&self, node: usize) -> usize {
        match self {
            Banding::Lanes(plan) => plan.of_node[node],
            Banding::Rings(plan) => plan.of_node[node],
            Banding::Free | Banding::Tiers(_) => 0,
        }
    }

    fn bands(&self) -> Bands<'_> {
        match self {
            Banding::Lanes(plan) => Bands { lane_room: Some(&plan.room), tiers: None },
            Banding::Rings(plan) => Bands { lane_room: Some(&plan.lane_room), tiers: None },
            Banding::Tiers(plan) => Bands { lane_room: None, tiers: Some(&plan.of_node) },
            Banding::Free => Bands::default(),
        }
    }

    fn direction(&self, params: &Params, kind: Kind) -> Direction {
        match self {
            Banding::Lanes(_) => Direction::Right,
            Banding::Tiers(_) => Direction::Down,
            Banding::Free | Banding::Rings(_) => params.direction_for(kind),
        }
    }

    fn gutter(&self) -> f64 {
        match self {
            Banding::Lanes(plan) => plan.gutter(),
            Banding::Tiers(plan) => plan.gutter(),
            Banding::Free | Banding::Rings(_) => 0.0,
        }
    }
}

/// The edges of a view sorted for layout: links between ranks, self loops, and the dependency cycles
/// that are drawn around the outside instead of through the ranks.
struct Wiring<'a> {
    links: Vec<&'a EdgeInput>,
    loops: Vec<&'a EdgeInput>,
    back: Vec<&'a EdgeInput>,
    cycles: Vec<Cycle>,
    pins: Vec<Pin>,
}

impl<'a> Wiring<'a> {
    fn of(kind: Kind, edges: &'a [EdgeInput], count: usize) -> Self {
        let (loops, between): (Vec<&EdgeInput>, Vec<&EdgeInput>) = edges.iter().partition(|edge| edge.source == edge.target);
        let mut links = Vec::new();
        let mut back = Vec::new();
        if kind == Kind::Dependency {
            let pairs: Vec<(usize, usize)> = between.iter().map(|edge| (edge.source, edge.target)).collect();
            for (edge, reversed) in between.into_iter().zip(back_edges(count, &pairs, None)) {
                if reversed { back.push(edge) } else { links.push(edge) }
            }
        } else {
            links = between;
        }
        let mut pins = vec![Pin::Free; count];
        let back_pairs: Vec<(usize, usize)> = back.iter().map(|edge| (edge.source, edge.target)).collect();
        let cycles = cycle::assign_sides(&back_pairs, &mut pins);
        Wiring { links, loops, back, cycles, pins }
    }

    fn ordered(&self) -> Vec<&'a EdgeInput> {
        [self.links.as_slice(), &self.loops, &self.back].concat()
    }

    fn is_cycle(&self, index: usize) -> bool {
        index >= self.links.len() + self.loops.len()
    }
}

struct Routes {
    rects: Vec<Rect>,
    paths: Vec<Vec<Point>>,
    preferred: Vec<Option<Segment>>,
}

/// The layered layout with the node contents of `view`'s kind.
pub fn layered(view: &View, params: &Params) -> Scene {
    let (_, contents) = unique_nodes(view);
    layered_with(view, params, contents)
}

/// The layered layout with node contents a kind sized itself, one per id of `unique_nodes(view)`, in
/// that order.
pub fn layered_with(view: &View, params: &Params, contents: Vec<NodeContent>) -> Scene {
    let (ids, _) = unique_nodes(view);
    let edges = known_edges(view, &ids);
    let banding = Banding::for_view(view, &ids);
    let direction = banding.direction(params, view.kind);
    let wiring = Wiring::of(view.kind, &edges, ids.len());
    let boxes: Vec<NodeBox> = contents
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let loops = wiring.loops.iter().filter(|edge| edge.source == index).count();
            node_box(node, direction, loops, banding.lane_of(index), wiring.pins[index])
        })
        .collect();
    let pairs: Vec<(usize, usize)> = wiring.links.iter().map(|edge| (edge.source, edge.target)).collect();
    let layered = layer(&boxes, &pairs, params.node_gap, params.ordering_sweeps, banding.bands());
    let needs: Vec<f64> = wiring.links.iter().map(|edge| run_need(&edge.text, direction)).collect();
    let min_channel = if view.kind == Kind::Er { params.min_channel.max(ER_CHANNEL) } else { params.min_channel };
    let routed = route(&layered, contents.len(), &needs, direction, min_channel, params.track_gap, params.uniform_rank_size);
    let left = MARGIN + banding.gutter();
    let offset = |point: Point| Point::new(point.x + left, point.y + MARGIN);
    let mut routes = Routes {
        rects: routed.rects.iter().map(|rect| Rect::new(rect.x + left, rect.y + MARGIN, rect.w, rect.h)).collect(),
        paths: routed.paths.iter().map(|path| path.iter().copied().map(offset).collect()).collect(),
        preferred: routed.preferred.iter().map(|segment| segment.map(|segment| Segment::new(offset(segment.a), offset(segment.b)))).collect(),
    };
    add_self_loops(&mut routes, &wiring.loops, direction);
    for (path, outside) in cycle::route(&wiring.cycles, &routes.rects, &routes.paths, direction) {
        routes.paths.push(path);
        routes.preferred.push(Some(outside));
    }
    let scene_edges = scene_edges(&wiring, &routes);
    let nodes: Vec<SceneNode> = ids
        .into_iter()
        .zip(contents)
        .zip(&routes.rects)
        .map(|((id, content), &rect)| SceneNode::new(id, rect, content))
        .collect();
    let mut scene = Scene { nodes, edges: scene_edges, lane_gutter: left - LANE_RULE_INSET, ..Scene::empty(view.kind) };
    let frame_room = if matches!(banding, Banding::Tiers(_)) { LAYER_PADDING } else { 0.0 };
    match banding {
        Banding::Lanes(plan) => scene.lanes = plan.placed(&layered.lane_bands),
        Banding::Tiers(plan) => scene.lanes = plan.bands(&routes.rects),
        Banding::Rings(plan) => {
            let lines: Vec<Segment> = routes.paths.iter().flat_map(|path| segments(path)).collect();
            let taken: Vec<Rect> = scene.boxes().collect();
            scene.rings = plan.rings(&routes.rects);
            scene.rings.iter_mut().for_each(|ring| ring.clear_label(&lines, &taken));
        }
        Banding::Free => (scene.zones, scene.skipped_zones) = zones(view, &scene.nodes),
    }
    let mut scene = scene.measured();
    scene.width += frame_room;
    scene
}

fn add_self_loops(routes: &mut Routes, loops: &[&EdgeInput], direction: Direction) {
    let mut turns: HashMap<usize, usize> = HashMap::new();
    for edge in loops {
        let turn = turns.entry(edge.source).or_insert(0);
        let path = self_loop(&routes.rects[edge.source], direction, *turn);
        *turn += 1;
        routes.preferred.push(Some(Segment::new(path[1], path[2])));
        routes.paths.push(path);
    }
}

fn scene_edges(wiring: &Wiring, routes: &Routes) -> Vec<SceneEdge> {
    let ordered = wiring.ordered();
    let ends: Vec<Option<Ends>> = ordered.iter().map(|edge| edge.ends.clone()).collect();
    let mut marks = cardinality::place(&ends, &routes.paths, &routes.rects);
    let obstacles: Vec<Rect> = routes.rects.iter().copied().chain(marks.iter().flatten().map(|mark| mark.rect)).collect();
    let texts: Vec<String> = ordered
        .iter()
        .enumerate()
        .map(|(index, edge)| if wiring.is_cycle(index) { cycle_label(&edge.text) } else { edge.text.clone() })
        .collect();
    let requests: Vec<LabelRequest> = texts
        .iter()
        .enumerate()
        .filter(|(_, text)| !text.is_empty())
        .map(|(index, text)| LabelRequest {
            edge: index,
            mask_width: mask_width(text),
            preferred: routes.preferred[index],
            overhang: ordered[index].source == ordered[index].target,
        })
        .collect();
    let masks = place(&requests, &routes.paths, &obstacles);
    let mut labels: HashMap<usize, Rect> = requests.iter().zip(masks).map(|(request, mask)| (request.edge, mask)).collect();
    ordered
        .iter()
        .zip(&routes.paths)
        .zip(texts)
        .enumerate()
        .map(|(index, ((edge, points), text))| SceneEdge {
            cycle: wiring.is_cycle(index),
            label: labels.remove(&index).map(|rect| LabelBox { rect, text }),
            marks: std::mem::take(&mut marks[index]),
            ..SceneEdge::new(edge, points.clone())
        })
        .collect()
}

fn cycle_label(text: &str) -> String {
    if text.is_empty() {
        CYCLE_LABEL.to_string()
    } else {
        format!("{CYCLE_LABEL} · {text}")
    }
}

struct LanePlan {
    ids: Vec<String>,
    labels: Vec<String>,
    of_node: Vec<usize>,
    room: Vec<(f64, f64)>,
}

impl LanePlan {
    fn gutter(&self) -> f64 {
        let widest = self.labels.iter().map(|label| mono_width(label, LANE_LABEL_SIZE, LANE_LABEL_TRACKING)).fold(0.0, f64::max);
        snap_up(widest + 2.0 * LANE_RULE_INSET)
    }

    fn placed(self, bands: &[(f64, f64)]) -> Vec<Lane> {
        self.ids
            .into_iter()
            .zip(self.labels)
            .zip(bands)
            .map(|((id, label), &(top, bottom))| Lane { id, label, top: top + MARGIN, bottom: bottom + MARGIN })
            .collect()
    }
}

fn lane_plan(view: &View, ids: &[String]) -> Option<LanePlan> {
    if view.groups.is_empty() {
        return None;
    }
    let mut labels: Vec<String> = view
        .groups
        .iter()
        .map(|group| if group.label.is_empty() { group.id.to_uppercase() } else { group.label.to_uppercase() })
        .collect();
    let mut lane_ids: Vec<String> = view.groups.iter().map(|group| group.id.clone()).collect();
    let outside = labels.len();
    let of_node: Vec<usize> = ids
        .iter()
        .map(|id| view.groups.iter().position(|group| group.members.contains(id)).unwrap_or(outside))
        .collect();
    if of_node.contains(&outside) {
        labels.push(String::new());
        lane_ids.push(String::new());
    }
    let room = vec![(LANE_ROOM, LANE_ROOM); labels.len()];
    Some(LanePlan { ids: lane_ids, labels, of_node, room })
}

pub fn unique_nodes(view: &View) -> (Vec<String>, Vec<NodeContent>) {
    let mut specs: Vec<&NodeSpec> = Vec::new();
    for spec in &view.nodes {
        if !specs.iter().any(|known| known.id == spec.id) {
            specs.push(spec);
        }
    }
    let ids: Vec<String> = specs.iter().map(|spec| spec.id.clone()).collect();
    let contents = specs.iter().map(|spec| content(spec, view.kind, links_of(view, &ids, &spec.id))).collect();
    (ids, contents)
}

fn links_of(view: &View, ids: &[String], id: &str) -> Links {
    let known = |end: &String| ids.contains(end);
    let between = view.edges.iter().filter(|edge| edge.source != edge.target && known(&edge.source) && known(&edge.target));
    between.fold(Links::default(), |links, edge| Links {
        incoming: links.incoming + usize::from(edge.target == id),
        outgoing: links.outgoing + usize::from(edge.source == id),
    })
}

pub fn known_edges(view: &View, ids: &[String]) -> Vec<EdgeInput> {
    let index_of = |id: &str| ids.iter().position(|known| known == id);
    view.edges
        .iter()
        .filter_map(|edge| {
            let upper = edge.label.trim().to_uppercase();
            let (text, ends) = if matches!(view.kind, Kind::Er | Kind::UmlClass) { cardinality::split(&upper) } else { (upper, None) };
            let id = if edge.id.is_empty() { format!("{}->{}", edge.source, edge.target) } else { edge.id.clone() };
            Some(EdgeInput {
                id,
                source: index_of(&edge.source)?,
                target: index_of(&edge.target)?,
                style: drawn_style(view.kind, edge.style),
                focal: edge.focal && view.kind != Kind::Dependency,
                text,
                ends,
            })
        })
        .collect()
}

fn drawn_style(kind: Kind, style: EdgeStyle) -> EdgeStyle {
    match (kind, style) {
        (Kind::Tree | Kind::Dependency, EdgeStyle::Sync) | (Kind::Er, _) => EdgeStyle::Data,
        _ => style,
    }
}

pub fn mask_width(text: &str) -> f64 {
    mono_width(text, LABEL_SIZE, LABEL_TRACKING) + 2.0 * MASK_PADDING
}

fn run_need(text: &str, direction: Direction) -> f64 {
    match (text.is_empty(), direction) {
        (true, _) => 0.0,
        (false, Direction::Right) => mask_width(text) + LABEL_RUN_ROOM,
        (false, Direction::Down) => MASK_HEIGHT + LABEL_RUN_ROOM,
    }
}

fn node_box(node: &NodeContent, direction: Direction, loops: usize, lane: usize, pin: Pin) -> NodeBox {
    let room_low = if loops == 0 { 0.0 } else { LOOP_RISE + 2.0 * MASK_HEIGHT + 4.0 };
    match direction {
        Direction::Right => NodeBox { su: node.width, sv: node.height, room_low, lane, pin },
        Direction::Down => NodeBox { su: node.height, sv: node.width, room_low, lane, pin },
    }
}

fn self_loop(rect: &Rect, direction: Direction, turn: usize) -> Vec<Point> {
    let shift = turn as f64 * (LOOP_WIDTH + LOOP_INSET);
    match direction {
        Direction::Right => {
            let enter = rect.right() - LOOP_INSET - shift;
            let leave = enter - LOOP_WIDTH;
            let top = rect.y - LOOP_RISE;
            vec![Point::new(leave, rect.y), Point::new(leave, top), Point::new(enter, top), Point::new(enter, rect.y)]
        }
        Direction::Down => {
            let enter = rect.bottom() - LOOP_INSET - shift;
            let leave = enter - LOOP_WIDTH;
            let left = rect.x - LOOP_RISE;
            vec![Point::new(rect.x, leave), Point::new(left, leave), Point::new(left, enter), Point::new(rect.x, enter)]
        }
    }
}

fn zones(view: &View, nodes: &[SceneNode]) -> (Vec<Zone>, usize) {
    let mut zones: Vec<Zone> = Vec::new();
    let mut skipped = 0;
    for group in &view.groups {
        let members: Vec<&SceneNode> = nodes.iter().filter(|node| group.members.contains(&node.id)).collect();
        let Some(rect) = bounding(members.iter().map(|node| node.rect)) else { continue };
        let zone = Rect::new(
            rect.x - ZONE_PADDING,
            rect.y - ZONE_PADDING - ZONE_HEADER,
            rect.w + 2.0 * ZONE_PADDING,
            rect.h + 2.0 * ZONE_PADDING + ZONE_HEADER,
        );
        let hits_outsider = nodes.iter().any(|node| !group.members.contains(&node.id) && zone.overlaps(&node.rect));
        let hits_zone = zones.iter().any(|other| zone.overlaps(&other.rect));
        if hits_outsider || hits_zone {
            skipped += 1;
            continue;
        }
        let label = if group.label.is_empty() { group.id.clone() } else { group.label.clone() };
        zones.push(Zone { id: group.id.clone(), label: label.to_uppercase(), rect: zone });
    }
    (zones, skipped)
}

fn bounding(rects: impl Iterator<Item = Rect>) -> Option<Rect> {
    rects.reduce(|a, b| {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        Rect::new(x, y, a.right().max(b.right()) - x, a.bottom().max(b.bottom()) - y)
    })
}
