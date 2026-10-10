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
mod spec;
mod svg;
mod text;

use std::time::Instant;

use serde::Serialize;

pub use check::{Measures, Violation};
pub use params::{Direction, Params, Theme};
pub use spec::{EdgeStyle, Kind, Shape, View};

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

    fn backend() -> View {
        serde_json::from_str(include_str!("../tests/backend.json")).expect("fixture parses")
    }

    #[test]
    fn the_backend_reference_breaks_no_connector_rule() {
        let rendered = render(&backend(), &Params::default());
        assert!(rendered.report.violations.is_empty(), "{:#?}", rendered.report.violations);
    }

    #[test]
    fn the_svg_is_an_accessible_figure() {
        let rendered = render(&backend(), &Params::default());
        assert!(rendered.svg.starts_with("<svg"));
        assert!(rendered.svg.contains(r#"role="img""#));
        assert!(rendered.svg.contains(r#"<title id="sketch-1-title">How our backend looks</title>"#));
    }
}
