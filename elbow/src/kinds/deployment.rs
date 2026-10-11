//! Deployment: hosts, pods and managed services inside environment and network zones. A node shows its
//! type tag, its replica badge and one chip per deployed artifact; a network path is link blue when it
//! leaves its zone and muted inside one zone. Zones are the nested rings of the layered layout, drawn as
//! dashed sections.

use crate::figure::{Anchor, Figure, LegendKey, Marker, Paint, Section, Swatch, Text, Tone};
use crate::geom::{segments, snap_up, Point, Rect, Segment};
use crate::node::{field_list, name_and_sublabel, tag_width, NodeContent, Treatment, NAME_LINE, NAME_SIZE, TAG_SIZE, TAG_TRACKING};
use crate::params::Params;
use crate::rings::{Ring, RING_LABEL_PADDING, RING_LABEL_SIZE, RING_LABEL_TRACKING};
use crate::scene::{layered_with, unique_nodes, Scene, SceneEdge};
use crate::spec::{EdgeStyle, NodeSpec, Shape, View};
use crate::text::{mono_width, sans_width};

const TYPE_TAGS: [&str; 6] = ["POD", "VM", "MANAGED", "CDN", "HOST", "FUNCTION"];
const BOX_RADIUS: f64 = 6.0;
const INSET: f64 = 8.0;
const CHIP_ROW_HEIGHT: f64 = 12.0;
const CHIP_BASELINE: f64 = 9.0;
const CHIP_STROKE: f64 = 0.8;
const BADGE_SIZE: f64 = 8.0;
const BADGE_PADDING: f64 = 12.0;
const FIRST_BASELINE: f64 = 40.0;
const NAME_TO_CHIPS: f64 = 12.0;
const BARE_BOTTOM: f64 = 16.0;
const NAME_PADDING: f64 = 16.0;
const MIN_WIDTH: f64 = 144.0;
const ARTIFACT_HEIGHT: f64 = 24.0;
const ARTIFACT_GAP: f64 = 8.0;
const ARTIFACT_RADIUS: f64 = 4.0;
const ARTIFACT_PADDING: f64 = 8.0;
const ARTIFACT_TEXT_GAP: f64 = 12.0;
const ARTIFACT_BASELINE: f64 = 16.0;
const ARTIFACT_SIZE: f64 = 12.0;
const VERSION_SIZE: f64 = 9.0;
const ZONE_RADIUS: f64 = 8.0;
const ZONE_STROKE: f64 = 0.8;
const ZONE_LABEL_INSET: f64 = 12.0;
const ZONE_LABEL_TOP: f64 = 4.0;
const ZONE_LABEL_HEIGHT: f64 = 12.0;
const ZONE_LABEL_BASELINE: f64 = 9.0;
const LINE_WIDTH: f64 = 1.2;
const FOCAL_LINE_WIDTH: f64 = 1.4;
const ASYNC_DASH: &str = "5,4";
const NODE_KEYS: [(Treatment, &str); 6] = [
    (Treatment::Focal, "FOCUS"),
    (Treatment::Service, "HOST"),
    (Treatment::Store, "MANAGED"),
    (Treatment::External, "EXTERNAL"),
    (Treatment::Input, "CLIENT"),
    (Treatment::Note, "NOTE"),
];

/// Lays out `view` as a deployment: the layered layout with nodes sized for their chips, the group
/// rings turned into zone sections, and paths coloured by whether they leave their zone.
pub fn build(view: &View, params: &Params) -> Scene {
    let (ids, contents) = unique_nodes(view);
    let hosts: Vec<Host> = node_specs(view, &ids).into_iter().map(|spec| Host::parse(&spec.label, spec.shape)).collect();
    let sized = hosts.iter().zip(contents).map(|(host, content)| host.sized(content)).collect();
    let mut scene = layered_with(view, params, sized);
    for (node, host) in scene.nodes.iter_mut().zip(&hosts) {
        node.figures = host.figures(node.rect, &node.content.lines, node.content.treatment);
    }
    let zones = innermost_zones(view, &ids);
    for edge in &mut scene.edges {
        edge.paint = Some(path_paint(edge, zones[edge.source] != zones[edge.target]));
    }
    let rings = std::mem::take(&mut scene.rings);
    let lines: Vec<Segment> = scene.edges.iter().flat_map(|edge| segments(&edge.points)).collect();
    let taken: Vec<Rect> = scene.nodes.iter().map(|node| node.rect).chain(scene.edges.iter().flat_map(SceneEdge::texts).map(|text| text.rect)).collect();
    scene.sections = rings.into_iter().map(|ring| zone_section(ring, &lines, &taken)).collect();
    scene.legend = Some(legend(&scene, &zones));
    scene.measured()
}

/// The first spec of every id, in the order of `ids`.
pub fn node_specs<'a>(view: &'a View, ids: &[String]) -> Vec<&'a NodeSpec> {
    ids.iter().filter_map(|id| view.nodes.iter().find(|spec| &spec.id == id)).collect()
}

/// The fill and outline of a node box with `treatment`, as the diagram-design node table gives them.
pub fn box_paint(treatment: Treatment) -> Paint {
    match treatment {
        Treatment::Focal => Paint::shape(Tone::AccentTint, Tone::Accent, 1.0),
        Treatment::Service | Treatment::State => Paint::shape(Tone::Fill, Tone::Ink, 1.0),
        Treatment::Store | Treatment::Leaf => Paint::shape(Tone::Wash(0.05), Tone::Muted, 1.0),
        Treatment::External => Paint::shape(Tone::Wash(0.03), Tone::Wash(0.30), 1.0),
        Treatment::Input => Paint::shape(Tone::MutedWash(0.10), Tone::Soft, 1.0),
        Treatment::Note => Paint::shape(Tone::Wash(0.02), Tone::Soft, 1.0).dashed("4,4"),
    }
}

/// A node box on paper: the outlined shape first, then its tint, so a translucent fill never shows
/// what lies under the node.
pub fn box_figures(rect: Rect, treatment: Treatment) -> Vec<Figure> {
    let paint = box_paint(treatment);
    vec![
        Figure::rect(rect, BOX_RADIUS, Paint { fill: Tone::Paper, ..paint }),
        Figure::rect(rect.inflate(-paint.width / 2.0), BOX_RADIUS - paint.width / 2.0, Paint::filled(paint.fill)),
    ]
}

/// One legend box for every entry whose look some node of `scene` has.
pub fn box_keys(scene: &Scene, entries: &[(Treatment, &str)]) -> Vec<LegendKey> {
    entries
        .iter()
        .filter(|(treatment, _)| scene.nodes.iter().any(|node| box_paint(node.content.treatment) == box_paint(*treatment)))
        .map(|(treatment, text)| LegendKey::new(Swatch::Box(box_paint(*treatment)), *text))
        .collect()
}

#[derive(Debug, PartialEq)]
struct Artifact {
    name: String,
    version: Option<String>,
}

impl Artifact {
    fn parse(item: &str) -> Artifact {
        match item.rsplit_once(char::is_whitespace) {
            Some((name, last)) if is_version(last) => Artifact { name: name.trim().to_string(), version: Some(last.to_string()) },
            _ if is_version(item) => Artifact { name: String::new(), version: Some(item.to_string()) },
            _ => Artifact { name: item.to_string(), version: None },
        }
    }

    fn width(&self) -> f64 {
        let version = self.version.as_deref().map_or(0.0, |version| ARTIFACT_TEXT_GAP + mono_width(version, VERSION_SIZE, 0.0));
        2.0 * ARTIFACT_PADDING + sans_width(&self.name, ARTIFACT_SIZE) + version
    }

    fn figures(&self, chip: Rect) -> Vec<Figure> {
        let baseline = chip.y + ARTIFACT_BASELINE;
        let mut figures = vec![Figure::rect(chip, ARTIFACT_RADIUS, Paint::shape(Tone::Wash(0.05), Tone::Muted, CHIP_STROKE))];
        if !self.name.is_empty() {
            let at = Point::new(chip.x + ARTIFACT_PADDING, baseline);
            figures.push(Figure::Text(Text::sans(at, self.name.clone(), ARTIFACT_SIZE).anchor(Anchor::Start)));
        }
        if let Some(version) = &self.version {
            let at = Point::new(chip.right() - ARTIFACT_PADDING, baseline);
            figures.push(Figure::Text(Text::mono(at, version.clone(), VERSION_SIZE).anchor(Anchor::End)));
        }
        figures
    }
}

fn is_version(token: &str) -> bool {
    let rest = token.strip_prefix(|c: char| c == 'v' || c == 'V').unwrap_or(token);
    rest.starts_with(|c: char| c.is_ascii_digit())
}

/// What a deployment node's bracket says: its type tag, replica badge and deployed artifacts.
#[derive(Debug, PartialEq)]
struct Host {
    tag: String,
    badge: Option<String>,
    artifacts: Vec<Artifact>,
}

impl Host {
    fn parse(label: &str, shape: Shape) -> Host {
        let items = name_and_sublabel(label).1.map(|bracket| field_list(&bracket)).unwrap_or_default();
        let tag = items.iter().find_map(|item| type_tag(item)).unwrap_or_else(|| shape_tag(shape));
        let badge = items.iter().find_map(|item| replicas(item));
        let artifacts = items
            .iter()
            .filter(|item| type_tag(item).is_none() && replicas(item).is_none())
            .map(|item| Artifact::parse(item))
            .collect();
        Host { tag: tag.to_string(), badge, artifacts }
    }

    fn sized(&self, content: NodeContent) -> NodeContent {
        let name = content.lines.iter().map(|line| sans_width(line, NAME_SIZE)).fold(0.0, f64::max) + 2.0 * NAME_PADDING;
        let badge = self.badge.as_deref().map_or(0.0, |badge| badge_width(badge) + INSET);
        let top_row = tag_width(&self.tag) + badge + 2.0 * INSET;
        let chips = self.artifacts.iter().map(Artifact::width).fold(0.0, f64::max) + 2.0 * INSET;
        let width = snap_up(name.max(top_row).max(chips).max(MIN_WIDTH));
        let height = snap_up(self.height(content.lines.len()));
        NodeContent { sublabel: None, fields: Vec::new(), tag: None, badge: None, width, height, ..content }
    }

    fn height(&self, names: usize) -> f64 {
        let count = self.artifacts.len() as f64;
        if self.artifacts.is_empty() {
            last_baseline(names) + BARE_BOTTOM
        } else {
            chips_top(names) + count * ARTIFACT_HEIGHT + (count - 1.0) * ARTIFACT_GAP + INSET
        }
    }

    fn figures(&self, rect: Rect, names: &[String], treatment: Treatment) -> Vec<Figure> {
        let mut figures = box_figures(rect, treatment);
        figures.extend(self.chips(rect, treatment));
        figures.extend(names.iter().enumerate().map(|(index, name)| {
            let at = Point::new(rect.cx(), rect.y + FIRST_BASELINE + index as f64 * NAME_LINE);
            Figure::Text(Text::sans(at, name.clone(), NAME_SIZE).weight(600))
        }));
        let top = rect.y + chips_top(names.len());
        for (index, artifact) in self.artifacts.iter().enumerate() {
            let y = top + index as f64 * (ARTIFACT_HEIGHT + ARTIFACT_GAP);
            figures.extend(artifact.figures(Rect::new(rect.x + INSET, y, rect.w - 2.0 * INSET, ARTIFACT_HEIGHT)));
        }
        figures
    }

    fn chips(&self, rect: Rect, treatment: Treatment) -> Vec<Figure> {
        let (stroke, text) = chip_tones(treatment);
        let top = rect.y + INSET;
        let tag = Rect::new(rect.x + INSET, top, tag_width(&self.tag), CHIP_ROW_HEIGHT);
        let mut figures = vec![
            Figure::rect(tag, 2.0, Paint::line(stroke, CHIP_STROKE)),
            Figure::Text(Text::mono(Point::new(tag.cx(), top + CHIP_BASELINE), self.tag.clone(), TAG_SIZE).tracking(TAG_TRACKING).tone(text)),
        ];
        if let Some(badge) = &self.badge {
            let width = badge_width(badge);
            let chip = Rect::new(rect.right() - INSET - width, top, width, CHIP_ROW_HEIGHT);
            figures.push(Figure::rect(chip, 2.0, Paint::line(stroke, CHIP_STROKE)));
            figures.push(Figure::Text(Text::mono(Point::new(chip.cx(), top + CHIP_BASELINE), badge.clone(), BADGE_SIZE).tone(Tone::Ink)));
        }
        figures
    }
}

fn last_baseline(names: usize) -> f64 {
    FIRST_BASELINE + names.saturating_sub(1) as f64 * NAME_LINE
}

fn chips_top(names: usize) -> f64 {
    last_baseline(names) + NAME_TO_CHIPS
}

fn badge_width(badge: &str) -> f64 {
    mono_width(badge, BADGE_SIZE, 0.0) + BADGE_PADDING
}

fn chip_tones(treatment: Treatment) -> (Tone, Tone) {
    match treatment {
        Treatment::Focal => (Tone::AccentWash(0.5), Tone::Accent),
        Treatment::Service | Treatment::State => (Tone::Wash(0.4), Tone::Ink),
        Treatment::Store | Treatment::Leaf => (Tone::MutedWash(0.5), Tone::Muted),
        Treatment::External | Treatment::Input | Treatment::Note => (Tone::Wash(0.22), Tone::Soft),
    }
}

fn type_tag(item: &str) -> Option<&'static str> {
    TYPE_TAGS.iter().copied().find(|tag| tag.eq_ignore_ascii_case(item))
}

fn replicas(item: &str) -> Option<String> {
    let count = item.strip_prefix(|c: char| c == 'x' || c == 'X')?;
    (!count.is_empty() && count.chars().all(|c| c.is_ascii_digit())).then(|| format!("x{count}"))
}

fn shape_tag(shape: Shape) -> &'static str {
    match shape {
        Shape::Database | Shape::Cache | Shape::Queue => "MANAGED",
        Shape::External => "EXT",
        Shape::Client => "CDN",
        _ => "HOST",
    }
}

/// The innermost group holding each node (the one with the fewest known members), None outside all groups.
fn innermost_zones(view: &View, ids: &[String]) -> Vec<Option<usize>> {
    let known = |members: &[String]| members.iter().filter(|member| ids.contains(member)).count();
    ids.iter()
        .map(|id| {
            let holding = view.groups.iter().enumerate().filter(|(_, group)| group.members.contains(id));
            holding.min_by_key(|(_, group)| known(&group.members)).map(|(index, _)| index)
        })
        .collect()
}

fn path_paint(edge: &SceneEdge, crosses_zone: bool) -> Paint {
    let paint = match (edge.focal, crosses_zone) {
        (true, _) => Paint::line(Tone::Accent, FOCAL_LINE_WIDTH),
        (false, true) => Paint::line(Tone::Link, LINE_WIDTH),
        (false, false) => Paint::line(Tone::Muted, LINE_WIDTH),
    };
    if edge.style == EdgeStyle::Async {
        paint.dashed(ASYNC_DASH)
    } else {
        paint
    }
}

/// The zone of `ring` as a section: the dashed rect, then its label inside the top-left corner on a paper
/// mask, slid along the border off every connector and node.
fn zone_section(mut ring: Ring, lines: &[Segment], taken: &[Rect]) -> Section {
    ring.label_rect = Rect::new(ring.rect.x + ZONE_LABEL_INSET, ring.rect.y + ZONE_LABEL_TOP, ring.label_rect.w, ZONE_LABEL_HEIGHT);
    ring.clear_label(lines, taken);
    let mask = ring.label_rect;
    let label = Text::mono(Point::new(mask.x + RING_LABEL_PADDING, mask.y + ZONE_LABEL_BASELINE), ring.label, RING_LABEL_SIZE)
        .tracking(RING_LABEL_TRACKING)
        .anchor(Anchor::Start);
    Section {
        id: ring.id,
        figures: vec![
            Figure::rect(ring.rect, ZONE_RADIUS, Paint::shape(Tone::Wash(0.02), Tone::Wash(0.20), ZONE_STROKE).dashed("4,4")),
            Figure::rect(mask, 2.0, Paint::filled(Tone::Paper)),
            Figure::Text(label),
        ],
    }
}

fn legend(scene: &Scene, zones: &[Option<usize>]) -> Vec<LegendKey> {
    let mut keys = box_keys(scene, &NODE_KEYS);
    let focal_node = scene.nodes.iter().any(|node| node.content.treatment == Treatment::Focal);
    let plain = || scene.edges.iter().filter(|edge| !edge.focal);
    let crossing = |edge: &&SceneEdge| zones[edge.source] != zones[edge.target];
    let lines = [
        (scene.edges.iter().any(|edge| edge.focal) && !focal_node, Paint::line(Tone::Accent, LINE_WIDTH), "FOCUS"),
        (plain().any(|edge| crossing(&edge)), Paint::line(Tone::Link, LINE_WIDTH), "CROSS-ZONE"),
        (plain().any(|edge| !crossing(&edge)), Paint::line(Tone::Muted, LINE_WIDTH), "IN-ZONE"),
        (scene.edges.iter().any(|edge| edge.style == EdgeStyle::Async), Paint::line(Tone::Muted, LINE_WIDTH).dashed(ASYNC_DASH), "ASYNC / REPLICATION"),
    ];
    keys.extend(lines.into_iter().filter(|(used, ..)| *used).map(|(_, paint, text)| LegendKey::new(Swatch::Line(paint, Marker::Arrow), text)));
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::check;
    use crate::scene;
    use crate::variants::candidates;

    fn fixture() -> View {
        serde_json::from_str(include_str!("../../tests/deployment.json")).expect("fixture parses")
    }

    fn edge<'a>(scene: &'a Scene, id: &str) -> &'a SceneEdge {
        scene.edges.iter().find(|edge| edge.id == id).expect("a known edge")
    }

    fn artifact(name: &str, version: Option<&str>) -> Artifact {
        Artifact { name: name.to_string(), version: version.map(str::to_string) }
    }

    #[test]
    fn a_type_item_becomes_the_tag_and_x_n_the_replica_badge() {
        let host = Host::parse("api (pod, api-server v2.4.1, X3)", Shape::Service);
        assert_eq!(host.tag, "POD");
        assert_eq!(host.badge.as_deref(), Some("x3"));
        assert_eq!(host.artifacts, vec![artifact("api-server", Some("v2.4.1"))]);
    }

    #[test]
    fn every_other_item_is_an_artifact_with_the_version_split_off_its_last_token() {
        let host = Host::parse("db (MANAGED, postgres 16.2, hashicorp vault, v3, pgbouncer)", Shape::Database);
        let expected = vec![
            artifact("postgres", Some("16.2")),
            artifact("hashicorp vault", None),
            artifact("", Some("v3")),
            artifact("pgbouncer", None),
        ];
        assert_eq!(host.artifacts, expected);
        assert_eq!(host.badge, None);
    }

    #[test]
    fn without_a_type_item_the_tag_comes_from_the_shape() {
        let tag = |shape| Host::parse("node (nginx v1)", shape).tag;
        for shape in [Shape::Database, Shape::Cache, Shape::Queue] {
            assert_eq!(tag(shape), "MANAGED");
        }
        assert_eq!(tag(Shape::External), "EXT");
        assert_eq!(tag(Shape::Client), "CDN");
        assert_eq!(tag(Shape::Service), "HOST");
        assert_eq!(Host::parse("fn (function)", Shape::Service).tag, "FUNCTION");
    }

    #[test]
    fn a_node_grows_one_chip_row_per_artifact_and_is_wide_enough_for_its_chips() {
        let content = |label: &str| {
            let (_, contents) = unique_nodes(&View {
                nodes: vec![NodeSpec { id: "n".into(), label: label.into(), shape: Shape::Service, focal: false }],
                ..View::default()
            });
            Host::parse(label, Shape::Service).sized(contents.into_iter().next().expect("one node"))
        };
        let one = content("app (POD, checkout-api v2.4.1)");
        let two = content("app (POD, checkout-api v2.4.1, sidecar-otel-collector v0.9)");
        assert_eq!(one.height, 84.0);
        assert_eq!(two.height - one.height, ARTIFACT_HEIGHT + ARTIFACT_GAP);
        assert!(two.width >= artifact("sidecar-otel-collector", Some("v0.9")).width() + 2.0 * INSET);
        assert_eq!(two.width % 4.0, 0.0);
    }

    #[test]
    fn a_path_leaving_its_zone_is_link_blue_and_one_inside_a_zone_muted() {
        let scene = scene::build(&fixture(), &Params::default());
        assert_eq!(edge(&scene, "cdn->ingress").paint, Some(Paint::line(Tone::Link, LINE_WIDTH)));
        assert_eq!(edge(&scene, "ingress->app").paint, Some(Paint::line(Tone::Muted, LINE_WIDTH)));
        assert_eq!(edge(&scene, "app->primary").paint, Some(Paint::line(Tone::Link, LINE_WIDTH)));
        assert_eq!(edge(&scene, "primary->standby").paint, Some(Paint::line(Tone::Muted, LINE_WIDTH).dashed(ASYNC_DASH)));
    }

    #[test]
    fn a_focal_path_is_accent_whatever_zones_it_joins() {
        let mut view = fixture();
        view.edges.iter_mut().for_each(|edge| edge.focal = edge.id == "cdn->ingress");
        let scene = scene::build(&view, &Params::default());
        assert_eq!(edge(&scene, "cdn->ingress").paint, Some(Paint::line(Tone::Accent, FOCAL_LINE_WIDTH)));
    }

    #[test]
    fn every_zone_is_a_section_that_holds_exactly_its_members() {
        let view = fixture();
        let scene = scene::build(&view, &Params::default());
        assert!(scene.rings.is_empty());
        assert_eq!(scene.sections.len(), view.groups.len());
        for (section, group) in scene.sections.iter().map(|section| (section, view.groups.iter().find(|group| group.id == section.id).expect("a group"))) {
            let zone = section.figures[0].bounds();
            for node in &scene.nodes {
                let inside = node.rect.x > zone.x && node.rect.right() < zone.right() && node.rect.y > zone.y && node.rect.bottom() < zone.bottom();
                assert_eq!(inside, group.members.contains(&node.id), "{} and zone {}", node.id, group.id);
                assert!(inside || !node.rect.overlaps(&zone), "{} cuts zone {}", node.id, group.id);
            }
        }
    }

    #[test]
    fn a_zone_label_sits_inside_the_top_left_corner_off_every_connector() {
        for params in candidates(&fixture(), &Params::default()) {
            let scene = scene::build(&fixture(), &params);
            let lines: Vec<Segment> = scene.edges.iter().flat_map(|edge| segments(&edge.points)).collect();
            for section in &scene.sections {
                let (zone, mask) = (section.figures[0].bounds(), section.figures[1].bounds());
                assert!(mask.x > zone.x && mask.y > zone.y && mask.right() < zone.right(), "{} {params:?}", section.id);
                assert!(lines.iter().all(|line| mask.distance_to(line) >= 2.0), "{} label on a connector {params:?}", section.id);
            }
        }
    }

    #[test]
    fn the_fixture_breaks_no_connector_rule_and_names_its_path_looks() {
        let scene = scene::build(&fixture(), &Params::default());
        assert_eq!(check(&scene).0, Vec::new());
        let texts: Vec<String> = scene.legend.expect("an own legend").into_iter().map(|key| key.text).collect();
        for text in ["FOCUS", "CROSS-ZONE", "IN-ZONE", "ASYNC / REPLICATION"] {
            assert!(texts.iter().any(|own| own == text), "{text} missing from {texts:?}");
        }
    }
}
