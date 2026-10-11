//! Draws live-sketch diagrams as editorial SVG: layered layout, orthogonal connectors with rounded
//! elbows, masked labels and a legend, with the diagram-design connector rules checked in code.

mod check;
mod geom;
mod label;
mod layout;
mod node;
mod params;
mod route;
mod scene;
mod sequence;
mod spec;
mod svg;
mod text;
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

    const REFERENCES: [(&str, &str); 6] = [
        ("backend", include_str!("../tests/backend.json")),
        ("hiring", include_str!("../tests/hiring.json")),
        ("issue", include_str!("../tests/issue.json")),
        ("login", include_str!("../tests/login.json")),
        ("order", include_str!("../tests/order.json")),
        ("org", include_str!("../tests/org.json")),
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
                let rects = scene
                    .nodes
                    .iter()
                    .map(|node| node.rect)
                    .chain(scene.edges.iter().filter_map(|edge| edge.label.as_ref().map(|label| label.rect)));
                for rect in rects {
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

    #[test]
    fn a_focal_edge_is_drawn_in_the_accent_colour() {
        for (name, json) in REFERENCES {
            let mut focused = view(json);
            focused.edges[0].focal = true;
            let svg = render(&focused, &Params::default()).svg;
            assert_eq!(svg.matches("url(#arrow-accent)").count(), 1, "{name}");
            assert_eq!(render(&view(json), &Params::default()).svg.matches("url(#arrow-accent)").count(), 0, "{name}");
        }
    }

    #[test]
    fn the_svg_is_an_accessible_figure() {
        let rendered = render(&backend(), &Params::default());
        assert!(rendered.svg.starts_with("<svg"));
        assert!(rendered.svg.contains(r#"role="img""#));
        assert!(rendered.svg.contains(r#"<title id="sketch-1-title">How our backend looks</title>"#));
    }
}
