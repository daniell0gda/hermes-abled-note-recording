//! Story map: the activities as a backbone in narrative order, their steps as the walking skeleton under
//! them, and the stories in release slices below, with the release cut under the first slice. The edges
//! only tie steps and stories to their activity; they are not drawn.

use std::collections::VecDeque;

use super::kanban::{first_specs, title_lines, CardLabel, TEXT_INSET};
use crate::figure::{Anchor, Figure, LegendKey, Marker, Paint, Section, Swatch, Text, Tone};
use crate::geom::{snap_up, Point, Rect};
use crate::node::{NAME_LINE, NAME_SIZE};
use crate::params::Params;
use crate::scene::{known_edges, unique_nodes, Scene, SceneNode, MARGIN};
use crate::spec::View;
use crate::text::{mono_width, sans_width};

const COLUMN_WIDTH: f64 = 200.0;
const COLUMN_GAP: f64 = 24.0;
const LABEL_MARGIN: f64 = 96.0;
const LABEL_INSET: f64 = 16.0;
const EYEBROW_SIZE: f64 = 8.0;
const TRACKING: f64 = 0.14;
const ACTIVITY_HEIGHT: f64 = 64.0;
const ACTIVITY_RADIUS: f64 = 6.0;
const STEP_TOP: f64 = 16.0;
const STEP_HEIGHT: f64 = 32.0;
const STEP_PITCH: f64 = 40.0;
const STEP_BASELINE: f64 = 20.0;
const RESERVED_STEPS: usize = 2;
const BAND_GAP: f64 = 24.0;
const BAND_PADDING: f64 = 16.0;
const STORY_HEIGHT: f64 = 48.0;
const STORY_ROOM: f64 = 18.0;
const STORY_GAP: f64 = 12.0;
const CARD_INSET: f64 = 16.0;
const CARD_RADIUS: f64 = 4.0;
const SEPARATOR_REACH: f64 = 4.0;
const CUT_WIDTH: f64 = 1.5;
const CUT_LABEL: &str = "RELEASE CUT";
const CUT_TRACKING: f64 = 0.10;
const CUT_MASK_PADDING: f64 = 6.0;
const CUT_MASK_HEIGHT: f64 = 12.0;
const CUT_END_INSET: f64 = 16.0;
const RISK_LABEL: &str = "RISK";
const TAG_WIDTH: f64 = 40.0;
const TAG_HEIGHT: f64 = 12.0;
const TAG_INSET: f64 = 8.0;
const TAG_RISE: f64 = 24.0;

/// A release slice: a group, top to bottom in group order.
struct Slice {
    id: String,
    label: String,
}

/// A story card: its node, its slice and its column (an activity, or the extra last column).
struct Story {
    node: usize,
    slice: usize,
    column: usize,
}

/// Which node plays which part: activities left to right, steps under their activity, stories in their
/// slice and column.
struct Map {
    activities: Vec<usize>,
    steps: Vec<Vec<usize>>,
    slices: Vec<Slice>,
    stories: Vec<Story>,
    columns: usize,
}

impl Map {
    fn of(view: &View, ids: &[String]) -> Map {
        let slice_of: Vec<Option<usize>> =
            ids.iter().map(|id| view.groups.iter().position(|group| group.members.contains(id))).collect();
        let incoming = incoming(view, ids);
        let loose: Vec<bool> = slice_of.iter().map(Option::is_none).collect();
        let roots: Vec<bool> =
            (0..ids.len()).map(|node| loose[node] && !incoming[node].iter().any(|&from| loose[from])).collect();
        let step_of: Vec<Option<usize>> = (0..ids.len())
            .map(|node| if loose[node] && !roots[node] { nearest_source(node, &incoming, &roots) } else { None })
            .collect();
        let is_activity: Vec<bool> = (0..ids.len()).map(|node| loose[node] && step_of[node].is_none()).collect();
        let activities: Vec<usize> = (0..ids.len()).filter(|&node| is_activity[node]).collect();
        let column_of = |activity: usize| activities.iter().position(|&known| known == activity);
        let mut steps = vec![Vec::new(); activities.len()];
        for (node, owner) in step_of.iter().enumerate() {
            if let Some(column) = owner.and_then(column_of) {
                steps[column].push(node);
            }
        }
        let stories: Vec<Story> = (0..ids.len())
            .filter_map(|node| {
                let column = nearest_source(node, &incoming, &is_activity).and_then(column_of).unwrap_or(activities.len());
                Some(Story { node, slice: slice_of[node]?, column })
            })
            .collect();
        let extra = stories.iter().any(|story| story.column == activities.len());
        let columns = activities.len() + usize::from(extra);
        let slices = view
            .groups
            .iter()
            .map(|group| {
                let label = if group.label.trim().is_empty() { &group.id } else { &group.label };
                Slice { id: group.id.clone(), label: label.trim().to_uppercase() }
            })
            .collect();
        Map { activities, steps, slices, stories, columns }
    }

    fn cell(&self, slice: usize, column: usize) -> impl Iterator<Item = &Story> + '_ {
        self.stories.iter().filter(move |story| story.slice == slice && story.column == column)
    }

    /// True when some slice has no story under some column.
    fn has_gap(&self) -> bool {
        (0..self.slices.len()).any(|slice| (0..self.columns).any(|column| self.cell(slice, column).next().is_none()))
    }

    fn skeleton_rows(&self) -> usize {
        self.steps.iter().map(Vec::len).max().filter(|&most| most > 0).map_or(0, |most| most.max(RESERVED_STEPS))
    }
}

fn incoming(view: &View, ids: &[String]) -> Vec<Vec<usize>> {
    let mut incoming = vec![Vec::new(); ids.len()];
    for edge in known_edges(view, ids).iter().filter(|edge| edge.source != edge.target) {
        incoming[edge.target].push(edge.source);
    }
    incoming
}

/// The nearest node marked in `targets` that `node` is reached from, following the edges backwards.
fn nearest_source(node: usize, incoming: &[Vec<usize>], targets: &[bool]) -> Option<usize> {
    let mut seen = vec![false; incoming.len()];
    seen[node] = true;
    let mut queue = VecDeque::from([node]);
    while let Some(current) = queue.pop_front() {
        for &from in &incoming[current] {
            if targets[from] {
                return Some(from);
            }
            if !std::mem::replace(&mut seen[from], true) {
                queue.push_back(from);
            }
        }
    }
    None
}

/// The horizontal grid: the slice label margin, then the columns.
struct Grid {
    left: f64,
    margin: f64,
    width: f64,
    columns: usize,
}

impl Grid {
    fn column_x(&self, column: usize) -> f64 {
        self.left + self.margin + column as f64 * (self.width + COLUMN_GAP)
    }

    fn separator_x(&self, boundary: usize) -> f64 {
        self.column_x(boundary) - COLUMN_GAP / 2.0
    }

    fn right(&self) -> f64 {
        self.separator_x(self.columns.max(1))
    }

    fn card(&self, column: usize, y: f64, height: f64) -> Rect {
        Rect::new(self.column_x(column) + CARD_INSET, y, self.width - 2.0 * CARD_INSET, height)
    }
}

fn story_room(column_width: f64) -> f64 {
    column_width - 2.0 * CARD_INSET - 2.0 * TEXT_INSET
}

fn label_margin(map: &Map) -> f64 {
    let widest = map.slices.iter().map(|slice| mono_width(&slice.label, EYEBROW_SIZE, TRACKING)).fold(0.0, f64::max);
    snap_up(widest + 2.0 * LABEL_INSET + COLUMN_GAP / 2.0).max(LABEL_MARGIN)
}

/// One width for every column: 200, or wider when an activity, step or story would not fit.
fn column_width(map: &Map, labels: &[CardLabel], risky: Option<usize>) -> f64 {
    let activities = map.activities.iter().map(|&node| {
        let lines = title_lines(&labels[node].title, COLUMN_WIDTH - 2.0 * TEXT_INSET);
        lines.iter().map(|line| sans_width(line, NAME_SIZE)).fold(0.0, f64::max) + 2.0 * TEXT_INSET
    });
    let steps = map.steps.iter().flatten().map(|&node| sans_width(&labels[node].title, NAME_SIZE) + 2.0 * (TEXT_INSET + CARD_INSET));
    let stories = map.stories.iter().map(|story| {
        let tag = if risky == Some(story.node) { TAG_WIDTH + TAG_INSET } else { 0.0 };
        labels[story.node].text_width(story_room(COLUMN_WIDTH)) + tag + 2.0 * (TEXT_INSET + CARD_INSET)
    });
    snap_up(activities.chain(steps).chain(stories).fold(COLUMN_WIDTH, f64::max))
}

fn activity_figures(card: Rect, title: &str, number: usize) -> Vec<Figure> {
    let mut figures = vec![Figure::rect(card, ACTIVITY_RADIUS, Paint::shape(Tone::Wash(0.05), Tone::Muted, 1.0))];
    let lines = title_lines(title, card.w - 2.0 * TEXT_INSET);
    let (first, eyebrow_drop) = if lines.len() > 1 { (21.0, 17.0) } else { (28.0, 20.0) };
    let mut baseline = card.y + first;
    for line in lines {
        figures.push(Figure::Text(Text::sans(Point::new(card.cx(), baseline), line, NAME_SIZE).weight(600)));
        baseline += NAME_LINE;
    }
    let eyebrow = Point::new(card.cx(), baseline - NAME_LINE + eyebrow_drop);
    figures.push(Figure::Text(Text::mono(eyebrow, format!("ACTIVITY {number}"), EYEBROW_SIZE).tracking(TRACKING)));
    figures
}

fn step_figures(card: Rect, title: &str) -> Vec<Figure> {
    let at = Point::new(card.x + TEXT_INSET, card.y + STEP_BASELINE);
    vec![
        Figure::rect(card, CARD_RADIUS, Paint::shape(Tone::Fill, Tone::Ink, 0.8)),
        Figure::Text(Text::sans(at, title, NAME_SIZE).anchor(Anchor::Start)),
    ]
}

fn story_paint(risky: bool) -> Paint {
    if risky {
        Paint::shape(Tone::AccentWash(0.05), Tone::Accent, 1.0).dashed("4,4")
    } else {
        Paint::shape(Tone::Fill, Tone::Ink, 1.0)
    }
}

fn risk_tag(card: Rect) -> [Figure; 2] {
    let tag = Rect::new(card.right() - TAG_INSET - TAG_WIDTH, card.bottom() - TAG_RISE, TAG_WIDTH, TAG_HEIGHT);
    let text = Text::mono(Point::new(tag.cx(), tag.y + 9.0), RISK_LABEL, EYEBROW_SIZE).tone(Tone::Accent).tracking(0.08);
    [Figure::rect(tag, 2.0, Paint::shape(Tone::None, Tone::Accent, 0.8)), Figure::Text(text)]
}

fn story_figures(card: Rect, label: &CardLabel, risky: bool) -> Vec<Figure> {
    let mut figures = vec![Figure::rect(card, CARD_RADIUS, story_paint(risky))];
    figures.extend(label.figures(card, TEXT_INSET));
    if risky {
        figures.extend(risk_tag(card));
    }
    figures
}

/// The top and bottom of each slice band, and the card of each story inside its band and column.
fn place_stories(map: &Map, grid: &Grid, heights: &[f64], top: f64) -> (Vec<(f64, f64)>, Vec<(usize, Rect)>) {
    let mut bands = Vec::new();
    let mut cards = Vec::new();
    let mut band_top = top;
    for slice in 0..map.slices.len() {
        let mut deepest = STORY_HEIGHT;
        for column in 0..map.columns {
            let mut y = band_top + BAND_PADDING;
            for story in map.cell(slice, column) {
                cards.push((story.node, grid.card(column, y, heights[story.node])));
                y += heights[story.node] + STORY_GAP;
            }
            deepest = deepest.max(y - STORY_GAP - band_top - BAND_PADDING);
        }
        let bottom = band_top + deepest + 2.0 * BAND_PADDING;
        bands.push((band_top, bottom));
        band_top = bottom;
    }
    (bands, cards)
}

fn band_section(slice: &Slice, index: usize, grid: &Grid, (top, bottom): (f64, f64)) -> Section {
    let fill = if index % 2 == 0 { Tone::Wash(0.02) } else { Tone::Transparent };
    let band = Rect::new(grid.left, top, grid.right() - grid.left, bottom - top);
    let at = Point::new(grid.left + LABEL_INSET, (top + bottom) / 2.0 + 3.0);
    let label = Text::mono(at, slice.label.clone(), EYEBROW_SIZE).tracking(TRACKING).anchor(Anchor::Start);
    Section { id: slice.id.clone(), figures: vec![Figure::rect(band, 0.0, Paint::filled(fill)), Figure::Text(label)] }
}

fn separators(grid: &Grid, top: f64, bottom: f64) -> Vec<Figure> {
    let paint = Paint::line(Tone::Wash(0.10), 0.8).dashed("4,4");
    (0..=grid.columns)
        .map(|boundary| {
            let x = grid.separator_x(boundary);
            Figure::line(Point::new(x, top), Point::new(x, bottom), paint)
        })
        .collect()
}

/// The full-width accent rule at `y` with its masked `RELEASE CUT` label at the right end.
fn release_cut(grid: &Grid, y: f64) -> [Figure; 3] {
    let right = grid.right();
    let width = mono_width(CUT_LABEL, EYEBROW_SIZE, CUT_TRACKING) + 2.0 * CUT_MASK_PADDING;
    let mask = Rect::new(right - CUT_END_INSET - width, y - CUT_MASK_HEIGHT / 2.0, width, CUT_MASK_HEIGHT);
    let text = Text::mono(Point::new(mask.cx(), y + 3.0), CUT_LABEL, EYEBROW_SIZE).tone(Tone::Accent).tracking(CUT_TRACKING);
    [
        Figure::line(Point::new(grid.left, y), Point::new(right, y), Paint::line(Tone::Accent, CUT_WIDTH)),
        Figure::rect(mask, 2.0, Paint::filled(Tone::Paper)),
        Figure::Text(text),
    ]
}

fn legend(map: &Map, risky: Option<usize>) -> Vec<LegendKey> {
    let story = (!map.stories.is_empty()).then(|| LegendKey::new(Swatch::Box(story_paint(false)), "STORY"));
    let risk = risky.map(|_| LegendKey::new(Swatch::Box(story_paint(true)), "RISKIEST STORY"));
    let cut = (!map.slices.is_empty()).then(|| LegendKey::new(Swatch::Line(Paint::line(Tone::Accent, CUT_WIDTH), Marker::None), CUT_LABEL));
    let keys: Vec<LegendKey> = [story, risk, cut].into_iter().flatten().collect();
    if keys.len() < 2 {
        Vec::new()
    } else {
        keys
    }
}

/// Lays out `view` as a story map: activities and their steps across the top, a band per release slice
/// below with each story under its activity, and the release cut under the first slice.
pub fn build(view: &View, _params: &Params) -> Scene {
    let (ids, contents) = unique_nodes(view);
    let specs = first_specs(view);
    let map = Map::of(view, &ids);
    let labels: Vec<CardLabel> = specs.iter().map(|spec| CardLabel::of(&spec.label)).collect();
    let risky = map.stories.iter().map(|story| story.node).find(|&node| specs[node].focal);
    let width = column_width(&map, &labels, risky);
    let grid = Grid { left: MARGIN, margin: label_margin(&map), width, columns: map.columns };
    let mut nodes = Vec::new();
    let mut node = |index: usize, rect: Rect, figures: Vec<Figure>| {
        nodes.push(SceneNode { figures, ..SceneNode::new(ids[index].clone(), rect, contents[index].clone()) });
    };
    let top = MARGIN;
    for (column, &activity) in map.activities.iter().enumerate() {
        let card = Rect::new(grid.column_x(column), top, width, ACTIVITY_HEIGHT);
        node(activity, card, activity_figures(card, &labels[activity].title, column + 1));
        for (row, &step) in map.steps[column].iter().enumerate() {
            let card = grid.card(column, top + ACTIVITY_HEIGHT + STEP_TOP + row as f64 * STEP_PITCH, STEP_HEIGHT);
            node(step, card, step_figures(card, &labels[step].title));
        }
    }
    let rows = map.skeleton_rows();
    let skeleton = if rows == 0 { 0.0 } else { STEP_TOP + rows as f64 * STEP_PITCH - (STEP_PITCH - STEP_HEIGHT) };
    let backbone_bottom = top + if map.activities.is_empty() { 0.0 } else { ACTIVITY_HEIGHT };
    let bands_top = if map.activities.is_empty() { top } else { backbone_bottom + skeleton + BAND_GAP };
    let heights: Vec<f64> = labels.iter().map(|label| label.height(story_room(width), STORY_HEIGHT, STORY_ROOM)).collect();
    let (bands, cards) = place_stories(&map, &grid, &heights, bands_top);
    for (story, card) in cards {
        node(story, card, story_figures(card, &labels[story], risky == Some(story)));
    }
    let sections: Vec<Section> =
        map.slices.iter().zip(&bands).enumerate().map(|(index, (slice, &band))| band_section(slice, index, &grid, band)).collect();
    let mut decor = Vec::new();
    if let (Some(first), Some(last)) = (bands.first(), bands.last()) {
        if map.has_gap() {
            decor.extend(separators(&grid, backbone_bottom + SEPARATOR_REACH, last.1));
        }
        decor.extend(release_cut(&grid, first.1));
    }
    let legend = legend(&map, risky);
    Scene { nodes, sections, decor, legend: Some(legend), ..Scene::empty(view.kind) }.measured()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> View {
        serde_json::from_str(include_str!("../../tests/story-map.json")).expect("fixture parses")
    }

    fn rect_of(scene: &Scene, id: &str) -> Rect {
        scene.nodes.iter().find(|node| node.id == id).unwrap_or_else(|| panic!("node {id}")).rect
    }

    fn cut_line(scene: &Scene) -> (f64, f64, f64) {
        let lines: Vec<(f64, f64, f64)> = scene
            .decor
            .iter()
            .filter_map(|figure| match figure {
                Figure::Line { points, paint, .. } if paint.stroke == Tone::Accent => Some((points[0].y, points[0].x, points[1].x)),
                _ => None,
            })
            .collect();
        assert_eq!(lines.len(), 1, "one release cut");
        lines[0]
    }

    fn band(scene: &Scene, index: usize) -> Rect {
        scene.sections[index].figures[0].bounds()
    }

    fn small_map(edges: serde_json::Value) -> View {
        serde_json::from_value(json!({
            "kind": "story-map",
            "nodes": [
                {"id": "a1", "label": "Find"}, {"id": "a2", "label": "Share"}, {"id": "s1", "label": "Search"},
                {"id": "st1", "label": "Keyword search (RPT-1 · 3pt)"}, {"id": "st2", "label": "Public link (RPT-2 · 2pt)"},
                {"id": "st3", "label": "Orphan (RPT-3 · 1pt)"}
            ],
            "edges": edges,
            "groups": [{"id": "mvp", "label": "MVP", "members": ["st1", "st2"]}, {"id": "later", "label": "Later", "members": ["st3"]}]
        }))
        .expect("a view")
    }

    fn ids(nodes: &[usize], view: &View) -> Vec<String> {
        nodes.iter().map(|&node| view.nodes[node].id.clone()).collect()
    }

    #[test]
    fn nodes_in_no_group_without_a_loose_parent_are_the_activities_in_view_order() {
        let view = fixture();
        let (all, _) = unique_nodes(&view);
        let map = Map::of(&view, &all);
        assert_eq!(ids(&map.activities, &view), vec!["capture", "submit", "approve", "reimburse"]);
        assert_eq!(ids(&map.steps[0], &view), vec!["snap", "forward"]);
        assert_eq!(map.columns, 4);
    }

    #[test]
    fn a_story_takes_the_column_of_the_activity_behind_its_step() {
        let view = small_map(json!([
            {"source": "a1", "target": "s1"}, {"source": "s1", "target": "st1"}, {"source": "a2", "target": "st2"}
        ]));
        let (all, _) = unique_nodes(&view);
        let map = Map::of(&view, &all);
        let column = |id: &str| map.stories.iter().find(|story| view.nodes[story.node].id == id).expect("a story").column;
        assert_eq!((column("st1"), column("st2")), (0, 1));
        let scene = build(&view, &Params::default());
        assert_eq!(rect_of(&scene, "st1").cx(), rect_of(&scene, "a1").cx());
        assert_eq!(rect_of(&scene, "st2").cx(), rect_of(&scene, "a2").cx());
    }

    #[test]
    fn a_story_no_activity_reaches_goes_to_an_extra_last_column() {
        let view = small_map(json!([{"source": "a1", "target": "s1"}, {"source": "s1", "target": "st1"}, {"source": "a2", "target": "st2"}]));
        let scene = build(&view, &Params::default());
        assert!(rect_of(&scene, "st3").x > rect_of(&scene, "a2").right());
    }

    #[test]
    fn the_release_cut_runs_right_under_the_first_slice_across_the_map() {
        let scene = build(&fixture(), &Params::default());
        let (y, from, to) = cut_line(&scene);
        let first = band(&scene, 0);
        assert_eq!(y, first.bottom());
        assert_eq!((from, to), (first.x, first.right()));
        let mvp = ["upload", "onepage", "decide", "transfer"];
        assert!(mvp.iter().all(|id| rect_of(&scene, id).bottom() < y));
        assert!(["ocr", "mileage"].iter().all(|id| rect_of(&scene, id).y > y));
    }

    #[test]
    fn every_story_sits_inside_its_slice_band_and_its_activity_column() {
        let view = fixture();
        let scene = build(&view, &Params::default());
        let (all, _) = unique_nodes(&view);
        let map = Map::of(&view, &all);
        for story in &map.stories {
            let card = scene.nodes.iter().find(|node| node.id == all[story.node]).expect("a card").rect;
            let slice = band(&scene, story.slice);
            assert!(card.y > slice.y && card.bottom() < slice.bottom(), "{} outside its band", all[story.node]);
            let activity = rect_of(&scene, &all[map.activities[story.column]]);
            assert!(card.x > activity.x && card.right() < activity.right(), "{} outside its column", all[story.node]);
        }
        for (index, a) in scene.nodes.iter().enumerate() {
            for b in &scene.nodes[index + 1..] {
                assert!(!a.rect.overlaps(&b.rect), "{} overlaps {}", a.id, b.id);
            }
        }
    }

    #[test]
    fn two_stories_of_one_slice_and_activity_stack_inside_the_band() {
        let scene = build(&fixture(), &Params::default());
        let (upper, lower) = (rect_of(&scene, "ocr"), rect_of(&scene, "inbox"));
        assert_eq!(upper.x, lower.x);
        assert_eq!(lower.y, upper.bottom() + STORY_GAP);
        assert!(lower.bottom() < band(&scene, 1).bottom());
    }

    #[test]
    fn only_the_first_focal_story_is_accented_and_tagged_as_the_risk() {
        let mut view = fixture();
        view.nodes.iter_mut().filter(|node| node.id == "status" || node.id == "capture").for_each(|node| node.focal = true);
        let scene = build(&view, &Params::default());
        let accented: Vec<&str> = scene
            .nodes
            .iter()
            .filter(|node| matches!(&node.figures[0], Figure::Rect { paint, .. } if paint.stroke == Tone::Accent))
            .map(|node| node.id.as_str())
            .collect();
        assert_eq!(accented, vec!["ocr"]);
        let ocr = scene.nodes.iter().find(|node| node.id == "ocr").expect("the risky story");
        assert!(ocr.figures.iter().any(|figure| matches!(figure, Figure::Text(text) if text.text == RISK_LABEL)));
    }

    #[test]
    fn column_separators_appear_only_when_a_slice_has_a_gap() {
        let dashed = |scene: &Scene| scene.decor.iter().filter(|figure| matches!(figure, Figure::Line { paint, .. } if paint.dash.is_some())).count();
        assert_eq!(dashed(&build(&fixture(), &Params::default())), 5, "four columns, five separators");
        let full: View = serde_json::from_value(json!({
            "kind": "story-map",
            "nodes": [
                {"id": "a1", "label": "Find"}, {"id": "a2", "label": "Share"},
                {"id": "st1", "label": "One"}, {"id": "st2", "label": "Two"}, {"id": "st3", "label": "Three"}, {"id": "st4", "label": "Four"}
            ],
            "edges": [
                {"source": "a1", "target": "st1"}, {"source": "a2", "target": "st2"},
                {"source": "a1", "target": "st3"}, {"source": "a2", "target": "st4"}
            ],
            "groups": [{"id": "mvp", "members": ["st1", "st2"]}, {"id": "later", "members": ["st3", "st4"]}]
        }))
        .expect("a view");
        assert_eq!(dashed(&build(&full, &Params::default())), 0);
    }

    #[test]
    fn the_skeleton_keeps_room_for_two_steps_under_every_activity() {
        let scene = build(&fixture(), &Params::default());
        let second_step = rect_of(&scene, "forward");
        let lone_step = rect_of(&scene, "review");
        assert_eq!(second_step.y, lone_step.y + STEP_PITCH);
        assert!(band(&scene, 0).y >= second_step.bottom() + BAND_GAP);
    }

    #[test]
    fn a_story_map_draws_no_edges_and_names_its_looks() {
        let scene = build(&fixture(), &Params::default());
        assert!(scene.edges.is_empty());
        let legend: Vec<String> = scene.legend.expect("a legend").iter().map(|key| key.text.clone()).collect();
        assert_eq!(legend, vec!["STORY", "RISKIEST STORY", CUT_LABEL]);
        let ids: Vec<&str> = scene.sections.iter().map(|section| section.id.as_str()).collect();
        assert_eq!(ids, vec!["mvp", "r2", "later"]);
    }
}
