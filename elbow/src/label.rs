//! Edge label placement: every label tries spots next to its own connector, in order of preference,
//! and takes the first one that covers no node, no other label and no other connector.

use crate::check::is_ambiguous;
use crate::geom::{segments, Point, Rect, Segment};

pub const LABEL_SIZE: f64 = 8.0;
pub const LABEL_TRACKING: f64 = 0.06;
pub const MASK_HEIGHT: f64 = 12.0;
pub const MASK_PADDING: f64 = 4.0;
pub const LABEL_GAP: f64 = 7.0;
const CORNER_ROOM: f64 = 10.0;
const NODE_CLEARANCE: f64 = 2.0;
const LINE_CLEARANCE: f64 = 2.0;

/// A label to place: the edge it belongs to, its mask width, the run it should prefer, and whether
/// it may overhang that run (a self loop's short top run).
pub struct LabelRequest {
    pub edge: usize,
    pub mask_width: f64,
    pub preferred: Option<Segment>,
    pub overhang: bool,
}

/// Mask rectangles for each request, in request order. Every request gets a spot, even a crowded one.
pub fn place(requests: &[LabelRequest], paths: &[Vec<Point>], nodes: &[Rect]) -> Vec<Rect> {
    let mut placed: Vec<Rect> = Vec::with_capacity(requests.len());
    for request in requests {
        let candidates = candidates(request, &paths[request.edge]);
        let best = candidates
            .iter()
            .enumerate()
            .min_by_key(|(order, candidate)| (conflicts(candidate, request.edge, paths, nodes, &placed), *order))
            .map(|(_, candidate)| *candidate)
            .unwrap_or_else(|| fallback(request, &paths[request.edge]));
        placed.push(best);
    }
    placed
}

fn fallback(request: &LabelRequest, path: &[Point]) -> Rect {
    let mid = path.first().copied().unwrap_or(Point::new(0.0, 0.0));
    Rect::new(mid.x, mid.y - LABEL_GAP - MASK_HEIGHT, request.mask_width, MASK_HEIGHT)
}

fn candidates(request: &LabelRequest, path: &[Point]) -> Vec<Rect> {
    let mut runs: Vec<Segment> = segments(path);
    runs.sort_by(|a, b| b.length().total_cmp(&a.length()));
    let mut spots = Vec::new();
    if let Some(preferred) = request.preferred {
        if request.overhang {
            spots.extend(overhanging(&preferred, request.mask_width));
        }
        spots.extend(beside(&preferred, request.mask_width));
    }
    spots.extend(runs.iter().flat_map(|run| beside(run, request.mask_width)));
    spots
}

fn overhanging(run: &Segment, width: f64) -> Vec<Rect> {
    let mid = run.mid();
    vec![Rect::new(mid.x - width / 2.0, mid.y - LABEL_GAP - MASK_HEIGHT, width, MASK_HEIGHT)]
}

fn along(lo: f64, hi: f64, size: f64) -> Vec<f64> {
    let free = hi - lo - size - 2.0 * CORNER_ROOM;
    if free < 0.0 {
        return Vec::new();
    }
    let middle = lo + CORNER_ROOM + free / 2.0;
    vec![middle, lo + CORNER_ROOM, lo + CORNER_ROOM + free]
}

fn beside(run: &Segment, width: f64) -> Vec<Rect> {
    let (lo, hi) = run.span();
    if run.is_horizontal() {
        let y = run.a.y;
        along(lo, hi, width)
            .into_iter()
            .flat_map(|x| {
                [Rect::new(x, y - LABEL_GAP - MASK_HEIGHT, width, MASK_HEIGHT), Rect::new(x, y + LABEL_GAP, width, MASK_HEIGHT)]
            })
            .collect()
    } else {
        let x = run.a.x;
        along(lo, hi, MASK_HEIGHT)
            .into_iter()
            .flat_map(|y| [Rect::new(x + LABEL_GAP, y, width, MASK_HEIGHT), Rect::new(x - LABEL_GAP - width, y, width, MASK_HEIGHT)])
            .collect()
    }
}

fn conflicts(mask: &Rect, edge: usize, paths: &[Vec<Point>], nodes: &[Rect], placed: &[Rect]) -> usize {
    let runs: Vec<Vec<Segment>> = paths.iter().map(|path| segments(path)).collect();
    let others: Vec<&[Segment]> = runs.iter().enumerate().filter(|(other, _)| *other != edge).map(|(_, run)| run.as_slice()).collect();
    let unclear = usize::from(is_ambiguous(mask, &runs[edge], &others));
    let on_nodes = nodes.iter().filter(|node| mask.inflate(NODE_CLEARANCE).overlaps(node)).count();
    let on_labels = placed.iter().filter(|other| mask.inflate(NODE_CLEARANCE).overlaps(other)).count();
    let on_lines: usize = paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let clearance = if index == edge { LABEL_GAP - 1.0 } else { LINE_CLEARANCE };
            segments(path).iter().filter(|segment| mask.distance_to(segment) < clearance).count()
        })
        .sum();
    on_nodes * 4 + on_labels * 4 + on_lines * 2 + unclear
}
