//! Journey: one persona's stages as columns in view order, each with a `STAGE n` eyebrow and its name;
//! under them a sentiment band with five ordinal levels and a curve through one dot per stage, the
//! trough (lowest stage, the first on a tie) and its incoming segment in accent. Note nodes linked to a
//! stage are pain markers: dashed accent tags under that stage. A touchpoint row follows when any stage
//! names one. View edges between neighbouring stages are the curve's segments.

use crate::figure::{Anchor, Figure, LegendKey, Marker, Paint, Swatch, Text, Tone};
use crate::geom::{snap_up, Point, Rect};
use crate::node::{field_list, name_and_sublabel, NodeContent};
use crate::params::Params;
use crate::scene::{known_edges, unique_nodes, Bend, EdgeInput, Scene, SceneEdge, SceneNode};
use crate::spec::{Shape, View};
use crate::text::{mono_width, sans_width, split_in_two};

const COLUMN: f64 = 200.0;
const COLUMN_GAP: f64 = 24.0;
const CELL_PADDING: f64 = 12.0;
const MARGIN_LABEL_SIZE: f64 = 8.0;
const MARGIN_LABEL_TRACKING: f64 = 0.14;
const MARGIN_LABEL_GAP: f64 = 8.0;
const EYEBROW_BASELINE: f64 = 12.0;
const EYEBROW_SIZE: f64 = 8.0;
const EYEBROW_TRACKING: f64 = 0.14;
const NAME_BASELINE: f64 = 32.0;
const NAME_SIZE: f64 = 12.0;
const LINE: f64 = 16.0;
const HEADER_ROOM: f64 = 8.0;
const BAND_GAP: f64 = 32.0;
const LEVEL_STEP: f64 = 40.0;
const DOT: f64 = 5.0;
const CURVE_WIDTH: f64 = 1.5;
const ROWS_GAP: f64 = 24.0;
const ROW_PADDING: f64 = 12.0;
const TAG_HEIGHT: f64 = 16.0;
const TAG_GAP: f64 = 8.0;
const TAG_SIZE: f64 = 8.0;
const TAG_TRACKING: f64 = 0.04;
const TAG_PADDING: f64 = 8.0;
const TOUCHPOINT_ROW: f64 = 36.0;
const TOUCHPOINT_BASELINE: f64 = 22.0;
const TOUCHPOINT_SIZE: f64 = 9.0;

/// The five ordinal sentiment levels, highest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    High,
    MedHigh,
    Neutral,
    MedLow,
    Low,
}

const LEVEL_WORDS: [(Level, &[&str]); 5] = [
    (Level::High, &["high", "great", "happy", "delighted", "excited", "love", "positive", "very-good"]),
    (Level::MedHigh, &["med-high", "medium-high", "good", "pleased", "ok-good"]),
    (Level::Neutral, &["neutral", "ok", "fine", "mixed"]),
    (Level::MedLow, &["med-low", "medium-low", "meh", "unsure", "confused", "annoyed", "slow"]),
    (Level::Low, &["low", "bad", "angry", "frustrated", "painful", "stuck", "negative", "hate"]),
];

impl Level {
    /// The level a sentiment word names, ignoring case; neutral for a word it does not know.
    fn of(word: &str) -> Level {
        let key = word.to_lowercase().split(|c: char| c.is_whitespace() || c == '_' || c == '-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-");
        LEVEL_WORDS.iter().find(|(_, words)| words.contains(&key.as_str())).map_or(Level::Neutral, |(level, _)| *level)
    }

    fn rank(self) -> f64 {
        self as usize as f64
    }

    fn name(self) -> &'static str {
        match self {
            Level::High => "HIGH",
            Level::MedHigh => "MED-HIGH",
            Level::Neutral => "NEUTRAL",
            Level::MedLow => "MED-LOW",
            Level::Low => "LOW",
        }
    }

    fn is_main(self) -> bool {
        matches!(self, Level::High | Level::Neutral | Level::Low)
    }
}

struct Stage {
    node: usize,
    lines: Vec<String>,
    level: Level,
    touchpoint: Option<String>,
}

impl Stage {
    fn new(node: usize, label: &str) -> Stage {
        let (name, bracket) = name_and_sublabel(label);
        let items = bracket.as_deref().map(field_list).unwrap_or_default();
        Stage {
            node,
            lines: name_lines(&name),
            level: items.first().map_or(Level::Neutral, |word| Level::of(word)),
            touchpoint: items.get(1).cloned(),
        }
    }

    fn width_needed(&self) -> f64 {
        let names = self.lines.iter().map(|line| sans_width(line, NAME_SIZE));
        let touchpoint = self.touchpoint.iter().map(|touchpoint| mono_width(touchpoint, TOUCHPOINT_SIZE, 0.0));
        names.chain(touchpoint).fold(0.0, f64::max)
    }
}

fn name_lines(name: &str) -> Vec<String> {
    if sans_width(name, NAME_SIZE) <= COLUMN - 2.0 * CELL_PADDING {
        return vec![name.to_string()];
    }
    split_in_two(name).map_or_else(|| vec![name.to_string()], |(first, second)| vec![first, second])
}

/// A pain marker: a note node, the stage it hangs under and its place in that stage's stack.
struct Pain {
    node: usize,
    text: String,
    stage: usize,
    slot: usize,
}

impl Pain {
    fn width(&self) -> f64 {
        mono_width(&self.text, TAG_SIZE, TAG_TRACKING) + 2.0 * TAG_PADDING
    }
}

/// Where a view edge goes: a segment of the curve between neighbouring stages, the link of a pain
/// marker to a stage, or nowhere (any other pair is not drawn).
enum Role {
    Segment(usize),
    PainLink { note: usize, stage: usize },
    Skipped,
}

/// The columns and rows every figure is placed on.
struct Grid {
    plot_left: f64,
    column: f64,
    header: f64,
    band_top: f64,
    count: usize,
}

impl Grid {
    fn left(&self, stage: usize) -> f64 {
        self.plot_left + stage as f64 * (self.column + COLUMN_GAP)
    }

    fn centre(&self, stage: usize) -> f64 {
        self.left(stage) + self.column / 2.0
    }

    fn plot_right(&self) -> f64 {
        self.left(self.count.max(1) - 1) + self.column
    }

    fn level_y(&self, level: Level) -> f64 {
        self.band_top + level.rank() * LEVEL_STEP
    }

    fn rows_top(&self) -> f64 {
        self.level_y(Level::Low) + ROWS_GAP
    }
}

/// The parts of a journey before placement: stages in view order, pain markers, edge roles, trough.
struct Plan {
    stages: Vec<Stage>,
    pains: Vec<Pain>,
    roles: Vec<Role>,
    trough: Option<usize>,
}

impl Plan {
    fn new(view: &View, ids: &[String], edges: &[EdgeInput]) -> Plan {
        let specs: Vec<_> = ids.iter().map(|id| view.nodes.iter().find(|node| &node.id == id).expect("a node of the view")).collect();
        let is_note: Vec<bool> = specs.iter().map(|spec| spec.shape == Shape::Note).collect();
        let stages: Vec<Stage> = specs.iter().enumerate().filter(|(node, _)| !is_note[*node]).map(|(node, spec)| Stage::new(node, &spec.label)).collect();
        let stage_of = |node: usize| stages.iter().position(|stage| stage.node == node);
        let roles: Vec<Role> = edges.iter().map(|edge| role(edge, stage_of(edge.source), stage_of(edge.target), &is_note)).collect();
        let trough = trough(&stages);
        let mut slots = vec![0usize; stages.len().max(1)];
        let pains = specs
            .iter()
            .enumerate()
            .filter(|(node, _)| is_note[*node])
            .map(|(node, spec)| {
                let linked = roles.iter().find_map(|role| match role {
                    Role::PainLink { note, stage } if *note == node => Some(*stage),
                    _ => None,
                });
                let stage = linked.or(trough).unwrap_or(0);
                let slot = slots[stage];
                slots[stage] += 1;
                Pain { node, text: spec.label.trim().to_uppercase(), stage, slot }
            })
            .collect();
        Plan { stages, pains, roles, trough }
    }

    fn deepest_stack(&self) -> usize {
        self.pains.iter().map(|pain| pain.slot + 1).max().unwrap_or(0)
    }

    fn has_touchpoints(&self) -> bool {
        self.stages.iter().any(|stage| stage.touchpoint.is_some())
    }

    fn levels_shown(&self) -> Vec<Level> {
        [Level::High, Level::MedHigh, Level::Neutral, Level::MedLow, Level::Low]
            .into_iter()
            .filter(|level| level.is_main() || self.stages.iter().any(|stage| stage.level == *level))
            .collect()
    }

    fn row_labels(&self) -> Vec<&'static str> {
        let pains = (!self.pains.is_empty()).then_some("PAIN POINTS");
        let touchpoints = self.has_touchpoints().then_some("TOUCHPOINTS");
        pains.into_iter().chain(touchpoints).collect()
    }

    fn grid(&self) -> Grid {
        let margin_labels = self.levels_shown().into_iter().map(Level::name).chain(self.row_labels());
        let widest_label = margin_labels.map(|label| mono_width(label, MARGIN_LABEL_SIZE, MARGIN_LABEL_TRACKING)).fold(0.0, f64::max);
        let widest_cell = self.stages.iter().map(Stage::width_needed).chain(self.pains.iter().map(Pain::width)).fold(0.0, f64::max);
        let lines = self.stages.iter().map(|stage| stage.lines.len()).max().unwrap_or(1) as f64;
        let header = snap_up(NAME_BASELINE + (lines - 1.0) * LINE + HEADER_ROOM);
        Grid {
            plot_left: snap_up(widest_label + 2.0 * MARGIN_LABEL_GAP),
            column: COLUMN.max(snap_up(widest_cell + 2.0 * CELL_PADDING)),
            header,
            band_top: header + BAND_GAP,
            count: self.stages.len(),
        }
    }

    fn is_trough_segment(&self, segment: usize) -> bool {
        self.trough == Some(segment + 1)
    }
}

fn role(edge: &EdgeInput, source: Option<usize>, target: Option<usize>, is_note: &[bool]) -> Role {
    match (source, target) {
        (Some(from), Some(to)) if from.abs_diff(to) == 1 => Role::Segment(from.min(to)),
        (Some(stage), None) if is_note[edge.target] => Role::PainLink { note: edge.target, stage },
        (None, Some(stage)) if is_note[edge.source] => Role::PainLink { note: edge.source, stage },
        _ => Role::Skipped,
    }
}

/// The lowest stage; the first of them when several share the lowest level.
fn trough(stages: &[Stage]) -> Option<usize> {
    stages.iter().enumerate().fold(None, |lowest: Option<(usize, Level)>, (index, stage)| match lowest {
        Some((_, level)) if level >= stage.level => lowest,
        _ => Some((index, stage.level)),
    }).map(|(index, _)| index)
}

fn curve_tone(plan: &Plan, segment: usize) -> Tone {
    if plan.is_trough_segment(segment) {
        Tone::Accent
    } else {
        Tone::Muted
    }
}

fn dot_at(plan: &Plan, grid: &Grid, stage: usize) -> Point {
    Point::new(grid.centre(stage), grid.level_y(plan.stages[stage].level))
}

fn tag_rect(pain: &Pain, grid: &Grid) -> Rect {
    let width = pain.width().min(grid.column);
    let top = grid.rows_top() + ROW_PADDING + pain.slot as f64 * (TAG_HEIGHT + TAG_GAP);
    Rect::new(grid.centre(pain.stage) - width / 2.0, top, width, TAG_HEIGHT)
}

fn tag_paint() -> Paint {
    Paint::shape(Tone::AccentTint, Tone::AccentWash(0.5), 0.8).dashed("3,3")
}

fn stage_node(stage: &Stage, number: usize, grid: &Grid, id: String, content: NodeContent) -> SceneNode {
    let centre = grid.centre(number);
    let rect = Rect::new(grid.left(number), 0.0, grid.column, grid.header);
    let eyebrow = Text::mono(Point::new(centre, EYEBROW_BASELINE), format!("STAGE {}", number + 1), EYEBROW_SIZE).tracking(EYEBROW_TRACKING);
    let names = stage.lines.iter().enumerate().map(|(index, line)| {
        Figure::Text(Text::sans(Point::new(centre, NAME_BASELINE + index as f64 * LINE), line.clone(), NAME_SIZE).weight(600))
    });
    let figures = [Figure::hit_area(rect), Figure::Text(eyebrow)].into_iter().chain(names).collect();
    SceneNode { figures, ..SceneNode::new(id, rect, content) }
}

fn pain_node(pain: &Pain, grid: &Grid, id: String, content: NodeContent) -> SceneNode {
    let rect = tag_rect(pain, grid);
    let text = Text::mono(Point::new(rect.cx(), rect.y + 12.0), pain.text.clone(), TAG_SIZE).tone(Tone::Accent).tracking(TAG_TRACKING);
    let figures = vec![Figure::rect(rect, 2.0, tag_paint()), Figure::Text(text)];
    SceneNode { figures, ..SceneNode::new(id, rect, content) }
}

fn nodes(plan: &Plan, grid: &Grid, ids: Vec<String>, contents: Vec<NodeContent>) -> Vec<SceneNode> {
    ids.into_iter()
        .zip(contents)
        .enumerate()
        .map(|(node, (id, content))| match plan.stages.iter().position(|stage| stage.node == node) {
            Some(number) => stage_node(&plan.stages[number], number, grid, id, content),
            None => {
                let pain = plan.pains.iter().find(|pain| pain.node == node).expect("a node is a stage or a pain marker");
                pain_node(pain, grid, id, content)
            }
        })
        .collect()
}

fn free_edge(input: &EdgeInput, points: Vec<Point>, paint: Paint) -> SceneEdge {
    SceneEdge { bend: Bend::Free, markers: Some((Marker::None, Marker::None)), paint: Some(paint), ..SceneEdge::new(input, points) }
}

/// Where a pain link meets its stage: under the previous tag of the stack, or under the stage's dot.
fn link_anchor(plan: &Plan, grid: &Grid, pain: &Pain, stage: usize) -> Point {
    let dot = dot_at(plan, grid, stage);
    if stage != pain.stage || pain.slot == 0 {
        return Point::new(dot.x, dot.y + DOT);
    }
    Point::new(dot.x, tag_rect(pain, grid).y - TAG_GAP)
}

fn edges(plan: &Plan, grid: &Grid, inputs: &[EdgeInput]) -> Vec<SceneEdge> {
    inputs
        .iter()
        .zip(&plan.roles)
        .filter_map(|(input, role)| match *role {
            Role::Segment(segment) => {
                let from = plan.stages.iter().position(|stage| stage.node == input.source)?;
                let to = plan.stages.iter().position(|stage| stage.node == input.target)?;
                let paint = Paint::line(curve_tone(plan, segment), CURVE_WIDTH);
                Some(free_edge(input, vec![dot_at(plan, grid, from), dot_at(plan, grid, to)], paint))
            }
            Role::PainLink { note, stage } => {
                let pain = plan.pains.iter().find(|pain| pain.node == note)?;
                let tag = tag_rect(pain, grid);
                let points = vec![link_anchor(plan, grid, pain, stage), Point::new(tag.cx(), tag.y)];
                Some(free_edge(input, points, Paint::line(Tone::AccentWash(0.5), 0.8)))
            }
            Role::Skipped => None,
        })
        .collect()
}

/// Curve segments between neighbouring stages that no view edge draws, so the curve stays whole.
fn plain_segments(plan: &Plan, grid: &Grid) -> Vec<Figure> {
    (1..plan.stages.len())
        .filter(|&to| !plan.roles.iter().any(|role| matches!(role, Role::Segment(segment) if *segment == to - 1)))
        .map(|to| Figure::line(dot_at(plan, grid, to - 1), dot_at(plan, grid, to), Paint::line(curve_tone(plan, to - 1), CURVE_WIDTH)))
        .collect()
}

fn dots(plan: &Plan, grid: &Grid) -> Vec<Figure> {
    (0..plan.stages.len())
        .map(|stage| {
            let tone = if plan.trough == Some(stage) { Tone::Accent } else { Tone::Muted };
            Figure::circle(dot_at(plan, grid, stage), DOT, Paint::filled(tone))
        })
        .collect()
}

fn margin_label(grid: &Grid, y: f64, text: &str) -> Figure {
    let at = Point::new(grid.plot_left - MARGIN_LABEL_GAP, y + 3.0);
    Figure::Text(Text::mono(at, text, MARGIN_LABEL_SIZE).anchor(Anchor::End).tracking(MARGIN_LABEL_TRACKING))
}

fn hairline(grid: &Grid, y: f64, alpha: f64) -> Figure {
    Figure::line(Point::new(grid.plot_left, y), Point::new(grid.plot_right(), y), Paint::line(Tone::Wash(alpha), 0.8))
}

fn band(plan: &Plan, grid: &Grid) -> Vec<Figure> {
    plan.levels_shown()
        .into_iter()
        .flat_map(|level| {
            let y = grid.level_y(level);
            let alpha = if level.is_main() { 0.10 } else { 0.06 };
            [hairline(grid, y, alpha), margin_label(grid, y, level.name())]
        })
        .collect()
}

fn rows(plan: &Plan, grid: &Grid) -> Vec<Figure> {
    let mut figures = Vec::new();
    let mut top = grid.rows_top();
    let stack = plan.deepest_stack() as f64;
    if stack > 0.0 {
        let height = 2.0 * ROW_PADDING + stack * TAG_HEIGHT + (stack - 1.0) * TAG_GAP;
        figures.extend([hairline(grid, top, 0.10), margin_label(grid, top + height / 2.0, "PAIN POINTS")]);
        top += height;
    }
    if plan.has_touchpoints() {
        figures.extend([hairline(grid, top, 0.10), margin_label(grid, top + TOUCHPOINT_ROW / 2.0, "TOUCHPOINTS")]);
        let texts = plan.stages.iter().enumerate().filter_map(|(number, stage)| {
            let at = Point::new(grid.centre(number), top + TOUCHPOINT_BASELINE);
            stage.touchpoint.as_ref().map(|touchpoint| Figure::Text(Text::mono(at, touchpoint.clone(), TOUCHPOINT_SIZE)))
        });
        figures.extend(texts);
    }
    figures
}

fn legend(plan: &Plan) -> Vec<LegendKey> {
    let mut keys = vec![
        LegendKey::new(Swatch::Line(Paint::line(Tone::Muted, CURVE_WIDTH), Marker::None), "SENTIMENT"),
        LegendKey::new(Swatch::Line(Paint::line(Tone::Accent, CURVE_WIDTH), Marker::None), "TROUGH STAGE"),
    ];
    if !plan.pains.is_empty() {
        keys.push(LegendKey::new(Swatch::Box(tag_paint()), "PAIN MARKER"));
    }
    keys
}

/// Lays out `view` as a user journey map.
pub fn build(view: &View, _params: &Params) -> Scene {
    let (ids, contents) = unique_nodes(view);
    let inputs = known_edges(view, &ids);
    let plan = Plan::new(view, &ids, &inputs);
    let grid = plan.grid();
    let mut decor = band(&plan, &grid);
    decor.extend(rows(&plan, &grid));
    decor.extend(plain_segments(&plan, &grid));
    Scene {
        edges: edges(&plan, &grid, &inputs),
        nodes: nodes(&plan, &grid, ids, contents),
        decor,
        overlay: dots(&plan, &grid),
        legend: Some(legend(&plan)),
        ..Scene::empty(view.kind)
    }
    .measured()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{EdgeSpec, Kind, NodeSpec};

    fn node(id: &str, label: &str, shape: Shape) -> NodeSpec {
        NodeSpec { id: id.into(), label: label.into(), shape, focal: false }
    }

    fn edge(source: &str, target: &str) -> EdgeSpec {
        EdgeSpec { id: format!("{source}->{target}"), source: source.into(), target: target.into(), label: String::new(), style: Default::default(), focal: false }
    }

    fn journey(labels: &[&str]) -> View {
        let nodes = labels.iter().enumerate().map(|(index, label)| node(&format!("s{index}"), label, Shape::Service)).collect();
        let edges = (1..labels.len()).map(|index| edge(&format!("s{}", index - 1), &format!("s{index}"))).collect();
        View { kind: Kind::Journey, nodes, edges, ..View::default() }
    }

    fn line_tone(edge: &SceneEdge) -> Tone {
        edge.paint.expect("a painted curve").stroke
    }

    fn dot_tones(scene: &Scene) -> Vec<Tone> {
        scene.overlay.iter().filter_map(|figure| match figure {
            Figure::Circle { paint, .. } => Some(paint.fill),
            _ => None,
        }).collect()
    }

    #[test]
    fn sentiment_words_map_to_five_levels() {
        assert_eq!(Level::of("Delighted"), Level::High);
        assert_eq!(Level::of("very good"), Level::High);
        assert_eq!(Level::of("pleased"), Level::MedHigh);
        assert_eq!(Level::of("OK-good"), Level::MedHigh);
        assert_eq!(Level::of("mixed"), Level::Neutral);
        assert_eq!(Level::of("confused"), Level::MedLow);
        assert_eq!(Level::of("FRUSTRATED"), Level::Low);
        assert_eq!(Level::of("med-low"), Level::MedLow);
        assert_eq!(Level::of("Med High"), Level::MedHigh);
        assert_eq!(Level::of("low"), Level::Low);
        assert_eq!(Level::of("sleepy"), Level::Neutral);
    }

    #[test]
    fn the_first_bracket_item_is_the_sentiment_and_the_second_the_touchpoint() {
        let stage = Stage::new(0, "Hit the limit (frustrated, in-app modal)");
        assert_eq!(stage.lines, vec!["Hit the limit".to_string()]);
        assert_eq!(stage.level, Level::Low);
        assert_eq!(stage.touchpoint.as_deref(), Some("in-app modal"));
        assert_eq!(Stage::new(0, "Sign up").level, Level::Neutral);
    }

    #[test]
    fn the_lowest_stage_is_the_trough_and_the_first_wins_a_tie() {
        let stages: Vec<Stage> = ["A (happy)", "B (bad)", "C (ok)", "D (angry)"].iter().enumerate().map(|(node, label)| Stage::new(node, label)).collect();
        assert_eq!(trough(&stages), Some(1));
    }

    #[test]
    fn the_trough_dot_and_its_incoming_segment_are_the_only_accent_on_the_curve() {
        let scene = build(&journey(&["Sign up (happy)", "First run (ok)", "Hit the limit (frustrated)", "Upgrade (good)"]), &Params::default());
        assert_eq!(scene.edges.iter().map(line_tone).collect::<Vec<_>>(), vec![Tone::Muted, Tone::Accent, Tone::Muted]);
        assert_eq!(dot_tones(&scene), vec![Tone::Muted, Tone::Muted, Tone::Accent, Tone::Muted]);
    }

    #[test]
    fn a_lower_sentiment_puts_the_dot_lower() {
        let scene = build(&journey(&["A (great)", "B (good)", "C (neutral)", "D (meh)", "E (bad)"]), &Params::default());
        let ys: Vec<f64> = scene.overlay.iter().map(|figure| figure.bounds().y).collect();
        assert!(ys.windows(2).all(|pair| pair[1] - pair[0] == LEVEL_STEP), "{ys:?}");
    }

    #[test]
    fn an_edge_between_neighbouring_stages_is_a_selectable_curve_segment() {
        let mut view = journey(&["A (happy)", "B (ok)", "C (bad)"]);
        view.edges.remove(0);
        view.edges.push(edge("s0", "s2"));
        let scene = build(&view, &Params::default());
        assert_eq!(scene.edges.len(), 1, "the skip from A to C is not drawn");
        assert_eq!(scene.edges[0].bend, Bend::Free);
        assert_eq!(scene.edges[0].points.len(), 2);
        let plain = scene.decor.iter().filter(|figure| matches!(figure, Figure::Line { paint, .. } if paint.width == CURVE_WIDTH)).count();
        assert_eq!(plain, 1, "A to B is still drawn as a plain segment");
    }

    #[test]
    fn a_pain_marker_hangs_under_its_stage_below_the_band() {
        let mut view = journey(&["Sign up (happy)", "Hit the limit (frustrated)", "Upgrade (good)"]);
        view.nodes.push(node("p1", "No warning at 80%", Shape::Note));
        view.nodes.push(node("p2", "Pricing is hidden", Shape::Note));
        view.edges.push(edge("p1", "s1"));
        view.edges.push(edge("s1", "p2"));
        let scene = build(&view, &Params::default());
        let stage = scene.nodes.iter().find(|node| node.id == "s1").expect("stage");
        let lowest_dot = scene.overlay.iter().map(|figure| figure.bounds().bottom()).fold(0.0, f64::max);
        let tags: Vec<&SceneNode> = scene.nodes.iter().filter(|node| node.id.starts_with('p')).collect();
        for tag in &tags {
            assert_eq!(tag.rect.cx(), stage.rect.cx());
            assert!(tag.rect.y > lowest_dot);
            assert!(matches!(&tag.figures[0], Figure::Rect { paint, .. } if paint.dash.is_some() && paint.stroke == Tone::AccentWash(0.5)));
        }
        assert!(tags[1].rect.y >= tags[0].rect.bottom() + TAG_GAP);
        let links: Vec<&SceneEdge> = scene.edges.iter().filter(|edge| edge.id.contains('p')).collect();
        assert_eq!(links.len(), 2);
        assert!(links.iter().all(|link| link.points[0].x == stage.rect.cx() && link.points[1].x == stage.rect.cx()));
    }

    #[test]
    fn a_stage_is_its_header_cell() {
        let scene = build(&journey(&["Sign up (happy, email)", "First run (ok, app)"]), &Params::default());
        for node in &scene.nodes {
            assert_eq!(node.figures[0], Figure::hit_area(node.rect));
            assert_eq!(node.rect.y, 0.0);
        }
        let eyebrows: Vec<&str> = scene.nodes.iter().flat_map(|node| &node.figures).filter_map(|figure| match figure {
            Figure::Text(text) if text.text.starts_with("STAGE") => Some(text.text.as_str()),
            _ => None,
        }).collect();
        assert_eq!(eyebrows, vec!["STAGE 1", "STAGE 2"]);
    }

    #[test]
    fn the_touchpoint_row_appears_only_when_a_stage_names_one() {
        let has_row = |view: &View| build(view, &Params::default()).decor.iter().any(|figure| matches!(figure, Figure::Text(text) if text.text == "TOUCHPOINTS"));
        assert!(has_row(&journey(&["A (happy, email)", "B (bad)"])));
        assert!(!has_row(&journey(&["A (happy)", "B (bad)"])));
    }
}
