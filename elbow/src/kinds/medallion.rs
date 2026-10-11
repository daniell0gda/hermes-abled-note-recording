//! Medallion: data tiers from raw to curated, left to right. The nodes are the tiers, laid out by the
//! layered layout along their promotion edges; client, actor and external nodes without inputs are the
//! sources in front of the first tier and stay small boxes. A promotion between two neighbouring tiers of
//! one row is an arc over the top of the cards, as the diagram-design type draws it; any other edge keeps
//! its orthogonal route. The promotion into the focal tier is accent.

mod card;

use std::collections::HashMap;

use card::{Card, Style, FOCAL_STROKE};

use crate::figure::{LegendKey, Marker, Paint, Swatch, Tone};
use crate::geom::{Point, Rect};
use crate::label::MASK_HEIGHT;
use crate::node::{content, Links, NodeContent};
use crate::params::{Direction, Params};
use crate::scene::{known_edges, layered_with, mask_width, unique_nodes, Bend, EdgeInput, LabelBox, Scene, SceneEdge};
use crate::spec::{EdgeSpec, EdgeStyle, Kind, NodeSpec, Shape, View};

const ARC_RISE: f64 = 56.0;
const ARC_LABEL_DROP: f64 = 22.0;
const PROMOTION_WIDTH: f64 = 1.4;
const LIFECYCLE_DASH: &str = "4,3";

/// Lays out `view` as a medallion: tall tier cards in a row, promotions arcing over them.
pub fn build(view: &View, params: &Params) -> Scene {
    let params = Params { direction: Some(Direction::Right), ..*params };
    let (ids, contents) = unique_nodes(view);
    let edges = known_edges(view, &ids);
    let specs: Vec<&NodeSpec> = ids.iter().filter_map(|id| view.nodes.iter().find(|node| &node.id == id)).collect();
    let cards: Vec<Option<Card>> = specs
        .iter()
        .zip(&contents)
        .enumerate()
        .map(|(index, (spec, content))| tier_style(spec, index, &edges).map(|style| Card::of(content, style)))
        .collect();
    let sized = sized(&specs, contents, &cards, &edges);
    let tiers: Vec<bool> = cards.iter().map(Option::is_some).collect();
    let (mut scene, arcs) = laid_out(view, &params, &sized, &tiers);
    let texts: HashMap<&str, &str> = edges.iter().map(|edge| (edge.id.as_str(), edge.text.as_str())).collect();
    let rects: Vec<Rect> = scene.nodes.iter().map(|node| node.rect).collect();
    for edge in scene.edges.iter_mut().zip(&arcs).filter(|(_, arc)| **arc).map(|(edge, _)| edge) {
        let text = texts.get(edge.id.as_str()).copied().unwrap_or_default();
        promote_over_the_top(edge, rects[edge.source], rects[edge.target], text);
    }
    let focal: Vec<bool> = cards.iter().map(|card| card.as_ref().is_some_and(|card| card.style == Style::Focal)).collect();
    for edge in &mut scene.edges {
        edge.paint = promotion_paint(edge, &focal);
    }
    for (node, card) in scene.nodes.iter_mut().zip(&cards) {
        if let Some(card) = card {
            node.figures = card.figures(node.rect);
        }
    }
    scene.legend = Some(legend(&scene.edges, &cards));
    scene.measured()
}

/// The tier look of a node, or None for a source: a client, actor or external node no edge feeds.
fn tier_style(spec: &NodeSpec, index: usize, edges: &[EdgeInput]) -> Option<Style> {
    let fed = edges.iter().any(|edge| edge.target == index && edge.source != index);
    let source_shape = matches!(spec.shape, Shape::Client | Shape::Actor | Shape::External);
    if source_shape && !fed {
        return None;
    }
    Some(if spec.focal {
        Style::Focal
    } else if spec.shape == Shape::External {
        Style::Cold
    } else {
        Style::Default
    })
}

/// Every tier at one card size (the widest and tallest card), every source as a standard small box.
fn sized(specs: &[&NodeSpec], contents: Vec<NodeContent>, cards: &[Option<Card>], edges: &[EdgeInput]) -> Vec<NodeContent> {
    let width = cards.iter().flatten().map(Card::width).fold(card::MIN_WIDTH, f64::max);
    let height = cards.iter().flatten().map(Card::height).fold(0.0, f64::max);
    contents
        .into_iter()
        .zip(cards)
        .enumerate()
        .map(|(index, (content, card))| match card {
            Some(_) => NodeContent { width, height, ..content },
            None => source_content(specs[index], index, edges),
        })
        .collect()
}

fn source_content(spec: &NodeSpec, index: usize, edges: &[EdgeInput]) -> NodeContent {
    let outgoing = edges.iter().filter(|edge| edge.source == index && edge.target != index).count();
    content(spec, Kind::Flow, Links { incoming: 0, outgoing })
}

/// The layered layout with the arc promotions marked. Arcs need no label room between the cards, so the
/// layout runs again without their labels; the first run stays when that changes which edges are arcs.
fn laid_out(view: &View, params: &Params, contents: &[NodeContent], tiers: &[bool]) -> (Scene, Vec<bool>) {
    let first = layered_with(view, params, contents.to_vec());
    let arcs = arcs_over(&first, tiers);
    if !arcs.contains(&true) {
        return (first, arcs);
    }
    let arc_ids: Vec<&str> = first.edges.iter().zip(&arcs).filter(|(_, arc)| **arc).map(|(edge, _)| edge.id.as_str()).collect();
    let second = layered_with(&without_labels(view, &arc_ids), params, contents.to_vec());
    if arcs_over(&second, tiers) == arcs {
        (second, arcs)
    } else {
        (first, arcs)
    }
}

fn edge_id(edge: &EdgeSpec) -> String {
    if edge.id.is_empty() {
        format!("{}->{}", edge.source, edge.target)
    } else {
        edge.id.clone()
    }
}

fn without_labels(view: &View, ids: &[&str]) -> View {
    let mut plain = view.clone();
    for edge in plain.edges.iter_mut().filter(|edge| ids.contains(&edge_id(edge).as_str())) {
        edge.label.clear();
    }
    plain
}

/// For each edge of `scene`: true when it is a promotion between two tiers of one row, routed straight from
/// the left one to the right one, with nothing in the band its arc rises into.
fn arcs_over(scene: &Scene, tiers: &[bool]) -> Vec<bool> {
    scene
        .edges
        .iter()
        .map(|edge| {
            let (from, to) = (scene.nodes[edge.source].rect, scene.nodes[edge.target].rect);
            let neighbours = tiers[edge.source] && tiers[edge.target] && (from.y - to.y).abs() < 0.5 && from.right() < to.x;
            let band = arc_band(from, to);
            neighbours && edge.points.len() == 2 && scene.nodes.iter().all(|node| !node.rect.overlaps(&band))
        })
        .collect()
}

fn arc_band(from: Rect, to: Rect) -> Rect {
    Rect::new(from.cx(), from.y - ARC_RISE, to.cx() - from.cx(), ARC_RISE)
}

/// Turns `edge` into an arc from the top centre of `from` to the top centre of `to`, rising `ARC_RISE`
/// above the cards, with its label inside the arc. The middle point is the arc's apex, so the canvas
/// makes room for it.
fn promote_over_the_top(edge: &mut SceneEdge, from: Rect, to: Rect, text: &str) {
    let (start, end) = (Point::new(from.cx(), from.y), Point::new(to.cx(), to.y));
    let chord = end.x - start.x;
    let radius = (chord * chord / 4.0 + ARC_RISE * ARC_RISE) / (2.0 * ARC_RISE);
    let apex = Point::new((start.x + end.x) / 2.0, start.y - ARC_RISE);
    edge.points = vec![start, apex, end];
    edge.bend = Bend::Arc { radius, clockwise: true };
    edge.label = (!text.is_empty()).then(|| {
        let width = mask_width(text);
        LabelBox { rect: Rect::new(apex.x - width / 2.0, apex.y + ARC_LABEL_DROP, width, MASK_HEIGHT), text: text.to_string() }
    });
}

/// The look of a promotion: muted, dashed for a lifecycle move (async), accent into the focal tier. An
/// edge the view marks focal keeps the standard focal look.
fn promotion_paint(edge: &SceneEdge, focal: &[bool]) -> Option<Paint> {
    if edge.focal {
        return None;
    }
    let paint = if focal[edge.target] { Paint::line(Tone::Accent, FOCAL_STROKE) } else { Paint::line(Tone::Muted, PROMOTION_WIDTH) };
    Some(if edge.style == EdgeStyle::Async { paint.dashed(LIFECYCLE_DASH) } else { paint })
}

fn legend(edges: &[SceneEdge], cards: &[Option<Card>]) -> Vec<LegendKey> {
    let promotion = Paint::line(Tone::Muted, PROMOTION_WIDTH);
    let used = |lifecycle: bool| edges.iter().any(|edge| (edge.style == EdgeStyle::Async) == lifecycle);
    let styled = |style: Style| cards.iter().flatten().any(|card| card.style == style);
    let mut keys = Vec::new();
    if used(false) {
        keys.push(LegendKey::new(Swatch::Line(promotion, Marker::Arrow), "PROMOTION"));
    }
    if used(true) {
        keys.push(LegendKey::new(Swatch::Line(promotion.dashed(LIFECYCLE_DASH), Marker::Arrow), "LIFECYCLE"));
    }
    if styled(Style::Focal) {
        keys.push(LegendKey::new(Swatch::Box(Style::Focal.card_paint()), "FOCAL TIER"));
    }
    if styled(Style::Cold) {
        keys.push(LegendKey::new(Swatch::Box(Style::Cold.card_paint()), "COLD TIER"));
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::card::COLD_DASH;
    use super::*;
    use crate::figure::Figure;
    use crate::scene::SceneNode;

    fn fixture() -> View {
        serde_json::from_str(include_str!("../../tests/medallion.json")).expect("fixture parses")
    }

    fn node<'a>(scene: &'a Scene, id: &str) -> &'a SceneNode {
        scene.nodes.iter().find(|node| node.id == id).expect("a known node")
    }

    fn edge<'a>(scene: &'a Scene, id: &str) -> &'a SceneEdge {
        scene.edges.iter().find(|edge| edge.id == id).expect("a known edge")
    }

    fn card_paint(node: &SceneNode) -> Paint {
        match node.figures.first() {
            Some(Figure::Rect { paint, .. }) => *paint,
            other => panic!("{} is not a tier card: {other:?}", node.id),
        }
    }

    const TIERS: [&str; 5] = ["raw", "anon", "curated", "marts", "archive"];

    #[test]
    fn every_tier_card_has_the_same_size_in_one_row() {
        let scene = build(&fixture(), &Params::default());
        let first = node(&scene, "raw").rect;
        for id in TIERS {
            let rect = node(&scene, id).rect;
            assert_eq!((rect.y, rect.w, rect.h), (first.y, first.w, first.h), "{id}");
            assert!(rect.w >= card::MIN_WIDTH && rect.h > rect.w, "{id} is a tall card");
        }
        let lefts: Vec<f64> = TIERS.iter().map(|id| node(&scene, id).rect.x).collect();
        assert!(lefts.windows(2).all(|pair| pair[0] < pair[1]), "tiers run left to right: {lefts:?}");
    }

    #[test]
    fn a_client_without_inputs_is_a_small_source_box_in_front_of_the_tiers() {
        let scene = build(&fixture(), &Params::default());
        let shop = node(&scene, "shop");
        assert!(shop.figures.is_empty(), "a source is a standard box");
        assert!(shop.rect.h < node(&scene, "raw").rect.h);
        assert!(shop.rect.right() < node(&scene, "raw").rect.x);
    }

    #[test]
    fn an_external_tier_with_an_input_is_the_dashed_cold_tier() {
        let scene = build(&fixture(), &Params::default());
        assert_eq!(card_paint(node(&scene, "archive")).dash, Some(COLD_DASH));
        assert_eq!(card_paint(node(&scene, "raw")).dash, None);
    }

    #[test]
    fn only_the_focal_tier_card_and_the_promotion_into_it_are_accent() {
        let scene = build(&fixture(), &Params::default());
        for id in TIERS {
            assert_eq!(card_paint(node(&scene, id)).stroke == Tone::Accent, id == "marts", "{id}");
        }
        for edge in &scene.edges {
            let accent = edge.paint.is_some_and(|paint| paint.stroke == Tone::Accent);
            assert_eq!(accent, edge.id == "curated->marts", "{}", edge.id);
        }
    }

    #[test]
    fn a_lifecycle_move_is_dashed() {
        let scene = build(&fixture(), &Params::default());
        assert_eq!(edge(&scene, "marts->archive").paint.and_then(|paint| paint.dash), Some(LIFECYCLE_DASH));
        assert_eq!(edge(&scene, "raw->anon").paint.and_then(|paint| paint.dash), None);
    }

    #[test]
    fn promotions_between_neighbouring_tiers_arc_over_the_top_with_their_label_inside() {
        let scene = build(&fixture(), &Params::default());
        for pair in TIERS.windows(2) {
            let promotion = edge(&scene, &format!("{}->{}", pair[0], pair[1]));
            let (from, to) = (node(&scene, pair[0]).rect, node(&scene, pair[1]).rect);
            assert!(matches!(promotion.bend, Bend::Arc { clockwise: true, .. }), "{}", promotion.id);
            assert_eq!(promotion.points.first(), Some(&Point::new(from.cx(), from.y)));
            assert_eq!(promotion.points.last(), Some(&Point::new(to.cx(), to.y)));
            let label = promotion.label.as_ref().expect("a promotion label").rect;
            assert!(label.y > from.y - ARC_RISE && label.bottom() < from.y, "{} label inside the arc", promotion.id);
            assert!(label.x > from.cx() && label.right() < to.cx());
        }
        assert!(edge(&scene, "shop->raw").is_elbow(), "the source feeds the first tier with a connector");
    }

    #[test]
    fn arcs_leave_no_label_room_between_the_cards() {
        let scene = build(&fixture(), &Params::default());
        let gap = node(&scene, "anon").rect.x - node(&scene, "raw").rect.right();
        assert!(gap <= Params::default().min_channel, "gap {gap}");
    }

    #[test]
    fn a_backflow_between_tiers_keeps_its_connector() {
        let mut view = fixture();
        view.edges.push(EdgeSpec {
            id: "anon->raw".into(),
            source: "anon".into(),
            target: "raw".into(),
            label: "replay".into(),
            style: EdgeStyle::Sync,
            focal: false,
        });
        let scene = build(&view, &Params::default());
        assert!(edge(&scene, "anon->raw").is_elbow());
    }

    #[test]
    fn the_legend_names_promotion_lifecycle_and_the_tier_looks() {
        let scene = build(&fixture(), &Params::default());
        let texts: Vec<&str> = scene.legend.iter().flatten().map(|key| key.text.as_str()).collect();
        assert_eq!(texts, vec!["PROMOTION", "LIFECYCLE", "FOCAL TIER", "COLD TIER"]);
    }
}
