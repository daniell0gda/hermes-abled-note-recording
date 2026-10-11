//! Venn: up to three equal set circles. Groups are the sets; every item sits in the region its group
//! membership names (A only, A∩B, A∩B∩C, ...), and items in no set form a row below the circles.

use std::collections::BTreeMap;

use crate::figure::{bounds_of, Anchor, Figure, Paint, Section, Text, Tone};
use crate::geom::{snap, snap_up, Point, Rect};
use crate::node::{name_and_sublabel, NodeContent, Treatment, NAME_LINE, NAME_SIZE, SUB_LINE, SUB_SIZE};
use crate::params::Params;
use crate::scene::{unique_nodes, Scene, SceneNode};
use crate::spec::{GroupSpec, View};
use crate::text::{mono_width, sans_width};

const MAX_SETS: usize = 3;
const MIN_RADIUS: f64 = 96.0;
const MAX_RADIUS: f64 = 480.0;
const RADIUS_STEP: f64 = 8.0;
const OVERLAPS: [f64; 6] = [1.0, 0.9, 1.1, 0.8, 1.2, 0.7];
const SCAN_STEP: f64 = 8.0;
const CLEARANCE: f64 = 4.0;
const ITEM_PADDING_X: f64 = 8.0;
const ITEM_PADDING_Y: f64 = 4.0;
const ITEM_GAP: f64 = 4.0;
const ITEM_WIDTH_STEP: f64 = 8.0;
const FIRST_BASELINE: f64 = 11.0;
const TAG_RADIUS: f64 = 6.0;
const SET_NAME_SIZE: f64 = 13.0;
const SET_SUB_SIZE: f64 = 9.0;
const SET_SUB_LINE: f64 = 13.0;
const SET_LABEL_GAP: f64 = 12.0;
const SET_LABEL_SPACING: f64 = 16.0;
const ROW_GAP: f64 = 24.0;
const OUTSIDE_GAP: f64 = 32.0;
const SET_STROKES: [Tone; MAX_SETS] = [Tone::Ink, Tone::Muted, Tone::Soft];
const SET_FILLS: [Tone; MAX_SETS] = [Tone::Wash(0.04), Tone::MutedWash(0.05), Tone::MutedWash(0.03)];

#[derive(Clone, Copy, Debug, PartialEq)]
struct Circle {
    centre: Point,
    radius: f64,
}

impl Circle {
    fn farthest(&self, rect: &Rect) -> f64 {
        let dx = (self.centre.x - rect.x).abs().max((rect.right() - self.centre.x).abs());
        let dy = (self.centre.y - rect.y).abs().max((rect.bottom() - self.centre.y).abs());
        dx.hypot(dy)
    }

    fn nearest(&self, rect: &Rect) -> f64 {
        let dx = (rect.x - self.centre.x).max(self.centre.x - rect.right()).max(0.0);
        let dy = (rect.y - self.centre.y).max(self.centre.y - rect.bottom()).max(0.0);
        dx.hypot(dy)
    }

    /// How far `rect` stays inside the circle (`inside`) or outside it; negative when it crosses the stroke.
    fn clearance(&self, rect: &Rect, inside: bool) -> f64 {
        if inside {
            self.radius - self.farthest(rect)
        } else {
            self.nearest(rect) - self.radius
        }
    }
}

/// The set bits of `region` name the circles an item is inside of; it is outside all the others.
fn clearance(circles: &[Circle], region: u8, rect: &Rect) -> f64 {
    circles.iter().enumerate().map(|(set, circle)| circle.clearance(rect, region & (1 << set) != 0)).fold(f64::INFINITY, f64::min)
}

/// The region an item belongs to: one bit for every set whose members list it.
fn region_of(sets: &[&GroupSpec], id: &str) -> u8 {
    sets.iter().enumerate().filter(|(_, set)| set.members.iter().any(|member| member == id)).fold(0, |region, (set, _)| region | (1 << set))
}

/// Equal circles `overlap * radius` apart: one, a pair side by side, or three on a triangle, the first on top.
fn arrangement(count: usize, radius: f64, overlap: f64) -> Vec<Circle> {
    let distance = overlap * radius;
    let spread = distance / 3.0_f64.sqrt();
    let centres = match count {
        1 => vec![Point::new(0.0, 0.0)],
        2 => vec![Point::new(-distance / 2.0, 0.0), Point::new(distance / 2.0, 0.0)],
        _ => {
            let side = spread * 30.0_f64.to_radians().cos();
            vec![Point::new(0.0, -spread), Point::new(-side, spread / 2.0), Point::new(side, spread / 2.0)]
        }
    };
    centres.into_iter().map(|centre| Circle { centre: Point::new(snap(centre.x), snap(centre.y)), radius }).collect()
}

fn centroid(circles: &[Circle]) -> Point {
    let count = circles.len() as f64;
    let (x, y) = circles.iter().fold((0.0, 0.0), |(x, y), circle| (x + circle.centre.x, y + circle.centre.y));
    Point::new(x / count, y / count)
}

/// The direction from the middle of the circles towards `region`: the sum of the directions to its circles,
/// which is the region's axis of symmetry. None for the region every circle shares.
fn region_axis(circles: &[Circle], region: u8) -> Option<(f64, f64)> {
    let middle = centroid(circles);
    let (x, y) = circles
        .iter()
        .enumerate()
        .filter(|(set, _)| region & (1 << set) != 0)
        .fold((0.0, 0.0), |(x, y), (_, circle)| (x + circle.centre.x - middle.x, y + circle.centre.y - middle.y));
    let length = x.hypot(y);
    (length > 1.0).then(|| (x / length, y / length))
}

/// The item blocks of one region stacked around `centre`, top to bottom.
fn stacked(sizes: &[(f64, f64)], centre: Point) -> impl Iterator<Item = Rect> + '_ {
    let total = sizes.iter().map(|(_, height)| height).sum::<f64>() + ITEM_GAP * sizes.len().saturating_sub(1) as f64;
    let mut top = snap(centre.y - total / 2.0);
    sizes.iter().map(move |&(width, height)| {
        let rect = Rect::new(centre.x - width / 2.0, top, width, height);
        top += height + ITEM_GAP;
        rect
    })
}

/// The best spot along the region's axis for its stack, and how far the stack stays clear of every stroke there.
fn place_region(circles: &[Circle], region: u8, sizes: &[(f64, f64)]) -> (Vec<Rect>, f64) {
    let middle = centroid(circles);
    let reach = circles.iter().map(|circle| circle.radius + (circle.centre.x - middle.x).hypot(circle.centre.y - middle.y)).fold(0.0, f64::max);
    let axis = region_axis(circles, region);
    let steps = axis.map_or(0, |_| (reach / SCAN_STEP) as usize);
    let (dx, dy) = axis.unwrap_or((0.0, 0.0));
    let spot = |step: usize| {
        let along = step as f64 * SCAN_STEP;
        Point::new(snap(middle.x + dx * along), snap(middle.y + dy * along))
    };
    let (best, clear) = (0..=steps)
        .map(|step| {
            let clear = stacked(sizes, spot(step)).map(|rect| clearance(circles, region, &rect)).fold(f64::INFINITY, f64::min);
            (step, clear)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((0, f64::NEG_INFINITY));
    (stacked(sizes, spot(best)).collect(), clear)
}

type Regions = BTreeMap<u8, Vec<usize>>;

/// The item rects of every region for one arrangement of circles, and whether all of them fit.
struct Placement {
    rects: BTreeMap<u8, Vec<Rect>>,
    fits: bool,
}

fn place_all(circles: &[Circle], regions: &Regions, sizes: &[(f64, f64)]) -> Placement {
    let mut fits = true;
    let rects = regions
        .iter()
        .map(|(&region, items)| {
            let own: Vec<(f64, f64)> = items.iter().map(|&item| sizes[item]).collect();
            let (rects, clear) = place_region(circles, region, &own);
            fits &= clear >= CLEARANCE;
            (region, rects)
        })
        .collect();
    Placement { rects, fits }
}

fn radius_of(step: usize) -> f64 {
    MIN_RADIUS + step as f64 * RADIUS_STEP
}

/// The smallest radius step at which `overlap` holds every region's items, by bisection.
fn smallest_radius_step(count: usize, overlap: f64, regions: &Regions, sizes: &[(f64, f64)]) -> Option<usize> {
    let fits = |step: usize| place_all(&arrangement(count, radius_of(step), overlap), regions, sizes).fits;
    let last = ((MAX_RADIUS - MIN_RADIUS) / RADIUS_STEP) as usize;
    if !fits(last) {
        return None;
    }
    let (mut low, mut high) = (0, last);
    while low < high {
        let middle = (low + high) / 2;
        if fits(middle) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    Some(high)
}

/// The smallest circles, over the overlaps tried, whose regions hold all their items; the largest
/// circles at the default overlap when none do.
fn fit(count: usize, regions: &Regions, sizes: &[(f64, f64)]) -> (Vec<Circle>, BTreeMap<u8, Vec<Rect>>) {
    let best = OVERLAPS
        .iter()
        .filter_map(|&overlap| smallest_radius_step(count, overlap, regions, sizes).map(|step| (step, overlap)))
        .min_by_key(|&(step, _)| step);
    let circles = match best {
        Some((step, overlap)) => arrangement(count, radius_of(step), overlap),
        None => arrangement(count, MAX_RADIUS, OVERLAPS[0]),
    };
    let rects = place_all(&circles, regions, sizes).rects;
    (circles, rects)
}

fn item_size(content: &NodeContent) -> (f64, f64) {
    let widest = content.lines.iter().map(|line| sans_width(line, NAME_SIZE)).fold(0.0, f64::max);
    let sub = content.sublabel.as_deref().map_or(0.0, |sub| mono_width(sub, SUB_SIZE, 0.0));
    let width = ((widest.max(sub) + 2.0 * ITEM_PADDING_X) / ITEM_WIDTH_STEP).ceil() * ITEM_WIDTH_STEP;
    (width, snap_up(content.block_height() + 2.0 * ITEM_PADDING_Y))
}

fn item_texts(content: &NodeContent, rect: &Rect, focal: bool) -> Vec<Figure> {
    let mut baseline = rect.y + (rect.h - content.block_height()) / 2.0 + FIRST_BASELINE;
    let name_tone = if focal { Tone::Accent } else { Tone::Ink };
    let mut texts = Vec::new();
    for line in &content.lines {
        texts.push(Figure::Text(Text::sans(Point::new(rect.cx(), baseline), line.clone(), NAME_SIZE).weight(600).tone(name_tone)));
        baseline += NAME_LINE;
    }
    if let Some(sub) = &content.sublabel {
        texts.push(Figure::Text(Text::mono(Point::new(rect.cx(), baseline - NAME_LINE + SUB_LINE), sub.clone(), SUB_SIZE)));
    }
    texts
}

fn item_node(id: String, content: NodeContent, rect: Rect, focal: bool) -> SceneNode {
    let shape = if focal { Figure::rect(rect, TAG_RADIUS, Paint::filled(Tone::AccentWash(0.14))) } else { Figure::hit_area(rect) };
    let figures = std::iter::once(shape).chain(item_texts(&content, &rect, focal)).collect();
    SceneNode { figures, ..SceneNode::new(id, rect, content) }
}

/// Where a set's label sits: above its circle, or beside it at the height of its centre.
#[derive(Clone, Copy, PartialEq)]
enum Side {
    Above(Anchor),
    Left,
    Right,
}

fn label_sides(sets: &[&GroupSpec], circles: &[Circle]) -> Vec<Side> {
    match circles.len() {
        3 => vec![Side::Above(Anchor::Middle), Side::Left, Side::Right],
        2 => {
            let widths: Vec<f64> = sets.iter().map(|set| set_label_width(set)).collect();
            let room = circles[1].centre.x - circles[0].centre.x;
            if (widths[0] + widths[1]) / 2.0 + SET_LABEL_SPACING <= room {
                vec![Side::Above(Anchor::Middle); 2]
            } else {
                vec![Side::Above(Anchor::End), Side::Above(Anchor::Start)]
            }
        }
        _ => vec![Side::Above(Anchor::Middle)],
    }
}

fn set_name(set: &GroupSpec) -> (String, Option<String>) {
    name_and_sublabel(if set.label.is_empty() { &set.id } else { &set.label })
}

fn set_label_width(set: &GroupSpec) -> f64 {
    let (name, sub) = set_name(set);
    sans_width(&name, SET_NAME_SIZE).max(sub.map_or(0.0, |sub| mono_width(&sub, SET_SUB_SIZE, 0.0)))
}

fn set_label(set: &GroupSpec, circle: &Circle, side: Side, middle_x: f64) -> Vec<Figure> {
    let (name, sub) = set_name(set);
    let sub_room = if sub.is_some() { SET_SUB_LINE } else { 0.0 };
    let (at, anchor) = match side {
        Side::Above(anchor) => {
            let x = match anchor {
                Anchor::Middle => circle.centre.x,
                Anchor::End => middle_x - SET_LABEL_SPACING / 2.0,
                Anchor::Start => middle_x + SET_LABEL_SPACING / 2.0,
            };
            (Point::new(x, circle.centre.y - circle.radius - SET_LABEL_GAP - sub_room), anchor)
        }
        Side::Left => (Point::new(circle.centre.x - circle.radius - SET_LABEL_GAP, circle.centre.y + 4.0 - sub_room / 2.0), Anchor::End),
        Side::Right => (Point::new(circle.centre.x + circle.radius + SET_LABEL_GAP, circle.centre.y + 4.0 - sub_room / 2.0), Anchor::Start),
    };
    let name = Text::sans(at, name, SET_NAME_SIZE).weight(600).anchor(anchor);
    let sub = sub.map(|sub| Text::mono(Point::new(at.x, at.y + SET_SUB_LINE), sub, SET_SUB_SIZE).anchor(anchor));
    std::iter::once(name).chain(sub).map(Figure::Text).collect()
}

fn set_sections(sets: &[&GroupSpec], circles: &[Circle]) -> Vec<Section> {
    let middle_x = centroid(circles).x;
    sets.iter()
        .zip(circles)
        .zip(label_sides(sets, circles))
        .enumerate()
        .map(|(index, ((set, circle), side))| {
            let shape = Figure::circle(circle.centre, circle.radius, Paint::shape(SET_FILLS[index], SET_STROKES[index], 1.0));
            Section { id: set.id.clone(), figures: std::iter::once(shape).chain(set_label(set, circle, side, middle_x)).collect() }
        })
        .collect()
}

/// The items in no set, side by side below everything else.
fn outside_row(sizes: &[(f64, f64)], top: f64, middle_x: f64) -> Vec<Rect> {
    let total = sizes.iter().map(|(width, _)| width).sum::<f64>() + ROW_GAP * sizes.len().saturating_sub(1) as f64;
    let mut left = snap(middle_x - total / 2.0);
    sizes
        .iter()
        .map(|&(width, height)| {
            let rect = Rect::new(left, top, width, height);
            left += width + ROW_GAP;
            rect
        })
        .collect()
}

/// Lays out `view` as a Venn diagram: the first three groups are the sets, edges are not drawn.
pub fn build(view: &View, _params: &Params) -> Scene {
    let (ids, contents) = unique_nodes(view);
    let sets: Vec<&GroupSpec> = view.groups.iter().take(MAX_SETS).collect();
    let sizes: Vec<(f64, f64)> = contents.iter().map(item_size).collect();
    let mut regions = Regions::new();
    for (item, id) in ids.iter().enumerate() {
        regions.entry(region_of(&sets, id)).or_default().push(item);
    }
    let outside = regions.remove(&0).unwrap_or_default();
    let (circles, placed) = if sets.is_empty() { (Vec::new(), BTreeMap::new()) } else { fit(sets.len(), &regions, &sizes) };
    let sections = set_sections(&sets, &circles);
    let mut rects: Vec<Option<Rect>> = vec![None; ids.len()];
    for (region, items) in &regions {
        for (&item, &rect) in items.iter().zip(&placed[region]) {
            rects[item] = Some(rect);
        }
    }
    let above = bounds_of(sections.iter().flat_map(|section| &section.figures));
    let row_top = above.map_or(0.0, |rect| snap_up(rect.bottom() + OUTSIDE_GAP));
    let middle_x = if circles.is_empty() { 0.0 } else { centroid(&circles).x };
    let outside_sizes: Vec<(f64, f64)> = outside.iter().map(|&item| sizes[item]).collect();
    for (&item, rect) in outside.iter().zip(outside_row(&outside_sizes, row_top, middle_x)) {
        rects[item] = Some(rect);
    }
    let focal = contents.iter().position(|content| content.treatment == Treatment::Focal);
    let nodes = ids
        .into_iter()
        .zip(contents)
        .zip(rects)
        .enumerate()
        .map(|(index, ((id, content), rect))| item_node(id, content, rect.unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)), focal == Some(index)))
        .collect();
    Scene { nodes, sections, legend: Some(Vec::new()), ..Scene::empty(view.kind) }.measured()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn venn() -> View {
        serde_json::from_str(include_str!("../../tests/venn.json")).expect("fixture parses")
    }

    fn circles_of(scene: &Scene) -> Vec<Circle> {
        scene
            .sections
            .iter()
            .map(|section| match section.figures[0] {
                Figure::Circle { centre, radius, .. } => Circle { centre, radius },
                ref other => panic!("a set is a circle, not {other:?}"),
            })
            .collect()
    }

    fn node<'a>(scene: &'a Scene, id: &str) -> &'a SceneNode {
        scene.nodes.iter().find(|node| node.id == id).expect("a known node")
    }

    #[test]
    fn an_items_region_is_the_sets_that_list_it() {
        let view = venn();
        let sets: Vec<&GroupSpec> = view.groups.iter().collect();
        assert_eq!(region_of(&sets, "science"), 0b111);
        assert_eq!(region_of(&sets, "ml"), 0b011);
        assert_eq!(region_of(&sets, "research"), 0b101);
        assert_eq!(region_of(&sets, "danger"), 0b110);
        assert_eq!(region_of(&sets, "tests"), 0b001);
        assert_eq!(region_of(&sets, "decks"), 0);
    }

    #[test]
    fn every_item_is_inside_its_sets_and_outside_the_others() {
        let view = venn();
        let scene = build(&view, &Params::default());
        let circles = circles_of(&scene);
        let sets: Vec<&GroupSpec> = view.groups.iter().collect();
        for node in scene.nodes.iter().filter(|node| region_of(&sets, &node.id) != 0) {
            assert!(clearance(&circles, region_of(&sets, &node.id), &node.rect) >= CLEARANCE, "{}", node.id);
        }
    }

    #[test]
    fn an_item_in_no_set_sits_below_the_circles() {
        let scene = build(&venn(), &Params::default());
        let lowest = circles_of(&scene).iter().map(|circle| circle.centre.y + circle.radius).fold(f64::MIN, f64::max);
        let decks = node(&scene, "decks");
        assert!(decks.rect.y > lowest);
        assert!(circles_of(&scene).iter().all(|circle| circle.clearance(&decks.rect, false) > 0.0));
    }

    #[test]
    fn set_labels_sit_outside_every_circle() {
        let scene = build(&venn(), &Params::default());
        let circles = circles_of(&scene);
        for section in &scene.sections {
            for label in &section.figures[1..] {
                assert!(clearance(&circles, 0, &label.bounds()) > 0.0, "{} label crosses a stroke", section.id);
            }
        }
    }

    #[test]
    fn no_two_labels_or_items_overlap() {
        let scene = build(&venn(), &Params::default());
        let boxes: Vec<Rect> = scene
            .nodes
            .iter()
            .map(|node| node.rect)
            .chain(scene.sections.iter().flat_map(|section| section.figures[1..].iter().map(Figure::bounds)))
            .collect();
        for (index, a) in boxes.iter().enumerate() {
            assert!(boxes[index + 1..].iter().all(|b| !a.overlaps(b)), "{a:?} overlaps");
        }
    }

    #[test]
    fn several_items_in_one_region_stack_without_overlapping() {
        let mut view = venn();
        view.groups[0].members.extend(["decks".to_string(), "sizing".to_string()]);
        view.groups[2].members.retain(|member| member != "sizing");
        let scene = build(&view, &Params::default());
        let circles = circles_of(&scene);
        let only_first: Vec<&SceneNode> = ["tests", "sizing", "decks"].iter().map(|id| node(&scene, id)).collect();
        for item in &only_first {
            assert!(clearance(&circles, 0b001, &item.rect) >= CLEARANCE, "{}", item.id);
        }
        assert!(only_first.windows(2).all(|pair| pair[0].rect.bottom() < pair[1].rect.y));
    }

    #[test]
    fn the_focal_item_is_the_only_accent() {
        let scene = build(&venn(), &Params::default());
        let accented: Vec<&str> = scene
            .nodes
            .iter()
            .filter(|node| node.figures.iter().any(|figure| matches!(figure, Figure::Text(text) if text.tone == Tone::Accent)))
            .map(|node| node.id.as_str())
            .collect();
        assert_eq!(accented, vec!["science"]);
        assert!(matches!(node(&scene, "science").figures[0], Figure::Rect { paint, .. } if paint.fill == Tone::AccentWash(0.14)));
    }

    #[test]
    fn every_set_is_a_selectable_section_with_the_group_id() {
        let mut view = venn();
        view.selected = vec!["code".to_string()];
        let svg = crate::render(&view, &Params::default()).svg;
        assert!(svg.contains(r#"<g class="sk-group" data-id="stats"><circle class="sk-shape""#));
        assert!(svg.contains(r#"<g class="sk-group is-selected" data-id="code"><circle class="sk-shape""#));
        assert!(svg.contains(r#"<g class="sk-group" data-id="domain"><circle class="sk-shape""#));
    }

    #[test]
    fn only_the_first_three_groups_become_circles() {
        let mut view = venn();
        view.groups.push(GroupSpec { id: "extra".into(), label: "Extra".into(), members: vec!["decks".into()] });
        let scene = build(&view, &Params::default());
        assert_eq!(scene.sections.len(), 3);
        assert!(circles_of(&scene).iter().all(|circle| circle.clearance(&node(&scene, "decks").rect, false) > 0.0));
    }

    #[test]
    fn one_group_draws_one_circle_and_no_groups_a_plain_row() {
        let mut view = venn();
        view.groups.truncate(1);
        let scene = build(&view, &Params::default());
        assert_eq!(scene.sections.len(), 1);
        let circle = circles_of(&scene)[0];
        assert!(circle.clearance(&node(&scene, "ml").rect, true) >= CLEARANCE);
        assert!(circle.clearance(&node(&scene, "danger").rect, false) > 0.0);
        view.groups.clear();
        let row = build(&view, &Params::default());
        assert!(row.sections.is_empty());
        assert!(row.nodes.windows(2).all(|pair| pair[0].rect.y == pair[1].rect.y && pair[0].rect.right() < pair[1].rect.x));
    }

    #[test]
    fn two_sets_place_the_shared_items_in_the_overlap() {
        let mut view = venn();
        view.groups.truncate(2);
        let scene = build(&view, &Params::default());
        let circles = circles_of(&scene);
        assert_eq!(circles.len(), 2);
        assert!(clearance(&circles, 0b11, &node(&scene, "ml").rect) >= CLEARANCE);
        assert!(clearance(&circles, 0b10, &node(&scene, "etl").rect) >= CLEARANCE);
    }
}
