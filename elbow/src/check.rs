//! The six diagram-design connector rules as code. Each broken rule becomes one `Violation`.

use serde::Serialize;

use crate::geom::{segments, Point, Rect, Segment};
use crate::scene::Scene;

const MIN_PARALLEL_GAP: f64 = 11.5;
const MIN_ATTACH_GAP: f64 = 11.5;
const MIN_LABEL_GAP: f64 = 5.9;
const ON_BORDER: f64 = 0.5;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Violation {
    pub rule: u8,
    pub detail: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Measures {
    pub crossings: usize,
    pub bends: usize,
    pub ambiguous_labels: usize,
    pub area: f64,
}

const AMBIGUITY_MARGIN: f64 = 6.0;

/// True when some other edge's line is about as close to `mask` as the label's own line,
/// so a reader cannot tell which connector the label belongs to.
pub fn is_ambiguous(mask: &Rect, own: &[Segment], others: &[&[Segment]]) -> bool {
    let nearest = |segments: &[Segment]| segments.iter().map(|segment| mask.distance_to(segment)).fold(f64::INFINITY, f64::min);
    let own_distance = nearest(own);
    others.iter().any(|segments| nearest(segments) < own_distance + AMBIGUITY_MARGIN)
}

fn ambiguous_labels(scene: &Scene, runs: &[Vec<Segment>]) -> usize {
    scene
        .edges
        .iter()
        .enumerate()
        .filter(|(edge, data)| {
            data.label.as_ref().is_some_and(|label| {
                let others: Vec<&[Segment]> =
                    runs.iter().enumerate().filter(|(other, _)| other != edge).map(|(_, segments)| segments.as_slice()).collect();
                is_ambiguous(&label.rect, &runs[*edge], &others)
            })
        })
        .count()
}

/// Checks the scene against the connector rules. Free and arc edges are exempt: they are data lines
/// placed by their kind's geometry, not connectors; only their labels are still checked against nodes
/// and other labels.
pub fn check(scene: &Scene) -> (Vec<Violation>, Measures) {
    let mut violations = Vec::new();
    let runs: Vec<Vec<Segment>> =
        scene.edges.iter().map(|edge| if edge.is_elbow() { segments(&edge.points) } else { Vec::new() }).collect();
    diagonals(scene, &runs, &mut violations);
    label_gaps(scene, &runs, &mut violations);
    overlapping_runs(scene, &runs, &mut violations);
    shared_attach_points(scene, &mut violations);
    runs_through_nodes(scene, &runs, &mut violations);
    labels_on_things(scene, &runs, &mut violations);
    let measures = Measures {
        crossings: crossings(&runs),
        bends: runs.iter().map(|edge| edge.len().saturating_sub(1)).sum(),
        ambiguous_labels: ambiguous_labels(scene, &runs),
        area: scene.width * scene.height,
    };
    (violations, measures)
}

fn name(scene: &Scene, edge: usize) -> String {
    let edge = &scene.edges[edge];
    format!("{}->{}", scene.nodes[edge.source].id, scene.nodes[edge.target].id)
}

fn diagonals(scene: &Scene, runs: &[Vec<Segment>], violations: &mut Vec<Violation>) {
    for (edge, segments) in runs.iter().enumerate() {
        if segments.iter().any(|segment| !segment.is_horizontal() && !segment.is_vertical()) {
            violations.push(Violation { rule: 1, detail: format!("{} has a diagonal run", name(scene, edge)) });
        }
    }
}

fn label_gaps(scene: &Scene, runs: &[Vec<Segment>], violations: &mut Vec<Violation>) {
    for (edge, segments) in runs.iter().enumerate() {
        let Some(label) = &scene.edges[edge].label else { continue };
        if segments.iter().any(|segment| label.rect.distance_to(segment) < MIN_LABEL_GAP) {
            violations.push(Violation { rule: 2, detail: format!("label of {} is closer than 6px to its line", name(scene, edge)) });
        }
    }
}

fn overlapping_runs(scene: &Scene, runs: &[Vec<Segment>], violations: &mut Vec<Violation>) {
    for a in 0..runs.len() {
        for b in a + 1..runs.len() {
            let clash = runs[a].iter().any(|first| runs[b].iter().any(|second| too_close_parallel(first, second)));
            if clash {
                violations.push(Violation {
                    rule: 3,
                    detail: format!("{} and {} run on top of each other", name(scene, a), name(scene, b)),
                });
            }
        }
    }
}

/// True when two parallel runs share a stretch closer than the minimum parallel gap (rule 3).
pub fn too_close_parallel(first: &Segment, second: &Segment) -> bool {
    let (across_first, across_second) = match (first.is_horizontal(), second.is_horizontal()) {
        (true, true) => (first.a.y, second.a.y),
        (false, false) => (first.a.x, second.a.x),
        _ => return false,
    };
    let (lo_first, hi_first) = first.span();
    let (lo_second, hi_second) = second.span();
    let shared = hi_first.min(hi_second) - lo_first.max(lo_second);
    (across_first - across_second).abs() < MIN_PARALLEL_GAP && shared > ON_BORDER
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

fn side_of(rect: &Rect, point: Point) -> Option<(Side, f64)> {
    if (point.x - rect.x).abs() < ON_BORDER {
        Some((Side::Left, point.y))
    } else if (point.x - rect.right()).abs() < ON_BORDER {
        Some((Side::Right, point.y))
    } else if (point.y - rect.y).abs() < ON_BORDER {
        Some((Side::Top, point.x))
    } else if (point.y - rect.bottom()).abs() < ON_BORDER {
        Some((Side::Bottom, point.x))
    } else {
        None
    }
}

fn shared_attach_points(scene: &Scene, violations: &mut Vec<Violation>) {
    for (index, node) in scene.nodes.iter().enumerate() {
        let mut attached: Vec<(Side, f64)> = Vec::new();
        for edge in scene.edges.iter().filter(|edge| edge.is_elbow()) {
            let ends = [(edge.source, edge.points.first()), (edge.target, edge.points.last())];
            for (owner, point) in ends {
                if let (true, Some(&point)) = (owner == index, point) {
                    attached.extend(side_of(&node.rect, point));
                }
            }
        }
        let crowded = attached.iter().enumerate().any(|(i, a)| {
            attached[i + 1..].iter().any(|b| a.0 == b.0 && (a.1 - b.1).abs() < MIN_ATTACH_GAP)
        });
        if crowded {
            violations.push(Violation { rule: 4, detail: format!("{} has attach points closer than 12px", node.id) });
        }
    }
}

fn runs_through_nodes(scene: &Scene, runs: &[Vec<Segment>], violations: &mut Vec<Violation>) {
    for (edge, segments) in runs.iter().enumerate() {
        for node in &scene.nodes {
            if segments.iter().any(|segment| node.rect.crossed_by(segment)) {
                violations.push(Violation { rule: 5, detail: format!("{} passes behind {}", name(scene, edge), node.id) });
            }
        }
    }
}

fn labels_on_things(scene: &Scene, runs: &[Vec<Segment>], violations: &mut Vec<Violation>) {
    let labels: Vec<(usize, Rect)> =
        scene.edges.iter().enumerate().flat_map(|(edge, data)| data.texts().map(move |text| (edge, text.rect))).collect();
    for (position, &(edge, rect)) in labels.iter().enumerate() {
        for node in scene.nodes.iter().filter(|node| rect.overlaps(&node.rect)) {
            violations.push(Violation { rule: 6, detail: format!("label of {} covers {}", name(scene, edge), node.id) });
        }
        for &(other, other_rect) in &labels[position + 1..] {
            if rect.overlaps(&other_rect) {
                violations.push(Violation {
                    rule: 6,
                    detail: format!("labels of {} and {} overlap", name(scene, edge), name(scene, other)),
                });
            }
        }
        for (other, segments) in runs.iter().enumerate().filter(|(other, _)| *other != edge) {
            if segments.iter().any(|segment| rect.distance_to(segment) < 1.0) {
                violations.push(Violation {
                    rule: 6,
                    detail: format!("label of {} hides {}", name(scene, edge), name(scene, other)),
                });
            }
        }
    }
}

fn crossings(runs: &[Vec<Segment>]) -> usize {
    let mut count = 0;
    for a in 0..runs.len() {
        for b in a + 1..runs.len() {
            count += runs[a].iter().map(|first| runs[b].iter().filter(|second| first.crossing(second).is_some()).count()).sum::<usize>();
        }
    }
    count
}
