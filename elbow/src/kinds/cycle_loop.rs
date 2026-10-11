//! Loop: stations clockwise on a ring around a shared hub. The hub is the first database, cache or queue
//! node; the stations stand on a circle around it, clockwise from the top in the order the view's edges
//! lead. Edges between neighbouring stations are arcs on the station circle, edges between a station and
//! the hub dashed radial spokes, and any other edge a straight muted chord.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::figure::{Figure, LegendKey, Marker, Paint, Swatch, Tone};
use crate::geom::{snap, Point, Rect};
use crate::label::{LABEL_GAP, MASK_HEIGHT};
use crate::node::NodeContent;
use crate::params::Params;
use crate::scene::{known_edges, mask_width, unique_nodes, Bend, EdgeInput, LabelBox, Scene, SceneEdge, SceneNode};
use crate::spec::{Shape, View};

const START_RADIUS: f64 = 96.0;
const RADIUS_STEP: f64 = 8.0;
const MAX_RADIUS: f64 = 4000.0;
const STATION_GAP: f64 = 24.0;
const HUB_RING_ROOM: f64 = 36.0;
const MIN_SPOKE: f64 = 48.0;
const SPOKE_LABEL_ROOM: f64 = 16.0;
const MIN_ARC: f64 = 56.0;
const MARKER_OVERHANG: f64 = 1.2;
const SPOKE_GAP: f64 = 6.0;
const SPOKE_FAN: f64 = 12.0;
const TWIN_INSET: f64 = 20.0;
const HUB_GROWTH_X: f64 = 32.0;
const HUB_GROWTH_Y: f64 = 16.0;
const ARC_SAMPLES: usize = 24;
const LABEL_SIDE_ROOM: f64 = 1.0;
const LABEL_CLEARANCE: f64 = 2.0;
const LINE_CLEARANCE: f64 = 3.0;
const LABEL_FRACTIONS: [f64; 3] = [0.5, 0.35, 0.65];
const LABEL_STEPS_OUT: [f64; 3] = [0.0, 16.0, 32.0];
const SELF_LOOP_HALF: f64 = 14.0;
const SELF_LOOP_RISE: f64 = 20.0;
const RING_WIDTH: f64 = 1.2;
const FOCAL_WIDTH: f64 = 1.4;
const CHORD_WIDTH: f64 = 1.0;
const SPOKE_DASH: &str = "5,4";
const EPSILON: f64 = 1e-9;

type Vector = (f64, f64);

/// The hub node, if any, and the station nodes in ring order, clockwise from the top.
struct Roles {
    hub: Option<usize>,
    stations: Vec<usize>,
}

impl Roles {
    fn position(&self, node: usize) -> Option<usize> {
        self.stations.iter().position(|&station| station == node)
    }

    fn is_hub(&self, node: usize) -> bool {
        self.hub == Some(node)
    }
}

/// How one edge is drawn: an arc on a circle around the centre, a spoke between a station and the hub
/// (`fan` shifts it sideways when a station has several), or a straight chord.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Route {
    Arc { from: usize, to: usize, inset: f64, clockwise: bool },
    Spoke { station: usize, fan: f64 },
    Chord,
}

/// The placed ring: its centre and radius and one box per node.
struct Ring {
    centre: Point,
    radius: f64,
    rects: Vec<Rect>,
}

/// A drawn edge line: its points and bend, the polyline it covers, and the spots its label may take.
struct Line {
    points: Vec<Point>,
    bend: Bend,
    covered: Vec<Point>,
    spots: Vec<Spot>,
}

/// A point on a line and the side a label beside it moves to.
#[derive(Clone, Copy)]
struct Spot {
    at: Point,
    normal: Vector,
}

/// Lays out `view` as a loop.
pub fn build(view: &View, _params: &Params) -> Scene {
    let (ids, contents) = unique_nodes(view);
    let edges = known_edges(view, &ids);
    let roles = roles(view, &ids, &edges);
    let routes = routes(&roles, &edges);
    let ring = ring(&roles, &contents, &spoke_needs(&roles, &edges, &routes));
    let lines: Vec<Line> = edges.iter().zip(&routes).map(|(edge, route)| line(edge, *route, &ring, &roles)).collect();
    let labels = labels(&edges, &lines, &ring.rects);
    let scene_edges = edges
        .iter()
        .zip(&routes)
        .zip(lines)
        .zip(labels)
        .map(|(((edge, route), line), label)| scene_edge(edge, *route, line, label))
        .collect();
    let nodes = ids.into_iter().zip(contents).zip(&ring.rects).map(|((id, content), &rect)| SceneNode::new(id, rect, content)).collect();
    let decor = ring_extent(&ring, &routes).into_iter().collect();
    let legend = Some(legend(view, &routes));
    Scene { nodes, edges: scene_edges, decor, legend, ..Scene::empty(view.kind) }.measured()
}

fn roles(view: &View, ids: &[String], edges: &[EdgeInput]) -> Roles {
    let hub = hub_of(view, ids);
    let pairs: Vec<(usize, usize)> = edges.iter().map(|edge| (edge.source, edge.target)).collect();
    Roles { hub, stations: station_order(ids.len(), hub, &pairs) }
}

/// The first node in view order drawn as shared state: a database, cache or queue.
fn hub_of(view: &View, ids: &[String]) -> Option<usize> {
    view.nodes
        .iter()
        .find(|spec| matches!(spec.shape, Shape::Database | Shape::Cache | Shape::Queue))
        .and_then(|spec| ids.iter().position(|id| *id == spec.id))
}

/// The stations clockwise from the top: from the first station in view order along the edges to
/// stations not yet placed; when a chain ends, the next unplaced station in view order starts another.
fn station_order(count: usize, hub: Option<usize>, pairs: &[(usize, usize)]) -> Vec<usize> {
    let stations: Vec<usize> = (0..count).filter(|&node| Some(node) != hub).collect();
    let mut order: Vec<usize> = Vec::with_capacity(stations.len());
    while let Some(start) = stations.iter().copied().find(|station| !order.contains(station)) {
        let mut current = start;
        order.push(current);
        while let Some(next) = next_station(current, pairs, &order, hub) {
            order.push(next);
            current = next;
        }
    }
    order
}

fn next_station(current: usize, pairs: &[(usize, usize)], placed: &[usize], hub: Option<usize>) -> Option<usize> {
    pairs
        .iter()
        .find(|&&(source, target)| source == current && Some(target) != hub && !placed.contains(&target))
        .map(|&(_, target)| target)
}

fn routes(roles: &Roles, edges: &[EdgeInput]) -> Vec<Route> {
    let mut taken: Vec<(usize, usize)> = Vec::new();
    let mut routes: Vec<Route> = edges.iter().map(|edge| route(edge.source, edge.target, roles, &mut taken)).collect();
    fan_spokes(&mut routes);
    routes
}

/// The route of an edge from `source` to `target`. Between neighbouring stations it takes the ring slot
/// clockwise between them, whichever way the view points it; a second edge on a taken slot runs on an
/// inner arc in its own direction.
fn route(source: usize, target: usize, roles: &Roles, taken: &mut Vec<(usize, usize)>) -> Route {
    if source == target {
        return Route::Chord;
    }
    if roles.is_hub(target) && roles.position(source).is_some() {
        return Route::Spoke { station: source, fan: 0.0 };
    }
    if roles.is_hub(source) && roles.position(target).is_some() {
        return Route::Spoke { station: target, fan: 0.0 };
    }
    let (Some(from), Some(to)) = (roles.position(source), roles.position(target)) else { return Route::Chord };
    let count = roles.stations.len();
    let forward = (from + 1) % count == to;
    let backward = (to + 1) % count == from;
    if !forward && !backward {
        return Route::Chord;
    }
    let slot = if forward { (source, target) } else { (target, source) };
    if taken.contains(&slot) {
        return Route::Arc { from: source, to: target, inset: TWIN_INSET, clockwise: forward };
    }
    taken.push(slot);
    Route::Arc { from: slot.0, to: slot.1, inset: 0.0, clockwise: true }
}

fn spoke_station(route: &Route) -> Option<usize> {
    match route {
        Route::Spoke { station, .. } => Some(*station),
        _ => None,
    }
}

/// Spreads the spokes of one station side by side, `SPOKE_FAN` apart.
fn fan_spokes(routes: &mut [Route]) {
    let stations: Vec<Option<usize>> = routes.iter().map(spoke_station).collect();
    for (index, route) in routes.iter_mut().enumerate() {
        let Route::Spoke { station, fan } = route else { continue };
        let total = stations.iter().filter(|other| **other == Some(*station)).count();
        let before = stations[..index].iter().filter(|other| **other == Some(*station)).count();
        *fan = (before as f64 - (total - 1) as f64 / 2.0) * SPOKE_FAN;
    }
}

/// The visible spoke length each ring position needs: room for the spoke's label beside it, or a
/// minimum spoke; 0 for a station without a spoke.
fn spoke_needs(roles: &Roles, edges: &[EdgeInput], routes: &[Route]) -> Vec<f64> {
    let count = roles.stations.len();
    let mut needs = vec![0.0_f64; count];
    for (edge, route) in edges.iter().zip(routes) {
        let Some(position) = spoke_station(route).and_then(|station| roles.position(station)) else { continue };
        let direction = polar_unit(station_angle(position, count));
        let need = if edge.text.is_empty() { MIN_SPOKE } else { extent_along(direction, mask_width(&edge.text), MASK_HEIGHT) + 2.0 * SPOKE_LABEL_ROOM };
        needs[position] = needs[position].max(need);
    }
    needs
}

fn extent_along(direction: Vector, width: f64, height: f64) -> f64 {
    direction.0.abs() * width + direction.1.abs() * height
}

fn station_angle(position: usize, count: usize) -> f64 {
    -FRAC_PI_2 + position as f64 * TAU / count as f64
}

/// The smallest ring, grown in steps, on which no boxes come close, every arc has a visible length,
/// the hub stays inside the ring and every spoke has the length it needs.
fn ring(roles: &Roles, contents: &[NodeContent], needs: &[f64]) -> Ring {
    let station_size = roles
        .stations
        .iter()
        .map(|&station| (contents[station].width, contents[station].height))
        .fold((0.0_f64, 0.0_f64), |a, b| (a.0.max(b.0), a.1.max(b.1)));
    let hub_size = roles.hub.map(|hub| (contents[hub].width.max(station_size.0 + HUB_GROWTH_X), contents[hub].height.max(station_size.1 + HUB_GROWTH_Y)));
    let mut radius = START_RADIUS;
    loop {
        let ring = placed(roles, contents.len(), station_size, hub_size, radius);
        if radius >= MAX_RADIUS || fits(&ring, roles, needs) {
            return ring;
        }
        radius += RADIUS_STEP;
    }
}

fn placed(roles: &Roles, count: usize, station: (f64, f64), hub: Option<(f64, f64)>, radius: f64) -> Ring {
    let centre = Point::new(0.0, 0.0);
    let mut rects = vec![Rect::new(0.0, 0.0, 0.0, 0.0); count];
    for (position, &node) in roles.stations.iter().enumerate() {
        rects[node] = box_around(polar(centre, radius, station_angle(position, roles.stations.len())), station);
    }
    if let (Some(node), Some(size)) = (roles.hub, hub) {
        rects[node] = box_around(centre, size);
    }
    Ring { centre, radius, rects }
}

fn box_around(at: Point, (width, height): (f64, f64)) -> Rect {
    Rect::new(snap(at.x - width / 2.0), snap(at.y - height / 2.0), width, height)
}

fn fits(ring: &Ring, roles: &Roles, needs: &[f64]) -> bool {
    stations_apart(ring, roles) && hub_clear(ring, roles, needs) && arcs_visible(ring, roles)
}

fn stations_apart(ring: &Ring, roles: &Roles) -> bool {
    let rects: Vec<Rect> = roles.stations.iter().map(|&station| ring.rects[station].inflate(STATION_GAP / 2.0)).collect();
    rects.iter().enumerate().all(|(index, rect)| rects[index + 1..].iter().all(|other| !rect.overlaps(other)))
}

fn hub_clear(ring: &Ring, roles: &Roles, needs: &[f64]) -> bool {
    let Some(hub) = roles.hub.map(|hub| ring.rects[hub]) else { return true };
    let corner = (hub.w * hub.w + hub.h * hub.h).sqrt() / 2.0 + distance(ring.centre, centre_of(&hub));
    let inside = corner <= ring.radius - HUB_RING_ROOM;
    inside
        && roles.stations.iter().enumerate().all(|(position, &station)| {
            let rect = ring.rects[station];
            !hub.inflate(STATION_GAP).overlaps(&rect) && spoke_length(ring.centre, &hub, &rect) >= needs[position]
        })
}

/// The length of the radius between a station's border and the hub's border.
fn spoke_length(centre: Point, hub: &Rect, station: &Rect) -> f64 {
    let middle = centre_of(station);
    let outward = unit(centre, middle);
    distance(centre, middle) - exit_distance(station, middle, scaled(outward, -1.0)) - exit_distance(hub, centre, outward)
}

fn arcs_visible(ring: &Ring, roles: &Roles) -> bool {
    let count = roles.stations.len();
    if count < 2 {
        return true;
    }
    (0..count).all(|position| {
        let from = &ring.rects[roles.stations[position]];
        let to = &ring.rects[roles.stations[(position + 1) % count]];
        arc(ring.centre, ring.radius, from, to, true).is_some_and(|[start, end]| {
            let turn = sweep(ring.centre, start, end, true);
            turn * ring.radius >= MIN_ARC && turn < PI
        })
    })
}

/// The arc on the circle around `centre` from where it leaves `from` to just before it enters `to`, so
/// the arrowhead's tip lands on the border of `to`.
fn arc(centre: Point, radius: f64, from: &Rect, to: &Rect, clockwise: bool) -> Option<[Point; 2]> {
    let start = border_crossing(centre, radius, from, clockwise)?;
    let entry = border_crossing(centre, radius, to, !clockwise)?;
    let back = if clockwise { -MARKER_OVERHANG } else { MARKER_OVERHANG } / radius;
    Some([start, polar(centre, radius, angle(centre, entry) + back)])
}

/// Where the circle crosses the border of `rect`: on the clockwise side of the box or the other.
fn border_crossing(centre: Point, radius: f64, rect: &Rect, clockwise_side: bool) -> Option<Point> {
    let middle = angle(centre, centre_of(rect));
    let offset = |point: &Point| (angle(centre, *point) - middle).rem_euclid(TAU);
    let crossings = circle_crossings(centre, radius, rect).into_iter();
    if clockwise_side {
        crossings.min_by(|a, b| offset(a).total_cmp(&offset(b)))
    } else {
        crossings.max_by(|a, b| offset(a).total_cmp(&offset(b)))
    }
}

fn circle_crossings(centre: Point, radius: f64, rect: &Rect) -> Vec<Point> {
    let across = |offset: f64| (offset.abs() <= radius).then(|| (radius * radius - offset * offset).sqrt());
    let mut crossings = Vec::new();
    for x in [rect.x, rect.right()] {
        let Some(rise) = across(x - centre.x) else { continue };
        let ys = [centre.y - rise, centre.y + rise].into_iter().filter(|y| (rect.y..=rect.bottom()).contains(y));
        crossings.extend(ys.map(|y| Point::new(x, y)));
    }
    for y in [rect.y, rect.bottom()] {
        let Some(run) = across(y - centre.y) else { continue };
        let xs = [centre.x - run, centre.x + run].into_iter().filter(|x| (rect.x..=rect.right()).contains(x));
        crossings.extend(xs.map(|x| Point::new(x, y)));
    }
    crossings
}

/// The angle turned from `from` to `to` around `centre`, in the given direction.
fn sweep(centre: Point, from: Point, to: Point, clockwise: bool) -> f64 {
    let turn = angle(centre, to) - angle(centre, from);
    if clockwise {
        turn.rem_euclid(TAU)
    } else {
        (-turn).rem_euclid(TAU)
    }
}

fn line(edge: &EdgeInput, route: Route, ring: &Ring, roles: &Roles) -> Line {
    match route {
        Route::Arc { from, to, inset, clockwise } => arc_line(ring, from, to, ring.radius - inset, clockwise)
            .unwrap_or_else(|| chord_line(&ring.rects[from], &ring.rects[to])),
        Route::Spoke { station, fan } => spoke_line(ring, roles, station, fan, edge.source == station),
        Route::Chord if edge.source == edge.target => {
            let rect = ring.rects[edge.source];
            self_loop_line(&rect, unit(ring.centre, centre_of(&rect)))
        }
        Route::Chord => chord_line(&ring.rects[edge.source], &ring.rects[edge.target]),
    }
}

/// A small loop out of the side of `rect` that faces away from the ring's centre.
fn self_loop_line(rect: &Rect, outward: Vector) -> Line {
    let (normal, along) =
        if outward.1.abs() >= outward.0.abs() { ((0.0, outward.1.signum()), (1.0, 0.0)) } else { ((outward.0.signum(), 0.0), (0.0, 1.0)) };
    let side = offset(centre_of(rect), normal, extent_along(normal, rect.w, rect.h) / 2.0);
    let (start, end) = (offset(side, along, -SELF_LOOP_HALF), offset(side, along, SELF_LOOP_HALF));
    let points = vec![start, offset(start, normal, SELF_LOOP_RISE), offset(end, normal, SELF_LOOP_RISE), end];
    let spots = vec![Spot { at: offset(side, normal, SELF_LOOP_RISE), normal }];
    Line { covered: points.clone(), points, bend: Bend::Free, spots }
}

fn arc_line(ring: &Ring, from: usize, to: usize, radius: f64, clockwise: bool) -> Option<Line> {
    let [start, end] = arc(ring.centre, radius, &ring.rects[from], &ring.rects[to], clockwise)?;
    let turn = sweep(ring.centre, start, end, clockwise);
    let heading = if clockwise { 1.0 } else { -1.0 };
    let at = |fraction: f64| angle(ring.centre, start) + heading * turn * fraction;
    let covered = (0..=ARC_SAMPLES).map(|step| polar(ring.centre, radius, at(step as f64 / ARC_SAMPLES as f64))).collect();
    let spots = LABEL_FRACTIONS
        .iter()
        .flat_map(|&fraction| {
            let point = polar(ring.centre, radius, at(fraction));
            let outward = polar_unit(at(fraction));
            [Spot { at: point, normal: outward }, Spot { at: point, normal: scaled(outward, -1.0) }]
        })
        .collect();
    Some(Line { points: vec![start, end], bend: Bend::Arc { radius, clockwise }, covered, spots })
}

/// A dashed spoke along the radius through `station`, from the source's border to just before the
/// target's border.
fn spoke_line(ring: &Ring, roles: &Roles, station: usize, fan: f64, outward_source: bool) -> Line {
    let Some(hub) = roles.hub.map(|hub| ring.rects[hub]) else { return chord_line(&ring.rects[station], &ring.rects[station]) };
    let rect = ring.rects[station];
    let outward = unit(ring.centre, centre_of(&rect));
    let side = (-outward.1, outward.0);
    let on_station = offset(centre_of(&rect), side, fan);
    let on_hub = offset(ring.centre, side, fan);
    let station_end = offset(on_station, outward, -exit_distance(&rect, on_station, scaled(outward, -1.0)));
    let hub_end = offset(on_hub, outward, exit_distance(&hub, on_hub, outward));
    let points = if outward_source {
        vec![station_end, offset(hub_end, outward, SPOKE_GAP)]
    } else {
        vec![hub_end, offset(station_end, outward, -SPOKE_GAP)]
    };
    straight(points)
}

fn chord_line(from: &Rect, to: &Rect) -> Line {
    let (from_middle, to_middle) = (centre_of(from), centre_of(to));
    straight(vec![exit_point(from, from_middle, to_middle), exit_point(to, to_middle, from_middle)])
}

fn straight(points: Vec<Point>) -> Line {
    let (start, end) = (points[0], points[points.len() - 1]);
    let along = unit(start, end);
    let side = (-along.1, along.0);
    let spots = LABEL_FRACTIONS
        .iter()
        .flat_map(|&fraction| {
            let at = Point::new(start.x + (end.x - start.x) * fraction, start.y + (end.y - start.y) * fraction);
            [Spot { at, normal: side }, Spot { at, normal: scaled(side, -1.0) }]
        })
        .collect();
    Line { covered: points.clone(), points, bend: Bend::Free, spots }
}

/// One masked label per labelled edge: the first spot beside its own line that covers no box, no other
/// label and no other line, or the least crowded one.
fn labels(edges: &[EdgeInput], lines: &[Line], boxes: &[Rect]) -> Vec<Option<Rect>> {
    let covered: Vec<&[Point]> = lines.iter().map(|line| line.covered.as_slice()).collect();
    let mut placed: Vec<Rect> = Vec::new();
    edges
        .iter()
        .zip(lines)
        .enumerate()
        .map(|(index, (edge, line))| {
            if edge.text.is_empty() {
                return None;
            }
            let (width, height) = (mask_width(&edge.text), MASK_HEIGHT);
            let candidates = LABEL_STEPS_OUT.iter().flat_map(|&out| line.spots.iter().map(move |spot| beside(*spot, width, height, out)));
            let best = candidates.enumerate().min_by_key(|(order, rect)| (conflicts(rect, index, &covered, boxes, &placed), *order)).map(|(_, rect)| rect)?;
            placed.push(best);
            Some(best)
        })
        .collect()
}

fn beside(spot: Spot, width: f64, height: f64, out: f64) -> Rect {
    let away = LABEL_GAP + LABEL_SIDE_ROOM + out + extent_along(spot.normal, width, height) / 2.0;
    let middle = offset(spot.at, spot.normal, away);
    Rect::new(middle.x - width / 2.0, middle.y - height / 2.0, width, height)
}

fn conflicts(rect: &Rect, own: usize, lines: &[&[Point]], boxes: &[Rect], placed: &[Rect]) -> usize {
    let padded = rect.inflate(LABEL_CLEARANCE);
    let on_boxes = boxes.iter().filter(|other| padded.overlaps(other)).count();
    let on_labels = placed.iter().filter(|other| padded.overlaps(other)).count();
    let on_lines = lines
        .iter()
        .enumerate()
        .filter(|(index, line)| polyline_distance(rect, line) < if *index == own { LABEL_GAP - 1.0 } else { LINE_CLEARANCE })
        .count();
    on_boxes * 4 + on_labels * 4 + on_lines * 2
}

fn scene_edge(edge: &EdgeInput, route: Route, line: Line, label: Option<Rect>) -> SceneEdge {
    SceneEdge {
        bend: line.bend,
        markers: Some((Marker::None, Marker::Arrow)),
        paint: Some(paint(route, edge.focal)),
        label: label.map(|rect| LabelBox { rect, text: edge.text.clone() }),
        ..SceneEdge::new(edge, line.points)
    }
}

fn paint(route: Route, focal: bool) -> Paint {
    match (route, focal) {
        (Route::Spoke { .. }, true) => Paint::line(Tone::Accent, FOCAL_WIDTH).dashed(SPOKE_DASH),
        (Route::Spoke { .. }, false) => Paint::line(Tone::Soft, RING_WIDTH).dashed(SPOKE_DASH),
        (_, true) => Paint::line(Tone::Accent, FOCAL_WIDTH),
        (Route::Arc { .. }, false) => Paint::line(Tone::Muted, RING_WIDTH),
        (Route::Chord, false) => Paint::line(Tone::Muted, CHORD_WIDTH),
    }
}

/// An invisible square around the station circle, so the canvas holds the arcs where they bulge past
/// the boxes.
fn ring_extent(ring: &Ring, routes: &[Route]) -> Option<Figure> {
    let has_arcs = routes.iter().any(|route| matches!(route, Route::Arc { .. }));
    let side = 2.0 * ring.radius;
    has_arcs.then(|| Figure::rect(Rect::new(ring.centre.x - ring.radius, ring.centre.y - ring.radius, side, side), 0.0, Paint::filled(Tone::None)))
}

fn legend(view: &View, routes: &[Route]) -> Vec<LegendKey> {
    let has = |wanted: fn(&Route) -> bool| routes.iter().any(wanted);
    let focus = view.nodes.iter().any(|node| node.focal) || view.edges.iter().any(|edge| edge.focal);
    let keys = [
        has(|route| matches!(route, Route::Arc { .. })).then(|| LegendKey::new(Swatch::Line(Paint::line(Tone::Muted, RING_WIDTH), Marker::Arrow), "RING FLOW")),
        has(|route| matches!(route, Route::Spoke { .. }))
            .then(|| LegendKey::new(Swatch::Line(paint(Route::Spoke { station: 0, fan: 0.0 }, false), Marker::Arrow), "WRITE-BACK")),
        has(|route| *route == Route::Chord).then(|| LegendKey::new(Swatch::Line(paint(Route::Chord, false), Marker::Arrow), "LINK")),
        focus.then(|| LegendKey::new(Swatch::Box(Paint::shape(Tone::AccentTint, Tone::Accent, 1.0)), "FOCUS")),
    ];
    keys.into_iter().flatten().collect()
}

/// The point where a ray from `from` (inside `rect`) toward `toward` leaves the rectangle.
pub fn exit_point(rect: &Rect, from: Point, toward: Point) -> Point {
    let direction = unit(from, toward);
    if direction == (0.0, 0.0) {
        return from;
    }
    offset(from, direction, exit_distance(rect, from, direction))
}

fn exit_distance(rect: &Rect, from: Point, direction: Vector) -> f64 {
    let along = |step: f64, low: f64, high: f64, at: f64| {
        if step > EPSILON {
            (high - at) / step
        } else if step < -EPSILON {
            (low - at) / step
        } else {
            f64::INFINITY
        }
    };
    let distance = along(direction.0, rect.x, rect.right(), from.x).min(along(direction.1, rect.y, rect.bottom(), from.y));
    if distance.is_finite() { distance } else { 0.0 }
}

/// The shortest distance between a rectangle and a polyline in any direction.
pub fn polyline_distance(rect: &Rect, line: &[Point]) -> f64 {
    line.windows(2).map(|pair| segment_distance(rect, pair[0], pair[1])).fold(f64::INFINITY, f64::min)
}

fn segment_distance(rect: &Rect, a: Point, b: Point) -> f64 {
    if segment_enters(rect, a, b) {
        return 0.0;
    }
    let corners = [Point::new(rect.x, rect.y), Point::new(rect.right(), rect.y), Point::new(rect.x, rect.bottom()), Point::new(rect.right(), rect.bottom())];
    let from_corners = corners.iter().map(|&corner| point_to_segment(corner, a, b)).fold(f64::INFINITY, f64::min);
    from_corners.min(point_to_rect(rect, a)).min(point_to_rect(rect, b))
}

/// True when some part of the segment lies inside the rectangle (Liang-Barsky clipping).
fn segment_enters(rect: &Rect, a: Point, b: Point) -> bool {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let limits = [(-dx, a.x - rect.x), (dx, rect.right() - a.x), (-dy, a.y - rect.y), (dy, rect.bottom() - a.y)];
    let mut span = (0.0_f64, 1.0_f64);
    for (step, room) in limits {
        if step.abs() < EPSILON {
            if room < 0.0 {
                return false;
            }
            continue;
        }
        let at = room / step;
        span = if step < 0.0 { (span.0.max(at), span.1) } else { (span.0, span.1.min(at)) };
        if span.0 > span.1 {
            return false;
        }
    }
    true
}

fn point_to_segment(point: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length = dx * dx + dy * dy;
    let along = if length < EPSILON { 0.0 } else { (((point.x - a.x) * dx + (point.y - a.y) * dy) / length).clamp(0.0, 1.0) };
    distance(point, Point::new(a.x + along * dx, a.y + along * dy))
}

fn point_to_rect(rect: &Rect, point: Point) -> f64 {
    let dx = (rect.x - point.x).max(point.x - rect.right()).max(0.0);
    let dy = (rect.y - point.y).max(point.y - rect.bottom()).max(0.0);
    (dx * dx + dy * dy).sqrt()
}

fn centre_of(rect: &Rect) -> Point {
    Point::new(rect.cx(), rect.y + rect.h / 2.0)
}

fn distance(a: Point, b: Point) -> f64 {
    (b.x - a.x).hypot(b.y - a.y)
}

fn unit(from: Point, to: Point) -> Vector {
    let length = distance(from, to);
    if length < EPSILON {
        (0.0, 0.0)
    } else {
        ((to.x - from.x) / length, (to.y - from.y) / length)
    }
}

fn scaled(vector: Vector, by: f64) -> Vector {
    (vector.0 * by, vector.1 * by)
}

fn offset(point: Point, direction: Vector, by: f64) -> Point {
    Point::new(point.x + direction.0 * by, point.y + direction.1 * by)
}

fn angle(centre: Point, point: Point) -> f64 {
    (point.y - centre.y).atan2(point.x - centre.x)
}

fn polar_unit(angle: f64) -> Vector {
    (angle.cos(), angle.sin())
}

fn polar(centre: Point, radius: f64, angle: f64) -> Point {
    offset(centre, polar_unit(angle), radius)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{EdgeSpec, Kind, NodeSpec};

    fn node(id: &str, shape: Shape) -> NodeSpec {
        NodeSpec { id: id.into(), label: format!("{id} (step)"), shape, focal: false }
    }

    fn edge(source: &str, target: &str, label: &str) -> EdgeSpec {
        EdgeSpec { id: format!("{source}->{target}"), source: source.into(), target: target.into(), label: label.into(), style: Default::default(), focal: false }
    }

    fn ring_view(stations: usize, with_hub: bool) -> View {
        let names: Vec<String> = (0..stations).map(|index| format!("s{index}")).collect();
        let mut nodes: Vec<NodeSpec> = names.iter().map(|name| node(name, Shape::Service)).collect();
        let mut edges: Vec<EdgeSpec> = (0..stations).map(|index| edge(&names[index], &names[(index + 1) % stations], "")).collect();
        if with_hub {
            nodes.insert(0, node("hub", Shape::Database));
            edges.extend(names.iter().map(|name| edge(name, "hub", "writes back")));
        }
        View { kind: Kind::Loop, nodes, edges, ..View::default() }
    }

    fn scene(view: &View) -> Scene {
        build(view, &Params::default())
    }

    fn on_border(rect: &Rect, point: Point, tolerance: f64) -> bool {
        let inside_x = point.x >= rect.x - tolerance && point.x <= rect.right() + tolerance;
        let inside_y = point.y >= rect.y - tolerance && point.y <= rect.bottom() + tolerance;
        let near_side = [(point.x - rect.x).abs(), (point.x - rect.right()).abs(), (point.y - rect.y).abs(), (point.y - rect.bottom()).abs()]
            .into_iter()
            .fold(f64::INFINITY, f64::min);
        inside_x && inside_y && near_side <= tolerance
    }

    #[test]
    fn a_self_loop_leaves_its_station_on_the_outer_side_and_its_label_clears_every_box() {
        let mut view = ring_view(4, true);
        view.edges.push(edge("s0", "s0", "retries"));
        let scene = scene(&view);
        let looped = scene.edges.iter().find(|edge| edge.id == "s0->s0").expect("the self loop");
        let station = scene.nodes.iter().find(|node| node.id == "s0").expect("s0").rect;
        assert_eq!(looped.points.len(), 4);
        assert!(looped.points.iter().all(|point| point.y <= station.y + 0.01), "s0 is the top station: the loop rises above it");
        let label = looped.label.as_ref().expect("a label").rect;
        assert!(scene.nodes.iter().all(|node| !label.overlaps(&node.rect)));
    }

    #[test]
    fn the_hub_is_the_first_database_cache_or_queue() {
        let mut view = ring_view(4, false);
        view.nodes.push(node("cache", Shape::Cache));
        view.nodes.push(node("db", Shape::Database));
        let (ids, _) = unique_nodes(&view);
        assert_eq!(hub_of(&view, &ids).map(|hub| ids[hub].as_str()), Some("cache"));
    }

    #[test]
    fn without_a_store_node_there_is_no_hub_and_every_node_is_a_station() {
        let view = ring_view(5, false);
        let (ids, _) = unique_nodes(&view);
        let roles = roles(&view, &ids, &known_edges(&view, &ids));
        assert_eq!(roles.hub, None);
        assert_eq!(roles.stations, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn stations_follow_the_edges_clockwise_from_the_first_station_in_view_order() {
        let pairs = [(1, 3), (3, 2), (2, 4), (4, 1), (0, 3)];
        assert_eq!(station_order(5, Some(0), &pairs), vec![1, 3, 2, 4]);
    }

    #[test]
    fn a_broken_chain_continues_from_the_next_unplaced_station_in_view_order() {
        let pairs = [(0, 1), (2, 3)];
        assert_eq!(station_order(5, None, &pairs), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn ring_arcs_start_and_end_on_the_station_borders_and_on_one_circle() {
        let scene = scene(&ring_view(6, true));
        let arcs: Vec<&SceneEdge> = scene.edges.iter().filter(|edge| matches!(edge.bend, Bend::Arc { .. })).collect();
        assert_eq!(arcs.len(), 6);
        for arc in arcs {
            let Bend::Arc { radius, clockwise } = arc.bend else { unreachable!() };
            assert!(clockwise);
            let (start, end) = (arc.points[0], arc.points[1]);
            assert!(on_border(&scene.nodes[arc.source].rect, start, 0.01), "{} starts off its border", arc.id);
            assert!(on_border(&scene.nodes[arc.target].rect, end, MARKER_OVERHANG + 0.01), "{} ends off its border", arc.id);
            for point in [start, end] {
                assert!((distance(Point::new(0.0, 0.0), point) - radius).abs() < 0.01);
            }
        }
    }

    #[test]
    fn stations_never_overlap_and_every_arc_has_a_visible_length() {
        for count in 2..=8 {
            for with_hub in [false, true] {
                let scene = scene(&ring_view(count, with_hub));
                let rects: Vec<Rect> = scene.nodes.iter().map(|node| node.rect).collect();
                for (index, rect) in rects.iter().enumerate() {
                    assert!(rects[index + 1..].iter().all(|other| !rect.overlaps(other)), "{count} stations overlap");
                }
                for arc in scene.edges.iter().filter(|edge| matches!(edge.bend, Bend::Arc { .. })) {
                    let Bend::Arc { radius, .. } = arc.bend else { unreachable!() };
                    let turn = sweep(Point::new(0.0, 0.0), arc.points[0], arc.points[1], true);
                    assert!(turn * radius >= MIN_ARC - MARKER_OVERHANG, "{count} stations: {} is too short", arc.id);
                }
            }
        }
    }

    #[test]
    fn a_spoke_runs_along_the_radius_from_the_station_border_to_just_before_the_hub() {
        let scene = scene(&ring_view(5, true));
        let hub = scene.nodes[0].rect;
        for spoke in scene.edges.iter().filter(|edge| edge.target == 0) {
            let (start, end) = (spoke.points[0], spoke.points[1]);
            let cross = start.x * end.y - start.y * end.x;
            assert!(cross.abs() < 1.0, "{} is not a radius", spoke.id);
            assert!(on_border(&scene.nodes[spoke.source].rect, start, 0.01));
            let hub_border = exit_point(&hub, Point::new(0.0, 0.0), end);
            assert!((distance(hub_border, end) - SPOKE_GAP).abs() < 0.01, "{} stops {SPOKE_GAP}px before the hub", spoke.id);
            assert_eq!(spoke.paint.and_then(|paint| paint.dash), Some(SPOKE_DASH));
        }
    }

    #[test]
    fn spoke_labels_clear_every_box_label_and_other_line() {
        for count in 3..=8 {
            let scene = scene(&ring_view(count, true));
            let lines: Vec<Vec<Point>> = scene.edges.iter().map(|edge| covered(edge)).collect();
            let labels: Vec<(usize, Rect)> = scene.edges.iter().enumerate().filter_map(|(index, edge)| edge.label.as_ref().map(|label| (index, label.rect))).collect();
            assert_eq!(labels.len(), count);
            for (position, (own, rect)) in labels.iter().enumerate() {
                assert!(scene.nodes.iter().all(|node| !rect.overlaps(&node.rect)), "{count}: label on a box");
                assert!(labels[position + 1..].iter().all(|(_, other)| !rect.overlaps(other)), "{count}: labels overlap");
                for (index, line) in lines.iter().enumerate() {
                    let clearance = if index == *own { LABEL_GAP - 1.0 } else { LINE_CLEARANCE };
                    assert!(polyline_distance(rect, line) >= clearance, "{count}: label of edge {own} on line {index}");
                }
            }
        }
    }

    fn covered(edge: &SceneEdge) -> Vec<Point> {
        match edge.bend {
            Bend::Arc { radius, clockwise } => {
                let centre = Point::new(0.0, 0.0);
                let turn = sweep(centre, edge.points[0], edge.points[1], clockwise);
                let heading = if clockwise { 1.0 } else { -1.0 };
                (0..=ARC_SAMPLES).map(|step| polar(centre, radius, angle(centre, edge.points[0]) + heading * turn * step as f64 / ARC_SAMPLES as f64)).collect()
            }
            _ => edge.points.clone(),
        }
    }

    #[test]
    fn an_edge_against_the_ring_is_drawn_clockwise_and_a_second_one_on_an_inner_arc() {
        let mut view = ring_view(4, false);
        view.edges[1] = edge("s2", "s1", "");
        view.edges.push(edge("s1", "s0", ""));
        let scene = scene(&view);
        let against = &scene.edges[1];
        assert!(matches!(against.bend, Bend::Arc { clockwise: true, .. }));
        assert!(on_border(&scene.nodes[1].rect, against.points[0], 0.01), "the arc leaves s1 clockwise");
        let twin = scene.edges.last().expect("the twin");
        let outer = match scene.edges[0].bend {
            Bend::Arc { radius, .. } => radius,
            _ => 0.0,
        };
        assert_eq!(twin.bend, Bend::Arc { radius: outer - TWIN_INSET, clockwise: false });
    }

    #[test]
    fn a_chord_joins_stations_that_are_not_neighbours_in_a_straight_muted_line() {
        let mut view = ring_view(6, false);
        view.edges.push(edge("s0", "s3", ""));
        let scene = scene(&view);
        let chord = scene.edges.last().expect("the chord");
        assert_eq!(chord.bend, Bend::Free);
        assert_eq!(chord.paint, Some(Paint::line(Tone::Muted, CHORD_WIDTH)));
        assert!(on_border(&scene.nodes[0].rect, chord.points[0], 0.01));
        assert!(on_border(&scene.nodes[3].rect, chord.points[1], 0.01));
    }

    #[test]
    fn the_hub_has_the_store_look_and_the_focal_station_the_accent() {
        let mut view = ring_view(5, true);
        view.nodes[2].focal = true;
        let scene = scene(&view);
        assert_eq!(scene.nodes[0].content.treatment, crate::node::Treatment::Store);
        assert_eq!(scene.nodes[2].content.treatment, crate::node::Treatment::Focal);
        assert!(scene.nodes.iter().all(|node| node.figures.is_empty()));
    }
}
