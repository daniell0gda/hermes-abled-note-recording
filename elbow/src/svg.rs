//! SVG output in the diagram-design editorial style.

use std::fmt::Write;

use crate::cardinality::MARK_SIZE;
use crate::geom::{segments, Point, Rect, Segment};
use crate::label::{LABEL_SIZE, LABEL_TRACKING};
use crate::node::{
    tag_width, Treatment, FIELD_INSET, FIELD_LINE, FIELD_SIZE, NAME_LINE, NAME_SIZE, SUB_LINE, SUB_SIZE, TAG_SIZE, TAG_TRACKING,
};
use crate::params::Theme;
use crate::rings::{Ring, RING_LABEL_PADDING, RING_LABEL_SIZE, RING_LABEL_TRACKING};
use crate::scene::{Lane, Scene, SceneEdge, SceneNode, Zone, LANE_LABEL_SIZE, LANE_LABEL_TRACKING, MARGIN};
use crate::spec::{EdgeStyle, Kind, View};
use crate::text::mono_width;

const CORNER: f64 = 8.0;
const LAYER_INSET: f64 = 16.0;
const HIT_WIDTH: f64 = 12.0;
const HALO: f64 = 8.0;
const OUTLINE: f64 = 4.0;
const HOP: f64 = 8.0;
const HOP_ROOM: f64 = 12.0;
const FOCAL_LINE_WIDTH: f64 = 1.4;
const LEGEND_TOP: f64 = 16.0;
const LEGEND_ROW: f64 = 24.0;
const LEGEND_HEIGHT: f64 = 56.0;
const SANS: &str = "'Geist', system-ui, sans-serif";
const MONO: &str = "'Geist Mono', ui-monospace, monospace";

struct Palette {
    paper: &'static str,
    box_fill: &'static str,
    ink: &'static str,
    muted: &'static str,
    soft: &'static str,
    accent: &'static str,
    accent_tint: &'static str,
    link: &'static str,
    point: &'static str,
    wash: &'static str,
    muted_wash: &'static str,
}

const LIGHT: Palette = Palette {
    paper: "#f5f5f5",
    box_fill: "#ffffff",
    ink: "#2d3142",
    muted: "#4f5d75",
    soft: "#7a8399",
    accent: "#eb6c36",
    accent_tint: "rgba(235,108,54,0.08)",
    link: "#2e5aa8",
    point: "rgba(46,90,168,0.12)",
    wash: "45,49,66",
    muted_wash: "79,93,117",
};

const DARK: Palette = Palette {
    paper: "#2d3142",
    box_fill: "#393e53",
    ink: "#f5f5f5",
    muted: "#bfc0c0",
    soft: "#8e98ac",
    accent: "#f08a59",
    accent_tint: "rgba(240,138,89,0.10)",
    link: "#6a95d8",
    point: "rgba(106,149,216,0.18)",
    wash: "245,245,245",
    muted_wash: "191,192,192",
};

/// The elements the viewer selected and the one pointed at, as the view gave them.
struct Marking<'a> {
    selected: &'a [String],
    pointed: Option<&'a str>,
}

impl Marking<'_> {
    fn classes(&self, id: &str) -> &'static str {
        match (self.selected.iter().any(|selected| selected == id), self.pointed == Some(id)) {
            (true, true) => " is-selected is-pointed",
            (true, false) => " is-selected",
            (false, true) => " is-pointed",
            (false, false) => "",
        }
    }
}

struct BoxLook {
    fill: String,
    stroke: String,
    dash: Option<&'static str>,
}

struct LineLook {
    stroke: &'static str,
    marker: &'static str,
    dash: Option<&'static str>,
    width: f64,
}

/// The diagram as one standalone `<svg>` element.
pub fn draw(scene: &Scene, view: &View, theme: Theme, legend: bool) -> String {
    let palette = match theme {
        Theme::Light => &LIGHT,
        Theme::Dark => &DARK,
    };
    let legend_items = if legend { legend_items(scene, palette) } else { Vec::new() };
    let legend_width = legend_items.iter().map(|item| item.width()).sum::<f64>() + 2.0 * MARGIN + 80.0;
    let width = scene.width.max(if legend_items.is_empty() { 0.0 } else { legend_width });
    let height = scene.height + if legend_items.is_empty() { 0.0 } else { LEGEND_HEIGHT };
    let slug = format!("sketch-{}", view.number);
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}" role="img" aria-labelledby="{slug}-title {slug}-desc">"#
    );
    let _ = write!(out, r#"<title id="{slug}-title">{}</title>"#, escape(&title(view)));
    let _ = write!(out, r#"<desc id="{slug}-desc">{}</desc>"#, escape(&description(scene, view)));
    markers(&mut out, palette);
    marking_style(&mut out, palette);
    let marking = Marking { selected: &view.selected, pointed: view.pointed.as_deref() };
    let _ = write!(out, r#"<rect width="100%" height="100%" fill="{}"/>"#, palette.paper);
    if scene.kind == Kind::Layers {
        draw_layers(&mut out, scene, width, &marking, palette);
    } else {
        draw_lanes(&mut out, scene, width, &marking, palette);
    }
    for zone in &scene.zones {
        draw_zone(&mut out, zone, &marking, palette);
    }
    for ring in &scene.rings {
        draw_ring(&mut out, ring, &marking, palette);
    }
    for lifeline in &scene.lifelines {
        let _ = write!(
            out,
            r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="rgba({},0.30)" stroke-width="1" stroke-dasharray="4,4"/>"#,
            lifeline.a.x,
            lifeline.a.y,
            lifeline.b.x,
            lifeline.b.y,
            palette.wash
        );
    }
    let hops = hops(scene);
    let arrows = scene.kind != Kind::Er;
    for (index, edge) in scene.edges.iter().enumerate() {
        let _ = write!(out, r#"<g class="sk-edge{}" data-id="{}">"#, marking.classes(&edge.id), escape(&edge.id));
        draw_edge(&mut out, edge, &hops[index], arrows, palette);
        draw_label(&mut out, edge, palette);
        draw_marks(&mut out, edge, palette);
        out.push_str("</g>");
    }
    for node in &scene.nodes {
        let _ = write!(out, r#"<g class="sk-node{}" data-id="{}">"#, marking.classes(&node.id), escape(&node.id));
        draw_node(&mut out, node, palette);
        out.push_str("</g>");
    }
    if !legend_items.is_empty() {
        draw_legend(&mut out, &legend_items, scene.height, width, palette);
    }
    out.push_str("</svg>");
    out
}

fn title(view: &View) -> String {
    if view.title.is_empty() {
        format!("Diagram {}", view.number)
    } else {
        view.title.clone()
    }
}

fn description(scene: &Scene, view: &View) -> String {
    let names: Vec<String> = scene.nodes.iter().map(|node| node.content.lines.join(" ")).collect();
    format!("{} diagram of {} with {} connections.", capitalised(view.kind.name()), names.join(", "), scene.edges.len())
}

fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn markers(out: &mut String, palette: &Palette) {
    out.push_str("<defs>");
    for (id, fill) in [("arrow", palette.muted), ("arrow-accent", palette.accent), ("arrow-link", palette.link)] {
        let _ = write!(
            out,
            r#"<marker id="{id}" markerWidth="8" markerHeight="6" refX="7" refY="3" orient="auto"><polygon points="0 0, 8 3, 0 6" fill="{fill}"/></marker>"#
        );
    }
    out.push_str("</defs>");
}

/// The selection and pointer looks, switched by the `is-selected` and `is-pointed` classes on a node or
/// edge group. The look of shapes and lines is inline style, so only these rules (and no page stylesheet)
/// can change it.
fn marking_style(out: &mut String, palette: &Palette) {
    let _ = write!(
        out,
        "<style>.sk-halo,.sk-outline{{opacity:0}}.is-pointed>.sk-halo,.is-selected>.sk-outline{{opacity:1}}\
         .sk-edge.is-pointed>.sk-hit{{stroke:{point}}}.sk-edge.is-pointed .sk-edge-label-bg{{fill:{point}!important}}\
         .sk-edge.is-selected>.sk-line,.sk-group.is-selected>.sk-shape{{stroke:{link}!important;stroke-width:2px!important}}\
         .sk-group.is-pointed>.sk-shape{{fill:{point}!important}}</style>",
        point = palette.point,
        link = palette.link
    );
}

fn line_look(style: EdgeStyle, palette: &Palette) -> LineLook {
    match style {
        EdgeStyle::Sync => LineLook { stroke: palette.link, marker: "arrow-link", dash: None, width: 1.2 },
        EdgeStyle::Async => LineLook { stroke: palette.muted, marker: "arrow", dash: Some("5,4"), width: 1.2 },
        EdgeStyle::Data | EdgeStyle::Transition => LineLook { stroke: palette.muted, marker: "arrow", dash: None, width: 1.2 },
        EdgeStyle::Dependency => LineLook { stroke: palette.muted, marker: "arrow", dash: Some("4,3"), width: 1.0 },
    }
}

fn cycle_look(palette: &Palette) -> LineLook {
    LineLook { stroke: palette.accent, marker: "arrow-accent", dash: Some("5,4"), width: 1.2 }
}

fn edge_look(edge: &SceneEdge, palette: &Palette) -> LineLook {
    let look = line_look(edge.style, palette);
    if edge.cycle {
        cycle_look(palette)
    } else if edge.focal {
        LineLook { stroke: palette.accent, marker: "arrow-accent", width: FOCAL_LINE_WIDTH, ..look }
    } else {
        look
    }
}

fn box_look(treatment: Treatment, palette: &Palette) -> BoxLook {
    let wash = |alpha: f64| format!("rgba({},{alpha})", palette.wash);
    let (fill, stroke, dash) = match treatment {
        Treatment::Focal => (palette.accent_tint.to_string(), palette.accent.to_string(), None),
        Treatment::Service | Treatment::State => (palette.box_fill.to_string(), palette.ink.to_string(), None),
        Treatment::Store | Treatment::Leaf => (wash(0.05), palette.muted.to_string(), None),
        Treatment::External => (wash(0.03), wash(0.30), None),
        Treatment::Input => (format!("rgba({},0.10)", palette.muted_wash), palette.soft.to_string(), None),
        Treatment::Note => (wash(0.02), wash(0.20), Some("4,3")),
    };
    BoxLook { fill, stroke, dash }
}

fn dash_attribute(dash: Option<&str>) -> String {
    dash.map(|pattern| format!(r#" stroke-dasharray="{pattern}""#)).unwrap_or_default()
}

fn hops(scene: &Scene) -> Vec<Vec<Vec<f64>>> {
    let runs: Vec<Vec<Segment>> = scene.edges.iter().map(|edge| segments(&edge.points)).collect();
    runs.iter()
        .enumerate()
        .map(|(index, own)| {
            own.iter()
                .map(|run| {
                    if !run.is_horizontal() {
                        return Vec::new();
                    }
                    let (lo, hi) = run.span();
                    let mut at: Vec<f64> = runs
                        .iter()
                        .enumerate()
                        .filter(|(other, _)| *other != index)
                        .flat_map(|(_, theirs)| theirs.iter().filter_map(|segment| run.crossing(segment)))
                        .map(|point| point.x)
                        .filter(|&x| x - lo > HOP_ROOM && hi - x > HOP_ROOM)
                        .collect();
                    at.sort_by(f64::total_cmp);
                    at
                })
                .collect()
        })
        .collect()
}

fn draw_edge(out: &mut String, edge: &SceneEdge, hops: &[Vec<f64>], arrow: bool, palette: &Palette) {
    let look = edge_look(edge, palette);
    let marker = if arrow { format!(r#" marker-end="url(#{})""#, look.marker) } else { String::new() };
    let data = path_data(&edge.points, hops);
    let _ = write!(out, r#"<path class="sk-hit" d="{data}" fill="none" stroke="transparent" stroke-width="{HIT_WIDTH}"/>"#);
    let _ = write!(
        out,
        r#"<path class="sk-line" d="{data}" style="fill:none;stroke:{};stroke-width:{}{}"{marker}/>"#,
        look.stroke,
        look.width,
        look.dash.map(|dash| format!(";stroke-dasharray:{dash}")).unwrap_or_default(),
    );
}

fn direction(from: Point, to: Point) -> (f64, f64) {
    (sign(to.x - from.x), sign(to.y - from.y))
}

fn sign(value: f64) -> f64 {
    if value.abs() < 0.01 {
        0.0
    } else {
        value.signum()
    }
}

fn path_data(points: &[Point], hops: &[Vec<f64>]) -> String {
    let mut data = String::new();
    let Some(&first) = points.first() else { return data };
    let _ = write!(data, "M{},{}", first.x, first.y);
    let mut cursor = first;
    for index in 1..points.len() {
        let corner = points[index];
        let incoming = direction(points[index - 1], corner);
        let radius = if index + 1 < points.len() {
            let before = Segment::new(points[index - 1], corner).length() / 2.0;
            let after = Segment::new(corner, points[index + 1]).length() / 2.0;
            CORNER.min(before).min(after)
        } else {
            0.0
        };
        let run_end = Point::new(corner.x - incoming.0 * radius, corner.y - incoming.1 * radius);
        for x in hops_in_travel_order(hops.get(index - 1), incoming.0) {
            let _ = write!(data, " L{},{}", x - incoming.0 * HOP, cursor.y);
            let sweep = u8::from(incoming.0 > 0.0);
            let _ = write!(data, " a{HOP},{HOP} 0 0,{sweep} {},0", incoming.0 * 2.0 * HOP);
        }
        let _ = write!(data, " L{},{}", run_end.x, run_end.y);
        if radius > 0.0 {
            let outgoing = direction(corner, points[index + 1]);
            let after = Point::new(corner.x + outgoing.0 * radius, corner.y + outgoing.1 * radius);
            let _ = write!(data, " Q{},{} {},{}", corner.x, corner.y, after.x, after.y);
            cursor = after;
        } else {
            cursor = run_end;
        }
    }
    data
}

fn hops_in_travel_order(hops: Option<&Vec<f64>>, heading: f64) -> Vec<f64> {
    let mut ordered: Vec<f64> = match hops {
        Some(hops) if heading != 0.0 => hops.clone(),
        _ => return Vec::new(),
    };
    if heading < 0.0 {
        ordered.reverse();
    }
    ordered
}

fn draw_label(out: &mut String, edge: &SceneEdge, palette: &Palette) {
    let Some(label) = &edge.label else { return };
    let look = edge_look(edge, palette);
    let rect = label.rect;
    draw_label_mask(out, &rect, palette);
    let _ = write!(
        out,
        r#"<text x="{}" y="{}" fill="{}" font-size="{LABEL_SIZE}" font-family="{MONO}" text-anchor="middle" letter-spacing="{LABEL_TRACKING}em">{}</text>"#,
        rect.cx(),
        rect.y + 9.0,
        look.stroke,
        escape(&label.text)
    );
}

fn draw_label_mask(out: &mut String, rect: &Rect, palette: &Palette) {
    let _ = write!(
        out,
        r#"<rect class="sk-edge-label-bg" x="{}" y="{}" width="{}" height="{}" rx="2" style="fill:{}"/>"#,
        rect.x, rect.y, rect.w, rect.h, palette.paper
    );
}

fn draw_marks(out: &mut String, edge: &SceneEdge, palette: &Palette) {
    let look = edge_look(edge, palette);
    for mark in &edge.marks {
        let rect = mark.rect;
        draw_label_mask(out, &rect, palette);
        let _ = write!(
            out,
            r#"<text x="{}" y="{}" fill="{}" font-size="{MARK_SIZE}" font-weight="600" font-family="{MONO}" text-anchor="middle">{}</text>"#,
            rect.cx(),
            rect.y + 9.0,
            look.stroke,
            escape(&mark.text)
        );
    }
}

fn draw_chip(out: &mut String, x: f64, y: f64, text: &str, stroke: &str, fill: &str) {
    let width = tag_width(text);
    let _ = write!(
        out,
        r#"<rect x="{x}" y="{y}" width="{width}" height="12" rx="2" fill="none" stroke="{stroke}" stroke-opacity="0.4" stroke-width="0.8"/>"#
    );
    let _ = write!(
        out,
        r#"<text x="{}" y="{}" fill="{fill}" fill-opacity="0.8" font-size="{TAG_SIZE}" font-family="{MONO}" text-anchor="middle" letter-spacing="{TAG_TRACKING}em">{}</text>"#,
        x + width / 2.0,
        y + 9.0,
        escape(text)
    );
}

fn draw_header(out: &mut String, node: &SceneNode, look: &BoxLook, palette: &Palette) {
    let rect = node.rect;
    let header = node.content.header_height();
    let tint = if node.content.treatment == Treatment::Focal { palette.accent_tint.to_string() } else { format!("rgba({},0.04)", palette.wash) };
    let _ = write!(out, r#"<rect x="{}" y="{}" width="{}" height="{header}" rx="6" fill="{tint}"/>"#, rect.x, rect.y, rect.w);
    let _ = write!(out, r#"<rect x="{}" y="{}" width="{}" height="8" fill="{tint}"/>"#, rect.x, rect.y + header - 8.0, rect.w);
    let _ = write!(
        out,
        r#"<line x1="{}" y1="{y}" x2="{}" y2="{y}" stroke="{}" stroke-opacity="0.3" stroke-width="1"/>"#,
        rect.x,
        rect.right(),
        look.stroke,
        y = rect.y + header
    );
}

fn draw_fields(out: &mut String, node: &SceneNode, palette: &Palette) {
    let rect = node.rect;
    let header = node.content.header_height();
    let fields = &node.content.fields;
    let body = rect.h - header;
    let mut baseline = rect.y + header + (body - fields.len() as f64 * FIELD_LINE) / 2.0 + 10.0;
    for field in fields {
        let _ = write!(
            out,
            r#"<text x="{}" y="{baseline}" fill="{}" font-size="{FIELD_SIZE}" font-family="{MONO}">{}</text>"#,
            rect.x + FIELD_INSET,
            palette.ink,
            escape(field)
        );
        baseline += FIELD_LINE;
    }
}

fn draw_node(out: &mut String, node: &SceneNode, palette: &Palette) {
    let rect = node.rect;
    let look = box_look(node.content.treatment, palette);
    let halo = rect.inflate(HALO);
    let _ = write!(
        out,
        r#"<rect class="sk-halo" x="{}" y="{}" width="{}" height="{}" rx="10" fill="{}" pointer-events="none"/>"#,
        halo.x, halo.y, halo.w, halo.h, palette.point
    );
    let _ = write!(out, r#"<rect x="{}" y="{}" width="{}" height="{}" rx="6" fill="{}"/>"#, rect.x, rect.y, rect.w, rect.h, palette.paper);
    let _ = write!(
        out,
        r#"<rect class="sk-shape" x="{}" y="{}" width="{}" height="{}" rx="6" style="fill:{};stroke:{};stroke-width:1{}"/>"#,
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        look.fill,
        look.stroke,
        look.dash.map(|dash| format!(";stroke-dasharray:{dash}")).unwrap_or_default()
    );
    let has_fields = !node.content.fields.is_empty();
    if has_fields {
        draw_header(out, node, &look, palette);
        draw_fields(out, node, palette);
    }
    if let Some(tag) = node.content.tag {
        draw_chip(out, rect.x + 8.0, rect.y + 6.0, tag, &look.stroke, &look.stroke);
    }
    if let Some(badge) = &node.content.badge {
        draw_chip(out, rect.right() - 8.0 - tag_width(badge), rect.y + 6.0, badge, &look.stroke, palette.ink);
    }
    let room = node.content.top_room();
    let name_area = if has_fields { node.content.header_height() } else { rect.h };
    let block_top = rect.y + room + (name_area - room - node.content.block_height()) / 2.0;
    let mut baseline = block_top + 11.0;
    for line in &node.content.lines {
        let _ = write!(
            out,
            r#"<text x="{}" y="{baseline}" fill="{}" font-size="{NAME_SIZE}" font-weight="600" font-family="{SANS}" text-anchor="middle">{}</text>"#,
            rect.cx(),
            palette.ink,
            escape(line)
        );
        baseline += NAME_LINE;
    }
    if let Some(sub) = &node.content.sublabel {
        let _ = write!(
            out,
            r#"<text x="{}" y="{}" fill="{}" font-size="{SUB_SIZE}" font-family="{MONO}" text-anchor="middle">{}</text>"#,
            rect.cx(),
            baseline - NAME_LINE + SUB_LINE,
            palette.soft,
            escape(sub)
        );
    }
    let outline = rect.inflate(OUTLINE);
    let _ = write!(
        out,
        r#"<rect class="sk-outline" x="{}" y="{}" width="{}" height="{}" rx="8" fill="none" stroke="{}" stroke-width="2" pointer-events="none"/>"#,
        outline.x, outline.y, outline.w, outline.h, palette.link
    );
}

/// The fill and stroke of a section's area.
struct Look {
    fill: String,
    stroke: String,
    width: f64,
}

/// Opens the selectable group of a section; a section without an id (nodes no group names) stays plain.
fn open_section(out: &mut String, id: &str, marking: &Marking) -> bool {
    if id.is_empty() {
        return false;
    }
    let _ = write!(out, r#"<g class="sk-group{}" data-id="{}">"#, marking.classes(id), escape(id));
    true
}

fn close_section(out: &mut String, opened: bool) {
    if opened {
        out.push_str("</g>");
    }
}

fn section_area(out: &mut String, rect: &Rect, radius: f64, look: &Look) {
    let _ = write!(
        out,
        r#"<rect class="sk-shape" x="{}" y="{}" width="{}" height="{}" rx="{radius}" style="fill:{};stroke:{};stroke-width:{}"/>"#,
        rect.x, rect.y, rect.w, rect.h, look.fill, look.stroke, look.width
    );
}

fn draw_lanes(out: &mut String, scene: &Scene, width: f64, marking: &Marking, palette: &Palette) {
    let (Some(first), Some(last)) = (scene.lanes.first(), scene.lanes.last()) else { return };
    let rule = |out: &mut String, y: f64| {
        let _ = write!(
            out,
            r#"<line x1="{MARGIN}" y1="{y}" x2="{}" y2="{y}" stroke="rgba({},0.14)" stroke-width="1"/>"#,
            width - MARGIN,
            palette.wash
        );
    };
    for lane in &scene.lanes {
        rule(out, lane.top);
        let opened = open_section(out, &lane.id, marking);
        let band = Rect::new(MARGIN, lane.top, width - 2.0 * MARGIN, lane.bottom - lane.top);
        section_area(out, &band, 0.0, &Look { fill: "transparent".into(), stroke: "none".into(), width: 1.0 });
        let _ = write!(
            out,
            r#"<text x="{MARGIN}" y="{}" fill="{}" font-size="{LANE_LABEL_SIZE}" font-family="{MONO}" letter-spacing="{LANE_LABEL_TRACKING}em">{}</text>"#,
            (lane.top + lane.bottom) / 2.0 + 3.0,
            palette.muted,
            escape(&lane.label)
        );
        close_section(out, opened);
    }
    rule(out, last.bottom);
    let _ = write!(
        out,
        r#"<line x1="{x}" y1="{}" x2="{x}" y2="{}" stroke="rgba({},0.14)" stroke-width="1"/>"#,
        first.top,
        last.bottom,
        palette.wash,
        x = scene.lane_gutter
    );
}

fn draw_layers(out: &mut String, scene: &Scene, width: f64, marking: &Marking, palette: &Palette) {
    let (Some(first), Some(last)) = (scene.lanes.first(), scene.lanes.last()) else { return };
    let hairline = format!("rgba({},0.12)", palette.wash);
    let named = scene.lanes.iter().filter(|lane| !lane.label.is_empty()).count();
    for (index, lane) in scene.lanes.iter().enumerate() {
        let opened = open_section(out, &lane.id, marking);
        let band = Rect::new(MARGIN, lane.top, width - 2.0 * MARGIN, lane.bottom - lane.top);
        section_area(out, &band, 0.0, &Look { fill: "transparent".into(), stroke: "none".into(), width: 1.0 });
        if !lane.label.is_empty() {
            draw_layer_name(out, lane, named - index, palette);
        }
        close_section(out, opened);
    }
    let _ = write!(
        out,
        r#"<rect x="{MARGIN}" y="{}" width="{}" height="{}" fill="none" stroke="{}" stroke-width="1"/>"#,
        first.top,
        width - 2.0 * MARGIN,
        last.bottom - first.top,
        palette.muted
    );
    let _ = write!(
        out,
        r#"<line x1="{x}" y1="{}" x2="{x}" y2="{}" stroke="{hairline}" stroke-width="1"/>"#,
        first.top,
        last.bottom,
        x = scene.lane_gutter
    );
    for lane in scene.lanes.iter().take(scene.lanes.len() - 1) {
        let _ = write!(
            out,
            r#"<line x1="{MARGIN}" y1="{y}" x2="{}" y2="{y}" stroke="{hairline}" stroke-width="1"/>"#,
            width - MARGIN,
            y = lane.bottom
        );
    }
}

fn draw_layer_name(out: &mut String, lane: &Lane, number: usize, palette: &Palette) {
    let middle = (lane.top + lane.bottom) / 2.0;
    let _ = write!(
        out,
        r#"<text x="{}" y="{}" fill="{}" font-size="8" font-family="{MONO}" letter-spacing="0.14em">L{number}</text>"#,
        MARGIN + LAYER_INSET,
        middle - 6.0,
        palette.muted
    );
    let _ = write!(
        out,
        r#"<text x="{}" y="{}" fill="{}" font-size="{NAME_SIZE}" font-weight="600" font-family="{SANS}">{}</text>"#,
        MARGIN + LAYER_INSET,
        middle + 10.0,
        palette.ink,
        escape(&lane.label)
    );
}

fn draw_ring(out: &mut String, ring: &Ring, marking: &Marking, palette: &Palette) {
    let rect = ring.rect;
    let (fill, stroke) = match ring.depth {
        0 => (0.015, format!("rgba({},0.30)", palette.wash)),
        1 => (0.02, format!("rgba({},0.35)", palette.wash)),
        2 => (0.025, format!("rgba({},0.45)", palette.wash)),
        _ => (0.03, palette.muted.to_string()),
    };
    let opened = open_section(out, &ring.id, marking);
    section_area(out, &rect, 8.0, &Look { fill: format!("rgba({},{fill})", palette.wash), stroke, width: 1.0 });
    let mask = ring.label_rect;
    let _ = write!(out, r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}"/>"#, mask.x, mask.y, mask.w, mask.h, palette.paper);
    let _ = write!(
        out,
        r#"<text x="{}" y="{}" fill="{}" font-size="{RING_LABEL_SIZE}" font-family="{MONO}" letter-spacing="{RING_LABEL_TRACKING}em">{}</text>"#,
        mask.x + RING_LABEL_PADDING,
        rect.y + 3.0,
        palette.muted,
        escape(&ring.label)
    );
    close_section(out, opened);
}

fn draw_zone(out: &mut String, zone: &Zone, marking: &Marking, palette: &Palette) {
    let (rect, label) = (&zone.rect, zone.label.as_str());
    let opened = open_section(out, &zone.id, marking);
    let look = Look { fill: format!("rgba({},0.02)", palette.wash), stroke: format!("rgba({},0.10)", palette.wash), width: 0.8 };
    section_area(out, rect, 8.0, &look);
    let width = mono_width(label, 7.0, 0.14) + 8.0;
    let _ = write!(out, r#"<rect x="{}" y="{}" width="{width}" height="12" rx="2" fill="{}"/>"#, rect.x + 8.0, rect.y + 4.0, palette.paper);
    let _ = write!(
        out,
        r#"<text x="{}" y="{}" fill="rgba({},0.55)" font-size="7" font-family="{MONO}" letter-spacing="0.14em">{}</text>"#,
        rect.x + 12.0,
        rect.y + 13.0,
        palette.wash,
        escape(label)
    );
    close_section(out, opened);
}

enum LegendItem {
    Box(Treatment, &'static str),
    Line(LineLook, &'static str),
}

impl LegendItem {
    fn text(&self) -> &'static str {
        match self {
            LegendItem::Box(_, text) | LegendItem::Line(_, text) => text,
        }
    }

    fn width(&self) -> f64 {
        let swatch = match self {
            LegendItem::Box(..) => 14.0,
            LegendItem::Line(..) => 28.0,
        };
        swatch + 8.0 + mono_width(self.text(), 8.0, 0.1) + 28.0
    }
}

fn legend_items(scene: &Scene, palette: &Palette) -> Vec<LegendItem> {
    let boxes = [
        (Treatment::Focal, "FOCUS"),
        (Treatment::Service, "SERVICE"),
        (Treatment::State, "STATE"),
        (Treatment::Store, "STORE"),
        (Treatment::Leaf, "LEAF"),
        (Treatment::External, "EXTERNAL"),
        (Treatment::Input, "CLIENT"),
        (Treatment::Note, "NOTE"),
    ];
    let lines = [
        (EdgeStyle::Sync, "CALL"),
        (EdgeStyle::Data, "DATA"),
        (EdgeStyle::Transition, "TRANSITION"),
        (EdgeStyle::Async, "ASYNC"),
        (EdgeStyle::Dependency, "DEPENDS ON"),
    ];
    let used_boxes: Vec<LegendItem> = boxes
        .into_iter()
        .filter(|(treatment, _)| scene.nodes.iter().any(|node| node.content.treatment == *treatment))
        .map(|(treatment, text)| LegendItem::Box(treatment, text))
        .collect();
    let cycle = scene.edges.iter().any(|edge| edge.cycle).then(|| LegendItem::Line(cycle_look(palette), "CYCLE"));
    let used_lines: Vec<LegendItem> = lines
        .into_iter()
        .filter(|(style, _)| scene.edges.iter().any(|edge| !edge.cycle && edge.style == *style))
        .map(|(style, text)| LegendItem::Line(line_look(style, palette), text))
        .chain(cycle)
        .collect();
    [used_boxes, used_lines].into_iter().filter(|kinds| kinds.len() >= 2).flatten().collect()
}

fn draw_legend(out: &mut String, items: &[LegendItem], top: f64, width: f64, palette: &Palette) {
    let rule_y = top - MARGIN / 2.0 + LEGEND_TOP;
    let row_y = rule_y + LEGEND_ROW;
    let _ = write!(
        out,
        r#"<line x1="{MARGIN}" y1="{rule_y}" x2="{}" y2="{rule_y}" stroke="rgba({},0.10)" stroke-width="0.8"/>"#,
        width - MARGIN,
        palette.wash
    );
    let _ = write!(
        out,
        r#"<text x="{MARGIN}" y="{}" fill="{}" font-size="8" font-family="{MONO}" letter-spacing="0.14em">LEGEND</text>"#,
        row_y + 3.0,
        palette.muted
    );
    let mut x = MARGIN + 80.0;
    for item in items {
        let text_x = match item {
            LegendItem::Box(treatment, _) => {
                let look = box_look(*treatment, palette);
                let _ = write!(
                    out,
                    r#"<rect x="{x}" y="{}" width="14" height="10" rx="2" fill="{}" stroke="{}" stroke-width="1"{}/>"#,
                    row_y - 5.0,
                    look.fill,
                    look.stroke,
                    dash_attribute(look.dash)
                );
                x + 22.0
            }
            LegendItem::Line(look, _) => {
                let _ = write!(
                    out,
                    r#"<line x1="{x}" y1="{row_y}" x2="{}" y2="{row_y}" stroke="{}" stroke-width="{}"{} marker-end="url(#{})"/>"#,
                    x + 26.0,
                    look.stroke,
                    look.width,
                    dash_attribute(look.dash),
                    look.marker
                );
                x + 36.0
            }
        };
        let _ = write!(
            out,
            r#"<text x="{text_x}" y="{}" fill="{}" font-size="8" font-family="{MONO}" letter-spacing="0.1em">{}</text>"#,
            row_y + 3.0,
            palette.muted,
            item.text()
        );
        x += item.width();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_elbow_is_drawn_with_straight_runs_and_a_rounded_corner() {
        let points = [Point::new(0.0, 0.0), Point::new(40.0, 0.0), Point::new(40.0, 40.0)];
        assert_eq!(path_data(&points, &[]), "M0,0 L32,0 Q40,0 40,8 L40,40");
    }

    #[test]
    fn a_hop_bridges_a_crossing_on_a_leftward_run() {
        let points = [Point::new(100.0, 0.0), Point::new(0.0, 0.0)];
        assert_eq!(path_data(&points, &[vec![50.0]]), "M100,0 L58,0 a8,8 0 0,0 -16,0 L0,0");
    }
}
