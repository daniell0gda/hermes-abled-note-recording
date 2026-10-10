//! The placed diagram: node boxes, edge polylines, label masks and group zones in final page coordinates.

use std::collections::HashMap;

use crate::geom::{snap_up, Point, Rect, Segment};
use crate::label::{place, LabelRequest, LABEL_SIZE, LABEL_TRACKING, MASK_HEIGHT, MASK_PADDING};
use crate::layout::{layer, NodeBox};
use crate::node::{content, NodeContent};
use crate::params::{Direction, Params};
use crate::route::route;
use crate::sequence;
use crate::spec::{EdgeStyle, Kind, View};
use crate::text::mono_width;

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

pub struct SceneNode {
    pub id: String,
    pub rect: Rect,
    pub content: NodeContent,
}

pub struct LabelBox {
    pub rect: Rect,
    pub text: String,
}

pub struct SceneEdge {
    pub source: usize,
    pub target: usize,
    pub points: Vec<Point>,
    pub style: EdgeStyle,
    pub label: Option<LabelBox>,
}

pub struct Zone {
    pub label: String,
    pub rect: Rect,
}

pub struct Lane {
    pub label: String,
    pub top: f64,
    pub bottom: f64,
}

pub struct Scene {
    pub kind: Kind,
    pub nodes: Vec<SceneNode>,
    pub edges: Vec<SceneEdge>,
    pub zones: Vec<Zone>,
    pub skipped_zones: usize,
    pub lifelines: Vec<Segment>,
    pub lanes: Vec<Lane>,
    pub lane_gutter: f64,
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
            skipped_zones: 0,
            lifelines: Vec::new(),
            lanes: Vec::new(),
            lane_gutter: 0.0,
            width: 0.0,
            height: 0.0,
        }
    }
}

pub struct EdgeInput {
    pub source: usize,
    pub target: usize,
    pub style: EdgeStyle,
    pub text: String,
}

/// Lays out, routes and labels `view` with the layout its kind calls for.
pub fn build(view: &View, params: &Params) -> Scene {
    match view.kind {
        Kind::Sequence => sequence::build(view),
        _ => layered(view, params),
    }
}

fn layered(view: &View, params: &Params) -> Scene {
    let (ids, contents) = unique_nodes(view);
    let edges = known_edges(view, &ids);
    let lanes = lane_plan(view, &ids);
    let direction = if lanes.is_some() { Direction::Right } else { params.direction_for(view.kind) };
    let (loops, links): (Vec<&EdgeInput>, Vec<&EdgeInput>) = edges.iter().partition(|edge| edge.source == edge.target);
    let boxes: Vec<NodeBox> = contents
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let lane = lanes.as_ref().map_or(0, |plan| plan.of_node[index]);
            node_box(node, direction, loops.iter().filter(|edge| edge.source == index).count(), lane)
        })
        .collect();
    let pairs: Vec<(usize, usize)> = links.iter().map(|edge| (edge.source, edge.target)).collect();
    let layered = layer(&boxes, &pairs, params.node_gap, params.ordering_sweeps, lanes.is_some());
    let needs: Vec<f64> = links.iter().map(|edge| run_need(&edge.text, direction)).collect();
    let routed = route(&layered, contents.len(), &needs, direction, params.min_channel, params.track_gap, params.uniform_rank_size);
    let gutter = lanes.as_ref().map_or(0.0, LanePlan::gutter);
    let left = MARGIN + gutter;
    let offset = |point: Point| Point::new(point.x + left, point.y + MARGIN);
    let rects: Vec<Rect> = routed.rects.iter().map(|rect| Rect::new(rect.x + left, rect.y + MARGIN, rect.w, rect.h)).collect();
    let mut paths: Vec<Vec<Point>> = routed.paths.iter().map(|path| path.iter().copied().map(offset).collect()).collect();
    let mut preferred: Vec<Option<Segment>> = routed
        .preferred
        .iter()
        .map(|segment| segment.map(|segment| Segment::new(offset(segment.a), offset(segment.b))))
        .collect();
    let mut ordered: Vec<&EdgeInput> = links.clone();
    let mut loop_counts: HashMap<usize, usize> = HashMap::new();
    for edge in &loops {
        let turn = loop_counts.entry(edge.source).or_insert(0);
        let path = self_loop(&rects[edge.source], direction, *turn);
        *turn += 1;
        preferred.push(Some(Segment::new(path[1], path[2])));
        paths.push(path);
        ordered.push(edge);
    }
    let requests: Vec<LabelRequest> = ordered
        .iter()
        .enumerate()
        .filter(|(_, edge)| !edge.text.is_empty())
        .map(|(index, edge)| LabelRequest { edge: index, mask_width: mask_width(&edge.text), preferred: preferred[index] })
        .collect();
    let masks = place(&requests, &paths, &rects);
    let mut labels: HashMap<usize, Rect> = requests.iter().zip(masks).map(|(request, mask)| (request.edge, mask)).collect();
    let scene_edges: Vec<SceneEdge> = ordered
        .iter()
        .zip(paths)
        .enumerate()
        .map(|(index, (edge, points))| SceneEdge {
            source: edge.source,
            target: edge.target,
            points,
            style: edge.style,
            label: labels.remove(&index).map(|rect| LabelBox { rect, text: edge.text.clone() }),
        })
        .collect();
    let nodes: Vec<SceneNode> = ids
        .into_iter()
        .zip(contents)
        .zip(&rects)
        .map(|((id, content), &rect)| SceneNode { id, rect, content })
        .collect();
    let scene_lanes: Vec<Lane> = lanes.map_or_else(Vec::new, |plan| plan.placed(&layered.lane_bands));
    let (zones, skipped_zones) = if scene_lanes.is_empty() { zones(view, &nodes) } else { (Vec::new(), 0) };
    let (width, content_height) = extent(&nodes, &scene_edges, &zones);
    let height = scene_lanes.last().map_or(content_height, |lane| content_height.max(lane.bottom + MARGIN));
    Scene {
        nodes,
        edges: scene_edges,
        zones,
        skipped_zones,
        lanes: scene_lanes,
        lane_gutter: MARGIN + gutter - LANE_RULE_INSET,
        width,
        height,
        ..Scene::empty(view.kind)
    }
}

struct LanePlan {
    labels: Vec<String>,
    of_node: Vec<usize>,
}

impl LanePlan {
    fn gutter(&self) -> f64 {
        let widest = self.labels.iter().map(|label| mono_width(label, LANE_LABEL_SIZE, LANE_LABEL_TRACKING)).fold(0.0, f64::max);
        snap_up(widest + 2.0 * LANE_RULE_INSET)
    }

    fn placed(self, bands: &[(f64, f64)]) -> Vec<Lane> {
        self.labels
            .into_iter()
            .zip(bands)
            .map(|(label, &(top, bottom))| Lane { label, top: top + MARGIN, bottom: bottom + MARGIN })
            .collect()
    }
}

fn lane_plan(view: &View, ids: &[String]) -> Option<LanePlan> {
    if view.kind != Kind::Swimlane || view.groups.is_empty() {
        return None;
    }
    let mut labels: Vec<String> = view
        .groups
        .iter()
        .map(|group| if group.label.is_empty() { group.id.to_uppercase() } else { group.label.to_uppercase() })
        .collect();
    let outside = labels.len();
    let of_node: Vec<usize> = ids
        .iter()
        .map(|id| view.groups.iter().position(|group| group.members.contains(id)).unwrap_or(outside))
        .collect();
    if of_node.contains(&outside) {
        labels.push(String::new());
    }
    Some(LanePlan { labels, of_node })
}

pub fn unique_nodes(view: &View) -> (Vec<String>, Vec<NodeContent>) {
    let mut ids: Vec<String> = Vec::new();
    let mut contents = Vec::new();
    for spec in &view.nodes {
        if !ids.contains(&spec.id) {
            ids.push(spec.id.clone());
            contents.push(content(spec, view.kind));
        }
    }
    (ids, contents)
}

pub fn known_edges(view: &View, ids: &[String]) -> Vec<EdgeInput> {
    let index_of = |id: &str| ids.iter().position(|known| known == id);
    view.edges
        .iter()
        .filter_map(|edge| {
            Some(EdgeInput {
                source: index_of(&edge.source)?,
                target: index_of(&edge.target)?,
                style: drawn_style(view.kind, edge.style),
                text: edge.label.trim().to_uppercase(),
            })
        })
        .collect()
}

fn drawn_style(kind: Kind, style: EdgeStyle) -> EdgeStyle {
    match (kind, style) {
        (Kind::Tree, EdgeStyle::Sync) => EdgeStyle::Data,
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

fn node_box(node: &NodeContent, direction: Direction, loops: usize, lane: usize) -> NodeBox {
    let room_low = if loops == 0 { 0.0 } else { LOOP_RISE + 2.0 * MASK_HEIGHT + 4.0 };
    match direction {
        Direction::Right => NodeBox { su: node.width, sv: node.height, room_low, lane },
        Direction::Down => NodeBox { su: node.height, sv: node.width, room_low, lane },
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
        zones.push(Zone { label: label.to_uppercase(), rect: zone });
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

fn extent(nodes: &[SceneNode], edges: &[SceneEdge], zones: &[Zone]) -> (f64, f64) {
    let rects = nodes
        .iter()
        .map(|node| node.rect)
        .chain(zones.iter().map(|zone| zone.rect))
        .chain(edges.iter().filter_map(|edge| edge.label.as_ref().map(|label| label.rect)));
    let points = edges.iter().flat_map(|edge| edge.points.iter().copied());
    let mut width: f64 = 0.0;
    let mut height: f64 = 0.0;
    for rect in rects {
        width = width.max(rect.right());
        height = height.max(rect.bottom());
    }
    for point in points {
        width = width.max(point.x);
        height = height.max(point.y);
    }
    (width + MARGIN, height + MARGIN)
}
