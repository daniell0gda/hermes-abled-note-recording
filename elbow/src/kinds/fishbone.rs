//! Fishbone: causes on bones that run into one effect. Edges point from cause to effect: the effect is the
//! node without outgoing edges that most edges reach, the categories are the nodes with an edge into it
//! (one 60° bone each, alternating above and below the spine), and the sub-causes the nodes with an edge
//! into a category or into another sub-cause (horizontal ticks on that category's bone). Nodes that reach
//! no category stand in a row below.

use super::cycle_loop::exit_point;
use crate::figure::{Anchor, Figure, LegendKey, Marker, Paint, Swatch, Text, Tone};
use crate::geom::{snap_up, Point, Rect};
use crate::node::{NodeContent, Treatment, TAG_ROW};
use crate::params::Params;
use crate::scene::{known_edges, unique_nodes, Bend, EdgeInput, Scene, SceneEdge, SceneNode};
use crate::spec::View;
use crate::text::{mono_width, sans_width};

const HEAD_GAP: f64 = 224.0;
const MIN_SPACING: f64 = 160.0;
const UNIT_RISE: f64 = 28.0;
const UNIT_RUN: f64 = 16.0;
const MIN_UNITS: usize = 6;
const TICK: f64 = 32.0;
const TICK_TEXT_GAP: f64 = 4.0;
const CAUSE_SIZE: f64 = 9.0;
const CAUSE_RAISE: f64 = 4.0;
const CAUSE_DROP: f64 = 12.0;
const CAUSE_CLEARANCE: f64 = 12.0;
const TAG_HEIGHT: f64 = 28.0;
const TAG_HEIGHT_WITH_SUB: f64 = 40.0;
const TAG_MIN_WIDTH: f64 = 120.0;
const TAG_PADDING: f64 = 16.0;
const TAG_GAP: f64 = 8.0;
const TAG_NAME_SIZE: f64 = 12.0;
const TAG_SUB_SIZE: f64 = 9.0;
const TAG_RADIUS: f64 = 4.0;
const TAG_STROKE: f64 = 0.8;
const SPINE_TAIL: f64 = 64.0;
const EFFECT_WIDTH: f64 = 200.0;
const EFFECT_HEIGHT: f64 = 96.0;
const EFFECT_TAG: &str = "EFFECT";
const LOOSE_GAP: f64 = 64.0;
const LOOSE_SPACING: f64 = 24.0;
const LOOSE_CAPTION: &str = "NOT ON A BONE";
const CAPTION_SIZE: f64 = 8.0;
const CAPTION_TRACKING: f64 = 0.14;
const CAPTION_RAISE: f64 = 12.0;
const BONE_WIDTH: f64 = 1.2;
const FOCAL_WIDTH: f64 = 1.4;
const TICK_WIDTH: f64 = 1.0;
const OTHER_WIDTH: f64 = 1.0;
const DOUBLE_GRID: f64 = 8.0;

/// What each node of the view is in the fishbone.
struct Bones {
    effect: usize,
    categories: Vec<usize>,
    causes: Vec<Vec<usize>>,
    loose: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum NodeRole {
    Effect,
    Category(usize),
    Cause(usize, usize),
    Loose,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum EdgeRole {
    Bone(usize),
    Tick(usize, usize),
    Other,
}

impl Bones {
    fn of(count: usize, pairs: &[(usize, usize)]) -> Option<Bones> {
        let effect = effect_of(count, pairs)?;
        let categories: Vec<usize> = (0..count).filter(|&node| node != effect && pairs.contains(&(node, effect))).collect();
        let mut bones = Bones { effect, causes: vec![Vec::new(); categories.len()], categories, loose: Vec::new() };
        let mut open: Vec<usize> = (0..count).filter(|&node| bones.role(node) == NodeRole::Loose).collect();
        while let Some(found) = open.iter().position(|&node| bones.bone_reached(node, pairs).is_some()) {
            let node = open.remove(found);
            if let Some(bone) = bones.bone_reached(node, pairs) {
                bones.causes[bone].push(node);
            }
        }
        bones.loose = open;
        Some(bones)
    }

    /// The bone a node's first edge into a category, or into a sub-cause already on a bone, reaches.
    fn bone_reached(&self, node: usize, pairs: &[(usize, usize)]) -> Option<usize> {
        pairs.iter().filter(|&&(source, _)| source == node).find_map(|&(_, target)| match self.role(target) {
            NodeRole::Category(bone) | NodeRole::Cause(bone, _) => Some(bone),
            NodeRole::Effect | NodeRole::Loose => None,
        })
    }

    fn role(&self, node: usize) -> NodeRole {
        if node == self.effect {
            return NodeRole::Effect;
        }
        if let Some(bone) = self.categories.iter().position(|&category| category == node) {
            return NodeRole::Category(bone);
        }
        self.causes
            .iter()
            .enumerate()
            .find_map(|(bone, causes)| causes.iter().position(|&cause| cause == node).map(|slot| NodeRole::Cause(bone, slot)))
            .unwrap_or(NodeRole::Loose)
    }

    /// The bone or tick an edge draws, once per category and sub-cause; any other edge is `Other`.
    fn edge_roles(&self, edges: &[EdgeInput]) -> Vec<EdgeRole> {
        let mut drawn: Vec<usize> = Vec::new();
        edges
            .iter()
            .map(|edge| {
                let role = match self.role(edge.source) {
                    NodeRole::Category(bone) if edge.target == self.effect => EdgeRole::Bone(bone),
                    NodeRole::Cause(bone, slot) if self.bone_reached(edge.source, &[(edge.source, edge.target)]) == Some(bone) => EdgeRole::Tick(bone, slot),
                    _ => EdgeRole::Other,
                };
                if role == EdgeRole::Other || drawn.contains(&edge.source) {
                    return EdgeRole::Other;
                }
                drawn.push(edge.source);
                role
            })
            .collect()
    }
}

/// The node without outgoing edges that most edges reach, the first in view order on a tie; when every
/// node has outgoing edges, the one most edges reach.
fn effect_of(count: usize, pairs: &[(usize, usize)]) -> Option<usize> {
    let outgoing = |node: usize| pairs.iter().filter(|&&(source, target)| source == node && target != node).count();
    let incoming = |node: usize| pairs.iter().filter(|&&(source, target)| target == node && source != node).count();
    let sinks: Vec<usize> = (0..count).filter(|&node| outgoing(node) == 0).collect();
    let pool = if sinks.is_empty() { (0..count).collect() } else { sinks };
    pool.into_iter().fold(None, |best, node| match best {
        Some(best) if incoming(best) >= incoming(node) => Some(best),
        _ => Some(node),
    })
}

/// The bones' shared geometry: the effect box's left edge sits at x = 0 on the spine at y = 0; every
/// bone is `units` steps of 28 down (or up) and 16 left, which is 60° to the spine.
struct Skeleton {
    units: usize,
    spacing: f64,
}

impl Skeleton {
    fn of(bones: &Bones, contents: &[NodeContent]) -> Skeleton {
        let most_causes = bones.causes.iter().map(Vec::len).max().unwrap_or(0);
        let units = MIN_UNITS.max(2 * most_causes + 2);
        let widest_cause = bones.causes.iter().flatten().map(|&cause| cause_width(&contents[cause])).fold(0.0, f64::max);
        let widest_tag = bones.categories.iter().map(|&category| tag_size(&contents[category]).0).fold(0.0, f64::max);
        let for_causes = (widest_cause + TICK + TICK_TEXT_GAP + CAUSE_CLEARANCE) / 2.0;
        let for_tags = (widest_tag + TAG_PADDING) / 2.0;
        Skeleton { units, spacing: snap_up(MIN_SPACING.max(for_causes).max(for_tags)) }
    }

    fn attach(&self, bone: usize) -> Point {
        Point::new(-HEAD_GAP - bone as f64 * self.spacing, 0.0)
    }

    fn far(&self, bone: usize) -> Point {
        self.along(bone, self.units as f64)
    }

    /// The point `steps` units out along a bone from where it meets the spine.
    fn along(&self, bone: usize, steps: f64) -> Point {
        let attach = self.attach(bone);
        Point::new(attach.x - steps * UNIT_RUN, side(bone) * steps * UNIT_RISE)
    }

    /// Where the tick of sub-cause `slot` of `count` meets its bone: spread evenly, on whole steps.
    fn tick(&self, bone: usize, slot: usize, count: usize) -> Point {
        let steps = (self.units as f64 * (slot + 1) as f64 / (count + 1) as f64).round();
        self.along(bone, steps)
    }

    fn spine_start(&self, bones: usize) -> f64 {
        self.attach(bones.saturating_sub(1)).x - SPINE_TAIL
    }
}

/// -1 for the bones above the spine (the first, third, ...), +1 for those below.
fn side(bone: usize) -> f64 {
    if bone % 2 == 0 {
        -1.0
    } else {
        1.0
    }
}

/// Lays out `view` as a fishbone.
pub fn build(view: &View, _params: &Params) -> Scene {
    let (ids, contents) = unique_nodes(view);
    let edges = known_edges(view, &ids);
    let pairs: Vec<(usize, usize)> = edges.iter().map(|edge| (edge.source, edge.target)).collect();
    let Some(bones) = Bones::of(ids.len(), &pairs) else { return Scene::empty(view.kind).measured() };
    let skeleton = Skeleton::of(&bones, &contents);
    let spine = spine(&skeleton, bones.categories.len());
    let mut placed: Vec<Option<SceneNode>> = ids
        .iter()
        .zip(&contents)
        .enumerate()
        .map(|(index, (id, content))| placed_node(id, content, bones.role(index), &bones, &skeleton))
        .collect();
    let (loose, caption) = loose_row(&bones.loose, &ids, &contents, &placed, &spine);
    for (&index, node) in bones.loose.iter().zip(loose) {
        placed[index] = Some(node);
    }
    let nodes: Vec<SceneNode> = placed.into_iter().flatten().collect();
    let roles = bones.edge_roles(&edges);
    let scene_edges = edges.iter().zip(&roles).map(|(edge, role)| scene_edge(edge, *role, &bones, &skeleton, &nodes)).collect();
    let legend = Some(legend(&bones, &nodes, &roles));
    let decor = std::iter::once(spine).chain(caption).collect();
    Scene { nodes, edges: scene_edges, decor, legend, ..Scene::empty(view.kind) }.measured()
}

fn spine(skeleton: &Skeleton, bones: usize) -> Figure {
    let start = Point::new(skeleton.spine_start(bones), 0.0);
    Figure::Line { points: vec![start, Point::new(0.0, 0.0)], paint: Paint::line(Tone::Ink, BONE_WIDTH), end: Marker::Arrow }
}

fn placed_node(id: &str, content: &NodeContent, role: NodeRole, bones: &Bones, skeleton: &Skeleton) -> Option<SceneNode> {
    match role {
        NodeRole::Effect => Some(effect_node(id, content)),
        NodeRole::Category(bone) => Some(category_node(id, content, skeleton.far(bone), side(bone))),
        NodeRole::Cause(bone, slot) => Some(cause_node(id, content, skeleton.tick(bone, slot, bones.causes[bone].len()), side(bone))),
        NodeRole::Loose => None,
    }
}

/// The headline: the standard box in the focal look with an EFFECT tag, its left edge on the spine's
/// arrowhead.
fn effect_node(id: &str, content: &NodeContent) -> SceneNode {
    let mut content = content.clone();
    content.treatment = Treatment::Focal;
    content.tag = Some(EFFECT_TAG);
    let width = snap_to(EFFECT_WIDTH.max(content.width), DOUBLE_GRID);
    let height = snap_to(EFFECT_HEIGHT.max(content.height + TAG_ROW), DOUBLE_GRID);
    SceneNode::new(id.to_string(), Rect::new(0.0, -height / 2.0, width, height), content)
}

fn snap_to(value: f64, step: f64) -> f64 {
    (value / step).ceil() * step
}

fn tag_size(content: &NodeContent) -> (f64, f64) {
    let name = sans_width(&tag_name(content), TAG_NAME_SIZE);
    let sub = content.sublabel.as_deref().map_or(0.0, |sub| mono_width(sub, TAG_SUB_SIZE, 0.0));
    let width = snap_to(TAG_MIN_WIDTH.max(name.max(sub) + 2.0 * TAG_PADDING), DOUBLE_GRID);
    (width, if content.sublabel.is_some() { TAG_HEIGHT_WITH_SUB } else { TAG_HEIGHT })
}

fn tag_name(content: &NodeContent) -> String {
    content.lines.join(" ").to_uppercase()
}

/// A category: the small tag box at the bone's far end, in the accent look for the confirmed root cause.
fn category_node(id: &str, content: &NodeContent, far: Point, side: f64) -> SceneNode {
    let (width, height) = tag_size(content);
    let top = if side < 0.0 { far.y - TAG_GAP - height } else { far.y + TAG_GAP };
    let rect = Rect::new(far.x - width / 2.0, top, width, height);
    let focal = content.treatment == Treatment::Focal;
    let (fill, stroke) = if focal { (Tone::AccentTint, Tone::Accent) } else { (Tone::Paper, Tone::Ink) };
    let name_baseline = if content.sublabel.is_some() { top + 17.0 } else { top + 18.0 };
    let name = Text::sans(Point::new(rect.cx(), name_baseline), tag_name(content), TAG_NAME_SIZE).weight(600).tone(stroke);
    let sub = content.sublabel.as_ref().map(|sub| Figure::Text(Text::mono(Point::new(rect.cx(), top + 31.0), sub.clone(), TAG_SUB_SIZE).tone(Tone::Soft)));
    let figures = [Figure::rect(rect, TAG_RADIUS, Paint::shape(fill, stroke, TAG_STROKE)), Figure::Text(name)].into_iter().chain(sub).collect();
    SceneNode { figures, ..SceneNode::new(id.to_string(), rect, content.clone()) }
}

fn cause_label(content: &NodeContent) -> String {
    let name = content.lines.join(" ");
    match &content.sublabel {
        Some(sub) => format!("{name} ({sub})"),
        None => name,
    }
}

fn cause_width(content: &NodeContent) -> f64 {
    mono_width(&cause_label(content), CAUSE_SIZE, 0.0)
}

/// A sub-cause: its label in Geist Mono past the open end of its tick, above the tick on upper bones
/// and below it on lower ones, over a hit area of the label's size.
fn cause_node(id: &str, content: &NodeContent, tick: Point, side: f64) -> SceneNode {
    let baseline = if side < 0.0 { tick.y - CAUSE_RAISE } else { tick.y + CAUSE_DROP };
    let tone = if content.treatment == Treatment::Focal { Tone::Accent } else { Tone::Muted };
    let at = Point::new(tick.x - TICK - TICK_TEXT_GAP, baseline);
    let text = Text::mono(at, cause_label(content), CAUSE_SIZE).anchor(Anchor::End).tone(tone);
    let rect = text.bounds();
    SceneNode { figures: vec![Figure::hit_area(rect), Figure::Text(text)], ..SceneNode::new(id.to_string(), rect, content.clone()) }
}

/// The nodes on no bone, as standard boxes in a row under everything else, with a caption.
fn loose_row(loose: &[usize], ids: &[String], contents: &[NodeContent], placed: &[Option<SceneNode>], spine: &Figure) -> (Vec<SceneNode>, Option<Figure>) {
    if loose.is_empty() {
        return (Vec::new(), None);
    }
    let taken = placed.iter().flatten().flat_map(|node| std::iter::once(node.rect).chain(node.figures.iter().map(Figure::bounds)));
    let (left, bottom) = taken.chain(std::iter::once(spine.bounds())).fold((f64::INFINITY, f64::NEG_INFINITY), |(left, bottom), rect| (left.min(rect.x), bottom.max(rect.bottom())));
    let (left, top) = (snap_up(left), snap_up(bottom + LOOSE_GAP));
    let mut x = left;
    let nodes = loose
        .iter()
        .map(|&node| {
            let content = &contents[node];
            let rect = Rect::new(x, top, content.width, content.height);
            x += content.width + LOOSE_SPACING;
            SceneNode::new(ids[node].clone(), rect, content.clone())
        })
        .collect();
    let caption = Text::mono(Point::new(left, top - CAPTION_RAISE), LOOSE_CAPTION, CAPTION_SIZE).anchor(Anchor::Start).tracking(CAPTION_TRACKING);
    (nodes, Some(Figure::Text(caption)))
}

fn scene_edge(edge: &EdgeInput, role: EdgeRole, bones: &Bones, skeleton: &Skeleton, nodes: &[SceneNode]) -> SceneEdge {
    let (points, paint, end) = match role {
        EdgeRole::Bone(bone) => {
            let root_cause = nodes[bones.categories[bone]].content.treatment == Treatment::Focal;
            let tone = if root_cause { Tone::Accent } else { Tone::Ink };
            (vec![skeleton.far(bone), skeleton.attach(bone)], Paint::line(tone, BONE_WIDTH), Marker::None)
        }
        EdgeRole::Tick(bone, slot) => {
            let tick = skeleton.tick(bone, slot, bones.causes[bone].len());
            (vec![Point::new(tick.x - TICK, tick.y), tick], Paint::line(Tone::Muted, TICK_WIDTH), Marker::None)
        }
        EdgeRole::Other => (other_points(&nodes[edge.source].rect, &nodes[edge.target].rect), Paint::line(Tone::Muted, OTHER_WIDTH), Marker::Arrow),
    };
    let paint = if edge.focal { Paint::line(Tone::Accent, FOCAL_WIDTH) } else { paint };
    SceneEdge { bend: Bend::Free, markers: Some((Marker::None, end)), paint: Some(paint), ..SceneEdge::new(edge, points) }
}

fn other_points(from: &Rect, to: &Rect) -> Vec<Point> {
    let middle = |rect: &Rect| Point::new(rect.cx(), rect.y + rect.h / 2.0);
    vec![exit_point(from, middle(from), middle(to)), exit_point(to, middle(to), middle(from))]
}

fn legend(bones: &Bones, nodes: &[SceneNode], roles: &[EdgeRole]) -> Vec<LegendKey> {
    let root_cause = bones.categories.iter().any(|&category| nodes[category].content.treatment == Treatment::Focal);
    let ticks = roles.iter().any(|role| matches!(role, EdgeRole::Tick(..)));
    let keys = [
        (!bones.categories.is_empty()).then(|| LegendKey::new(Swatch::Line(Paint::line(Tone::Ink, BONE_WIDTH), Marker::None), "CATEGORY")),
        root_cause.then(|| LegendKey::new(Swatch::Line(Paint::line(Tone::Accent, BONE_WIDTH), Marker::None), "ROOT CAUSE")),
        ticks.then(|| LegendKey::new(Swatch::Line(Paint::line(Tone::Muted, TICK_WIDTH), Marker::None), "SUB-CAUSE")),
        Some(LegendKey::new(Swatch::Box(Paint::shape(Tone::AccentTint, Tone::Accent, 1.0)), "EFFECT")),
    ];
    keys.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kinds::cycle_loop::polyline_distance;
    use crate::spec::{EdgeSpec, Kind, NodeSpec, Shape};

    fn node(id: &str, label: &str) -> NodeSpec {
        NodeSpec { id: id.into(), label: label.into(), shape: Shape::Service, focal: false }
    }

    fn edge(source: &str, target: &str) -> EdgeSpec {
        EdgeSpec { id: format!("{source}->{target}"), source: source.into(), target: target.into(), label: String::new(), style: Default::default(), focal: false }
    }

    /// An effect with `bones` categories of `causes` sub-causes each, the sub-cause labels long.
    fn fishbone(bones: usize, causes: usize) -> View {
        let mut nodes = vec![node("effect", "Checkout p99 latency 4x")];
        let mut edges = Vec::new();
        for bone in 0..bones {
            let category = format!("c{bone}");
            nodes.push(node(&category, &format!("Category {bone}")));
            edges.push(edge(&category, "effect"));
            for cause in 0..causes {
                let id = format!("c{bone}s{cause}");
                nodes.push(node(&id, &format!("a rather long sub-cause number {cause}")));
                edges.push(edge(&id, &category));
            }
        }
        View { kind: Kind::Fishbone, nodes, edges, ..View::default() }
    }

    fn scene(view: &View) -> Scene {
        build(view, &Params::default())
    }

    #[test]
    fn the_effect_is_the_node_without_outgoing_edges_that_most_edges_reach() {
        let pairs = [(1, 0), (2, 0), (3, 1), (4, 5)];
        assert_eq!(effect_of(6, &pairs), Some(0));
        assert_eq!(effect_of(3, &[(1, 0), (2, 1), (0, 2)]), Some(0), "a cycle falls back to the most reached node");
        assert_eq!(effect_of(3, &[(2, 0), (2, 1)]), Some(0), "a tie goes to the first in view order");
    }

    #[test]
    fn categories_point_at_the_effect_and_sub_causes_at_a_category() {
        let pairs = [(1, 0), (2, 0), (3, 1), (4, 2), (5, 2)];
        let bones = Bones::of(6, &pairs).expect("bones");
        assert_eq!(bones.effect, 0);
        assert_eq!(bones.categories, vec![1, 2]);
        assert_eq!(bones.causes, vec![vec![3], vec![4, 5]]);
        assert!(bones.loose.is_empty());
    }

    #[test]
    fn a_cause_of_a_sub_cause_joins_its_bone_and_an_unlinked_node_stays_loose() {
        let pairs = [(1, 0), (2, 1), (3, 2), (4, 5)];
        let bones = Bones::of(6, &pairs).expect("bones");
        assert_eq!(bones.causes, vec![vec![2, 3]]);
        assert_eq!(bones.loose, vec![4, 5]);
    }

    #[test]
    fn bones_alternate_above_and_below_the_spine_at_sixty_degrees() {
        let scene = scene(&fishbone(5, 2));
        let bones: Vec<&SceneEdge> = scene.edges.iter().filter(|edge| edge.target == 0).collect();
        assert_eq!(bones.len(), 5);
        for (index, bone) in bones.iter().enumerate() {
            let (far, attach) = (bone.points[0], bone.points[1]);
            assert_eq!(attach.y, 0.0, "a bone meets the spine");
            assert_eq!(far.y < 0.0, index % 2 == 0, "bone {index} is on the wrong side");
            let degrees = ((far.y - attach.y).abs() / (attach.x - far.x)).atan().to_degrees();
            assert!((degrees - 60.0).abs() < 1.0, "bone {index} at {degrees}°");
            assert_eq!(bone.bend, Bend::Free);
        }
    }

    #[test]
    fn a_sub_cause_tick_is_a_short_horizontal_line_into_its_bone() {
        let scene = scene(&fishbone(2, 3));
        for tick in scene.edges.iter().filter(|edge| edge.target != 0) {
            let (open, on_bone) = (tick.points[0], tick.points[1]);
            assert_eq!(open.y, on_bone.y);
            assert_eq!(on_bone.x - open.x, TICK);
            let label = scene.nodes[tick.source].rect;
            assert!(label.right() < open.x, "the label sits past the open end");
        }
    }

    #[test]
    fn six_bones_widen_the_canvas_instead_of_clipping() {
        let view = fishbone(6, 3);
        let scene = crate::scene::build(&view, &Params::default());
        for rect in scene.boxes() {
            let inside = rect.x >= 0.0 && rect.y >= 0.0 && rect.right() <= scene.width && rect.bottom() <= scene.height;
            assert!(inside, "{rect:?} outside {}x{}", scene.width, scene.height);
        }
    }

    #[test]
    fn no_sub_cause_label_touches_another_bone_or_tag() {
        for causes in 1..=4 {
            let scene = scene(&fishbone(6, causes));
            let bones: Vec<&SceneEdge> = scene.edges.iter().filter(|edge| edge.target == 0).collect();
            for tick in scene.edges.iter().filter(|edge| edge.target != 0) {
                let label = scene.nodes[tick.source].rect;
                for bone in bones.iter().filter(|bone| bone.source != tick.target) {
                    assert!(polyline_distance(&label, &bone.points) > 4.0, "{} touches {}", tick.id, bone.id);
                }
                assert!(scene.nodes.iter().enumerate().all(|(index, other)| index == tick.source || !label.overlaps(&other.rect)), "{} overlaps a node", tick.id);
            }
        }
    }

    #[test]
    fn more_than_three_sub_causes_lengthen_every_bone() {
        let rises = |scene: &Scene| -> Vec<f64> { scene.edges.iter().filter(|edge| edge.target == 0).map(|bone| bone.points[0].y.abs()).collect() };
        let mut view = fishbone(2, 5);
        view.edges.retain(|edge| !edge.source.starts_with("c1s") || edge.source == "c1s0");
        let (short, long) = (rises(&scene(&fishbone(2, 3))), rises(&scene(&view)));
        assert!(long[0] > short[0]);
        assert_eq!(long[0], long[1], "a bone with one sub-cause is as long as the busiest");
    }

    #[test]
    fn the_root_cause_bone_and_tag_take_the_accent_and_the_effect_is_the_headline() {
        let mut view = fishbone(3, 1);
        view.nodes[3].focal = true;
        let scene = scene(&view);
        let bone_tone = |category: usize| scene.edges.iter().find(|edge| edge.source == category).and_then(|edge| edge.paint).map(|paint| paint.stroke);
        assert_eq!(bone_tone(3), Some(Tone::Accent));
        assert_eq!(bone_tone(1), Some(Tone::Ink));
        assert!(matches!(scene.nodes[3].figures[0], Figure::Rect { paint: Paint { stroke: Tone::Accent, .. }, .. }));
        assert_eq!(scene.nodes[0].content.treatment, Treatment::Focal);
        assert_eq!(scene.nodes[0].content.tag, Some(EFFECT_TAG));
    }

    #[test]
    fn a_node_on_no_bone_stands_in_a_row_below_the_fishbone() {
        let mut view = fishbone(2, 1);
        view.nodes.push(node("stray", "Vendor outage"));
        let scene = scene(&view);
        let stray = scene.nodes.last().expect("the stray node");
        let lowest = scene.nodes[..scene.nodes.len() - 1].iter().map(|node| node.rect.bottom()).fold(f64::NEG_INFINITY, f64::max);
        assert!(stray.rect.y > lowest);
        assert!(stray.figures.is_empty(), "drawn as a standard box");
    }
}
