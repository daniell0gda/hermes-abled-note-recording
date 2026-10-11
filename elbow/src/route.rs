//! Orthogonal edge routing between ranks: each bending hop gets its own vertical track in the channel
//! between two ranks, ordered so that tracks cross as few horizontal runs as possible.

use std::collections::HashMap;

use crate::geom::{simplify, snap_up, Point, Rect, Segment};
use crate::layout::Layered;
use crate::params::Direction;

const MIN_ZONE: f64 = 24.0;
const JOG_LIMIT: f64 = 16.0;
const PORT_MARGIN: f64 = 8.0;

/// Which run of a hop carries the edge label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelRun {
    Source,
    Target,
    Whole,
}

#[derive(Clone, Copy, Debug)]
struct LabelPlan {
    hop: usize,
    run: LabelRun,
    need: f64,
}

/// The routed diagram in real coordinates, before margins.
pub struct Routed {
    pub rects: Vec<Rect>,
    pub paths: Vec<Vec<Point>>,
    pub preferred: Vec<Option<Segment>>,
    pub width: f64,
    pub height: f64,
}

struct Channel {
    tracks: HashMap<usize, usize>,
    source_zone: f64,
    track_gap: f64,
}

/// Routes every chain of `layered`. `label_needs[chain]` is the run length a chain's label needs (0: no label).
pub fn route(
    layered: &Layered,
    node_count: usize,
    label_needs: &[f64],
    direction: Direction,
    min_channel: f64,
    track_gap: f64,
    uniform: bool,
) -> Routed {
    let (from_v, to_v) = ports(layered);
    let plans: Vec<Option<LabelPlan>> = (0..layered.chains.len())
        .map(|chain| label_plan(layered, chain, label_needs[chain], &from_v, &to_v))
        .collect();
    let rank_size: Vec<f64> = layered
        .ranks
        .iter()
        .map(|rank| rank.iter().map(|&vertex| layered.vertices[vertex].su).fold(0.0, f64::max))
        .collect();
    let mut channels = Vec::new();
    let mut rank_start = vec![0.0; layered.ranks.len()];
    for rank in 0..layered.ranks.len() {
        if rank > 0 {
            let channel: &(Channel, f64) = &channels[rank - 1];
            rank_start[rank] = rank_start[rank - 1] + rank_size[rank - 1] + channel.1;
        }
        if rank + 1 < layered.ranks.len() {
            channels.push(channel(layered, rank, &from_v, &to_v, &plans, min_channel, track_gap));
        }
    }
    let geometry = Geometry { layered, rank_start: &rank_start, rank_size: &rank_size, uniform };
    let mut paths = Vec::with_capacity(layered.chains.len());
    let mut preferred = Vec::with_capacity(layered.chains.len());
    for (index, chain) in layered.chains.iter().enumerate() {
        let mut points = Vec::new();
        let mut label_run = None;
        for &hop_index in &chain.hops {
            let hop = &layered.hops[hop_index];
            let rank = layered.vertices[hop.from].rank;
            let (channel, _) = &channels[rank];
            let start = Point::new(geometry.out_u(hop.from), from_v[hop_index]);
            let end = Point::new(geometry.in_u(hop.to), to_v[hop_index]);
            let track = channel
                .tracks
                .get(&hop_index)
                .map(|&track| geometry.channel_start(rank) + channel.source_zone + track as f64 * channel.track_gap);
            points.push(start);
            if let Some(t) = track {
                points.push(Point::new(t, start.y));
                points.push(Point::new(t, end.y));
            }
            points.push(end);
            if layered.vertices[hop.to].dummy {
                points.push(Point::new(geometry.out_u(hop.to), end.y));
            }
            if let Some(plan) = plans[index].filter(|plan| plan.hop == hop_index) {
                label_run = Some(run_segment(plan.run, start, end, track));
            }
        }
        let mut points = simplify(&points);
        if chain.reversed {
            points.reverse();
        }
        paths.push(points.into_iter().map(|point| to_real(point, direction)).collect());
        preferred.push(label_run.map(|segment| Segment::new(to_real(segment.a, direction), to_real(segment.b, direction))));
    }
    let rects: Vec<Rect> = (0..node_count).map(|vertex| geometry.rect(vertex, direction)).collect();
    let (width, height) = extent(&rects, &paths);
    Routed { rects, paths, preferred, width, height }
}


fn run_segment(run: LabelRun, start: Point, end: Point, track: Option<f64>) -> Segment {
    match (run, track) {
        (LabelRun::Source, Some(t)) => Segment::new(start, Point::new(t, start.y)),
        (LabelRun::Target, Some(t)) => Segment::new(Point::new(t, end.y), end),
        _ => Segment::new(start, Point::new(end.x, start.y)),
    }
}

fn to_real(point: Point, direction: Direction) -> Point {
    match direction {
        Direction::Right => point,
        Direction::Down => Point::new(point.y, point.x),
    }
}

fn extent(rects: &[Rect], paths: &[Vec<Point>]) -> (f64, f64) {
    let mut width: f64 = 0.0;
    let mut height: f64 = 0.0;
    for rect in rects {
        width = width.max(rect.right());
        height = height.max(rect.bottom());
    }
    for point in paths.iter().flatten() {
        width = width.max(point.x);
        height = height.max(point.y);
    }
    (width, height)
}

struct Geometry<'a> {
    layered: &'a Layered,
    rank_start: &'a [f64],
    rank_size: &'a [f64],
    uniform: bool,
}

impl Geometry<'_> {
    fn su(&self, vertex: usize) -> f64 {
        let own = &self.layered.vertices[vertex];
        if self.uniform && !own.dummy {
            self.rank_size[own.rank]
        } else {
            own.su
        }
    }

    fn start_u(&self, vertex: usize) -> f64 {
        let rank = self.layered.vertices[vertex].rank;
        self.rank_start[rank] + (self.rank_size[rank] - self.su(vertex)) / 2.0
    }

    fn in_u(&self, vertex: usize) -> f64 {
        if self.layered.vertices[vertex].dummy {
            return self.rank_start[self.layered.vertices[vertex].rank];
        }
        self.start_u(vertex)
    }

    fn out_u(&self, vertex: usize) -> f64 {
        let rank = self.layered.vertices[vertex].rank;
        if self.layered.vertices[vertex].dummy {
            return self.rank_start[rank] + self.rank_size[rank];
        }
        self.start_u(vertex) + self.su(vertex)
    }

    fn channel_start(&self, rank: usize) -> f64 {
        self.rank_start[rank] + self.rank_size[rank]
    }

    fn rect(&self, vertex: usize, direction: Direction) -> Rect {
        let own = &self.layered.vertices[vertex];
        let u = self.start_u(vertex);
        let v = own.v - own.sv / 2.0;
        match direction {
            Direction::Right => Rect::new(u, v, self.su(vertex), own.sv),
            Direction::Down => Rect::new(v, u, own.sv, self.su(vertex)),
        }
    }
}

fn ports(layered: &Layered) -> (Vec<f64>, Vec<f64>) {
    let mut from_v: Vec<f64> = layered.hops.iter().map(|hop| layered.start_port(hop)).collect();
    let mut to_v: Vec<f64> = layered.hops.iter().map(|hop| layered.end_port(hop)).collect();
    for (index, hop) in layered.hops.iter().enumerate() {
        let jog = (to_v[index] - from_v[index]).abs();
        if jog == 0.0 || jog > JOG_LIMIT {
            continue;
        }
        if sole_port_reaches(layered, hop.to, hop.to_slot, from_v[index]) {
            to_v[index] = from_v[index];
        } else if sole_port_reaches(layered, hop.from, hop.from_slot, to_v[index]) {
            from_v[index] = to_v[index];
        }
    }
    separate_close_runs(layered, &mut from_v, &mut to_v);
    (from_v, to_v)
}

fn separate_close_runs(layered: &Layered, from_v: &mut [f64], to_v: &mut [f64]) {
    let rank_of = |hop: usize| layered.vertices[layered.hops[hop].from].rank;
    for i in 0..layered.hops.len() {
        for j in (0..layered.hops.len()).filter(|&j| j != i && rank_of(j) == rank_of(i)) {
            let bending = from_v[i] != to_v[i] && from_v[j] != to_v[j];
            if !bending || !runs_beside(to_v[i], from_v[j]) {
                continue;
            }
            let away = if to_v[i] >= from_v[j] { 1.0 } else { -1.0 };
            let (target, source) = (&layered.hops[i], &layered.hops[j]);
            let moved_end = from_v[j] + away * CLEARANCE;
            let moved_start = to_v[i] - away * CLEARANCE;
            if sole_port_reaches(layered, target.to, target.to_slot, moved_end) {
                to_v[i] = moved_end;
            } else if sole_port_reaches(layered, source.from, source.from_slot, moved_start) {
                from_v[j] = moved_start;
            } else if runs_beside(to_v[j], from_v[i]) {
                let entries: Vec<f64> = (0..layered.hops.len()).filter(|&k| k != i && layered.hops[k].to == target.to).map(|k| to_v[k]).collect();
                let exits: Vec<f64> = (0..layered.hops.len()).filter(|&k| k != j && layered.hops[k].from == source.from).map(|k| from_v[k]).collect();
                if port_fits(layered, target.to, moved_end, &entries) {
                    to_v[i] = moved_end;
                } else if port_fits(layered, source.from, moved_start, &exits) {
                    from_v[j] = moved_start;
                }
            }
        }
    }
}

/// True when a port of `vertex` can move to `v`: inside the node side and clear of its other ports there.
fn port_fits(layered: &Layered, vertex: usize, v: f64, others: &[f64]) -> bool {
    let own = &layered.vertices[vertex];
    let inside = (v - own.v).abs() <= own.sv / 2.0 - PORT_MARGIN;
    !own.dummy && inside && others.iter().all(|&other| (other - v).abs() >= CLEARANCE)
}

fn sole_port_reaches(layered: &Layered, vertex: usize, slot: (usize, usize), v: f64) -> bool {
    let own = &layered.vertices[vertex];
    let half = own.sv / 2.0 - PORT_MARGIN;
    !own.dummy && slot.1 == 1 && (v - own.v).abs() <= half
}

fn label_plan(layered: &Layered, chain: usize, need: f64, from_v: &[f64], to_v: &[f64]) -> Option<LabelPlan> {
    if need <= 0.0 {
        return None;
    }
    let hops = &layered.chains[chain].hops;
    let first = hops[0];
    let last = *hops.last().expect("a chain has at least one hop");
    let straight = |hop: usize| from_v[hop] == to_v[hop];
    let (hop, run) = if straight(last) {
        (last, LabelRun::Whole)
    } else if straight(first) {
        (first, LabelRun::Whole)
    } else if layered.hops[last].to_slot.1 == 1 {
        (last, LabelRun::Target)
    } else if layered.hops[first].from_slot.1 == 1 {
        (first, LabelRun::Source)
    } else {
        (last, LabelRun::Target)
    };
    Some(LabelPlan { hop, run, need })
}

fn channel(
    layered: &Layered,
    rank: usize,
    from_v: &[f64],
    to_v: &[f64],
    plans: &[Option<LabelPlan>],
    min_channel: f64,
    track_gap: f64,
) -> (Channel, f64) {
    let members: Vec<usize> = (0..layered.hops.len())
        .filter(|&hop| layered.vertices[layered.hops[hop].from].rank == rank)
        .collect();
    let tracks = assign_tracks(&members, from_v, to_v);
    let used = tracks.iter().flatten().max().map_or(0, |&last| last + 1);
    let band = used.saturating_sub(1) as f64 * track_gap;
    let mut source_zone = MIN_ZONE;
    let mut target_zone = MIN_ZONE;
    for plan in plans.iter().flatten().filter(|plan| members.contains(&plan.hop)) {
        match plan.run {
            LabelRun::Source => source_zone = source_zone.max(plan.need),
            LabelRun::Target | LabelRun::Whole => target_zone = target_zone.max(plan.need),
        }
    }
    let width = snap_up(min_channel.max(source_zone + band + target_zone));
    source_zone += (width - (source_zone + band + target_zone)) / 2.0;
    let tracks = members.iter().zip(tracks).filter_map(|(&hop, track)| track.map(|track| (hop, track))).collect();
    (Channel { tracks, source_zone, track_gap }, width)
}

fn interval(hop: usize, from_v: &[f64], to_v: &[f64]) -> (f64, f64) {
    (from_v[hop].min(to_v[hop]), from_v[hop].max(to_v[hop]))
}

fn strictly_inside(value: f64, (lo, hi): (f64, f64)) -> bool {
    value > lo + 0.5 && value < hi - 0.5
}

struct TrackOrder {
    must: Vec<Vec<usize>>,
    should: Vec<Vec<usize>>,
}

impl TrackOrder {
    fn before(&self, hop: usize) -> impl Iterator<Item = &usize> {
        self.must[hop].iter().chain(&self.should[hop])
    }
}

fn track_order(members: &[usize], bending: &[usize], from_v: &[f64], to_v: &[f64]) -> TrackOrder {
    let mut must: Vec<Vec<usize>> = vec![Vec::new(); members.len()];
    let mut should: Vec<Vec<usize>> = vec![Vec::new(); members.len()];
    for &i in bending {
        for &j in bending.iter().filter(|&&j| j != i) {
            let other = interval(members[j], from_v, to_v);
            let starts_inside = strictly_inside(from_v[members[i]], other);
            let ends_inside = strictly_inside(to_v[members[i]], other);
            if starts_inside && !ends_inside {
                should[j].push(i);
            } else if ends_inside && !starts_inside {
                should[i].push(j);
            }
            if runs_beside(to_v[members[i]], from_v[members[j]]) {
                must[i].push(j);
            }
        }
    }
    for (hop, earlier) in should.iter_mut().enumerate() {
        earlier.retain(|&p| !must[p].contains(&hop));
    }
    TrackOrder { must, should }
}

fn assign_tracks(members: &[usize], from_v: &[f64], to_v: &[f64]) -> Vec<Option<usize>> {
    let bending: Vec<usize> = (0..members.len()).filter(|&i| from_v[members[i]] != to_v[members[i]]).collect();
    let constraints = track_order(members, &bending, from_v, to_v);
    let order = topological(&bending, &constraints);
    let mut tracks: Vec<Option<usize>> = vec![None; members.len()];
    for &i in &order {
        let lowest = constraints.before(i).filter_map(|&p| tracks[p]).map(|track| track + 1).max().unwrap_or(0);
        let own = interval(members[i], from_v, to_v);
        let mut track = lowest;
        while order.iter().any(|&j| tracks[j] == Some(track) && too_close(own, interval(members[j], from_v, to_v))) {
            track += 1;
        }
        tracks[i] = Some(track);
    }
    tracks
}
const CLEARANCE: f64 = 12.0;

fn too_close(a: (f64, f64), b: (f64, f64)) -> bool {
    a.0 < b.1 + CLEARANCE && b.0 < a.1 + CLEARANCE
}

fn runs_beside(a: f64, b: f64) -> bool {
    (a - b).abs() < CLEARANCE
}

fn topological(nodes: &[usize], constraints: &TrackOrder) -> Vec<usize> {
    let mut pending: Vec<usize> = nodes.to_vec();
    let mut done: Vec<usize> = Vec::with_capacity(nodes.len());
    while !pending.is_empty() {
        let waiting = |list: &Vec<usize>| list.iter().filter(|p| pending.contains(p)).count();
        let next_position = (0..pending.len())
            .min_by_key(|&k| {
                let hop = pending[k];
                (waiting(&constraints.must[hop]), waiting(&constraints.should[hop]), hop)
            })
            .expect("pending is not empty");
        done.push(pending.remove(next_position));
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{layer, Bands, NodeBox, Pin};

    fn routed(edges: &[(usize, usize)], count: usize) -> Routed {
        let boxes = vec![NodeBox { su: 120.0, sv: 60.0, room_low: 0.0, lane: 0, pin: Pin::Free }; count];
        let layered = layer(&boxes, edges, 40.0, 8, Bands::default());
        route(&layered, count, &vec![0.0; edges.len()], Direction::Right, 64.0, 12.0, true)
    }

    #[test]
    fn every_path_is_axis_aligned() {
        let result = routed(&[(0, 1), (0, 2), (1, 3), (2, 3), (0, 3)], 4);
        for path in &result.paths {
            for pair in path.windows(2) {
                assert!(pair[0].x == pair[1].x || pair[0].y == pair[1].y, "diagonal in {path:?}");
            }
        }
    }

    #[test]
    fn a_reversed_edge_ends_at_its_target() {
        let result = routed(&[(0, 1), (1, 0)], 2);
        let back = result.paths[1].last().expect("path has points");
        assert_eq!(back.x, result.rects[0].right());
    }
}
