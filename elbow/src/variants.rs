//! Parameter variants: render a few layouts of one view, score them, and keep the best valid one.

use serde::Serialize;

use crate::params::{Direction, Params};
use crate::spec::{Kind, View};
use crate::{render, Rendered, Report};

const NODE_GAPS: [f64; 3] = [32.0, 40.0, 56.0];
const CHANNELS: [f64; 2] = [64.0, 112.0];
const VIOLATION_COST: f64 = 1000.0;
const CROSSING_COST: f64 = 40.0;
const AMBIGUITY_COST: f64 = 25.0;
const BEND_COST: f64 = 2.0;
const AREA_UNIT: f64 = 20_000.0;
const WIDEST_RATIO: f64 = 2.5;
const RATIO_COST: f64 = 30.0;
const TURNED_COST: f64 = 12.0;

/// One tried parameter set with its checker report and score (lower is better).
#[derive(Clone, Debug, Serialize)]
pub struct Variant {
    pub params: Params,
    pub score: f64,
    pub report: Report,
}

/// The parameter sets worth trying for `view`, all based on `base`.
pub fn candidates(view: &View, base: &Params) -> Vec<Params> {
    let directions: Vec<Option<Direction>> = match view.kind {
        Kind::Sequence | Kind::Swimlane | Kind::Layers => vec![base.direction],
        _ => vec![Some(Direction::Right), Some(Direction::Down)],
    };
    let mut all = Vec::new();
    for direction in directions {
        for node_gap in NODE_GAPS {
            for min_channel in CHANNELS {
                for uniform_rank_size in [true, false] {
                    all.push(Params { direction, node_gap, min_channel, uniform_rank_size, ..*base });
                }
            }
        }
    }
    all
}

/// Every candidate of `view`, rendered and scored, best first.
pub fn variants(view: &View, base: &Params) -> Vec<Variant> {
    let mut tried: Vec<Variant> = candidates(view, base)
        .into_iter()
        .map(|params| {
            let report = render(view, &params).report;
            let turned = params.direction_for(view.kind) != base.direction_for(view.kind);
            let score = score(&report) + if turned { TURNED_COST } else { 0.0 };
            Variant { params, score, report }
        })
        .collect();
    tried.sort_by(|a, b| a.score.total_cmp(&b.score));
    tried
}

/// The best-scoring variant of `view`, rendered.
pub fn render_best(view: &View, base: &Params) -> Rendered {
    let best = variants(view, base).into_iter().next().map_or(*base, |variant| variant.params);
    render(view, &best)
}

pub fn score(report: &Report) -> f64 {
    let measures = &report.measures;
    let ratio = (report.width / report.height).max(report.height / report.width);
    report.violations.len() as f64 * VIOLATION_COST
        + measures.crossings as f64 * CROSSING_COST
        + measures.ambiguous_labels as f64 * AMBIGUITY_COST
        + measures.bends as f64 * BEND_COST
        + measures.area / AREA_UNIT
        + (ratio - WIDEST_RATIO).max(0.0) * RATIO_COST
}
