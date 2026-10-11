//! Org chart: who owns what, from the front door down. A node names a person, team, role or agent, how to
//! invoke it and its scope; a parent reaches its reports over one bus (a drop from the parent, a run along
//! the bus, a drop to each report), muted and without arrowheads.

use super::deployment::{box_figures, box_keys, node_specs};
use crate::check::check;
use crate::figure::{Figure, Marker, Paint, Text, Tone};
use crate::geom::{snap, snap_up, Point, Rect, Segment};
use crate::label::{place, LabelRequest};
use crate::node::{field_list, name_and_sublabel, NodeContent, Treatment, NAME_LINE, NAME_SIZE, SUB_LINE, SUB_SIZE};
use crate::params::{Direction, Params};
use crate::scene::{layered_with, unique_nodes, Scene};
use crate::spec::{NodeSpec, Shape, View};
use crate::text::{mono_width, sans_width};

const PADDING: f64 = 16.0;
const VERTICAL_ROOM: f64 = 28.0;
const MIN_WIDTH: f64 = 136.0;
const MIN_HEIGHT: f64 = 48.0;
const NAME_BASELINE: f64 = 11.0;
const SCOPE_SEPARATOR: &str = " · ";
const STRAIGHT: f64 = 1.0;
const PORT_MARGIN: f64 = 16.0;
const LINE_WIDTH: f64 = 1.0;
const FOCAL_LINE_WIDTH: f64 = 1.4;
const LEGEND: [(Treatment, &str); 4] = [
    (Treatment::Focal, "FRONT DOOR"),
    (Treatment::Service, "TEAM / POD"),
    (Treatment::Store, "OWNER"),
    (Treatment::Note, "GAP / NOT LIVE"),
];

/// Lays out `view` as an org chart: the layered layout top-down with cards sized for their three lines,
/// and every parent's reports hung from one bus when that breaks no connector rule.
pub fn build(view: &View, params: &Params) -> Scene {
    let (ids, contents) = unique_nodes(view);
    let cards: Vec<Card> = node_specs(view, &ids).into_iter().map(Card::parse).collect();
    let sized = cards.iter().zip(contents).map(|(card, content)| card.sized(content)).collect();
    let mut scene = layered_with(view, params, sized);
    for (node, card) in scene.nodes.iter_mut().zip(&cards) {
        node.figures = card.figures(node.rect, &node.content.lines);
    }
    route_buses(&mut scene, Frame(params.direction_for(view.kind)));
    for edge in &mut scene.edges {
        edge.markers = Some((Marker::None, Marker::None));
        edge.paint = Some(line_paint(edge.focal));
    }
    let keys = box_keys(&scene, &LEGEND);
    scene.legend = Some(if keys.len() >= 2 { keys } else { Vec::new() });
    scene
}

/// What an org node's label says: `Name (invoke, scope words…)`.
#[derive(Debug, PartialEq)]
struct Card {
    invoke: Option<String>,
    scope: Option<String>,
    treatment: Treatment,
}

impl Card {
    fn parse(spec: &NodeSpec) -> Card {
        let items = name_and_sublabel(&spec.label).1.map(|bracket| field_list(&bracket)).unwrap_or_default();
        let mut items = items.into_iter();
        let invoke = items.next();
        let scope: Vec<String> = items.collect();
        Card { invoke, scope: (!scope.is_empty()).then(|| scope.join(SCOPE_SEPARATOR)), treatment: treatment(spec) }
    }

    fn mono_lines(&self) -> impl Iterator<Item = &String> {
        self.invoke.iter().chain(&self.scope)
    }

    fn block_height(&self, names: usize) -> f64 {
        names as f64 * NAME_LINE + self.mono_lines().count() as f64 * SUB_LINE
    }

    fn sized(&self, content: NodeContent) -> NodeContent {
        let name = content.lines.iter().map(|line| sans_width(line, NAME_SIZE)).fold(0.0, f64::max);
        let mono = self.mono_lines().map(|line| mono_width(line, SUB_SIZE, 0.0)).fold(0.0, f64::max);
        let width = snap_up(name.max(mono) + 2.0 * PADDING).max(MIN_WIDTH);
        let height = snap_up(self.block_height(content.lines.len()) + VERTICAL_ROOM).max(MIN_HEIGHT);
        NodeContent { sublabel: self.invoke.clone(), fields: Vec::new(), tag: None, badge: None, treatment: self.treatment, width, height, ..content }
    }

    fn figures(&self, rect: Rect, names: &[String]) -> Vec<Figure> {
        let mut figures = box_figures(rect, self.treatment);
        let mut baseline = rect.y + (rect.h - self.block_height(names.len())) / 2.0 + NAME_BASELINE;
        for name in names {
            figures.push(Figure::Text(Text::sans(Point::new(rect.cx(), baseline), name.clone(), NAME_SIZE).weight(600)));
            baseline += NAME_LINE;
        }
        baseline += SUB_LINE - NAME_LINE;
        let tones = [Tone::Muted, Tone::Soft];
        for (line, tone) in self.mono_lines().zip(tones) {
            figures.push(Figure::Text(Text::mono(Point::new(rect.cx(), baseline), line.clone(), SUB_SIZE).tone(tone)));
            baseline += SUB_LINE;
        }
        figures
    }
}

fn treatment(spec: &NodeSpec) -> Treatment {
    match spec.shape {
        _ if spec.focal => Treatment::Focal,
        Shape::Actor => Treatment::Store,
        Shape::External | Shape::Note => Treatment::Note,
        _ => Treatment::Service,
    }
}

fn line_paint(focal: bool) -> Paint {
    if focal {
        Paint::line(Tone::Accent, FOCAL_LINE_WIDTH)
    } else {
        Paint::line(Tone::Muted, LINE_WIDTH)
    }
}

/// The page axes of a layout direction: `along` runs from a parent's rank to its reports' rank,
/// `across` is the axis a bus runs on.
#[derive(Clone, Copy)]
struct Frame(Direction);

impl Frame {
    fn point(self, along: f64, across: f64) -> Point {
        match self.0 {
            Direction::Down => Point::new(across, along),
            Direction::Right => Point::new(along, across),
        }
    }

    fn start(self, rect: &Rect) -> f64 {
        match self.0 {
            Direction::Down => rect.y,
            Direction::Right => rect.x,
        }
    }

    fn end(self, rect: &Rect) -> f64 {
        match self.0 {
            Direction::Down => rect.bottom(),
            Direction::Right => rect.right(),
        }
    }

    fn span(self, rect: &Rect) -> (f64, f64) {
        match self.0 {
            Direction::Down => (rect.x, rect.right()),
            Direction::Right => (rect.y, rect.bottom()),
        }
    }

    fn middle(self, rect: &Rect) -> f64 {
        let (low, high) = self.span(rect);
        (low + high) / 2.0
    }
}

#[derive(Clone, Copy, Debug)]
struct Report {
    edge: usize,
    rect: Rect,
}

/// The edge points and label masks of a scene, to put back when the buses turn out worse.
struct Routing {
    points: Vec<Vec<Point>>,
    labels: Vec<Option<Rect>>,
}

impl Routing {
    fn of(scene: &Scene) -> Routing {
        Routing {
            points: scene.edges.iter().map(|edge| edge.points.clone()).collect(),
            labels: scene.edges.iter().map(|edge| edge.label.as_ref().map(|label| label.rect)).collect(),
        }
    }

    fn restore(self, scene: &mut Scene) {
        for ((edge, points), mask) in scene.edges.iter_mut().zip(self.points).zip(self.labels) {
            edge.points = points;
            if let (Some(label), Some(mask)) = (&mut edge.label, mask) {
                label.rect = mask;
            }
        }
    }
}

fn route_buses(scene: &mut Scene, frame: Frame) {
    for family in bus_families(scene, frame) {
        hang_on_bus(scene, family);
    }
}

/// Moves one family's edges onto their bus, and puts the layered routes back if that breaks more
/// connector rules than before.
fn hang_on_bus(scene: &mut Scene, family: Vec<(usize, Vec<Point>)>) {
    let before = check(scene).0.len();
    let routing = Routing::of(scene);
    let relabel = family.iter().any(|(edge, _)| scene.edges[*edge].label.is_some());
    for (edge, path) in family {
        scene.edges[edge].points = path;
    }
    if relabel {
        place_labels(scene);
    }
    if check(scene).0.len() > before {
        routing.restore(scene);
    }
}

/// The bus paths of every parent's family: its edges to the reports whose only parent it is and that sit
/// in a later rank.
fn bus_families(scene: &Scene, frame: Frame) -> Vec<Vec<(usize, Vec<Point>)>> {
    let rects: Vec<Rect> = scene.nodes.iter().map(|node| node.rect).collect();
    let parents = |node: usize| scene.edges.iter().filter(|edge| edge.target == node && edge.source != node).count();
    (0..rects.len())
        .map(|parent| {
            let reports: Vec<Report> = scene
                .edges
                .iter()
                .enumerate()
                .filter(|(_, edge)| edge.source == parent && edge.target != parent && parents(edge.target) == 1)
                .filter(|(_, edge)| frame.start(&rects[edge.target]) > frame.end(&rects[parent]))
                .map(|(edge, data)| Report { edge, rect: rects[data.target] })
                .collect();
            family_paths(frame, rects[parent], &reports)
        })
        .filter(|family| !family.is_empty())
        .collect()
}

/// One parent's bus: the trunk drops from the parent's middle to the bus, half way to the nearest report.
/// The farthest report on each side runs along the whole bus from the junction; the others drop from it.
/// A report right under the parent takes the trunk straight down; otherwise the trunk joins the first
/// side's run with a sharp corner when the other side starts at the junction too.
fn family_paths(frame: Frame, parent: Rect, reports: &[Report]) -> Vec<(usize, Vec<Point>)> {
    let Some(nearest) = reports.iter().map(|report| frame.start(&report.rect)).reduce(f64::min) else { return Vec::new() };
    let trunk = frame.middle(&parent);
    let top = frame.end(&parent);
    let bus = snap((top + nearest) / 2.0);
    let junction = frame.point(bus, trunk);
    let straight = straight_report(frame, trunk, reports);
    let (left, right) = sides(frame, trunk, reports, straight);
    let mut paths: Vec<(usize, Vec<Point>)> = [&left, &right].into_iter().flat_map(|side| side_paths(frame, bus, junction, side)).collect();
    let trunk_start = frame.point(top, trunk);
    match straight {
        Some(report) => paths.push((report.edge, vec![trunk_start, frame.point(frame.start(&report.rect), trunk)])),
        None => {
            let first = &mut paths[0].1;
            if !left.is_empty() && !right.is_empty() {
                first.insert(0, junction);
            }
            first.insert(0, trunk_start);
        }
    }
    paths
}

fn straight_report(frame: Frame, trunk: f64, reports: &[Report]) -> Option<Report> {
    let under = |report: &&Report| (frame.middle(&report.rect) - trunk).abs() < STRAIGHT;
    let sole_spans = |report: &&Report| {
        let (low, high) = frame.span(&report.rect);
        reports.len() == 1 && trunk > low + PORT_MARGIN && trunk < high - PORT_MARGIN
    };
    reports.iter().find(|report| under(report) || sole_spans(report)).copied()
}

/// The reports left and right of the trunk, farthest first.
fn sides(frame: Frame, trunk: f64, reports: &[Report], straight: Option<Report>) -> (Vec<Report>, Vec<Report>) {
    let others = reports.iter().copied().filter(|report| straight.map_or(true, |own| own.edge != report.edge));
    let (mut left, mut right): (Vec<Report>, Vec<Report>) = others.partition(|report| frame.middle(&report.rect) < trunk);
    left.sort_by(|a, b| frame.middle(&a.rect).total_cmp(&frame.middle(&b.rect)));
    right.sort_by(|a, b| frame.middle(&b.rect).total_cmp(&frame.middle(&a.rect)));
    (left, right)
}

fn side_paths(frame: Frame, bus: f64, junction: Point, side: &[Report]) -> Vec<(usize, Vec<Point>)> {
    side.iter()
        .enumerate()
        .map(|(order, report)| {
            let across = frame.middle(&report.rect);
            let on_bus = frame.point(bus, across);
            let arrival = frame.point(frame.start(&report.rect), across);
            let path = if order == 0 { vec![junction, on_bus, arrival] } else { vec![on_bus, arrival] };
            (report.edge, path)
        })
        .collect()
}

/// Places every edge label again, next to the last run of its edge (a report's drop).
fn place_labels(scene: &mut Scene) {
    let paths: Vec<Vec<Point>> = scene.edges.iter().map(|edge| edge.points.clone()).collect();
    let requests: Vec<LabelRequest> = scene
        .edges
        .iter()
        .enumerate()
        .filter_map(|(index, edge)| {
            let label = edge.label.as_ref()?;
            Some(LabelRequest { edge: index, mask_width: label.rect.w, preferred: last_run(&edge.points), overhang: false })
        })
        .collect();
    let nodes: Vec<Rect> = scene.nodes.iter().map(|node| node.rect).collect();
    for (request, mask) in requests.iter().zip(place(&requests, &paths, &nodes)) {
        if let Some(label) = &mut scene.edges[request.edge].label {
            label.rect = mask;
        }
    }
}

fn last_run(points: &[Point]) -> Option<Segment> {
    let [.., before, last] = points else { return None };
    Some(Segment::new(*before, *last))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene;
    use crate::{render, svg};

    fn fixture() -> View {
        serde_json::from_str(include_str!("../../tests/org-chart.json")).expect("fixture parses")
    }

    fn spec(label: &str, shape: Shape, focal: bool) -> NodeSpec {
        NodeSpec { id: "n".into(), label: label.into(), shape, focal }
    }

    fn texts(figures: &[Figure]) -> Vec<&Text> {
        figures.iter().filter_map(|figure| if let Figure::Text(text) = figure { Some(text) } else { None }).collect()
    }

    fn report(edge: usize, x: f64) -> Report {
        Report { edge, rect: Rect::new(x, 200.0, 120.0, 48.0) }
    }

    #[test]
    fn a_label_gives_the_name_then_how_to_invoke_then_the_scope_on_a_third_line() {
        let card = Card::parse(&spec("Lena (@lena, payments, disputes)", Shape::Actor, false));
        assert_eq!(card.invoke.as_deref(), Some("@lena"));
        assert_eq!(card.scope.as_deref(), Some("payments · disputes"));
        let figures = card.figures(Rect::new(0.0, 0.0, 160.0, 72.0), &["Lena".to_string()]);
        let lines = texts(&figures);
        let looks: Vec<(&str, f64, u16, Tone)> = lines.iter().map(|text| (text.text.as_str(), text.size, text.weight, text.tone)).collect();
        assert_eq!(looks, vec![("Lena", 12.0, 600, Tone::Ink), ("@lena", 9.0, 400, Tone::Muted), ("payments · disputes", 9.0, 400, Tone::Soft)]);
        assert!(lines.windows(2).all(|pair| pair[0].at.y < pair[1].at.y));
        assert!(lines.iter().all(|text| text.bounds().y > 0.0 && text.bounds().bottom() < 72.0));
    }

    #[test]
    fn a_card_is_wide_and_tall_enough_for_its_three_lines() {
        let card = Card::parse(&spec("Ivy (@ivy, databases, backups, restores)", Shape::Actor, false));
        let (_, contents) = unique_nodes(&View { nodes: vec![spec("Ivy (@ivy, databases, backups, restores)", Shape::Actor, false)], ..View::default() });
        let sized = card.sized(contents.into_iter().next().expect("one node"));
        assert!(sized.width >= mono_width("databases · backups · restores", SUB_SIZE, 0.0) + 2.0 * PADDING);
        assert!(sized.height >= NAME_LINE + 2.0 * SUB_LINE + VERTICAL_ROOM);
        assert_eq!((sized.width % 4.0, sized.height % 4.0), (0.0, 0.0));
    }

    #[test]
    fn the_front_door_is_focal_teams_are_boxes_owners_are_stores_and_gaps_are_dashed() {
        let look = |shape, focal| Card::parse(&spec("X (x)", shape, focal)).treatment;
        assert_eq!(look(Shape::Service, true), Treatment::Focal);
        assert_eq!(look(Shape::Service, false), Treatment::Service);
        assert_eq!(look(Shape::Actor, false), Treatment::Store);
        assert_eq!(look(Shape::External, false), Treatment::Note);
        assert_eq!(look(Shape::Note, false), Treatment::Note);
        assert!(super::super::deployment::box_paint(Treatment::Note).dash.is_some());
    }

    #[test]
    fn a_report_right_under_its_parent_takes_the_trunk_straight_down() {
        let parent = Rect::new(140.0, 104.0, 120.0, 48.0);
        let paths = family_paths(Frame(Direction::Down), parent, &[report(0, 0.0), report(1, 140.0), report(2, 280.0)]);
        let straight = &paths.iter().find(|(edge, _)| *edge == 1).expect("the middle report").1;
        assert_eq!(straight, &vec![Point::new(200.0, 152.0), Point::new(200.0, 200.0)]);
        for (edge, path) in paths.iter().filter(|(edge, _)| *edge != 1) {
            assert_eq!(path[0], Point::new(200.0, 176.0), "edge {edge} starts at the junction");
        }
    }

    #[test]
    fn without_a_report_under_it_the_trunk_turns_sharply_onto_the_first_side() {
        let parent = Rect::new(80.0, 104.0, 120.0, 48.0);
        let paths = family_paths(Frame(Direction::Down), parent, &[report(0, 0.0), report(1, 160.0)]);
        let trunk = &paths.iter().find(|(edge, _)| *edge == 0).expect("the left report").1;
        assert_eq!(trunk[..3], [Point::new(140.0, 152.0), Point::new(140.0, 176.0), Point::new(140.0, 176.0)]);
        let other = &paths.iter().find(|(edge, _)| *edge == 1).expect("the right report").1;
        assert_eq!(other[0], Point::new(140.0, 176.0));
    }

    #[test]
    fn every_parent_hangs_its_reports_from_one_bus_with_no_shared_attach_point() {
        let scene = scene::build(&fixture(), &Params::default());
        assert_eq!(check(&scene).0, Vec::new());
        for (index, parent) in scene.nodes.iter().enumerate() {
            let family: Vec<&Vec<Point>> = scene.edges.iter().filter(|edge| edge.source == index).map(|edge| &edge.points).collect();
            if family.is_empty() {
                continue;
            }
            let leaving = family.iter().filter(|points| points[0].y == parent.rect.bottom()).count();
            assert_eq!(leaving, 1, "{} has one trunk", parent.id);
            let bus_levels: Vec<f64> = family.iter().flat_map(|points| points.windows(2)).filter(|pair| pair[0].y == pair[1].y).map(|pair| pair[0].y).collect();
            assert!(bus_levels.windows(2).all(|pair| pair[0] == pair[1]), "{} runs on one bus", parent.id);
        }
        for edge in &scene.edges {
            let target = scene.nodes[edge.target].rect;
            assert_eq!(edge.points.last(), Some(&Point::new(target.cx(), target.y)), "{} drops into its report's middle", edge.id);
        }
    }

    #[test]
    fn a_report_with_two_parents_keeps_its_layered_route_and_no_variant_breaks_a_rule() {
        let view: View = serde_json::from_str(
            r#"{"kind": "org-chart", "nodes": [
                {"id": "ceo", "label": "Ada (@ada, company)", "focal": true}, {"id": "eng", "label": "Engineering (ENG-)"},
                {"id": "ops", "label": "Operations (#ops)"}, {"id": "web", "label": "Web (#web)"}, {"id": "app", "label": "App (#app)"},
                {"id": "design", "label": "Design (#design)", "shape": "actor"}],
               "edges": [{"source": "ceo", "target": "eng", "label": "reports"}, {"source": "ceo", "target": "ops"},
                {"source": "eng", "target": "web"}, {"source": "eng", "target": "app"},
                {"source": "eng", "target": "design"}, {"source": "ops", "target": "design"}]}"#,
        )
        .expect("view parses");
        for params in crate::variants::candidates(&view, &Params::default()) {
            let scene = scene::build(&view, &params);
            assert_eq!(check(&scene).0, Vec::new(), "{params:?}");
        }
        let scene = scene::build(&view, &Params::default());
        let ceo = scene.nodes[0].rect;
        let trunks = scene.edges.iter().filter(|edge| edge.source == 0 && edge.points[0] == Point::new(ceo.cx(), ceo.bottom())).count();
        assert_eq!(trunks, 1, "the front door hangs its two reports from one trunk");
    }

    #[test]
    fn org_lines_are_muted_without_arrowheads_and_the_legend_names_the_org_looks() {
        let rendered = render(&fixture(), &Params::default());
        assert!(!rendered.svg.contains("marker-end"));
        assert!(rendered.svg.contains(&format!(r#"<g class="sk-edge" data-id="{}"><path class="sk-hit""#, svg::escape("desk->billing"))));
        let scene = scene::build(&fixture(), &Params::default());
        assert!(scene.edges.iter().all(|edge| edge.paint == Some(Paint::line(Tone::Muted, LINE_WIDTH))));
        let texts: Vec<String> = scene.legend.expect("an own legend").into_iter().map(|key| key.text).collect();
        assert_eq!(texts, vec!["FRONT DOOR", "TEAM / POD", "OWNER", "GAP / NOT LIVE"]);
    }
}
