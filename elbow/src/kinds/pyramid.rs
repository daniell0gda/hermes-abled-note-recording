//! Pyramid and funnel: stacked trapezoid layers in view order, top first. A pyramid narrows upward to a
//! short apex layer, a funnel narrows downward; every layer is wide enough for its name and sublabel.

use crate::figure::{Figure, Paint, Text, Tone};
use crate::geom::{Point, Rect};
use crate::node::{NodeContent, Treatment};
use crate::params::Params;
use crate::scene::{unique_nodes, Scene, SceneNode};
use crate::spec::{Kind, View};

const LAYER_HEIGHT: f64 = 64.0;
const SLOPE: f64 = 48.0;
const NAME_SIZE: f64 = 13.0;
const SUB_SIZE: f64 = 9.0;
const NAME_BASELINE: f64 = 28.0;
const LONE_NAME_BASELINE: f64 = 36.0;
const SUB_BASELINE: f64 = 44.0;
const TEXT_PADDING: f64 = 12.0;
const MIN_NARROW: f64 = 48.0;
const WIDTH_STEP: f64 = 8.0;
const AXIS_GAP: f64 = 40.0;
const AXIS_LABEL_SIZE: f64 = 8.0;
const AXIS_TRACKING: f64 = 0.14;
const ARROW_LENGTH: f64 = 12.0;
const ARROW_HALF: f64 = 4.0;
const AXIS_LABEL_GAP: f64 = 8.0;

/// The trapezoid stack: `narrow` is the width of the narrow end, and every layer away from it is
/// `2 * SLOPE` wider.
struct Stack {
    funnel: bool,
    count: usize,
    narrow: f64,
}

impl Stack {
    fn fitted(funnel: bool, labels: &[Vec<Text>]) -> Stack {
        let pointed = Stack { funnel, count: labels.len(), narrow: 0.0 };
        let shortfall = labels
            .iter()
            .enumerate()
            .flat_map(|(layer, texts)| texts.iter().map(move |text| (layer, text)))
            .map(|(layer, text)| text.width() + 2.0 * TEXT_PADDING - pointed.room_for(layer, text))
            .fold(MIN_NARROW, f64::max);
        Stack { narrow: (shortfall / WIDTH_STEP).ceil() * WIDTH_STEP, ..pointed }
    }

    /// The width at the border between layers `edge - 1` and `edge`; edge 0 is the top of the stack.
    fn edge_width(&self, edge: usize) -> f64 {
        let steps = if self.funnel { self.count - edge } else { edge };
        self.narrow + 2.0 * SLOPE * steps as f64
    }

    fn width_at(&self, layer: usize, depth: f64) -> f64 {
        let (top, bottom) = (self.edge_width(layer), self.edge_width(layer + 1));
        top + (bottom - top) * depth / LAYER_HEIGHT
    }

    /// The narrowest width of `layer` over the height `text` covers, with the layer's top at y = 0.
    fn room_for(&self, layer: usize, text: &Text) -> f64 {
        let bounds = text.bounds();
        self.width_at(layer, bounds.y).min(self.width_at(layer, bounds.bottom()))
    }

    fn widest(&self) -> f64 {
        self.edge_width(0).max(self.edge_width(self.count))
    }

    fn height(&self) -> f64 {
        self.count as f64 * LAYER_HEIGHT
    }

    fn corners(&self, layer: usize) -> Vec<Point> {
        let top = layer as f64 * LAYER_HEIGHT;
        let (upper, lower) = (self.edge_width(layer) / 2.0, self.edge_width(layer + 1) / 2.0);
        vec![
            Point::new(-upper, top),
            Point::new(upper, top),
            Point::new(lower, top + LAYER_HEIGHT),
            Point::new(-lower, top + LAYER_HEIGHT),
        ]
    }

    fn silhouette(&self) -> Vec<Point> {
        let (upper, lower) = (self.edge_width(0) / 2.0, self.edge_width(self.count) / 2.0);
        vec![Point::new(-upper, 0.0), Point::new(upper, 0.0), Point::new(lower, self.height()), Point::new(-lower, self.height())]
    }
}

/// Lays out `view` as a pyramid or, for `Kind::Funnel`, a funnel. Edges and groups are not drawn.
pub fn build(view: &View, _params: &Params) -> Scene {
    let (ids, contents) = unique_nodes(view);
    if ids.is_empty() {
        return Scene { legend: Some(Vec::new()), ..Scene::empty(view.kind) }.measured();
    }
    let funnel = view.kind == Kind::Funnel;
    let focal = contents.iter().position(|content| content.treatment == Treatment::Focal);
    let labels: Vec<Vec<Text>> = contents.iter().map(layer_texts).collect();
    let stack = Stack::fitted(funnel, &labels);
    let nodes = ids
        .into_iter()
        .zip(contents)
        .zip(labels)
        .enumerate()
        .map(|(layer, ((id, content), texts))| layer_node(&stack, layer, id, content, texts, focal == Some(layer)))
        .collect();
    let mut decor = vec![Figure::Polygon { points: stack.silhouette(), paint: Paint::line(Tone::Muted, 1.0) }];
    if stack.count > 1 {
        decor.extend(axis(&stack));
    }
    Scene { nodes, decor, legend: Some(Vec::new()), ..Scene::empty(view.kind) }.measured()
}

fn layer_texts(content: &NodeContent) -> Vec<Text> {
    let name = content.lines.join(" ");
    let Some(sublabel) = &content.sublabel else {
        return vec![Text::sans(Point::new(0.0, LONE_NAME_BASELINE), name, NAME_SIZE).weight(600)];
    };
    vec![
        Text::sans(Point::new(0.0, NAME_BASELINE), name, NAME_SIZE).weight(600),
        Text::mono(Point::new(0.0, SUB_BASELINE), sublabel.clone(), SUB_SIZE),
    ]
}

fn layer_node(stack: &Stack, layer: usize, id: String, content: NodeContent, texts: Vec<Text>, focal: bool) -> SceneNode {
    let top = layer as f64 * LAYER_HEIGHT;
    let width = stack.edge_width(layer).max(stack.edge_width(layer + 1));
    let rect = Rect::new(-width / 2.0, top, width, LAYER_HEIGHT);
    let paint = if focal { Paint::shape(Tone::AccentTint, Tone::Accent, 1.0) } else { Paint::shape(Tone::Wash(0.04), Tone::Wash(0.12), 1.0) };
    let shape = Figure::Polygon { points: stack.corners(layer), paint };
    let labels = texts.into_iter().map(|text| {
        let mut figure = Figure::Text(text);
        figure.shift(0.0, top);
        figure
    });
    SceneNode { figures: std::iter::once(shape).chain(labels).collect(), ..SceneNode::new(id, rect, content) }
}

/// The left-margin arrow: `RARER` pointing up a pyramid, `DROP-OFF` pointing down a funnel.
fn axis(stack: &Stack) -> Vec<Figure> {
    let x = -stack.widest() / 2.0 - AXIS_GAP;
    let height = stack.height();
    let (tip, base, label_y, label) = if stack.funnel {
        (height, height - ARROW_LENGTH, height + AXIS_LABEL_GAP + AXIS_LABEL_SIZE, "DROP-OFF")
    } else {
        (0.0, ARROW_LENGTH, -AXIS_LABEL_GAP, "RARER")
    };
    let tail = if stack.funnel { 0.0 } else { height };
    vec![
        Figure::line(Point::new(x, tail), Point::new(x, base), Paint::line(Tone::Wash(0.30), 1.0)),
        Figure::Polygon {
            points: vec![Point::new(x - ARROW_HALF, base), Point::new(x + ARROW_HALF, base), Point::new(x, tip)],
            paint: Paint::filled(Tone::Wash(0.45)),
        },
        Figure::Text(Text::mono(Point::new(x, label_y), label, AXIS_LABEL_SIZE).tracking(AXIS_TRACKING)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(json: &str) -> View {
        serde_json::from_str(json).expect("fixture parses")
    }

    fn pyramid() -> View {
        fixture(include_str!("../../tests/pyramid.json"))
    }

    fn funnel() -> View {
        fixture(include_str!("../../tests/funnel.json"))
    }

    fn corners(node: &SceneNode) -> Vec<Point> {
        match &node.figures[0] {
            Figure::Polygon { points, .. } => points.clone(),
            other => panic!("a layer is a polygon, not {other:?}"),
        }
    }

    fn top_and_bottom_width(node: &SceneNode) -> (f64, f64) {
        let points = corners(node);
        (points[1].x - points[0].x, points[2].x - points[3].x)
    }

    fn inside_convex(points: &[Point], at: Point) -> bool {
        points.iter().zip(points.iter().cycle().skip(1)).all(|(a, b)| (b.x - a.x) * (at.y - a.y) - (b.y - a.y) * (at.x - a.x) >= 0.0)
    }

    #[test]
    fn the_layers_follow_the_view_order_from_the_top() {
        for view in [pyramid(), funnel()] {
            let scene = build(&view, &Params::default());
            let ids: Vec<&str> = scene.nodes.iter().map(|node| node.id.as_str()).collect();
            let expected: Vec<&str> = view.nodes.iter().map(|node| node.id.as_str()).collect();
            assert_eq!(ids, expected);
            assert!(scene.nodes.windows(2).all(|pair| pair[0].rect.bottom() == pair[1].rect.y));
        }
    }

    #[test]
    fn a_pyramid_widens_downward_by_the_same_step_each_layer() {
        let scene = build(&pyramid(), &Params::default());
        for node in &scene.nodes {
            let (top, bottom) = top_and_bottom_width(node);
            assert_eq!(bottom - top, 2.0 * SLOPE, "{}", node.id);
        }
        assert!(scene.nodes.windows(2).all(|pair| top_and_bottom_width(&pair[0]).1 == top_and_bottom_width(&pair[1]).0));
    }

    #[test]
    fn a_funnels_layers_get_narrower_downward() {
        let scene = build(&funnel(), &Params::default());
        for node in &scene.nodes {
            let (top, bottom) = top_and_bottom_width(node);
            assert_eq!(top - bottom, 2.0 * SLOPE, "{}", node.id);
        }
    }

    #[test]
    fn the_apex_is_a_short_trapezoid_not_a_point() {
        let scene = build(&pyramid(), &Params::default());
        assert!(top_and_bottom_width(&scene.nodes[0]).0 >= MIN_NARROW);
    }

    #[test]
    fn every_name_and_sublabel_fits_inside_its_layer() {
        for view in [pyramid(), funnel()] {
            for node in &build(&view, &Params::default()).nodes {
                let shape = corners(node);
                for figure in &node.figures[1..] {
                    let text = figure.bounds().inflate(TEXT_PADDING / 2.0);
                    for corner in [Point::new(text.x, text.y), Point::new(text.right(), text.y), Point::new(text.right(), text.bottom()), Point::new(text.x, text.bottom())] {
                        assert!(inside_convex(&shape, corner), "{} text pokes out at {corner:?}", node.id);
                    }
                }
            }
        }
    }

    #[test]
    fn a_layer_has_a_consistent_editorial_height() {
        let scene = build(&funnel(), &Params::default());
        assert!(scene.nodes.iter().all(|node| node.rect.h == LAYER_HEIGHT));
        assert!((56.0..=72.0).contains(&LAYER_HEIGHT));
    }

    #[test]
    fn only_the_focal_layer_is_drawn_in_the_accent() {
        let scene = build(&pyramid(), &Params::default());
        let accented: Vec<&str> = scene
            .nodes
            .iter()
            .filter(|node| matches!(&node.figures[0], Figure::Polygon { paint, .. } if paint.stroke == Tone::Accent))
            .map(|node| node.id.as_str())
            .collect();
        assert_eq!(accented, vec!["manual"]);
    }

    #[test]
    fn the_axis_points_up_a_pyramid_and_down_a_funnel() {
        let label_of = |view: &View| {
            build(view, &Params::default())
                .decor
                .iter()
                .find_map(|figure| match figure {
                    Figure::Text(text) => Some(text.text.clone()),
                    _ => None,
                })
                .expect("an axis label")
        };
        assert_eq!(label_of(&pyramid()), "RARER");
        assert_eq!(label_of(&funnel()), "DROP-OFF");
    }

    #[test]
    fn edges_and_groups_are_not_drawn() {
        let mut view = pyramid();
        view.edges = fixture(include_str!("../../tests/backend.json")).edges;
        let scene = build(&view, &Params::default());
        assert!(scene.edges.is_empty() && scene.sections.is_empty());
    }
}
