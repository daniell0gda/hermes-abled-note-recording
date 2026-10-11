//! Dependency cycles: a back edge leaves its node on the outer side of the ranked stack, runs past every
//! other element and comes back in on the outer side of its target, so it never cuts through the stack.
//! Both nodes are pinned to that end of their rank, so nothing sits between them and the outside.

use std::collections::HashMap;

use crate::geom::{Point, Rect, Segment};
use crate::layout::Pin;
use crate::params::Direction;

const OUTSIDE_GAP: f64 = 32.0;
const TRACK_GAP: f64 = 16.0;
const ATTACH_PITCH: f64 = 14.0;

/// A back edge drawn around the outside, on the low or high side of the ranks.
#[derive(Clone, Copy, Debug)]
pub struct Cycle {
    pub source: usize,
    pub target: usize,
    pub side: Pin,
}

/// Gives every back edge a side and pins its nodes to that side. The first cycle takes the high side,
/// the next the low side, and so on, unless one of its nodes is already pinned.
pub fn assign_sides(back: &[(usize, usize)], pins: &mut [Pin]) -> Vec<Cycle> {
    let mut next = Pin::High;
    back.iter()
        .map(|&(source, target)| {
            let side = [pins[source], pins[target]].into_iter().find(|&pin| pin != Pin::Free).unwrap_or_else(|| {
                let side = next;
                next = if next == Pin::High { Pin::Low } else { Pin::High };
                side
            });
            for node in [source, target] {
                if pins[node] == Pin::Free {
                    pins[node] = side;
                }
            }
            Cycle { source, target, side }
        })
        .collect()
}

/// The path of every cycle, and the outside run that should carry its label.
pub fn route(cycles: &[Cycle], rects: &[Rect], paths: &[Vec<Point>], direction: Direction) -> Vec<(Vec<Point>, Segment)> {
    let frame = Frame { direction };
    let points: Vec<Point> = paths.iter().flatten().copied().collect();
    let tracks = tracks(cycles, rects, &frame);
    let mut attach = Attachments::new(cycles);
    cycles
        .iter()
        .enumerate()
        .map(|(index, cycle)| {
            let sign = if cycle.side == Pin::High { 1.0 } else { -1.0 };
            let bound = frame.outer_bound(cycle.side, rects, &points);
            let out_v = bound + sign * (OUTSIDE_GAP + tracks[index] as f64 * TRACK_GAP);
            let start_u = frame.u_centre(&rects[cycle.source]) + attach.offset(cycle.source, cycle.side);
            let end_u = frame.u_centre(&rects[cycle.target]) + attach.offset(cycle.target, cycle.side);
            let start_v = frame.side_v(&rects[cycle.source], cycle.side);
            let end_v = frame.side_v(&rects[cycle.target], cycle.side);
            let outside = Segment::new(frame.point(start_u, out_v), frame.point(end_u, out_v));
            let path = vec![frame.point(start_u, start_v), outside.a, outside.b, frame.point(end_u, end_v)];
            (path, outside)
        })
        .collect()
}

fn tracks(cycles: &[Cycle], rects: &[Rect], frame: &Frame) -> Vec<usize> {
    let span = |cycle: &Cycle| (frame.u_centre(&rects[cycle.source]) - frame.u_centre(&rects[cycle.target])).abs();
    let mut tracks = vec![0; cycles.len()];
    for side in [Pin::Low, Pin::High] {
        let mut on_side: Vec<usize> = (0..cycles.len()).filter(|&index| cycles[index].side == side).collect();
        on_side.sort_by(|&a, &b| span(&cycles[a]).total_cmp(&span(&cycles[b])));
        for (track, index) in on_side.into_iter().enumerate() {
            tracks[index] = track;
        }
    }
    tracks
}

/// Spreads the cycle ends that share one side of a node: (ends so far, ends in total) per node side.
struct Attachments(HashMap<(usize, Pin), (usize, usize)>);

impl Attachments {
    fn new(cycles: &[Cycle]) -> Self {
        let mut ends = HashMap::new();
        for cycle in cycles {
            for node in [cycle.source, cycle.target] {
                ends.entry((node, cycle.side)).or_insert((0, 0)).1 += 1;
            }
        }
        Self(ends)
    }

    fn offset(&mut self, node: usize, side: Pin) -> f64 {
        let (used, total) = self.0.entry((node, side)).or_insert((0, 1));
        let offset = (*used as f64 - (*total as f64 - 1.0) / 2.0) * ATTACH_PITCH;
        *used += 1;
        offset
    }
}

/// Real coordinates seen as `u` along the ranks and `v` across them.
struct Frame {
    direction: Direction,
}

impl Frame {
    fn point(&self, u: f64, v: f64) -> Point {
        match self.direction {
            Direction::Right => Point::new(u, v),
            Direction::Down => Point::new(v, u),
        }
    }

    fn u_centre(&self, rect: &Rect) -> f64 {
        match self.direction {
            Direction::Right => rect.cx(),
            Direction::Down => rect.y + rect.h / 2.0,
        }
    }

    fn side_v(&self, rect: &Rect, side: Pin) -> f64 {
        match (self.direction, side) {
            (Direction::Right, Pin::High) => rect.bottom(),
            (Direction::Right, _) => rect.y,
            (Direction::Down, Pin::High) => rect.right(),
            (Direction::Down, _) => rect.x,
        }
    }

    fn v_of(&self, point: Point) -> f64 {
        match self.direction {
            Direction::Right => point.y,
            Direction::Down => point.x,
        }
    }

    fn outer_bound(&self, side: Pin, rects: &[Rect], points: &[Point]) -> f64 {
        let edges = rects.iter().map(|rect| self.side_v(rect, side)).chain(points.iter().map(|&point| self.v_of(point)));
        match side {
            Pin::High => edges.fold(f64::NEG_INFINITY, f64::max),
            _ => edges.fold(f64::INFINITY, f64::min),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_alternate_sides_unless_a_node_is_already_pinned() {
        let mut pins = vec![Pin::Free; 5];
        let cycles = assign_sides(&[(1, 0), (3, 2), (4, 1)], &mut pins);
        let sides: Vec<Pin> = cycles.iter().map(|cycle| cycle.side).collect();
        assert_eq!(sides, vec![Pin::High, Pin::Low, Pin::High]);
        assert_eq!(pins, vec![Pin::High, Pin::High, Pin::Low, Pin::Low, Pin::High]);
    }

    #[test]
    fn a_cycle_runs_outside_every_node() {
        let rects = [Rect::new(0.0, 0.0, 100.0, 40.0), Rect::new(160.0, 100.0, 100.0, 40.0)];
        let cycle = Cycle { source: 0, target: 1, side: Pin::High };
        let (path, outside) = route(&[cycle], &rects, &[], Direction::Down).remove(0);
        assert_eq!(path.first(), Some(&Point::new(100.0, 20.0)));
        assert_eq!(path.last(), Some(&Point::new(260.0, 120.0)));
        assert_eq!(outside.a.x, 260.0 + OUTSIDE_GAP);
    }
}
