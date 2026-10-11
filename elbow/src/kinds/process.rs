//! Process: numbered steps in role lanes. The groups are the lanes (top to bottom), the nodes the steps and
//! the edges the handoffs between them. The layered layout's ranks are the step columns, numbered in a header
//! row above the lanes; every step carries its lane's role key, and the focal step, its column chip and its
//! handoffs are drawn in the accent.

use crate::figure::{Anchor, Figure, LegendKey, Marker, Paint, Swatch, Text, Tone};
use crate::geom::{snap_up, Point, Rect};
use crate::node::{name_and_sublabel, tag_width, NodeContent, Treatment, NAME_LINE, NAME_SIZE, SUB_SIZE, TAG_ROW, TAG_SIZE, TAG_TRACKING};
use crate::params::{Direction, Params};
use crate::scene::{layered_with, unique_nodes, Scene, SceneEdge, SceneNode, LANE_LABEL_SIZE, LANE_LABEL_TRACKING, MARGIN};
use crate::spec::{EdgeStyle, GroupSpec, View};

const KEY_LENGTH: usize = 3;
const NODE_ROOM: f64 = 20.0;
const MIN_NODE_HEIGHT: f64 = 64.0;
const CORNER: f64 = 6.0;
const BADGE_INSET: f64 = 8.0;
const BADGE_TOP: f64 = 6.0;
const BADGE_HEIGHT: f64 = 12.0;
const NAME_BASELINE: f64 = 11.0;
const SUB_DROP: f64 = 10.0;
const CHIP_RADIUS: f64 = 8.0;
const CHIP_RISE: f64 = 20.0;
const CHIP_TEXT_SIZE: f64 = 8.0;
const CHIP_TEXT_DROP: f64 = 3.0;
const HANDOFF_WIDTH: f64 = 1.2;
const TRIGGER_DASH: &str = "4,3";
const HEADER_LABEL: &str = "STEP";

/// Lays out `view` as a process: role lanes left to right, a numbered step header and role badges.
pub fn build(view: &View, params: &Params) -> Scene {
    let params = Params { direction: Some(Direction::Right), ..*params };
    let (ids, contents) = unique_nodes(view);
    let keys: Vec<Option<String>> = ids.iter().map(|id| lane_of(view, id).map(role_key)).collect();
    let focal: Vec<bool> = contents.iter().map(|content| content.treatment == Treatment::Focal).collect();
    let mut scene = layered_with(&with_plain_lane_names(view), &params, uniform(contents));
    for (node, key) in scene.nodes.iter_mut().zip(&keys) {
        node.figures = step_figures(node, key.as_deref());
    }
    for edge in &mut scene.edges {
        edge.paint = handoff_paint(edge, &focal);
    }
    scene.decor = step_header(&scene, &focal);
    scene.legend = Some(legend(&scene, focal.contains(&true)));
    scene.measured()
}

fn lane_of<'a>(view: &'a View, id: &str) -> Option<&'a GroupSpec> {
    view.groups.iter().find(|group| group.members.iter().any(|member| member == id))
}

fn group_label(group: &GroupSpec) -> &str {
    if group.label.is_empty() {
        &group.id
    } else {
        &group.label
    }
}

/// The badge text of a lane: the bracket of its label (`Field services (FLD)`), or else the first three
/// letters or digits of the label, upper-cased.
fn role_key(group: &GroupSpec) -> String {
    match name_and_sublabel(group_label(group)) {
        (_, Some(key)) => key.to_uppercase(),
        (name, None) => name.chars().filter(|c| c.is_alphanumeric()).take(KEY_LENGTH).collect::<String>().to_uppercase(),
    }
}

/// `view` with every lane named without its role-key bracket, so the lane gutter shows the plain name.
fn with_plain_lane_names(view: &View) -> View {
    let mut plain = view.clone();
    for group in &mut plain.groups {
        if let (name, Some(_)) = name_and_sublabel(&group.label) {
            group.label = name;
        }
    }
    plain
}

fn step_height(content: &NodeContent) -> f64 {
    snap_up(TAG_ROW + content.block_height() + NODE_ROOM)
}

/// Every step at the size of the largest one, so the step grid stays regular.
fn uniform(contents: Vec<NodeContent>) -> Vec<NodeContent> {
    let width = contents.iter().map(|content| content.width).fold(0.0, f64::max);
    let height = contents.iter().map(step_height).fold(MIN_NODE_HEIGHT, f64::max);
    contents.into_iter().map(|content| NodeContent { width, height, ..content }).collect()
}

fn step_figures(node: &SceneNode, key: Option<&str>) -> Vec<Figure> {
    let focal = node.content.treatment == Treatment::Focal;
    let card = if focal { Paint::shape(Tone::AccentTint, Tone::Accent, 1.0) } else { Paint::shape(Tone::Fill, Tone::Wash(0.3), 1.0) };
    let mut figures = vec![Figure::rect(node.rect, CORNER, card)];
    figures.extend(key.into_iter().flat_map(|key| role_badge(node.rect, key, focal)));
    figures.extend(name_texts(node.rect, &node.content));
    figures
}

fn role_badge(rect: Rect, key: &str, focal: bool) -> [Figure; 2] {
    let chip = Rect::new(rect.x + BADGE_INSET, rect.y + BADGE_TOP, tag_width(key), BADGE_HEIGHT);
    let (fill, ink) = if focal { (Tone::AccentWash(0.18), Tone::Accent) } else { (Tone::Wash(0.08), Tone::Ink) };
    let text = Text::mono(Point::new(chip.cx(), chip.y + 9.0), key, TAG_SIZE).weight(600).tone(ink).tracking(TAG_TRACKING);
    [Figure::rect(chip, 2.0, Paint::filled(fill)), Figure::Text(text)]
}

fn name_texts(rect: Rect, content: &NodeContent) -> Vec<Figure> {
    let top = rect.y + TAG_ROW + (rect.h - TAG_ROW - content.block_height()) / 2.0;
    let names = content.lines.iter().enumerate().map(|(index, line)| {
        Text::sans(Point::new(rect.cx(), top + NAME_BASELINE + index as f64 * NAME_LINE), line.clone(), NAME_SIZE).weight(600)
    });
    let sub_baseline = top + content.lines.len() as f64 * NAME_LINE + SUB_DROP;
    let sub = content.sublabel.iter().map(|sub| Text::mono(Point::new(rect.cx(), sub_baseline), sub.clone(), SUB_SIZE).tone(Tone::Soft));
    names.chain(sub).map(Figure::Text).collect()
}

/// The look of a handoff: muted, dashed for a trigger (async), accent when it enters or leaves the focal
/// step. An edge the view marks focal keeps the standard focal look.
fn handoff_paint(edge: &SceneEdge, focal: &[bool]) -> Option<Paint> {
    if edge.focal {
        return None;
    }
    let tone = if focal[edge.source] || focal[edge.target] { Tone::Accent } else { Tone::Muted };
    let paint = Paint::line(tone, HANDOFF_WIDTH);
    Some(if edge.style == EdgeStyle::Async { paint.dashed(TRIGGER_DASH) } else { paint })
}

/// The centres of the step columns, left to right: one per rank of the layered layout.
fn step_columns(nodes: &[SceneNode]) -> Vec<f64> {
    let mut centres: Vec<f64> = nodes.iter().map(|node| node.rect.cx()).collect();
    centres.sort_by(f64::total_cmp);
    centres.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    centres
}

/// The numbered chip above every step column, the focal step's in the accent, and the `STEP` eyebrow in
/// the lane gutter.
fn step_header(scene: &Scene, focal: &[bool]) -> Vec<Figure> {
    let top = scene.lanes.first().map_or_else(|| lowest_node_top(scene), |lane| lane.top);
    let y = top - CHIP_RISE;
    let focal_columns: Vec<f64> = scene.nodes.iter().zip(focal).filter(|(_, focal)| **focal).map(|(node, _)| node.rect.cx()).collect();
    let mut figures: Vec<Figure> = step_columns(&scene.nodes)
        .into_iter()
        .enumerate()
        .flat_map(|(index, x)| step_chip(Point::new(x, y), index + 1, focal_columns.iter().any(|column| (column - x).abs() < 0.5)))
        .collect();
    if !scene.lanes.is_empty() {
        let eyebrow = Text::mono(Point::new(MARGIN, y + CHIP_TEXT_DROP), HEADER_LABEL, LANE_LABEL_SIZE).tracking(LANE_LABEL_TRACKING);
        figures.push(Figure::Text(eyebrow.anchor(Anchor::Start)));
    }
    figures
}

fn lowest_node_top(scene: &Scene) -> f64 {
    scene.nodes.iter().map(|node| node.rect.y).fold(f64::INFINITY, f64::min)
}

fn step_chip(centre: Point, number: usize, focal: bool) -> [Figure; 2] {
    let (fill, ink) = if focal { (Tone::AccentWash(0.2), Tone::Accent) } else { (Tone::Wash(0.12), Tone::Ink) };
    let text = Text::mono(Point::new(centre.x, centre.y + CHIP_TEXT_DROP), number.to_string(), CHIP_TEXT_SIZE).weight(600).tone(ink);
    [Figure::circle(centre, CHIP_RADIUS, Paint::filled(fill)), Figure::Text(text)]
}

fn legend(scene: &Scene, has_focal: bool) -> Vec<LegendKey> {
    let used = |trigger: bool| scene.edges.iter().any(|edge| (edge.style == EdgeStyle::Async) == trigger);
    let handoff = Paint::line(Tone::Muted, HANDOFF_WIDTH);
    let mut keys = Vec::new();
    if used(false) {
        keys.push(LegendKey::new(Swatch::Line(handoff, Marker::Arrow), "HANDOFF"));
    }
    if used(true) {
        keys.push(LegendKey::new(Swatch::Line(handoff.dashed(TRIGGER_DASH), Marker::Arrow), "TRIGGER"));
    }
    if has_focal {
        keys.push(LegendKey::new(Swatch::Line(Paint::line(Tone::Accent, HANDOFF_WIDTH), Marker::Arrow), "FOCAL HANDOFF"));
        keys.push(LegendKey::new(Swatch::Box(Paint::shape(Tone::AccentTint, Tone::Accent, 1.0)), "FOCAL STEP"));
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> View {
        serde_json::from_str(include_str!("../../tests/process.json")).expect("fixture parses")
    }

    fn group(label: &str) -> GroupSpec {
        GroupSpec { id: "lane".into(), label: label.into(), members: Vec::new() }
    }

    fn texts(figures: &[Figure]) -> Vec<&Text> {
        figures
            .iter()
            .filter_map(|figure| match figure {
                Figure::Text(text) => Some(text),
                _ => None,
            })
            .collect()
    }

    fn node<'a>(scene: &'a Scene, id: &str) -> &'a SceneNode {
        scene.nodes.iter().find(|node| node.id == id).expect("a known node")
    }

    #[test]
    fn a_role_key_is_the_first_three_letters_of_the_lane_label() {
        assert_eq!(role_key(&group("Hiring manager")), "HIR");
        assert_eq!(role_key(&group("R&D lab")), "RDL");
        assert_eq!(role_key(&group("HR")), "HR");
        assert_eq!(role_key(&GroupSpec { id: "ops".into(), label: String::new(), members: Vec::new() }), "OPS");
    }

    #[test]
    fn a_bracket_in_the_lane_label_is_the_role_key_and_the_gutter_shows_the_name_without_it() {
        assert_eq!(role_key(&group("Field services (FLD)")), "FLD");
        let scene = build(&fixture(), &Params::default());
        let labels: Vec<&str> = scene.lanes.iter().map(|lane| lane.label.as_str()).collect();
        assert!(labels.contains(&"FIELD SERVICES"), "{labels:?}");
        assert!(labels.iter().all(|label| !label.contains('(')));
    }

    #[test]
    fn every_step_shows_its_lanes_role_key() {
        let view = fixture();
        let scene = build(&view, &Params::default());
        for node in &scene.nodes {
            let key = role_key(lane_of(&view, &node.id).expect("every fixture step has a lane"));
            assert!(texts(&node.figures).iter().any(|text| text.text == key), "{} lacks {key}", node.id);
        }
    }

    #[test]
    fn steps_are_numbered_once_per_rank_from_left_to_right() {
        let scene = build(&fixture(), &Params::default());
        let columns = step_columns(&scene.nodes);
        let chips: Vec<&Text> = texts(&scene.decor).into_iter().filter(|text| text.text != HEADER_LABEL).collect();
        let numbers: Vec<(f64, String)> = chips.iter().map(|text| (text.at.x, text.text.clone())).collect();
        let expected: Vec<(f64, String)> = columns.iter().enumerate().map(|(index, &x)| (x, (index + 1).to_string())).collect();
        assert_eq!(numbers, expected);
        assert_eq!(columns.len(), 7, "the fixture's chain runs over seven ranks");
        assert!(chips.iter().all(|text| text.at.y < scene.lanes[0].top), "the header row sits above the lanes");
    }

    #[test]
    fn only_the_focal_steps_column_chip_is_in_the_accent() {
        let scene = build(&fixture(), &Params::default());
        let pilot = node(&scene, "pilot").rect.cx();
        for figure in &scene.decor {
            if let Figure::Circle { centre, paint, .. } = figure {
                assert_eq!(paint.fill == Tone::AccentWash(0.2), (centre.x - pilot).abs() < 0.5, "chip at {}", centre.x);
            }
        }
    }

    #[test]
    fn handoffs_into_and_out_of_the_focal_step_are_accent_and_the_rest_muted() {
        let scene = build(&fixture(), &Params::default());
        let pilot = scene.nodes.iter().position(|node| node.id == "pilot").expect("the focal step");
        for edge in &scene.edges {
            let paint = edge.paint.expect("every handoff is painted");
            let touches_focal = edge.source == pilot || edge.target == pilot;
            assert_eq!(paint.stroke == Tone::Accent, touches_focal, "{}", edge.id);
            assert_eq!(paint.dash.is_some(), edge.style == EdgeStyle::Async, "{}", edge.id);
        }
    }

    #[test]
    fn an_edge_the_view_marks_focal_keeps_the_standard_focal_look() {
        let mut view = fixture();
        view.edges[0].focal = true;
        let scene = build(&view, &Params::default());
        let marked = scene.edges.iter().find(|edge| edge.id == view.edges[0].id).expect("the marked edge");
        assert!(marked.paint.is_none());
    }

    #[test]
    fn a_process_without_lanes_still_numbers_its_steps_above_them_and_shows_no_badges() {
        let mut view = fixture();
        view.groups.clear();
        let scene = build(&view, &Params::default());
        let chips: Vec<&Text> = texts(&scene.decor);
        let top = lowest_node_top(&scene);
        assert_eq!(chips.len(), step_columns(&scene.nodes).len());
        assert!(chips.iter().all(|text| text.at.y < top && text.text != HEADER_LABEL));
        assert!(scene.nodes.iter().all(|node| texts(&node.figures).len() == node.content.lines.len() + usize::from(node.content.sublabel.is_some())));
    }

    #[test]
    fn every_step_has_the_same_size_and_room_for_its_badge_row() {
        let scene = build(&fixture(), &Params::default());
        let first = scene.nodes[0].rect;
        assert!(scene.nodes.iter().all(|node| node.rect.w == first.w && node.rect.h == first.h));
        for node in &scene.nodes {
            for text in texts(&node.figures) {
                let bounds = text.bounds();
                assert!(bounds.x >= node.rect.x && bounds.right() <= node.rect.right(), "{} text too wide", node.id);
                assert!(bounds.y >= node.rect.y && bounds.bottom() <= node.rect.bottom(), "{} text too tall", node.id);
            }
        }
    }
}
