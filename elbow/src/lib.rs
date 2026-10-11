//! Draws live-sketch diagrams as editorial SVG: layered layout, orthogonal connectors with rounded
//! elbows, masked labels and a legend, with the diagram-design connector rules checked in code.

mod cardinality;
mod check;
mod cycle;
mod geom;
mod label;
mod layout;
mod node;
mod params;
mod rings;
mod route;
mod scene;
mod sequence;
mod spec;
mod svg;
mod text;
mod tiers;
mod variants;

use std::time::Instant;

use serde::Serialize;

pub use check::{Measures, Violation};
pub use params::{Direction, Params, Theme};
pub use spec::{EdgeStyle, Kind, Shape, View};
pub use variants::{candidates, render_best, variants, Variant};

const FONTS_URL: &str = "https://fonts.googleapis.com/css2?family=Instrument+Serif&family=Geist:wght@400;500;600&family=Geist+Mono:wght@400;500;600&display=swap";

/// What the checker found, plus size and timing.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub violations: Vec<Violation>,
    pub measures: Measures,
    pub skipped_zones: usize,
    pub width: f64,
    pub height: f64,
    pub micros: u128,
}

pub struct Rendered {
    pub svg: String,
    pub report: Report,
}

/// Lays out and draws `view`, and checks the result against the connector rules.
pub fn render(view: &View, params: &Params) -> Rendered {
    let started = Instant::now();
    let scene = scene::build(view, params);
    let (violations, measures) = check::check(&scene);
    let svg = svg::draw(&scene, view, params.theme, params.legend);
    let report = Report {
        violations,
        measures,
        skipped_zones: scene.skipped_zones,
        width: scene.width,
        height: scene.height,
        micros: started.elapsed().as_micros(),
    };
    Rendered { svg, report }
}

/// A standalone HTML page around a rendered diagram, with the diagram-design header.
pub fn page(view: &View, rendered: &Rendered, theme: Theme) -> String {
    let (paper, ink, muted) = match theme {
        Theme::Light => ("#f5f5f5", "#2d3142", "#4f5d75"),
        Theme::Dark => ("#2d3142", "#f5f5f5", "#bfc0c0"),
    };
    let title = if view.title.is_empty() { format!("Diagram {}", view.number) } else { view.title.clone() };
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link href="{FONTS_URL}" rel="stylesheet">
<style>
*, *::before, *::after {{ box-sizing: border-box; margin: 0; padding: 0; }}
body {{ background: {paper}; color: {ink}; font-family: 'Geist', system-ui, sans-serif; padding: 3rem 2rem; }}
.frame {{ max-width: 1400px; margin: 0 auto; }}
.eyebrow {{ font-family: 'Geist Mono', ui-monospace, monospace; font-size: 0.66rem; font-weight: 500; letter-spacing: 0.18em; text-transform: uppercase; color: {muted}; margin-bottom: 0.5rem; }}
h1 {{ font-family: 'Instrument Serif', serif; font-size: 2rem; font-weight: 400; letter-spacing: -0.02em; line-height: 1.15; margin-bottom: 1.5rem; }}
svg {{ display: block; max-width: 100%; height: auto; }}
</style>
</head>
<body>
<div class="frame">
<p class="eyebrow">Diagram {number} · {kind}</p>
<h1>{escaped}</h1>
{svg}
</div>
</body>
</html>
"#,
        title = svg::escape(&title),
        escaped = svg::escape(&title),
        number = view.number,
        kind = view.kind.name(),
        svg = rendered.svg,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFERENCES: [(&str, &str); 10] = [
        ("backend", include_str!("../tests/backend.json")),
        ("hiring", include_str!("../tests/hiring.json")),
        ("issue", include_str!("../tests/issue.json")),
        ("login", include_str!("../tests/login.json")),
        ("order", include_str!("../tests/order.json")),
        ("org", include_str!("../tests/org.json")),
        ("er", include_str!("../tests/er.json")),
        ("nested", include_str!("../tests/nested.json")),
        ("layers", include_str!("../tests/layers.json")),
        ("dependency", include_str!("../tests/dependency.json")),
    ];

    fn view(json: &str) -> View {
        serde_json::from_str(json).expect("fixture parses")
    }

    fn backend() -> View {
        view(REFERENCES[0].1)
    }

    #[test]
    fn every_reference_talk_breaks_no_connector_rule() {
        for (name, json) in REFERENCES {
            let rendered = render(&view(json), &Params::default());
            assert!(rendered.report.violations.is_empty(), "{name}: {:#?}", rendered.report.violations);
        }
    }

    #[test]
    fn the_best_variant_is_valid_and_no_worse_than_the_default() {
        for (name, json) in REFERENCES {
            let reference = view(json);
            let default_score = variants::score(&render(&reference, &Params::default()).report);
            let best = render_best(&reference, &Params::default());
            assert!(best.report.violations.is_empty(), "{name}: {:#?}", best.report.violations);
            assert!(variants::score(&best.report) <= default_score, "{name}");
        }
    }

    #[test]
    fn every_element_stays_inside_the_canvas() {
        for (name, json) in REFERENCES {
            for variant in variants(&view(json), &Params::default()) {
                let scene = scene::build(&view(json), &variant.params);
                for rect in scene.boxes() {
                    let inside = rect.x >= 0.0 && rect.y >= 0.0 && rect.right() <= scene.width && rect.bottom() <= scene.height;
                    assert!(inside, "{name} {:?}: {rect:?} outside {}x{}", variant.params, scene.width, scene.height);
                }
            }
        }
    }

    #[test]
    fn the_dark_theme_uses_dark_paper() {
        let rendered = render(&backend(), &Params { theme: Theme::Dark, ..Params::default() });
        assert!(rendered.svg.contains(r##"<rect width="100%" height="100%" fill="#2d3142"/>"##));
    }

    fn reference(name: &str) -> View {
        view(REFERENCES.iter().find(|(own, _)| *own == name).expect("a known reference").1)
    }

    #[test]
    fn a_focal_edge_is_drawn_in_the_accent_colour() {
        let accent_lines = |view: &View| render(view, &Params::default()).svg.matches("stroke:#eb6c36;stroke-width:1.4").count();
        for (name, json) in REFERENCES.into_iter().filter(|(name, _)| *name != "dependency") {
            let mut focused = view(json);
            focused.edges[0].focal = true;
            assert_eq!(accent_lines(&focused), 1, "{name}");
            assert_eq!(accent_lines(&view(json)), 0, "{name}");
        }
    }

    #[test]
    fn a_dependency_cycle_is_the_only_accent_and_runs_outside_the_stack() {
        let mut dependency = reference("dependency");
        dependency.edges[0].focal = true;
        let rendered = render(&dependency, &Params { legend: false, ..Params::default() });
        assert_eq!(rendered.svg.matches("url(#arrow-accent)").count(), 1);
        assert!(rendered.svg.contains(">CYCLE<"));
        let scene = scene::build(&dependency, &Params::default());
        let cycle = scene.edges.iter().find(|edge| edge.cycle).expect("one cycle edge");
        let rightmost_node = scene.nodes.iter().map(|node| node.rect.right()).fold(0.0, f64::max);
        assert!(cycle.points.iter().any(|point| point.x > rightmost_node));
    }

    #[test]
    fn a_dependency_node_shows_how_many_depend_on_it() {
        let svg = render(&reference("dependency"), &Params::default()).svg;
        assert!(svg.contains(">3 IN<"), "shared types: two packages and the cycle depend on it");
        assert!(svg.contains(">0 IN<"));
    }

    #[test]
    fn an_er_relationship_has_cardinality_marks_and_no_arrowheads() {
        let scene = scene::build(&reference("er"), &Params::default());
        let places = scene.edges.iter().find(|edge| edge.label.as_ref().is_some_and(|label| label.text == "PLACES")).expect("places");
        let marks: Vec<&str> = places.marks.iter().map(|mark| mark.text.as_str()).collect();
        assert_eq!(marks, vec!["1", "N"]);
        assert!(!render(&reference("er"), &Params::default()).svg.contains("marker-end"));
    }

    #[test]
    fn every_layer_is_a_band_that_holds_its_nodes() {
        let layers = reference("layers");
        let scene = scene::build(&layers, &Params::default());
        assert_eq!(scene.lanes.len(), layers.groups.len());
        for (lane, group) in scene.lanes.iter().zip(&layers.groups) {
            for node in scene.nodes.iter().filter(|node| group.members.contains(&node.id)) {
                assert!(node.rect.y > lane.top && node.rect.bottom() < lane.bottom, "{} outside {}", node.id, lane.label);
            }
        }
    }

    #[test]
    fn a_nested_ring_encloses_its_members_and_nothing_else() {
        let nested = reference("nested");
        let scene = scene::build(&nested, &Params::default());
        assert_eq!(scene.rings.len(), nested.groups.len());
        for ring in &scene.rings {
            let group = nested.groups.iter().find(|group| group.label.to_uppercase() == ring.label).expect("a group");
            for node in &scene.nodes {
                let inside = node.rect.x > ring.rect.x && node.rect.right() < ring.rect.right()
                    && node.rect.y > ring.rect.y && node.rect.bottom() < ring.rect.bottom();
                assert_eq!(inside, group.members.contains(&node.id), "{} and ring {}", node.id, ring.label);
                assert!(inside || !node.rect.overlaps(&ring.rect), "{} cuts ring {}", node.id, ring.label);
            }
        }
    }

    #[test]
    fn every_node_and_edge_is_a_group_the_window_can_select_and_measure() {
        for (name, json) in REFERENCES {
            let reference = view(json);
            let svg = render(&reference, &Params::default()).svg;
            for node in &reference.nodes {
                let group = format!(r#"<g class="sk-node" data-id="{}"><rect class="sk-halo""#, node.id);
                assert!(svg.contains(&group), "{name}: node {}", node.id);
            }
            for edge in &reference.edges {
                let group = format!(r#"<g class="sk-edge" data-id="{}"><path class="sk-hit""#, svg::escape(&edge.id));
                assert!(svg.contains(&group), "{name}: edge {}", edge.id);
            }
            let sections = svg.matches(r#"<g class="sk-group""#).count();
            assert_eq!(svg.matches(r#"class="sk-shape""#).count(), reference.nodes.len() + sections, "{name}");
            assert_eq!(svg.matches(r#"class="sk-line""#).count(), reference.edges.len(), "{name}");
            assert_eq!(svg.matches(r#"class="sk-outline""#).count(), reference.nodes.len(), "{name}");
        }
    }

    #[test]
    fn every_section_is_a_group_the_window_can_select() {
        let mut kinds_with_sections = 0;
        for (name, json) in REFERENCES {
            let mut marked = view(json);
            let scene = scene::build(&marked, &Params::default());
            let ids: Vec<String> = scene
                .zones
                .iter()
                .map(|zone| zone.id.clone())
                .chain(scene.rings.iter().map(|ring| ring.id.clone()))
                .chain(scene.lanes.iter().map(|lane| lane.id.clone()).filter(|id| !id.is_empty()))
                .collect();
            let Some(first) = ids.first().cloned() else { continue };
            kinds_with_sections += 1;
            marked.selected = vec![first.clone()];
            let svg = render(&marked, &Params::default()).svg;
            for id in ids.iter().skip(1) {
                let section = format!(r#"<g class="sk-group" data-id="{id}"><rect class="sk-shape""#);
                assert!(svg.contains(&section), "{name}: section {id}");
            }
            assert!(svg.contains(&format!(r#"<g class="sk-group is-selected" data-id="{first}">"#)), "{name}");
        }
        assert!(kinds_with_sections >= 3);
    }

    #[test]
    fn a_section_comes_before_its_nodes_so_a_click_on_a_node_selects_the_node() {
        let svg = render(&reference("nested"), &Params::default()).svg;
        let last_section = svg.rfind(r#"<g class="sk-group""#).expect("sections");
        let first_node = svg.find(r#"<g class="sk-node""#).expect("nodes");
        let first_edge = svg.find(r#"<g class="sk-edge""#).expect("edges");
        assert!(last_section < first_edge && first_edge < first_node);
    }

    #[test]
    fn the_view_selection_and_pointer_mark_their_groups() {
        let mut marked = backend();
        let (node, edge) = (marked.nodes[0].id.clone(), marked.edges[0].id.clone());
        marked.selected = vec![node.clone(), edge.clone()];
        marked.pointed = Some(node.clone());
        let svg = render(&marked, &Params::default()).svg;
        assert!(svg.contains(&format!(r#"<g class="sk-node is-selected is-pointed" data-id="{node}">"#)));
        assert!(svg.contains(&format!(r#"<g class="sk-edge is-selected" data-id="{}">"#, svg::escape(&edge))));
        assert_eq!(svg.matches(" is-selected").count(), 2);
        assert!(svg.contains(".is-pointed>.sk-halo,.is-selected>.sk-outline{opacity:1}"));
    }

    #[test]
    fn the_svg_is_an_accessible_figure() {
        let rendered = render(&backend(), &Params::default());
        assert!(rendered.svg.starts_with("<svg"));
        assert!(rendered.svg.contains(r#"role="img""#));
        assert!(rendered.svg.contains(r#"<title id="sketch-1-title">How our backend looks</title>"#));
    }
}
