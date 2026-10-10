//! The placed diagram: node boxes, edge polylines, label masks and group zones in final page coordinates.

use std::collections::HashMap;

use crate::geom::{Point, Rect, Segment};
use crate::label::{place, LabelRequest, LABEL_SIZE, LABEL_TRACKING, MASK_HEIGHT, MASK_PADDING};
use crate::layout::{layer, NodeBox};
use crate::node::{content, NodeContent};
use crate::params::{Direction, Params};
use crate::route::route;
use crate::spec::{EdgeStyle, View};
use crate::text::mono_width;

pub const MARGIN: f64 = 40.0;
const LOOP_RISE: f64 = 20.0;
const LOOP_WIDTH: f64 = 36.0;
const LOOP_INSET: f64 = 16.0;
const LABEL_RUN_ROOM: f64 = 24.0;
const ZONE_PADDING: f64 = 16.0;
const ZONE_HEADER: f64 = 16.0;

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

pub struct Scene {
    pub nodes: Vec<SceneNode>,
    pub edges: Vec<SceneEdge>,
    pub zones: Vec<Zone>,
    pub skipped_zones: usize,
    pub width: f64,
    pub height: f64,
}

struct EdgeInput {
    source: usize,
    target: usize,
    style: EdgeStyle,
    text: String,
}

/// Lays out, routes and labels `view`.
pub fn build(view: &View, params: &Params) -> Scene {
    let direction = params.direction_for(view.kind);
    let (ids, contents) = unique_nodes(view);
    let edges = known_edges(view, &ids);
    let (loops, links): (Vec<&EdgeInput>, Vec<&EdgeInput>) = edges.iter().partition(|edge| edge.source == edge.target);
    let boxes: Vec<NodeBox> = contents
        .iter()
        .enumerate()
        .map(|(index, node)| node_box(node, direction, loops.iter().filter(|edge| edge.source == index).count()))
        .collect();
    let pairs: Vec<(usize, usize)> = links.iter().map(|edge| (edge.source, edge.target)).collect();
    let layered = layer(&boxes, &pairs, params.node_gap, params.ordering_sweeps);
    let needs: Vec<f64> = links.iter().map(|edge| run_need(&edge.text, direction)).collect();
    let routed = route(&layered, contents.len(), &needs, direction, params.min_channel, params.track_gap, params.uniform_rank_size);
    let offset = |point: Point| Point::new(point.x + MARGIN, point.y + MARGIN);
    let rects: Vec<Rect> = routed.rects.iter().map(|rect| Rect::new(rect.x + MARGIN, rect.y + MARGIN, rect.w, rect.h)).collect();
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
    let (zones, skipped_zones) = zones(view, &nodes);
    let (width, height) = extent(&nodes, &scene_edges, &zones);
    Scene { nodes, edges: scene_edges, zones, skipped_zones, width, height }
}

fn unique_nodes(view: &View) -> (Vec<String>, Vec<NodeContent>) {
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

fn known_edges(view: &View, ids: &[String]) -> Vec<EdgeInput> {
    let index_of = |id: &str| ids.iter().position(|known| known == id);
    view.edges
        .iter()
        .filter_map(|edge| {
            Some(EdgeInput {
                source: index_of(&edge.source)?,
                target: index_of(&edge.target)?,
                style: edge.style,
                text: edge.label.trim().to_uppercase(),
            })
        })
        .collect()
}

fn mask_width(text: &str) -> f64 {
    mono_width(text, LABEL_SIZE, LABEL_TRACKING) + 2.0 * MASK_PADDING
}

fn run_need(text: &str, direction: Direction) -> f64 {
    match (text.is_empty(), direction) {
        (true, _) => 0.0,
        (false, Direction::Right) => mask_width(text) + LABEL_RUN_ROOM,
        (false, Direction::Down) => MASK_HEIGHT + LABEL_RUN_ROOM,
    }
}

fn node_box(node: &NodeContent, direction: Direction, loops: usize) -> NodeBox {
    let room_low = if loops == 0 { 0.0 } else { LOOP_RISE + 2.0 * MASK_HEIGHT + 4.0 };
    match direction {
        Direction::Right => NodeBox { su: node.width, sv: node.height, room_low },
        Direction::Down => NodeBox { su: node.height, sv: node.width, room_low },
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
